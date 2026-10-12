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
            DELETE, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
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
    let (access, share) = if directory {
        (FILE_READ_ATTRIBUTES, FILE_SHARE_READ | FILE_SHARE_WRITE)
    } else {
        (FILE_READ_ATTRIBUTES | DELETE, FILE_SHARE_READ)
    };
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
mod tests {
    use super::*;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;

    #[test]
    fn child_open_keeps_directory_authority_after_name_replacement() {
        let fixture = tempfile::tempdir().unwrap();
        let held = fixture.path().join("held");
        let retained = fixture.path().join("retained");
        std::fs::create_dir(&held).unwrap();
        let original = held.join("file.bin");
        std::fs::write(&original, b"original").unwrap();
        let expected = crate::windows::identity(&original).unwrap().key;
        let canonical = std::fs::canonicalize(&held).unwrap();
        let mut native = native_root_name(&canonical);
        // Allow namespace substitution in this fixture; production directory handles deny delete sharing.
        let parent = native_open(
            &mut native,
            None,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap();
        std::fs::rename(&held, &retained).unwrap();
        std::fs::create_dir(&held).unwrap();
        std::fs::write(held.join("file.bin"), b"replacement").unwrap();
        let selected = open_child(&parent, OsStr::new("file.bin"), false).unwrap();
        assert_eq!(
            crate::windows::file_identity(&selected).unwrap().key,
            expected
        );
        assert_eq!(selected.metadata().unwrap().len(), 8);
        assert_ne!(
            crate::windows::identity(&held.join("file.bin"))
                .unwrap()
                .key,
            expected
        );
        for invalid in [
            "",
            ".",
            "..",
            "../file.bin",
            "file.bin:stream",
            "C:\\file.bin",
            "file\0.bin",
        ] {
            assert!(open_child(&parent, OsStr::new(invalid), false).is_err());
        }
        assert!(open_child(&parent, OsStr::new("missing.bin"), false).is_err());
        drop(selected);
        drop(parent);
        assert!(!retained.join("missing.bin").exists());
        assert_eq!(
            std::fs::read(retained.join("file.bin")).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read(held.join("file.bin")).unwrap(),
            b"replacement"
        );
    }

    #[test]
    fn root_names_preserve_disk_and_unc_namespaces() {
        for (input, expected) in [
            (r"C:\", r"\??\C:\"),
            (r"\\?\C:\", r"\??\C:\"),
            (r"\\server\share\", r"\??\UNC\server\share\"),
            (r"\\?\UNC\server\share\", r"\??\UNC\server\share\"),
        ] {
            assert_eq!(
                native_root_name(Path::new(input)),
                expected.encode_utf16().collect::<Vec<_>>()
            );
        }
        for invalid in ["", "relative", r"C:\child", r"C:child", r"\child"] {
            assert!(open_root(Path::new(invalid)).is_err());
        }
    }
}
