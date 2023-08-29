use std::sync::Mutex;
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;
use std::time::Instant;

mod saapi;

static PUMP: Mutex<Option<Sender<(Box<[u16]>, Instant)>>> = Mutex::new(None);

#[no_mangle]
unsafe extern "system" fn DllMain(
    hwnd: isize,
    reason: u32,
    _: *const (),
) {
    match reason {
        // we leak a reference of self to prevent the dll from being unloaded
        // to simplify cleanup (by not doing it)
        0/*DLL_PROCESS_DETACH*/ => (),

        1/*DLL_PROCESS_ATTACH*/ => init(hwnd),

        _ => (),
    }
}

unsafe fn init(hwnd: isize) {
    std::panic::set_hook(Box::new(|e| {
        log::debug!("err: {e:?}");
    }));

    if let Ok(mut pump) = PUMP.lock() {
        if pump.is_none() {
            let (send, recv) = mpsc::channel();
            *pump = Some(send);
            drop(pump);

            thread::spawn(move || {
                {
                    use windows_sys::Win32::System::LibraryLoader::*;

                    let mut buffer = [0; 0x1000];
                    GetModuleFileNameW(hwnd, buffer.as_mut_ptr(), 0xfff);

                    // leak reference to self
                    let _lib = LoadLibraryW(buffer.as_ptr());
                }

                if cfg!(debug_assertions) {
                    match std::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open("tts_air.log")
                    {
                        Ok(fd) => {
                            let _ = env_logger::builder()
                                .target(env_logger::fmt::Target::Pipe(Box::new(fd)))
                                .filter(None, log::LevelFilter::Trace)
                                //.filter(None, log::LevelFilter::Info)
                                .try_init();
                        }
                        _ => (),
                    }
                }

                let (pipe_send, pipe_recv) = mpsc::channel::<tts_air_ipc::NamedPipe>();

                thread::spawn(|| server_broadcast(recv, pipe_recv));
                thread::spawn(|| server_listen(pipe_send));

                log::debug!("successfully started tts-air-capture");
            });
        }
    }
}

fn server_broadcast(
    recv: Receiver<(Box<[u16]>, Instant)>,
    pipe_recv: Receiver<tts_air_ipc::NamedPipe>,
) {
    let recv = recv;
    let pipe_recv = pipe_recv;
    let mut pipes = Vec::new();

    let mut next_start = None;
    let mut buffer = String::new();
    loop {
        buffer.clear();
        match if let Some(next_start) = next_start.take() {
            Ok(next_start)
        } else {
            recv.try_recv()
        } {
            Ok((text, start)) => {
                let deadline = start + Duration::from_millis(2);
                for c in char::decode_utf16(text.into_iter().map(|b| *b)) {
                    buffer.push(c.unwrap_or('\u{FFFD}'));
                }
                thread::sleep(std::time::Duration::from_millis(1));

                while let Ok((more, next)) = recv.try_recv() {
                    if next < deadline {
                        buffer.push('\n');
                        for c in char::decode_utf16(more.into_iter().map(|b| *b)) {
                            buffer.push(c.unwrap_or('\u{FFFD}'));
                        }
                    } else {
                        next_start = Some((more, next));
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Err(e) => {
                log::error!("text pump had error {e:?}");
                break;
            }
        };

        if buffer.len() >= u16::MAX as usize {
            buffer.truncate(u16::MAX as usize - 1);
            continue;
        }

        buffer.push('\0');

        while let Ok(pipe) = pipe_recv.try_recv() {
            pipes.push(pipe);
        }

        log::debug!("text: {:?} to {}", buffer, pipes.len());
        pipes.retain_mut(|pipe| {
            match pipe.send(buffer.as_bytes()) {
                Ok(_) => true,
                Err(e) => {
                    log::error!("NamedPipe::send error {e:?}");
                    false
                }
            }
        });
    }
}

fn server_listen(
    pipe_send: Sender<tts_air_ipc::NamedPipe>,
) {
    let pipe_send = pipe_send;

    let mut pipe = None;
    loop {
        if pipe.is_none() {
            match tts_air_ipc::NamedPipeListener::bind(tts_air_ipc::WARTIDE_ADDRESS) {
                Ok(p) => pipe = Some(p),
                Err(e) => {
                    log::error!("failed to listen on {:?} with error {}", tts_air_ipc::WARTIDE_ADDRESS, e);
                    thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
            };
        }

        if let Some(mut p) = pipe.take() {
            match p.listen_timeout_ms(50) {
                Ok(p) => {
                    if let Err(e) = pipe_send.send(p) {
                        log::error!("pipe_send had error {e:?}");
                    } else {
                        log::info!("client connection established");
                    }
                }
                Err(e) if e == windows_sys::Win32::Foundation::ERROR_IO_PENDING => {
                    pipe = Some(p);
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(e) => {
                    drop(p);
                    log::error!("pipe listener had unexpected error {e:?}");
                    // reduce potential log spam
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };
        }
    }
}

unsafe fn send_wchar(wchar: *const u16) {
    if let Some(text) = box_wchar(wchar) {
        if let Ok(pump) = PUMP.lock() {
            if let Some(pump) = &*pump {
                if pump.send((text, std::time::Instant::now())).is_err() {
                    log::error!("failed to send text over PUMP");
                }
            }
        }
    }
}

unsafe fn box_wchar(text: *const u16) -> Option<Box<[u16]>> {
    if !text.is_null() {
        let mut len = 0;
        while *text.wrapping_offset(len) != 0 {
            len += 1
        }
        let slice = core::slice::from_raw_parts(text, len as usize);
        Some(Box::from(slice))
    } else {
        None
    }
}