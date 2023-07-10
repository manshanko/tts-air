use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::io;
use std::io::Read;
use std::net::TcpStream;
use std::thread;

pub struct NonblockingStream {
    inner: TcpStream,
    recv: Receiver<Vec<u8>>,
    current: Option<(Vec<u8>, usize)>,
    is_blocking: bool,
}

impl NonblockingStream {
    pub fn new(inner: TcpStream) -> io::Result<Self> {
        let reader = inner.try_clone()?;
        let (send, recv) = mpsc::channel();
        thread::spawn(|| {
            let mut reader = reader;
            let send = send;

            let mut buffer = [0; 0x40000];
            while let Ok(size) = reader.read(&mut buffer) {
                let buffer = buffer[..size].to_vec();
                if send.send(buffer).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            inner,
            recv,
            current: None,
            is_blocking: false,
        })
    }

    pub fn set_blocking(&mut self, blocking: bool) {
        self.is_blocking = blocking;
    }
}

impl Read for NonblockingStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let total = buffer.len();
        let mut read = buffer.len();
        if let Some((current, offset)) = self.current.take() {
            let slice = &current[offset..];
            let copy = total.min(slice.len());
            buffer[..copy].copy_from_slice(&slice[..copy]);
            read -= copy;
            if copy < slice.len() {
                self.current = Some((current, offset + copy));
            }
        }

        if read == 0 {
            Ok(buffer.len())
        } else {
            if read < total || !self.is_blocking {
                match self.recv.try_recv() {
                    Ok(current) => {
                        let offset = total - read;
                        let buffer = &mut buffer[offset..];

                        let copy = total.min(current.len());
                        buffer[..copy].copy_from_slice(&current[..copy]);
                        read -= copy;
                        if copy < current.len() {
                            self.current = Some((current, copy));
                        }

                        Ok(total - read)
                    }
                    Err(_) => {
                        if read < buffer.len() {
                            Ok(total - read)
                        } else {
                            Err(io::Error::new(io::ErrorKind::WouldBlock, ""))
                        }
                    }
                }
            } else {
                let slice = self.recv.recv().map_err(|_| io::Error::new(io::ErrorKind::Other, "Receiver error"))?;
                let copy = total.min(slice.len());
                buffer[..copy].copy_from_slice(&slice[..copy]);
                Ok(copy)
            }
        }
    }
}

impl io::Write for NonblockingStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.inner.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
