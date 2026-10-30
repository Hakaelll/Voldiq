use crate::{Error, Result};

#[cfg(windows)]
struct Apartment;

#[cfg(windows)]
impl Apartment {
    fn initialize() -> Result<Self> {
        use windows_sys::Win32::System::Com::*;
        let status = unsafe {
            CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        };
        if status < 0 {
            return Err(Error::Invalid(format!(
                "Windows Shell unavailable ({status:#010x})."
            )));
        }
        Ok(Self)
    }
}

#[cfg(windows)]
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
    }
}

#[cfg(windows)]
pub fn open_storage_settings() -> Result<()> {
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let _apartment = Apartment::initialize()?;
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

#[cfg(windows)]
pub fn open_recycle_bin() -> Result<()> {
    use windows_sys::Win32::UI::{Shell::*, WindowsAndMessaging::SW_SHOWNORMAL};
    let _apartment = Apartment::initialize()?;
    let mut folder = std::ptr::null_mut();
    let status = unsafe {
        SHGetKnownFolderIDList(
            &FOLDERID_RecycleBinFolder,
            0,
            std::ptr::null_mut(),
            &mut folder,
        )
    };
    if status != 0 || folder.is_null() {
        return Err(Error::Invalid(format!(
            "Windows Recycle Bin unavailable ({status:#010x})."
        )));
    }
    let verb = super::wide(std::path::Path::new("open"));
    let mut request = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_IDLIST | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb.as_ptr(),
        lpIDList: folder.cast(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // The SDK-owned folder ID list stays valid for the launch call and is then released.
    let launched = unsafe { ShellExecuteExW(&mut request) };
    let error = (launched == 0).then(std::io::Error::last_os_error);
    unsafe { ILFree(folder) };
    if let Some(error) = error {
        return Err(error.into());
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn open_recycle_bin() -> Result<()> {
    Err(Error::Invalid("Recycle Bin requires Windows.".into()))
}
