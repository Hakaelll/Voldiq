use super::{Checkpoint, Journal, ReadState, Records, parse_buffer, read};
use crate::{Error, Result, windows};
use std::{
    ffi::OsString,
    fs::File,
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{IO::DeviceIoControl, Ioctl::*},
};

pub struct VolumeReader {
    handle: File,
    pub root: PathBuf,
    pub serial: u64,
    max_name_units: u32,
}

impl VolumeReader {
    pub fn open(path: &Path) -> Result<Self> {
        let mut root = [0_u16; 32768];
        let path = windows::wide(path);
        if unsafe { GetVolumePathNameW(path.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut filesystem = [0_u16; 32];
        let (mut serial, mut max_name_units, mut flags) = (0, 0, 0);
        if unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                std::ptr::null_mut(),
                0,
                &mut serial,
                &mut max_name_units,
                &mut flags,
                filesystem.as_mut_ptr(),
                filesystem.len() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let name = String::from_utf16_lossy(
            &filesystem[..filesystem
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(filesystem.len())],
        );
        if name != "NTFS" || max_name_units == 0 {
            return Err(Error::Invalid(
                "NTFS enumeration is unavailable on this volume.".into(),
            ));
        }
        let mut device = [0_u16; 128];
        if unsafe {
            GetVolumeNameForVolumeMountPointW(
                root.as_ptr(),
                device.as_mut_ptr(),
                device.len() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let length = device
            .iter()
            .position(|c| *c == 0)
            .ok_or_else(|| Error::Invalid("Invalid NTFS volume name.".into()))?;
        if length < 2 || device[length - 1] != u16::from(b'\\') {
            return Err(Error::Invalid("Invalid NTFS volume name.".into()));
        }
        device[length - 1] = 0;
        let handle = unsafe {
            CreateFileW(
                device.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        let handle = unsafe { File::from_raw_handle(handle) };
        let root_length = root
            .iter()
            .position(|c| *c == 0)
            .ok_or_else(|| Error::Invalid("Invalid NTFS mount path.".into()))?;
        Ok(Self {
            handle,
            root: PathBuf::from(OsString::from_wide(&root[..root_length])),
            serial: u64::from(serial),
            max_name_units,
        })
    }

    fn control(&self, code: u32, input: &[u8], output: &mut [u8]) -> Result<usize> {
        let mut returned = 0;
        let input_size = u32::try_from(input.len())
            .map_err(|_| Error::Invalid("NTFS request is too large.".into()))?;
        let output_size = u32::try_from(output.len())
            .map_err(|_| Error::Invalid("NTFS output buffer is too large.".into()))?;
        let ok = unsafe {
            DeviceIoControl(
                self.handle.as_raw_handle(),
                code,
                if input.is_empty() {
                    std::ptr::null()
                } else {
                    input.as_ptr().cast()
                },
                input_size,
                output.as_mut_ptr().cast(),
                output_size,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let returned = returned as usize;
        if returned > output.len() {
            return Err(Error::Invalid("Invalid NTFS output length.".into()));
        }
        Ok(returned)
    }

    pub fn journal(&self) -> Result<Journal> {
        let mut buffer = [0_u8; 80];
        let returned = self.control(FSCTL_QUERY_USN_JOURNAL, &[], &mut buffer)?;
        let buffer = &buffer[..returned];
        if returned < 56 {
            return Err(Error::Invalid("Truncated NTFS journal information.".into()));
        }
        let journal = Journal {
            id: u64::from_le_bytes(read(buffer, 0)?),
            first: i64::from_le_bytes(read(buffer, 8)?),
            next: i64::from_le_bytes(read(buffer, 16)?),
            lowest: i64::from_le_bytes(read(buffer, 24)?),
        };
        if journal.first < 0
            || journal.lowest < 0
            || journal.next < journal.first.max(journal.lowest)
        {
            return Err(Error::Invalid("Invalid NTFS journal bounds.".into()));
        }
        Ok(journal)
    }

    pub fn enumerate(&self, limit: usize, cancel: &AtomicBool) -> Result<Records> {
        let journal = self.journal()?;
        let mut input = [0_u8; 24];
        input[16..24].copy_from_slice(&i64::MAX.to_le_bytes());
        let mut buffer = vec![0; 256 * 1024];
        let mut cursor = 0;
        let mut items = Vec::new();
        let mut complete = false;
        while items.len() < limit && !cancel.load(Ordering::Relaxed) {
            input[..8].copy_from_slice(&u64::to_le_bytes(cursor));
            let returned = match self.control(FSCTL_ENUM_USN_DATA, &input, &mut buffer) {
                Ok(returned) => returned,
                Err(Error::Io(error)) if error.raw_os_error() == Some(ERROR_HANDLE_EOF as i32) => {
                    complete = true;
                    break;
                }
                Err(error) => return Err(error),
            };
            let next = parse_buffer(&buffer[..returned], self.max_name_units, |record| {
                if items.len() < limit {
                    items.push(record);
                }
                Ok(())
            })?;
            if next <= cursor {
                return Err(Error::Invalid(
                    "NTFS enumeration cursor did not advance.".into(),
                ));
            }
            cursor = next;
        }
        let checkpoint = Checkpoint {
            volume: self.serial,
            journal: journal.id,
            next: journal.next,
        };
        if complete && !self.journal()?.can_resume(self.serial, checkpoint) {
            return Err(Error::Invalid(
                "NTFS journal changed during enumeration; rescan required.".into(),
            ));
        }
        Ok(Records {
            items,
            state: if cancel.load(Ordering::Relaxed) {
                ReadState::Cancelled
            } else if complete {
                ReadState::Complete(checkpoint)
            } else {
                ReadState::Limited
            },
        })
    }

    pub fn file(&self, reference: u64) -> Result<File> {
        let descriptor = FILE_ID_DESCRIPTOR {
            dwSize: std::mem::size_of::<FILE_ID_DESCRIPTOR>() as u32,
            Type: FileIdType,
            Anonymous: FILE_ID_DESCRIPTOR_0 {
                FileId: i64::from_le_bytes(reference.to_le_bytes()),
            },
        };
        let handle = unsafe {
            OpenFileById(
                self.handle.as_raw_handle(),
                &descriptor,
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    pub fn changes(
        &self,
        checkpoint: Checkpoint,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Records> {
        let journal = self.journal()?;
        if !journal.can_resume(self.serial, checkpoint) {
            return Err(Error::Invalid(
                "NTFS journal continuity was lost; rescan required.".into(),
            ));
        }
        let mut input = [0_u8; 40];
        input[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        input[32..40].copy_from_slice(&journal.id.to_le_bytes());
        let mut buffer = vec![0; 256 * 1024];
        let mut cursor = checkpoint.next;
        let mut items = Vec::new();
        let mut overflow = false;
        while cursor < journal.next && items.len() < limit && !cancel.load(Ordering::Relaxed) {
            input[..8].copy_from_slice(&cursor.to_le_bytes());
            let returned = self.control(FSCTL_READ_USN_JOURNAL, &input, &mut buffer)?;
            let mut last_usn = cursor - 1;
            let next = parse_buffer(&buffer[..returned], self.max_name_units, |record| {
                if record.usn < cursor || record.usn <= last_usn {
                    return Err(Error::Invalid(
                        "Invalid NTFS change record sequence.".into(),
                    ));
                }
                last_usn = record.usn;
                if record.usn < journal.next {
                    if items.len() < limit {
                        items.push(record);
                    } else {
                        overflow = true;
                    }
                }
                Ok(())
            })?;
            let next = i64::from_le_bytes(next.to_le_bytes());
            if next <= cursor || next <= last_usn {
                return Err(Error::Invalid(
                    "NTFS journal cursor did not advance.".into(),
                ));
            }
            cursor = next;
        }
        if !self.journal()?.can_resume(self.serial, checkpoint) {
            return Err(Error::Invalid(
                "NTFS journal changed during reading; rescan required.".into(),
            ));
        }
        let checkpoint = Checkpoint {
            next: journal.next,
            ..checkpoint
        };
        Ok(Records {
            items,
            state: if cancel.load(Ordering::Relaxed) {
                ReadState::Cancelled
            } else if cursor >= journal.next && !overflow {
                ReadState::Complete(checkpoint)
            } else {
                ReadState::Limited
            },
        })
    }
}
