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
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
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
