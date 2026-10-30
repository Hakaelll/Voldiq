use super::{StageIdentity, StagePin, invalid, local_ntfs, volume_root};
use crate::windows::{cleanup_io, security::source_access};
use std::{
    ffi::OsStr,
    fs::File,
    io,
    mem::{offset_of, size_of},
    os::windows::io::AsRawHandle,
    ptr,
};
use windows_sys::{
    Wdk::Storage::FileSystem::{
        FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
    },
    Win32::{Foundation::*, Storage::FileSystem::*, System::IO::IO_STATUS_BLOCK},
};

pub struct StageEntry(String);

impl StageEntry {
    pub fn parse(value: &str) -> io::Result<Self> {
        if value.is_empty()
            || value.encode_utf16().count() > 255
            || value.ends_with(['.', ' '])
            || value
                .chars()
                .any(|character| character < ' ' || "<>:\"/\\|?*".contains(character))
        {
            return Err(invalid(
                "Filename cannot be represented safely for recycling.",
            ));
        }
        let base = value
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        let port = base
            .strip_prefix("COM")
            .or_else(|| base.strip_prefix("LPT"));
        if matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
        ) || port.is_some_and(|suffix| {
            matches!(
                suffix,
                "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        }) {
            return Err(invalid("Reserved Windows device filename."));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn source_identity(file: &File) -> io::Result<(StageIdentity, u32)> {
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK {
        return Err(invalid("Stage source is not a disk file."));
    }
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
        return Err(io::Error::last_os_error());
    }
    if attributes.FileAttributes
        & (FILE_ATTRIBUTE_DIRECTORY
            | FILE_ATTRIBUTE_REPARSE_POINT
            | FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
    {
        return Err(invalid("Stage source is not a supported regular file."));
    }
    Ok((
        StageIdentity {
            volume: id.VolumeSerialNumber,
            file: id.FileId.Identifier,
        },
        attributes.FileAttributes,
    ))
}

fn contents(directory: &File, expected: Option<&StageEntry>) -> io::Result<()> {
    let mut buffer = vec![0u64; 512];
    if unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle(),
            FileFullDirectoryRestartInfo,
            buffer.as_mut_ptr().cast(),
            4096,
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) && expected.is_none() {
            return Ok(());
        }
        return Err(error);
    }
    let expected: Option<Vec<u16>> = expected.map(|entry| entry.0.encode_utf16().collect());
    let mut offset = 0usize;
    let mut found = 0;
    loop {
        if offset > 4096 - size_of::<FILE_FULL_DIR_INFO>()
            || !offset.is_multiple_of(align_of::<FILE_FULL_DIR_INFO>())
        {
            return Err(invalid("Invalid stage directory information."));
        }
        let pointer = unsafe {
            buffer
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<FILE_FULL_DIR_INFO>()
        };
        let bytes = unsafe { (*pointer).FileNameLength as usize };
        let header = offset_of!(FILE_FULL_DIR_INFO, FileName);
        if bytes == 0 || bytes % 2 != 0 || bytes > 4096 - offset - header {
            return Err(invalid("Invalid stage directory name."));
        }
        let name = unsafe {
            std::slice::from_raw_parts(ptr::addr_of!((*pointer).FileName).cast::<u16>(), bytes / 2)
        };
        if name != [46] && name != [46, 46] {
            if expected.as_deref() != Some(name) || found != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Stage contains unexpected entries.",
                ));
            }
            found += 1;
        }
        let next = unsafe { (*pointer).NextEntryOffset as usize };
        if next == 0 {
            break;
        }
        if next < header + bytes || next > 4096 - offset {
            return Err(invalid("Invalid stage directory offset."));
        }
        offset += next;
    }
    if found != usize::from(expected.is_some()) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Expected stage entry is missing.",
        ));
    }
    Ok(())
}

fn rename(source: &File, directory: &File, name: &StageEntry) -> io::Result<()> {
    if volume_root(source)? != volume_root(directory)? {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Stage moves must stay on the same volume.",
        ));
    }
    let name: Vec<u16> = name.0.encode_utf16().collect();
    let bytes = name.len() * size_of::<u16>();
    let size = size_of::<FILE_RENAME_INFORMATION>() + bytes;
    let mut buffer = vec![0u64; size.div_ceil(size_of::<u64>())];
    let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    unsafe {
        (*information).RootDirectory = directory.as_raw_handle();
        (*information).FileNameLength = bytes as u32;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            ptr::addr_of_mut!((*information).FileName).cast::<u16>(),
            name.len(),
        );
    }
    let mut status_block = IO_STATUS_BLOCK::default();
    // Zeroed SDK union leaves ReplaceIfExists false; no pathname-based or permanent-delete fallback.
    let status = unsafe {
        NtSetInformationFile(
            source.as_raw_handle(),
            &mut status_block,
            information.cast(),
            size as u32,
            FileRenameInformation,
        )
    };
    if status < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    if status != 0 {
        return Err(io::Error::other(format!(
            "Unexpected native rename status: {status:#x}"
        )));
    }
    Ok(())
}

impl StagePin {
    /// Requires committed relocation intent and an already verified strict source handle.
    /// An error is not recovery evidence; the transaction must reconcile before any retry.
    pub fn admit(&mut self, source: &File, name: &StageEntry) -> io::Result<()> {
        source_access(source)?;
        let (identity, attributes) = source_identity(source)?;
        if attributes & FILE_ATTRIBUTE_READONLY != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Read-only sources are not admitted.",
            ));
        }
        if identity.volume != self.identity.volume {
            return Err(invalid("Source and stage volume identities differ."));
        }
        contents(&self._file, None)?;
        rename(source, &self._file, name)
    }

    /// Returns an unrecycled staged source to this stage's captured parent, without replacing a target.
    /// This is an explicit rollback primitive, not Recycle Bin restoration or automatic recovery.
    pub fn return_to_parent(
        &mut self,
        source: &File,
        staged_name: &StageEntry,
        original_name: &StageEntry,
    ) -> io::Result<()> {
        source_access(source)?;
        self.verify_source(source, staged_name)?;
        local_ntfs(&self._parent)?;
        rename(source, &self._parent, original_name)
    }

    pub(crate) fn verify_source(
        &self,
        source: &File,
        staged_name: &StageEntry,
    ) -> io::Result<StageIdentity> {
        let (expected, _) = source_identity(source)?;
        contents(&self._file, Some(staged_name))?;
        let metadata = cleanup_io::open_component(
            &self._file,
            OsStr::new(staged_name.as_str()),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .map_err(|error| match error {
            crate::Error::Io(error) => error,
            error => io::Error::new(io::ErrorKind::InvalidData, error),
        })?;
        if source_identity(&metadata)?.0 != expected {
            return Err(invalid("Staged entry does not match the retained source."));
        }
        drop(metadata);
        Ok(expected)
    }
}
