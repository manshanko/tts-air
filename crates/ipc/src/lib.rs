#![allow(dead_code)]
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::IO::*;
use windows_sys::Win32::System::Threading::*;

fn get_last_error() -> u32 {
    unsafe {
        GetLastError()
    }
}

pub const WARTIDE_ADDRESS: &'static str = "\\\\.\\pipe\\net.wartide.d4.tts-air-0\0";

pub struct NamedPipeListener {
    inner: Option<(HANDLE, Event)>,
}

impl NamedPipeListener {
    pub fn bind(path: &str) -> Result<Self, u32> {
        if path.len() >= 1000 {
            return Err(u32::MAX);
        }

        let mut buffer = [0; 1024];
        let mut len = 0;
        for c in path.encode_utf16() {
            buffer[len] = c;
            len += 1;
        }

        let hwnd = unsafe {
            CreateNamedPipeW(
                buffer.as_ptr(),
                //PIPE_ACCESS_OUTBOUND,
                PIPE_ACCESS_DUPLEX
                | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE
                | PIPE_READMODE_BYTE
                | PIPE_WAIT
                | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                2048,
                0,
                0,
                core::ptr::null_mut(),
            )
        };

        if hwnd == INVALID_HANDLE_VALUE {
            return Err(get_last_error());
        }

        let event = unsafe {
            CreateEventW(
                core::ptr::null_mut(),
                0,
                0,
                core::ptr::null_mut(),
            )
        };

        if event == 0 {
            return Err(get_last_error());
        }

        let event = Event(event);

        Ok(Self {
            inner: Some((hwnd, event)),
        })
    }

    pub fn listen(&mut self) -> Result<NamedPipe, u32> {
        self.listen_timeout_ms_(None)
    }

    pub fn listen_timeout_ms(&mut self, ms: u32) -> Result<NamedPipe, u32> {
        self.listen_timeout_ms_(Some(ms))
    }

    fn listen_timeout_ms_(&mut self, ms: Option<u32>) -> Result<NamedPipe, u32> {
        if let Some((hwnd, event)) = self.inner.as_mut() {
            // TODO: lifetime with event in OVERLAPPED
            unsafe {
                let mut ow: OVERLAPPED = core::mem::zeroed();
                ow.hEvent = event.0;
                let err = ConnectNamedPipe(*hwnd, &mut ow);
                if err == 0 {
                    match get_last_error() {
                        ERROR_PIPE_CONNECTED => (),
                        ERROR_IO_PENDING => {
                            if let Some(ms) = ms {
                                WaitForSingleObject(event.0, ms);
                            } else {
                                WaitForSingleObject(event.0, INFINITE);
                            }

                            let mut ow2: OVERLAPPED = core::mem::zeroed();
                            ow2.hEvent = event.0;
                            let err = ConnectNamedPipe(*hwnd, &mut ow2);
                            if err == 0 {
                                match get_last_error() {
                                    ERROR_PIPE_CONNECTED => (),
                                    e => return Err(e),
                                }
                            }
                        }
                        e => return Err(e),
                    }
                }
            }
        }

        let Some((hwnd, event)) = self.inner.take() else {
            log::warn!("NamedPipeListener::inner is None");
            return Err(0x00000006/*ERROR_INVALID_HANDLE*/);
        };

        Ok(NamedPipe {
            hwnd,
            is_server: Some(event),
        })
    }
}

impl Drop for NamedPipeListener {
    fn drop(&mut self) {
        if let Some((hwnd, _)) = self.inner.take() {
            unsafe {
                FlushFileBuffers(hwnd);
                DisconnectNamedPipe(hwnd);
                CloseHandle(hwnd);
            }
        }
    }
}

pub struct NamedPipe {
    hwnd: HANDLE,
    is_server: Option<Event>,
}

unsafe impl Send for NamedPipe {}

impl NamedPipe {
    pub fn open(path: &str) -> Result<Self, u32> {
        if path.is_empty() || path.as_bytes()[path.len() - 1] != 0 {
            return Err(u32::MAX);
        }

        let hwnd = unsafe {
            CreateFileA(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                core::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                0,
            )
        };

        if hwnd != INVALID_HANDLE_VALUE {
            Ok(NamedPipe {
                hwnd,
                is_server: None,
            })
        } else {
            Err(get_last_error())
        }
    }

    pub fn recv(&mut self, buffer: &mut [u8]) -> Result<u64, u32> {
        debug_assert!(self.is_server.is_none());
        if self.hwnd == INVALID_HANDLE_VALUE {
            log::warn!("NamedPipe::hwnd is NULL");
            return Err(0x00000006/*ERROR_INVALID_HANDLE*/);
        }

        let mut read = 0;
        let err = unsafe {
            ReadFile(
                self.hwnd,
                buffer.as_mut_ptr() as *mut _,
                u32::try_from(buffer.len()).unwrap(),
                &mut read,
                core::ptr::null_mut(),
            )
        };

        if err != 0 {
            Ok(read as u64)
        } else {
            Err(get_last_error())
        }
    }

    pub fn send(&mut self, msg: &[u8]) -> Result<u64, u32> {
        if self.hwnd == INVALID_HANDLE_VALUE {
            log::warn!("NamedPipe::hwnd is NULL");
            return Err(0x00000006/*ERROR_INVALID_HANDLE*/);
        }

        if msg.len() > u32::MAX as usize {
            log::error!("message exceeds u32::MAX ({})", msg.len());
        }

        let mut wrote = 0;
        if unsafe {
            // server NamedPipe was opened with OVERLAPPED
            if let Some(event) = &mut self.is_server {
                let mut ow: OVERLAPPED = core::mem::zeroed();
                ow.hEvent = event.0;
                let err = WriteFile(
                    self.hwnd,
                    msg.as_ptr(),
                    msg.len().min(u32::MAX as usize) as u32,
                    &mut wrote,
                    &mut ow,
                );
                if err == 0 {
                    match get_last_error() {
                        ERROR_IO_PENDING => WaitForSingleObject(event.0, INFINITE),
                        _ => return Err(get_last_error()),
                    };
                }
                1
            } else {
                WriteFile(
                    self.hwnd,
                    msg.as_ptr(),
                    msg.len().min(u32::MAX as usize) as u32,
                    &mut wrote,
                    core::ptr::null_mut(),
                )
            }
        } == 0 {
            Err(get_last_error())
        } else {
            // TODO: verify size is correct when async (is_server.is_some())
            Ok(msg.len() as u64)
        }
    }
}

impl Drop for NamedPipe {
    fn drop(&mut self) {
        if self.hwnd != INVALID_HANDLE_VALUE {
            unsafe {
                if self.is_server.is_some() {
                    FlushFileBuffers(self.hwnd);
                    DisconnectNamedPipe(self.hwnd);
                }
                CloseHandle(self.hwnd);
            }
            self.hwnd = INVALID_HANDLE_VALUE;
        }
    }
}

#[repr(transparent)]
#[derive(Clone)]
struct Event(HANDLE);

impl Drop for Event {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                CloseHandle(self.0);
            }
            self.0 = 0;
        }
    }
}