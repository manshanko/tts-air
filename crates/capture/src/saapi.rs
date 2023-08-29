use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[no_mangle]
unsafe extern "C" fn SA_SayW(wchar: *const u16) -> u8 {
    static DID_PANIC: AtomicBool = AtomicBool::new(false);

    // avoid log spam if a panic happens
    if !DID_PANIC.load(Ordering::SeqCst) {
        if let Err(_) = std::panic::catch_unwind(|| {
            crate::send_wchar(wchar);
        }) {
            DID_PANIC.store(true, Ordering::SeqCst);
            log::debug!("failed to send tts event due to panic");
        }
    }
    true.into()
}

#[no_mangle]
unsafe extern "C" fn SA_BrlShowTextW(_wchar: *const u16) -> u8 {
    true.into()
}

#[no_mangle]
unsafe extern "C" fn SA_StopAudio() -> u8 {
    true.into()
}

#[no_mangle]
unsafe extern "C" fn SA_IsRunning() -> u8 {
    true.into()
}