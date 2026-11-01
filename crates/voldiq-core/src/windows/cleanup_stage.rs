//! Protected stage creation and independently acquired read-only namespace pins.
mod admission;
use super::security::{SecurityDescriptor, current_user_sid};
pub use admission::StageEntry;
pub(crate) use admission::restore_to_parent;
use std::{
    fs::File,
    io,
    os::windows::io::{AsRawHandle, FromRawHandle},
    ptr,
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_NO_RECALL,
            FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{
            Com::CoCreateGuid, IO::IO_STATUS_BLOCK, SystemServices::FILE_PERSISTENT_ACLS,
            WindowsProgramming::DRIVE_FIXED,
        },
    },
};

use crate::cleanup::STAGE_DIRECTORY_PREFIX as PREFIX;

pub struct StageName(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StageIdentity {
    pub volume: u64,
    pub file: [u8; 16],
}

pub struct StagePin {
    _file: File,
    _parent: File,
    identity: StageIdentity,
}

impl StageName {
    pub fn generate() -> io::Result<Self> {
        let mut guid = windows_sys::core::GUID::from_u128(0);
        let status = unsafe { CoCreateGuid(&mut guid) };
        if status < 0 {
            return Err(io::Error::other(format!("Stage GUID failed: {status:#x}")));
        }
        Ok(Self(format!(
            "{PREFIX}{:08x}{:04x}{:04x}{:016x}",
            guid.data1,
            guid.data2,
            guid.data3,
            u64::from_be_bytes(guid.data4)
        )))
    }

    pub fn parse(value: &str) -> io::Result<Self> {
        let suffix = value.strip_prefix(PREFIX).unwrap_or("");
        if suffix.len() != 32 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid("Invalid stage name."));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl StagePin {
    pub(crate) fn path(&self) -> io::Result<std::path::PathBuf> {
        use std::os::windows::ffi::OsStringExt;
        volume_root(&self._file)?;
        let mut path = vec![0u16; 32768];
        let length = unsafe {
            GetFinalPathNameByHandleW(
                self._file.as_raw_handle(),
                path.as_mut_ptr(),
                path.len() as u32,
                VOLUME_NAME_DOS,
            )
        };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if length as usize >= path.len() {
            return Err(invalid("Stage path exceeds the supported length."));
        }
        path.truncate(length as usize);
        let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
        if !path.starts_with(&prefix)
            || path.len() < 7
            || !matches!(path[4], 65..=90 | 97..=122)
            || path[5] != b':' as u16
            || path[6] != b'\\' as u16
        {
            return Err(invalid(
                "Shell recycling requires a global fixed-drive letter.",
            ));
        }
        let drive = [path[4], b':' as u16, 0];
        let global: Vec<u16> = "Global\\".encode_utf16().chain(drive).collect();
        let current = dos_target(&drive)?;
        let global = dos_target(&global)?;
        let native_prefix: Vec<u16> = r"\Device\HarddiskVolume".encode_utf16().collect();
        if current != global
            || !global.starts_with(&native_prefix)
            || global.len() == native_prefix.len()
            || !global[native_prefix.len()..]
                .iter()
                .all(|unit| (48..=57).contains(unit))
        {
            return Err(invalid(
                "Shell recycling rejects local aliases and substituted drives.",
            ));
        }
        let shell_path: std::path::PathBuf = std::ffi::OsString::from_wide(&path[4..]).into();
        let wide = super::wide(&shell_path);
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS
                    | FILE_FLAG_OPEN_REPARSE_POINT
                    | FILE_FLAG_OPEN_NO_RECALL,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let checked = unsafe { File::from_raw_handle(handle) };
        if identity(&checked)? != self.identity {
            return Err(invalid("Shell stage path does not match its retained pin."));
        }
        Ok(shell_path)
    }
}

fn dos_target(name: &[u16]) -> io::Result<Vec<u16>> {
    let mut target = vec![0u16; 32768];
    let length =
        unsafe { QueryDosDeviceW(name.as_ptr(), target.as_mut_ptr(), target.len() as u32) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    let end = target[..length as usize]
        .iter()
        .position(|unit| *unit == 0)
        .ok_or_else(|| invalid("Malformed drive mapping."))?;
    target.truncate(end);
    Ok(target)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn native_error(operation: &'static str) -> io::Error {
    let error = io::Error::last_os_error();
    io::Error::new(error.kind(), format!("{operation}: {error}"))
}

pub(crate) fn identity(file: &File) -> io::Result<StageIdentity> {
    let mut attributes: FILE_ATTRIBUTE_TAG_INFO = unsafe { std::mem::zeroed() };
    let mut id: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut attributes as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
        || unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
    {
        return Err(native_error("Read stage directory identity"));
    }
    if attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || attributes.FileAttributes
            & (FILE_ATTRIBUTE_REPARSE_POINT
                | FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0
    {
        return Err(invalid("Stage requires an ordinary local directory."));
    }
    Ok(StageIdentity {
        volume: id.VolumeSerialNumber,
        file: id.FileId.Identifier,
    })
}

fn local_ntfs(parent: &File) -> io::Result<()> {
    identity(parent)?;
    let mut filesystem = [0u16; 16];
    let mut flags = 0;
    if unsafe {
        GetVolumeInformationByHandleW(
            parent.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut flags,
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } == 0
    {
        return Err(native_error("Query stage volume"));
    }
    let expected: Vec<u16> = "NTFS\0".encode_utf16().collect();
    if !filesystem.starts_with(&expected) || flags & FILE_PERSISTENT_ACLS == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Protected stages require NTFS permissions.",
        ));
    }
    volume_root(parent).map(|_| ())
}

fn volume_root(parent: &File) -> io::Result<Vec<u16>> {
    let mut name = vec![0u16; 32768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            parent.as_raw_handle(),
            name.as_mut_ptr(),
            name.len() as u32,
            VOLUME_NAME_GUID,
        )
    };
    if length == 0 {
        return Err(native_error("Resolve stage volume path"));
    }
    let prefix: Vec<u16> = r"\\?\Volume{".encode_utf16().collect();
    let root_length = prefix.len() + 38;
    if length as usize >= name.len()
        || (length as usize) < root_length
        || !name.starts_with(&prefix)
        || name[root_length - 2] != b'}' as u16
        || name[root_length - 1] != b'\\' as u16
    {
        return Err(invalid("Stage volume has no stable local GUID path."));
    }
    let mut root = name[..root_length].to_vec();
    root.push(0);
    if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Protected stages require a fixed local volume.",
        ));
    }
    Ok(root)
}

