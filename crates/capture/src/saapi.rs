#[no_mangle]
unsafe extern "C" fn SA_SayW(wchar: *const u16) -> bool {
    crate::send_wchar(wchar);
    true
}

#[no_mangle]
unsafe extern "C" fn SA_BrlShowTextW(_wchar: *const u16) -> bool {
    true
}

#[no_mangle]
unsafe extern "C" fn SA_StopAudio() -> bool {
    true
}

#[no_mangle]
unsafe extern "C" fn SA_IsRunning() -> bool {
    true
}