//! Explicit NTFS basic metadata profile; observations are not file-operation capabilities.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

// Read-only, hidden, system, archive and not-content-indexed. NORMAL is represented by zero flags.
pub(crate) const DOS_FLAGS: u32 = 0x2027;
pub(crate) const LIMITATIONS: &str = "Copies the default data stream, recorded creation time when known, modification time and basic read-only/hidden/system/archive/indexing flags. Access and change times, permissions/owner/auditing, object IDs, short names and hard-link relationships are not preserved. Named streams, extended attributes, compression, sparse/encrypted/integrity/cloud or other special files are unsupported and rejected. This is not complete metadata or full restoration evidence.";
const UNIX_FILETIME: u128 = 116_444_736_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BasicMetadata {
    native_identity: String,
    created_windows_100ns: Option<String>,
    modified_windows_100ns: String,
    dos_attributes: u32,
}

impl BasicMetadata {
    pub(crate) fn native_identity(&self) -> &str {
        &self.native_identity
    }
    pub fn created_windows_100ns(&self) -> Option<&str> {
        self.created_windows_100ns.as_deref()
    }
    pub fn modified_windows_100ns(&self) -> &str {
        &self.modified_windows_100ns
    }
    pub fn dos_attributes(&self) -> u32 {
        self.dos_attributes
    }

