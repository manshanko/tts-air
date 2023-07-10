pub struct TtsAir {
    lib: isize,
    tts: unsafe extern "C" fn(*const u16) -> bool,
}

impl TtsAir {
    pub fn new() -> Self {
        let lib = unsafe {
            windows_sys::Win32::System::LibraryLoader::LoadLibraryA("saapi64.dll\0".as_ptr() as *const _)
        };
        let tts: unsafe extern "C" fn(text: *const u16) -> bool = unsafe {
            assert_ne!(0, lib);
            let running = windows_sys::Win32::System::LibraryLoader::GetProcAddress(lib, "SA_IsRunning\0".as_ptr() as *const _);
            let running: unsafe extern "C" fn() -> bool = core::mem::transmute(running.unwrap());
            running();
            let tts = windows_sys::Win32::System::LibraryLoader::GetProcAddress(lib, "SA_SayW\0".as_ptr() as *const _);
            core::mem::transmute(tts.unwrap())
        };

        Self {
            lib,
            tts,
        }
    }

    pub fn say(&mut self, text: &str) -> bool {
        let mut wchar = Vec::with_capacity(text.len());
        for c in text.encode_utf16() {
            wchar.push(c);
        }
        wchar.push(0);

        unsafe {
            (self.tts)(wchar.as_ptr())
        }
    }
}

impl Drop for TtsAir {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::LibraryLoader::FreeLibrary(self.lib);
        }
    }
}
