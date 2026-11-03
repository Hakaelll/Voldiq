use super::{CsvStreamRequest, StreamOutcome, StreamProgress, stream_csv};
use crate::{
    Error, Result,
    agents::{AuthorizedPath, ordinary_path, pin_directory},
    cleanup,
    index::Index,
    metadata::{Protections, Store},
    operations::Lease,
    windows::{self, cleanup_io, cleanup_stage, security},
};
use std::{
    fs::File,
    io::{BufWriter, Write},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
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

/// Human-selected destination only. The caller retains the shared operation lease until return.
/// Existing files are never replaced. Cancellation/error prefixes are never published.
pub fn save_csv(
    index: &Index,
    request: &CsvStreamRequest,
    destination: &Path,
    lease: &Lease,
    cancelled: &AtomicBool,
    progress: impl FnMut(StreamProgress),
) -> Result<StreamOutcome> {
    let destination = ordinary_path(destination)?;
    let leaf = destination.file_name().ok_or_else(invalid)?;
    let parent = pin_directory(destination.parent().ok_or_else(invalid)?)?;
    let mut policy = Store::open(lease.directory())?;
    validate(&parent, &destination, &policy.protections()?)?;
    let handle = windows::open_metadata(parent.path())?;
    if windows::file_identity(&handle)?.key != parent.identity() {
        return Err(invalid());
    }
    let handle = cleanup_io::reopen_directory(
        &handle,
        FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | FILE_ADD_FILE | FILE_TRAVERSE,
    )
    .map_err(|error| Error::Invalid(format!("CSV parent access failed: {error}")))?;
    let mut temporary = Temporary::create(&handle)
        .map_err(|error| Error::Invalid(format!("CSV temporary creation failed: {error}")))?;
    let result: Result<StreamOutcome> = (|| {
        let mut writer = BufWriter::with_capacity(64 * 1024, &mut temporary.file);
        let outcome = stream_csv(index, request, &mut writer, cancelled, progress)?;
        if let StreamOutcome::Complete(state) = outcome {
            writer.flush()?;
            drop(writer);
            temporary
                .file
                .sync_all()
                .map_err(|error| Error::Invalid(format!("CSV sync failed: {error}")))?;
            if cancelled.load(Ordering::Relaxed) {
                return Ok(StreamOutcome::Cancelled(state));
            }
            validate(&parent, &destination, &policy.protections()?)?;
            let _permissions = cleanup_io::reopen_directory(
                &handle,
                FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | FILE_ADD_FILE | FILE_TRAVERSE,
            )?;
            temporary.validate()?;
            if cancelled.load(Ordering::Relaxed) {
                return Ok(StreamOutcome::Cancelled(state));
            }
            publish(
                &temporary.file,
                &handle,
                &leaf.encode_wide().collect::<Vec<_>>(),
            )
            .map_err(|error| Error::Invalid(format!("CSV publication failed: {error}")))?;
            temporary.finished = true;
        }
        Ok(outcome)
    })();
    if !temporary.finished
        && let Err(cleanup) = temporary.abort()
    {
        return Err(Error::Invalid(format!(
            "CSV was not published. Temporary report cleanup is uncertain for {}: {cleanup}. Export result: {}",
            parent.path().join(&temporary.name).display(),
            match result {
                Ok(_) => "cancelled".into(),
                Err(error) => error.to_string(),
            }
        )));
    }
    result
}

fn validate(parent: &AuthorizedPath, destination: &Path, protections: &Protections) -> Result<()> {
    let ancestors = parent.ancestor_identities()?;
    protections.check_evidence(destination, parent.identity(), &ancestors)?;
    let system = cleanup::protected_identities()?;
    if cleanup::record::sensitive(destination)
        || ancestors.iter().any(|identity| system.contains(identity))
    {
        return Err(invalid());
    }
    let current = pin_directory(parent.path())?;
    if current.identity() != parent.identity() || current.ancestor_identities()? != ancestors {
        return Err(invalid());
    }
    Ok(())
}

struct Temporary {
    file: File,
    name: String,
    identity: Option<cleanup_stage::StageIdentity>,
    finished: bool,
    descriptor: security::SecurityDescriptor,
}

impl Temporary {
    fn create(parent: &File) -> Result<Self> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| Error::Invalid("Export randomness unavailable.".into()))?;
        let name = format!(".voldiq-export-{:032x}.tmp", u128::from_le_bytes(random));
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
                DELETE | READ_CONTROL | FILE_READ_ATTRIBUTES | FILE_WRITE_DATA | SYNCHRONIZE,
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
                    "Export temporary validation failed: {error}. Cleanup uncertain for {}: {cleanup}",
                    temporary.name
                )));
            }
            return Err(error);
        }
        Ok(temporary)
    }

    fn validate(&self) -> Result<()> {
        let metadata = self.file.metadata()?;
        if !metadata.is_file()
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

    fn abort(&mut self) -> Result<()> {
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

impl Drop for Temporary {
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
            "Unexpected CSV rename status: {status:#x}"
        )));
    }
    Ok(())
}

fn invalid() -> Error {
    Error::Invalid("CSV destination is protected, unavailable or changed.".into())
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