    pub(crate) fn check_shape(&self, modified_unix_ns: &str) -> Result<()> {
        let nanos = modified_unix_ns.parse::<u128>().map_err(|_| invalid())?;
        if !nanos.is_multiple_of(100) {
            return Err(invalid());
        }
        let modified = nanos
            .checked_div(100)
            .and_then(|ticks| ticks.checked_add(UNIX_FILETIME))
            .ok_or_else(invalid)?;
        if self.native_identity.len() != 48
            || !self
                .native_identity
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
            || self.dos_attributes & !DOS_FLAGS != 0
            || self.modified_windows_100ns != modified.to_string()
            || modified > i64::MAX as u128
            || self.created_windows_100ns.as_deref().is_some_and(|time| {
                !time
                    .parse::<i64>()
                    .is_ok_and(|n| n > 0 && n.to_string() == time)
            })
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn invalid() -> Error {
    Error::Invalid("Unsupported, unavailable or changed NTFS metadata profile.".into())
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{fs::File, os::windows::io::AsRawHandle, ptr};
    use windows_sys::Win32::System::SystemServices::{
        FILE_NAMED_STREAMS, FILE_SUPPORTS_EXTENDED_ATTRIBUTES,
    };
    use windows_sys::{
        Wdk::Storage::FileSystem::{
            FILE_EA_INFORMATION, FileEaInformation, NtQueryInformationFile,
        },
        Win32::{
            Foundation::RtlNtStatusToDosError, Storage::FileSystem::*, System::IO::IO_STATUS_BLOCK,
        },
    };

    impl BasicMetadata {
        /// Metadata-only queries; does not read the default stream, any ADS contents or an EA value.
        pub(crate) fn observe(file: &File) -> Result<Self> {
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || crate::windows::is_reparse(&metadata)
                || crate::windows::requires_recall(&metadata)
            {
                return Err(invalid());
            }
            let before = crate::windows::file_identity(file)?;
            let mut filesystem = [0u16; 16];
            let mut flags = 0;
            if unsafe {
                GetVolumeInformationByHandleW(
                    file.as_raw_handle(),
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
                return Err(std::io::Error::last_os_error().into());
            }
            let end = filesystem
                .iter()
                .position(|c| *c == 0)
                .ok_or_else(invalid)?;
            if String::from_utf16(&filesystem[..end]).map_err(|_| invalid())? != "NTFS"
                || flags & (FILE_NAMED_STREAMS | FILE_SUPPORTS_EXTENDED_ATTRIBUTES)
                    != (FILE_NAMED_STREAMS | FILE_SUPPORTS_EXTENDED_ATTRIBUTES)
            {
                return Err(invalid());
            }
            let mut basic = FILE_BASIC_INFO::default();
            let mut id = FILE_ID_INFO::default();
            if unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileBasicInfo,
                    (&mut basic as *mut FILE_BASIC_INFO).cast(),
                    size_of::<FILE_BASIC_INFO>() as u32,
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
                return Err(std::io::Error::last_os_error().into());
            }
            if basic.FileAttributes & !(DOS_FLAGS | FILE_ATTRIBUTE_NORMAL) != 0
                || basic.LastWriteTime <= 0
            {
                return Err(Error::Invalid("Special file attributes or an unavailable modification time are unsupported by the basic metadata profile.".into()));
            }
            let mut streams = [0u64; 512]; // Aligned, fixed 4 KiB metadata bound; never an unbounded retry.
            if unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileStreamInfo,
                    streams.as_mut_ptr().cast(),
                    size_of_val(&streams) as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            // Only one default stream is supported; never silently drop other streams or names.
            let stream = unsafe { &*streams.as_ptr().cast::<FILE_STREAM_INFO>() };
            if stream.NextEntryOffset != 0
                || stream.StreamNameLength != 14
                || stream.StreamSize < 0
                || stream.StreamSize as u64 != metadata.len()
            {
                return Err(Error::Invalid("Additional or unavailable data streams are unsupported by the basic metadata profile.".into()));
            }
            let name = unsafe {
                std::slice::from_raw_parts(ptr::addr_of!(stream.StreamName).cast::<u16>(), 7)
            };
            if name != [58, 58, 36, 68, 65, 84, 65] {
                return Err(Error::Invalid(
                    "Named data streams are unsupported by the basic metadata profile.".into(),
                ));
            }
            let mut ea = FILE_EA_INFORMATION::default();
            let mut status = IO_STATUS_BLOCK::default();
            let result = unsafe {
                NtQueryInformationFile(
                    file.as_raw_handle(),
                    &mut status,
                    (&mut ea as *mut FILE_EA_INFORMATION).cast(),
                    size_of::<FILE_EA_INFORMATION>() as u32,
                    FileEaInformation,
                )
            };
            if result != 0 {
                return Err(std::io::Error::from_raw_os_error(unsafe {
                    RtlNtStatusToDosError(result)
                } as i32)
                .into());
            }
            if status.Information < size_of::<FILE_EA_INFORMATION>() || ea.EaSize != 0 {
                return Err(Error::Invalid("Extended attributes are unavailable or unsupported by the basic metadata profile.".into()));
            }
            let after = crate::windows::file_identity(file)?;
            if before.key != after.key
                || before.change_time != after.change_time
                || before.links != after.links
                || file.metadata()?.len() != metadata.len()
            {
                return Err(invalid());
            }
            Ok(Self {
                native_identity: id
                    .VolumeSerialNumber
                    .to_le_bytes()
                    .iter()
                    .chain(id.FileId.Identifier.iter())
                    .map(|b| format!("{b:02x}"))
                    .collect(),
                created_windows_100ns: (basic.CreationTime > 0)
                    .then(|| basic.CreationTime.to_string()),
                modified_windows_100ns: basic.LastWriteTime.to_string(),
                dos_attributes: basic.FileAttributes & DOS_FLAGS,
            })
        }

        pub(crate) fn apply_and_verify(&self, destination: &File) -> Result<()> {
            let modified = self
                .modified_windows_100ns
                .parse::<i64>()
                .map_err(|_| invalid())?;
            let created = self
                .created_windows_100ns
                .as_deref()
                .map(str::parse::<i64>)
                .transpose()
                .map_err(|_| invalid())?;
            let basic = FILE_BASIC_INFO {
                CreationTime: created.unwrap_or(0),
                LastWriteTime: modified,
                FileAttributes: if self.dos_attributes == 0 {
                    FILE_ATTRIBUTE_NORMAL
                } else {
                    self.dos_attributes
                },
                ..Default::default()
            };
            if unsafe {
                SetFileInformationByHandle(
                    destination.as_raw_handle(),
                    FileBasicInfo,
                    (&basic as *const FILE_BASIC_INFO).cast(),
                    size_of::<FILE_BASIC_INFO>() as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            destination.sync_all()?;
            self.verify_destination(destination)
        }

        pub(crate) fn verify_destination(&self, destination: &File) -> Result<()> {
            let actual = Self::observe(destination)?;
            if actual.dos_attributes != self.dos_attributes
                || actual.modified_windows_100ns != self.modified_windows_100ns
                || (self.created_windows_100ns.is_some()
                    && actual.created_windows_100ns != self.created_windows_100ns)
            {
                return Err(invalid());
            }
            Ok(())
        }
    }
}
