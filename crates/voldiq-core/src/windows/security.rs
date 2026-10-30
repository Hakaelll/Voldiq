use std::{
    fs::File,
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{Authorization::*, *},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};
use windows_sys::{
    Wdk::{
        Foundation::{NtQueryObject, ObjectBasicInformation},
        Storage::FileSystem::{
            FILE_DELETE_ON_CLOSE, FILE_MODE_INFORMATION, FILE_SYNCHRONOUS_IO_NONALERT,
            FileModeInformation, NtQueryInformationFile,
        },
    },
    Win32::{
        Storage::FileSystem::*,
        System::{IO::IO_STATUS_BLOCK, WindowsProgramming::PUBLIC_OBJECT_BASIC_INFORMATION},
    },
};

pub(crate) fn granted_access(file: &File) -> io::Result<u32> {
    let mut information: PUBLIC_OBJECT_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let mut returned = 0;
    let status = unsafe {
        NtQueryObject(
            file.as_raw_handle(),
            ObjectBasicInformation,
            (&mut information as *mut PUBLIC_OBJECT_BASIC_INFORMATION).cast(),
            size_of::<PUBLIC_OBJECT_BASIC_INFORMATION>() as u32,
            &mut returned,
        )
    };
    if status < 0 || returned < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unable to query native handle access.",
        ));
    }
    Ok(information.GrantedAccess)
}

pub(crate) fn source_access(file: &File) -> io::Result<u32> {
    let granted = granted_access(file)?;
    let required = DELETE | FILE_READ_ATTRIBUTES;
    if granted & required != required || granted & !(required | FILE_READ_DATA | SYNCHRONIZE) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Cleanup source has unexpected access rights.",
        ));
    }
    let mut mode: FILE_MODE_INFORMATION = unsafe { std::mem::zeroed() };
    let mut status_block = IO_STATUS_BLOCK::default();
    let status = unsafe {
        NtQueryInformationFile(
            file.as_raw_handle(),
            &mut status_block,
            (&mut mode as *mut FILE_MODE_INFORMATION).cast(),
            size_of::<FILE_MODE_INFORMATION>() as u32,
            FileModeInformation,
        )
    };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unable to query source I/O mode.",
        ));
    }
    let mut standard: FILE_STANDARD_INFO = unsafe { std::mem::zeroed() };
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if mode.Mode & FILE_SYNCHRONOUS_IO_NONALERT == 0
        || mode.Mode & FILE_DELETE_ON_CLOSE != 0
        || standard.DeletePending
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Cleanup source requires nonalertable I/O without pending deletion.",
        ));
    }
    Ok(granted)
}

struct LocalMemory(*mut std::ffi::c_void);

pub(crate) struct SecurityDescriptor {
    memory: LocalMemory,
}

impl Drop for LocalMemory {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

fn text(pointer: *mut u16) -> io::Result<String> {
    let memory = LocalMemory(pointer.cast());
    let mut length = 0;
    unsafe {
        while *pointer.add(length) != 0 {
            length += 1;
        }
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
        .map_err(|_| io::Error::other("Invalid Windows security text."));
    drop(memory);
    result
}

pub(crate) fn current_user_sid() -> io::Result<String> {
    process_user_sid(unsafe { GetCurrentProcess() })
}

pub(crate) fn process_user_sid(
    process: windows_sys::Win32::Foundation::HANDLE,
) -> io::Result<String> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut bytes = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut bytes,
        );
        if io::Error::last_os_error().raw_os_error()
            != Some(windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER as i32)
            || !(size_of::<TOKEN_USER>() as u32..=65536).contains(&bytes)
        {
            return Err(io::Error::other("Invalid process token information size."));
        }
        let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut sid = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        text(sid)
    }
}

pub(crate) fn process_is_elevated(
    process: windows_sys::Win32::Foundation::HANDLE,
) -> io::Result<bool> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut elevation = TOKEN_ELEVATION::default();
    let mut bytes = 0;
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if bytes as usize != size_of::<TOKEN_ELEVATION>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid token elevation information.",
        ));
    }
    Ok(elevation.TokenIsElevated != 0)
}

impl SecurityDescriptor {
    pub(crate) fn from_sddl(policy: &str) -> io::Result<Self> {
        let policy: Vec<u16> = policy.encode_utf16().chain(Some(0)).collect();
        let mut descriptor = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                policy.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            memory: LocalMemory(descriptor),
        })
    }

    pub(crate) fn as_ptr(&self) -> *mut std::ffi::c_void {
        self.memory.0
    }

    fn canonical(&self) -> io::Result<String> {
        let mut value = ptr::null_mut();
        if unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                self.as_ptr(),
                SDDL_REVISION_1,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut value,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        text(value)
    }

    pub(crate) fn matches_file(&self, file: &File) -> io::Result<bool> {
        let mut descriptor = ptr::null_mut();
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let actual = Self {
            memory: LocalMemory(descriptor),
        };
        Ok(self.canonical()? == actual.canonical()?)
    }
}