fn policy() -> io::Result<SecurityDescriptor> {
    let sid = current_user_sid()?;
    SecurityDescriptor::from_sddl(&format!(
        "O:{sid}D:P(A;;FA;;;BA)(A;;0x001200a9;;;{sid})(A;;0x001200a9;;;OW)"
    ))
}

fn open(
    parent: &File,
    name: &StageName,
    descriptor: Option<&SecurityDescriptor>,
) -> io::Result<File> {
    let mut name: Vec<u16> = name.0.encode_utf16().collect();
    let length = (name.len() * 2) as u16;
    let text = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: name.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &text,
        SecurityDescriptor: descriptor.map_or(ptr::null(), |sd| sd.as_ptr().cast_const().cast()),
        ..Default::default()
    };
    let mut handle = ptr::null_mut();
    let mut status_block = IO_STATUS_BLOCK::default();
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            &attributes,
            &mut status_block,
            ptr::null(),
            FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_HIDDEN,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            if descriptor.is_some() {
                FILE_CREATE
            } else {
                FILE_OPEN
            },
            FILE_OPEN_REPARSE_POINT
                | FILE_SYNCHRONOUS_IO_NONALERT
                | if descriptor.is_some() {
                    FILE_DIRECTORY_FILE
                } else {
                    FILE_OPEN_NO_RECALL
                },
            ptr::null(),
            0,
        )
    };
    if status < 0 {
        let error = io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32);
        return Err(io::Error::new(
            error.kind(),
            format!("Open/create stage: {error} ({status:#x})"),
        ));
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

impl StagePin {
    /// Creates only an empty directory. The caller must record its chosen name before creation.
    /// A failure after native creation can leave that named empty directory for explicit reconciliation.
    pub fn create(parent: &File, name: &StageName) -> io::Result<Self> {
        local_ntfs(parent)?;
        let parent_pin = parent.try_clone()?;
        let descriptor = policy()?;
        let file = open(parent, name, Some(&descriptor))?;
        let identity = identity(&file)?;
        if !descriptor.matches_file(&file)? {
            return Err(invalid("Created stage permissions do not match."));
        }
        Ok(Self {
            _file: file,
            _parent: parent_pin,
            identity,
        })
    }

    pub fn acquire(parent: &File, name: &StageName, expected: StageIdentity) -> io::Result<Self> {
        local_ntfs(parent)?;
        let file = open(parent, name, None)?;
        let actual = identity(&file)?;
        if actual != expected || !policy()?.matches_file(&file)? {
            return Err(invalid("Stage identity or permissions changed."));
        }
        Ok(Self {
            _file: file,
            _parent: parent.try_clone()?,
            identity: actual,
        })
    }

    pub fn identity(&self) -> StageIdentity {
        self.identity
    }

    /// Removes only this verified empty stage. Never follows a replacement or deletes children.
    pub fn remove_empty(self, name: &StageName) -> io::Result<()> {
        let Self {
            _file,
            _parent,
            identity: expected,
        } = self;
        drop(_file);
        let directory = super::cleanup_io::open_component(
            &_parent,
            std::ffi::OsStr::new(name.as_str()),
            DELETE | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
        .map_err(|error| match error {
            crate::Error::Io(error) => error,
            error => io::Error::new(io::ErrorKind::InvalidData, error),
        })?;
        if identity(&directory)? != expected || !policy()?.matches_file(&directory)? {
            return Err(invalid("Cleanup stage identity or permissions changed."));
        }
        admission::contents(&directory, None)?;
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                directory.as_raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
