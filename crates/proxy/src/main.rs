use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::sync::mpsc::Receiver;
use std::io;
use std::io::Read;
use std::net::SocketAddrV4;
use std::net::Ipv4Addr;
use std::net::TcpListener;
use std::thread;

mod stream;
use stream::NonblockingStream;
mod tts;
use tts::TtsAir;

const LISTEN_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 61806);

fn main() {
    println!("{}@{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    let mut builder = if cfg!(debug_assertions) {
        env_logger::builder()
    } else {
        let fd = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open("tts-air-cli.log")
            .unwrap();

        let mut b = env_logger::builder();
        b.target(env_logger::fmt::Target::Pipe(Box::new(fd)));
        b
    };

    if cfg!(debug_assertions) {
        builder.filter_module("tts_air_proxy", log::LevelFilter::Trace);
    } else if std::env::var_os("RUST_LOG").is_none() {
        builder.filter_module("tts_air_proxy", log::LevelFilter::Info);
    }

    let _ = builder.try_init();

    let mut args = std::env::args();
    let _ = args.next();

    let mut do_default = true;
    let mut tts = None;
    let mut proxy = None;
    for arg in args {
        match arg.as_str() {
            "--proxy" => {
                do_default = false;
                proxy = Some(thread::spawn(start_proxy));
            }
            "--test" => {
                do_default = false;
                tts = Some(TtsAir::new());
            }
            _ => (),
        }
    }

    if do_default {
        if cfg!(debug_assertions) {

        } else {
            start_proxy();
        }
    }

    if tts.is_some() {
        println!("ECHO MODE ENABLED");
        let mut stdin = io::stdin().lock();
        let mut buffer = [0; 0x10000];
        while let Ok(read) = stdin.read(&mut buffer) {
            let slice = &buffer[..read];
            if let Ok(text) = std::str::from_utf8(slice) {
                let text = text.trim();
                match text {
                    "/quit" => {
                        break;
                    }
                    "/drop" => {
                        if let Some(tts) = tts.take() {
                            drop(tts);
                        }
                    }
                    "/init" => {
                        if tts.is_none() {
                            tts = Some(TtsAir::new());
                        }
                    }
                    text => {
                        if let Some(tts) = &mut tts {
                            tts.say(text);
                        }
                    }
                }
            }
        }
    } else if let Some(proxy) = proxy {
        proxy.join().unwrap();
    }
}

struct WebSocketContext {
    ws: tungstenite::WebSocket<NonblockingStream>,
    origin: String,
    pipe_notify_state: bool,
}

impl Drop for WebSocketContext {
    fn drop(&mut self) {
        log::debug!("websocket disconnect from {:?}", self.origin);
    }
}

fn start_proxy() {
    let pipe_connected: AtomicBool = AtomicBool::new(false);
    let websocket_connected: AtomicBool = AtomicBool::new(false);

    let (send, recv) = mpsc::channel();
    let (send_ws, recv_ws) = mpsc::channel();
    let server = TcpListener::bind(LISTEN_ADDR).unwrap();

    thread::scope(|s| {
        s.spawn(|| proxy_tts_listen(send, &pipe_connected, &websocket_connected));
        s.spawn(|| proxy_ws_listen(server, send_ws));
        s.spawn(|| proxy_ws_broadcast(recv, recv_ws, &pipe_connected, &websocket_connected));
    });
}

