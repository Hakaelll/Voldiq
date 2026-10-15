use crate::{Error, Result};
use std::{
    ffi::OsStr,
    fs::File,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path},
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_OPEN, FILE_OPEN_NO_RECALL, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
            NtCreateFile,
        },
    },
    Win32::{
        Foundation::{OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING},
        Storage::FileSystem::{
            DELETE, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ,
            FILE_SHARE_WRITE, SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

pub(crate) fn open_root(path: &Path) -> Result<File> {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Prefix(_)))
        || !matches!(components.next(), Some(Component::RootDir))
        || components.next().is_some()
    {
        return Err(Error::Invalid(
            "Cleanup requires an absolute filesystem root.".into(),
        ));
    }
    native_open(
        &mut native_root_name(path),
        None,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )
}

pub(crate) fn open_child(parent: &File, name: &OsStr, directory: bool) -> Result<File> {
    let (access, share) = if directory {
        (
            FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
    } else {
        (FILE_READ_ATTRIBUTES | DELETE, FILE_SHARE_READ)
    };
    open_component(parent, name, access, share)
}

pub(crate) enum LeafAccess {
    Remove,
    Retain,
    CompareAndRemove,
}

pub(crate) fn open_leaf(parent: &File, name: &OsStr, access: LeafAccess) -> Result<File> {
    let access = match access {
        LeafAccess::Remove => FILE_READ_ATTRIBUTES | DELETE,
        LeafAccess::Retain => FILE_READ_ATTRIBUTES | FILE_READ_DATA,
        LeafAccess::CompareAndRemove => FILE_READ_ATTRIBUTES | FILE_READ_DATA | DELETE,
    };
    open_component(parent, name, access, FILE_SHARE_READ)
}

fn open_component(parent: &File, name: &OsStr, access: u32, share: u32) -> Result<File> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(Error::Invalid(
            "Cleanup requires a single child name.".into(),
        ));
    }
    let mut name: Vec<u16> = name.encode_wide().collect();
    if name.iter().any(|&unit| matches!(unit, 0 | 47 | 58 | 92)) {
        return Err(Error::Invalid(
            "Cleanup child name contains a namespace separator.".into(),
        ));
    }
    native_open(&mut name, Some(parent), access, share)
}

fn native_root_name(path: &Path) -> Vec<u16> {
    let name: Vec<u16> = path.as_os_str().encode_wide().collect();
    let extended: Vec<u16> = r"\\?\".encode_utf16().collect();
    let network: Vec<u16> = r"\\".encode_utf16().collect();
    let (prefix, suffix) = if name.starts_with(&extended) {
        (r"\??\", &name[4..])
    } else if name.starts_with(&network) {
        (r"\??\UNC\", &name[2..])
    } else {
        (r"\??\", name.as_slice())
    };
    prefix
        .encode_utf16()
        .chain(suffix.iter().copied())
        .collect()
}

fn native_open(name: &mut [u16], parent: Option<&File>, access: u32, share: u32) -> Result<File> {
    let length = name
        .len()
        .checked_mul(2)
        .and_then(|n| u16::try_from(n).ok())
        .ok_or_else(|| Error::Invalid("Cleanup name is too long.".into()))?;
    let text = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: name.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.map_or(std::ptr::null_mut(), File::as_raw_handle),
        ObjectName: &text,
        Attributes: if parent.is_none() {
            OBJ_CASE_INSENSITIVE
        } else {
            0
        },
        ..Default::default()
    };
    let mut handle = std::ptr::null_mut();
    let mut status_block = IO_STATUS_BLOCK::default();
    // Name buffers and the parent handle remain alive throughout this synchronous, open-only call.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access | SYNCHRONIZE,
            &attributes,
            &mut status_block,
            std::ptr::null(),
            0,
            share,
            FILE_OPEN,
            FILE_OPEN_REPARSE_POINT | FILE_OPEN_NO_RECALL | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 {
        return Err(std::io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32
        )
        .into());
    }
    // NtCreateFile transferred ownership of a successful file handle; File closes it on every exit.
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(test)]
#[path = "cleanup_io_tests.rs"]
mod tests;
