//! Local, bounded, read-only registry metadata.

use crate::{Error, Result};
use std::ptr::{null, null_mut};
use windows_sys::Win32::{Foundation::*, System::Registry::*};

pub(crate) struct Key(pub(crate) HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    pub(crate) fn open(parent: HKEY, name: &str, access: u32) -> std::result::Result<Self, u32> {
        let name: Vec<_> = name.encode_utf16().chain([0]).collect();
        let mut key = null_mut();
        // This opens the local key without write access or registry link resolution.
        let code = unsafe {
            RegOpenKeyExW(
                parent,
                name.as_ptr(),
                REG_OPTION_OPEN_LINK,
                access,
                &mut key,
            )
        };
        if code == ERROR_SUCCESS {
            Ok(Self(key))
        } else {
            Err(code)
        }
    }

    pub(crate) fn revision(&self) -> Result<(u32, u32)> {
        let mut time = FILETIME::default();
        let code = unsafe {
            RegQueryInfoKeyW(
                self.0,
                null_mut(),
                null_mut(),
                null(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut time,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(Error::Invalid("Registration revision unavailable.".into()));
        }
        Ok((time.dwHighDateTime, time.dwLowDateTime))
    }

    pub(crate) fn string(&self, name: &str, max_bytes: usize) -> Result<Option<String>> {
        let mut buffer = [0u16; 4096];
        let capacity = std::mem::size_of_val(&buffer) as u32;
        let Some(bytes) = self.value(name, RRF_RT_REG_SZ, buffer.as_mut_ptr().cast(), capacity)?
        else {
            return Ok(None);
        };
        if !bytes.is_multiple_of(2) {
            return Err(invalid());
        }
        let units = buffer[..bytes as usize / 2]
            .strip_suffix(&[0])
            .ok_or_else(invalid)?;
        let text = String::from_utf16(units).map_err(|_| invalid())?;
        if text.len() > max_bytes || text.chars().any(char::is_control) {
            return Err(invalid());
        }
        Ok((!text.trim().is_empty()).then_some(text))
    }

    pub(crate) fn dword(&self, name: &str) -> Result<Option<u32>> {
        let mut value = 0u32;
        match self.value(name, RRF_RT_REG_DWORD, (&mut value as *mut u32).cast(), 4)? {
            None => Ok(None),
            Some(4) => Ok(Some(value)),
            Some(_) => Err(invalid()),
        }
    }

    fn value(
        &self,
        name: &str,
        kind: u32,
        buffer: *mut std::ffi::c_void,
        capacity: u32,
    ) -> Result<Option<u32>> {
        let name: Vec<_> = name.encode_utf16().chain([0]).collect();
        let mut bytes = capacity;
        // The caller supplies a fixed output buffer. Value types are checked and never expanded.
        let code = unsafe {
            RegGetValueW(
                self.0,
                null(),
                name.as_ptr(),
                kind | RRF_NOEXPAND,
                null_mut(),
                buffer,
                &mut bytes,
            )
        };
        match code {
            ERROR_FILE_NOT_FOUND => Ok(None),
            ERROR_SUCCESS if bytes <= capacity => Ok(Some(bytes)),
            _ => Err(invalid()),
        }
    }
}

fn invalid() -> Error {
    Error::Invalid("Registration value is unavailable or unsupported.".into())
}
