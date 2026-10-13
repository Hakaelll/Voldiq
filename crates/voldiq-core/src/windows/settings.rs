use crate::{Error, Result};

#[cfg(windows)]
pub fn open_storage_settings() -> Result<()> {
    use windows_sys::Win32::{
        System::Com::*,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    let status = unsafe {
        CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };
    if status < 0 {
        return Err(Error::Invalid(format!(
            "Windows settings unavailable ({status:#010x})."
        )));
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let operation = super::wide(std::path::Path::new("open"));
    let uri = super::wide(std::path::Path::new("ms-settings:storagesense"));
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            uri.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result <= 32 {
        return Err(Error::Invalid(format!(
            "Windows storage settings could not open ({result})."
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn open_storage_settings() -> Result<()> {
    Err(Error::Invalid("Storage settings require Windows.".into()))
}