fn proxy_tts_listen(
    send: Sender<String>,
    pipe_connected: &AtomicBool,
    websocket_connected: &AtomicBool,
) {
    loop {
        match tts_air_ipc::NamedPipe::open(tts_air_ipc::WARTIDE_ADDRESS) {
            Ok(mut pipe) => {
                log::info!("connected to text-to-speech capture at {:?}", tts_air_ipc::WARTIDE_ADDRESS);
                pipe_connected.store(true, Ordering::Relaxed);

                let mut text = Vec::with_capacity(0x10000);
                let mut buffer = [0; 0x10000];
                loop {
                    match pipe.recv(&mut buffer) {
                        Ok(read) => {
                            let buffer = &buffer[..read as usize];
                            for b in buffer {
                                let b = *b;

                                if b != 0 {
                                    text.push(b);
                                } else {
                                    let text_s = String::from_utf8_lossy(&text);
                                    log::debug!("tts string {text_s:?}");
                                    if websocket_connected.load(Ordering::Relaxed) {
                                        send.send(text_s.to_string()).unwrap();
                                    }
                                    text.clear();
                                }
                            }
                        }
                        Err(e) => {
                            log::trace!("failed connection read with error code {e:08x}");
                            break;
                        }
                    }
                }
            }
            Err(e) => log::trace!("failed connect to tts capture with error code 0x{e:08x}"),
        }

        pipe_connected.store(false, Ordering::Relaxed);
        thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn proxy_ws_listen(
    server: TcpListener,
    send_ws: Sender<(tungstenite::WebSocket<NonblockingStream>, String)>,
) {
    loop {
        let Ok((stream, _addr)) = server.accept() else {
            thread::sleep(std::time::Duration::from_millis(10));
            continue;
        };

        log::trace!("tcp connection attempt");
        let Ok(mut stream) = NonblockingStream::new(stream) else {
            log::debug!("failed to construct non-blocking stream");
            continue;
        };

        stream.set_blocking(true);

        let mut origin = None;
        let res = tungstenite::accept_hdr(stream, |
            req: &tungstenite::handshake::server::Request,
            res,
        | {
            let req = req.headers();
            let org = req.get("origin");
            log::debug!("websocket connection headers:\n  user-agent: {:?}\n  host: {:?}\n  origin: {:?}",
                req.get("user-agent"),
                req.get("host"),
                org,
            );

            origin = Some(org
                .map(|h| h.to_str().unwrap_or("<invalid-str>"))
                .unwrap_or("<null>").to_string());

            if org.map(|o| o.to_str().ok())
                .flatten()
                .filter(|o| *o == "https://d4.wartide.net"
                    // allow localhost connections
                    || *o == "null"
                    || (cfg!(debug_assertions) && cfg!(feature = "unsafe-connection")))
                .is_some()
            {
                return Ok(res);
            }

            log::debug!("failed connection from origin {org:?}");
            Err(tungstenite::handshake::server::Response::builder()
                .status(tungstenite::http::StatusCode::NOT_FOUND)
                .body(None)
                .unwrap()
            )
        });

        match res {
            Ok(mut ws) => {
                ws.get_mut().set_blocking(false);
                if let Some(origin) = origin.take() {
                    let _ = send_ws.send((ws, origin));
                } else {
                    log::debug!("invalid origin from websocket connection");
                }
            }
            Err(e) => log::trace!("failed websocket connection with error {e:?}"),
        }
    }
}

fn proxy_ws_broadcast(
    recv: Receiver<String>,
    recv_ws: Receiver<(tungstenite::WebSocket<NonblockingStream>, String)>,
    pipe_connected: &AtomicBool,
    websocket_connected: &AtomicBool,
) {
    let mut is_connected = None;
    let mut json_state = String::new();
    let mut connections = Vec::new();
    loop {
        let pipe_connected = pipe_connected.load(Ordering::Relaxed);
        if is_connected != Some(pipe_connected) {
            is_connected = Some(pipe_connected);
            json_state = serde_json::json!({
                "method": "info",
                "args": {
                    "proxy_version": env!("CARGO_PKG_VERSION"),
                    "is_connected": pipe_connected,
                },
            }).to_string();
        }

        if let Ok((ws, origin)) = recv_ws.try_recv() {
            let mut wsc = WebSocketContext {
                ws,
                origin,
                pipe_notify_state: pipe_connected,
            };

            'try_push: {
                if let Err(e) = wsc.ws.write_message(tungstenite::Message::Text(json_state.to_owned())) {
                    log::debug!("failed to update tts connection state to websocket with error {e:?}");
                    break 'try_push;
                }

                if connections.is_empty() {
                    while let Ok(_) = recv.try_recv() {}
                    websocket_connected.store(true, Ordering::Relaxed);
                }

                let total = connections.len() + 1;
                let origin = &wsc.origin;
                log::info!("websocket connect from {origin:?} ({total} total)");

                connections.push(wsc);
            }
        }

        let text = recv.try_recv();
        if !connections.is_empty() {
            let text = text.ok().map(|text| {
                serde_json::json!({
                    "method": "tts_message",
                    "args": {
                        "message": text,
                    }
                }).to_string()
            });

            connections.retain_mut(|wsc| {
                match wsc.ws.read_message() {
                    Ok(tungstenite::Message::Text(json)) => {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json) {
                            if json["id"].is_i64() {
                                let json = serde_json::json!({
                                    "id": json["id"],
                                    "data": "",
                                });

                                if let Err(_) = wsc.ws.write_message(tungstenite::Message::Text(json.to_string())) {
                                    log::debug!("failed to send message to websocket connection");
                                    return false;
                                }
                            } else {
                                log::debug!("received json missing \"id\" field");
                            }
                        } else {
                            log::debug!("expected json but received unknown");
                        }
                    },
                    Ok(tungstenite::Message::Close(_)) => return false,
                    Ok(_) => log::debug!("expected text from websocket connection"),
                    Err(tungstenite::Error::Io(err)) if err.kind() == io::ErrorKind::WouldBlock => (),
                    Err(e) => {
                        log::debug!("failed websocket connection with error {e:?}");
                        return false;
                    }
                }

                if let Some(text) = &text {
                    if let Err(e) = wsc.ws.write_message(tungstenite::Message::Text(text.to_owned())) {
                        log::debug!("failed to send tts event with error {e:?}");
                        return false;
                    }
                }

                if wsc.pipe_notify_state != pipe_connected {
                    if let Err(e) = wsc.ws.write_message(tungstenite::Message::Text(json_state.to_owned())) {
                        log::debug!("failed to update info with error {e:?}");
                        return false;
                    }
                    wsc.pipe_notify_state = pipe_connected;
                }

                true
            });

            if connections.is_empty() {
                websocket_connected.store(false, Ordering::Relaxed);
            }
        }

        thread::sleep(std::time::Duration::from_millis(5));
    }
}