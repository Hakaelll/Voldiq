//! One exclusively created generated file, aborted through its retained handle only.
use crate::{
    Error, Result,
    agents::ordinary_path,
    windows::{self, cleanup_stage, security},
};
use std::{
    fs::File,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    ptr,
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN_NO_RECALL, FILE_OPEN_REPARSE_POINT,
            FILE_RENAME_INFORMATION, FILE_SYNCHRONOUS_IO_NONALERT, FileRenameInformation,
            NtCreateFile, NtSetInformationFile,
        },
    },
    Win32::{
        Foundation::{RtlNtStatusToDosError, UNICODE_STRING},
        Storage::FileSystem::*,
        System::IO::IO_STATUS_BLOCK,
    },
};
pub(crate) struct OwnedFile {
    parent: File,
    parent_identity: cleanup_stage::StageIdentity,
    file: File,
    name: String,
    identity: Option<cleanup_stage::StageIdentity>,
    finished: bool,
    descriptor: security::SecurityDescriptor,
}

impl OwnedFile {
    pub(crate) fn create(parent: &File) -> Result<Self> {
        let metadata = parent.metadata()?;
        if !metadata.is_dir()
            || windows::is_reparse(&metadata)
            || windows::requires_recall(&metadata)
        {
            return Err(invalid());
        }
        let parent_identity = identity(parent)?;
        let parent = parent.try_clone()?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| Error::Invalid("Generated-file randomness unavailable.".into()))?;
        let name = format!(".voldiq-output-{:032x}.tmp", u128::from_le_bytes(random));
        let mut wide: Vec<u16> = name.encode_utf16().collect();
        let text = UNICODE_STRING {
            Length: (wide.len() * 2) as u16,
            MaximumLength: (wide.len() * 2) as u16,
            Buffer: wide.as_mut_ptr(),
        };
        let sid = security::current_user_sid()?;
        let descriptor = security::SecurityDescriptor::from_sddl(&format!(
            "O:{sid}D:P(A;;FA;;;{sid})(A;;FA;;;BA)"
        ))?;
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.as_raw_handle(),
            ObjectName: &text,
            SecurityDescriptor: descriptor.as_ptr().cast_const().cast(),
            ..Default::default()
        };
        let mut handle = ptr::null_mut();
        let mut status_block = IO_STATUS_BLOCK::default();
        // Create-only relative to the retained parent; exclusive sharing prevents substitution.
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                DELETE
                    | READ_CONTROL
                    | FILE_READ_ATTRIBUTES
                    | FILE_READ_DATA
                    | FILE_WRITE_DATA
                    | SYNCHRONIZE,
                &attributes,
                &mut status_block,
                ptr::null(),
                FILE_ATTRIBUTE_NORMAL,
                0,
                FILE_CREATE,
                FILE_OPEN_REPARSE_POINT
                    | FILE_OPEN_NO_RECALL
                    | FILE_NON_DIRECTORY_FILE
                    | FILE_SYNCHRONOUS_IO_NONALERT,
                ptr::null(),
                0,
            )
        };
        if status < 0 {
            return Err(std::io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(status) } as i32,
            )
            .into());
        }
        // Successful synchronous creation transfers this one new file handle to File.
        let mut temporary = Self {
            parent,
            parent_identity,
            file: unsafe { File::from_raw_handle(handle) },
            name,
            identity: None,
            finished: false,
            descriptor,
        };
        let validation = (|| {
            temporary.identity = Some(identity(&temporary.file)?);
            temporary.validate()
        })();
        if let Err(error) = validation {
            if let Err(cleanup) = temporary.abort() {
                return Err(Error::Invalid(format!(
                    "Generated-file validation failed: {error}. Cleanup uncertain for {}: {cleanup}",
                    temporary.name
                )));
            }
            return Err(error);
        }
        Ok(temporary)
    }

    pub(crate) fn file(&self) -> &File {
        &self.file
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn identity_text(&self) -> Result<String> {
        self.validate()?;
        let id = self.identity.ok_or_else(invalid)?;
        Ok(id
            .volume
            .to_le_bytes()
            .iter()
            .chain(id.file.iter())
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    pub(crate) fn is_finished(&self) -> bool {
        self.finished
    }
    pub(crate) fn publish(&mut self, name: &std::ffi::OsStr) -> Result<()> {
        let mut components = std::path::Path::new(name).components();
        if self.finished
            || !matches!(components.next(), Some(std::path::Component::Normal(leaf)) if leaf == name)
            || components.next().is_some()
        {
            return Err(invalid());
        }
        self.validate()?;
        let _permissions = windows::cleanup_io::reopen_directory(
            &self.parent,
            FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | FILE_ADD_FILE | FILE_TRAVERSE,
        )?;
        ordinary_path(&windows::final_path(&self.parent)?.join(name))?;
        publish(
            &self.file,
            &self.parent,
            &name.encode_wide().collect::<Vec<_>>(),
        )?;
        self.finished = true;
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let parent = self.parent.metadata()?;
        let metadata = self.file.metadata()?;
        if !parent.is_dir()
            || windows::is_reparse(&parent)
            || windows::requires_recall(&parent)
            || identity(&self.parent)? != self.parent_identity
            || !metadata.is_file()
            || windows::is_reparse(&metadata)
            || windows::requires_recall(&metadata)
            || Some(identity(&self.file)?) != self.identity
            || !self.descriptor.matches_file(&self.file)?
            || windows::file_identity(&self.file)?.links != 1
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub(crate) fn abort(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        // This handle came from FILE_CREATE and was never shared. No pathname is reopened/deleted.
        if unsafe {
            SetFileInformationByHandle(
                self.file.as_raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        self.finished = true;
        Ok(())
    }
}

impl Drop for OwnedFile {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.abort();
        }
    }
}

fn publish(file: &File, parent: &File, name: &[u16]) -> Result<()> {
    let bytes = size_of_val(name);
    let size = size_of::<FILE_RENAME_INFORMATION>() + bytes;
    let mut buffer = vec![0u64; size.div_ceil(size_of::<u64>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    // Aligned storage, bounded validated single leaf, and a zero ReplaceIfExists union.
    unsafe {
        (*info).RootDirectory = parent.as_raw_handle();
        (*info).FileNameLength = bytes as u32;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            name.len(),
        );
    }
    let mut status_block = IO_STATUS_BLOCK::default();
    let status = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status_block,
            info.cast(),
            size as u32,
            FileRenameInformation,
        )
    };
    if status < 0 {
        return Err(std::io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32
        )
        .into());
    }
    if status != 0 {
        return Err(Error::Invalid(format!(
            "Unexpected generated-file rename status: {status:#x}"
        )));
    }
    Ok(())
}

fn invalid() -> Error {
    Error::Invalid("Generated file or destination is unavailable or changed.".into())
}

fn identity(file: &File) -> Result<cleanup_stage::StageIdentity> {
    let mut id = FILE_ID_INFO::default();
    if unsafe {
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
    Ok(cleanup_stage::StageIdentity {
        volume: id.VolumeSerialNumber,
        file: id.FileId.Identifier,
    })
}
