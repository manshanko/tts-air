#[no_mangle]
unsafe extern "C" fn SA_SayW(wchar: *const u16) -> u8 {
    crate::send_wchar(wchar);
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