//! Single-item Shell recycling from a pinned protected stage with durable intent and result records.
pub mod batch;
use super::journal::{Journal, relocation::RelocationId};
use super::{Stamp, VerifiedFile, validate_file};
use crate::{
    Error, Result,
    windows::cleanup_stage::{StageEntry, StageIdentity, StagePin},
};
use ::windows::{
    Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
        UI::Shell::{
            BHID_Transfer, FOLDERID_RecycleBinFolder, IShellItem, ITransferSource, KF_FLAG_DEFAULT,
            SHCreateItemFromParsingName, SHCreateItemFromRelativeName, SHGetKnownFolderItem,
            SIGDN_FILESYSPATH,
        },
    },
    core::PCWSTR,
};
use std::{
    fs::{File, OpenOptions},
    marker::PhantomData,
    os::windows::{
        ffi::OsStringExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    rc::Rc,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE},
    Storage::FileSystem::*,
};

/// Must be created and dropped on the same dedicated STA worker thread.
pub struct ShellApartment(PhantomData<Rc<()>>);

impl ShellApartment {
    pub fn new() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(shell_error)?;
        Ok(Self(PhantomData))
    }

    /// Check Shell binding and the recycling destination before any privileged admission.
    pub fn check_stage(&self, stage: &StagePin) -> Result<()> {
        self.stage_items(stage).map(|_| ())
    }

    fn stage_items(&self, stage: &StagePin) -> Result<(IShellItem, ITransferSource, IShellItem)> {
        let parent = shell_item(&stage.path()?)?;
        check_shell_identity(&parent, stage.identity(), true)?;
        let transfer: ITransferSource = unsafe { parent.BindToHandler(None, &BHID_Transfer) }
            .map_err(|error| shell_context("bind stage transfer handler", error))?;
        let bin: IShellItem =
            unsafe { SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None) }
                .map_err(|error| shell_context("resolve recycling destination", error))?;
        Ok((parent, transfer, bin))
    }
}

impl Drop for ShellApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub struct StagedSource<'stage> {
    guard: File,
    original_path: PathBuf,
    _ancestors: Vec<File>,
    stage: &'stage StagePin,
    entry: StageEntry,
    stamp: Stamp,
    identity: StageIdentity,
    attempted: bool,
}

pub struct RecycledItem<'a, 'stage> {
    _item: IShellItem,
    _apartment: &'a ShellApartment,
    source: &'a mut StagedSource<'stage>,
    locator: PathBuf,
    journal_error: Option<String>,
}

pub struct PreparationError {
    pub source: VerifiedFile,
    pub error: Error,
}

impl VerifiedFile {
    /// Acquire a content guard before releasing the strict source handle. Stage admission must already be verified and logged.
    pub fn prepare_recycling(
        self,
        stage: &StagePin,
        entry: StageEntry,
    ) -> std::result::Result<StagedSource<'_>, Box<PreparationError>> {
        let prepared = (|| {
            let identity = stage.verify_source(&self.file, &entry)?;
            let guard = self.content_guard()?;
            Ok::<_, Error>((identity, guard))
        })();
        let (identity, guard) = match prepared {
            Ok(value) => value,
            Err(error) => {
                return Err(Box::new(PreparationError {
                    source: self,
                    error,
                }));
            }
        };
        let Self {
            file,
            parents,
            stamp,
            path,
        } = self;
        drop(file);
        Ok(StagedSource {
            guard,
            original_path: path,
            _ancestors: parents,
            stage,
            entry,
            stamp,
            identity,
            attempted: false,
        })
    }

    fn check_recycling_access(&self) -> Result<()> {
        self.check_stamp()?;
        self.content_guard().map(drop)
    }

    fn content_guard(&self) -> Result<File> {
        check_content(&self.file, &self.stamp)?;
        let identity = file_identity(&self.file)?;
        let handle = unsafe {
            ReOpenFile(
                self.file.as_raw_handle(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_DELETE,
                FILE_FLAG_OPEN_REPARSE_POINT,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        let guard = unsafe { File::from_raw_handle(handle) };
        if file_identity(&guard)? != identity {
            return Err(Error::Invalid(
                "Content guard does not match the staged source.".into(),
            ));
        }
        check_content(&guard, &self.stamp)?;
        Ok(guard)
    }
}

impl<'stage> StagedSource<'stage> {
    /// Restore strict authority only before a native recycling attempt, without releasing the read guard first.
    fn into_verified(self) -> Result<VerifiedFile> {
        if self.attempted {
            return Err(Error::Invalid(
                "Recycling outcome requires reconciliation.".into(),
            ));
        }
        self.stage.verify_source(&self.guard, &self.entry)?;
        check_content(&self.guard, &self.stamp)?;
        let handle = unsafe {
            ReOpenFile(
                self.guard.as_raw_handle(),
                DELETE | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ,
                FILE_FLAG_OPEN_REPARSE_POINT,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        let file = unsafe { File::from_raw_handle(handle) };
        if file_identity(&file)? != self.identity {
            return Err(Error::Invalid("Rollback guard identity changed.".into()));
        }
        check_content(&file, &self.stamp)?;
        drop(self.guard);
        Ok(VerifiedFile {
            file,
            path: self.original_path,
            parents: self._ancestors,
            stamp: self.stamp,
        })
    }

    /// Commits bound intent before Shell. A returned item with a journal error is a partial result; stop the batch.
    pub fn recycle<'a>(
        &'a mut self,
        apartment: &'a ShellApartment,
        journal: &mut Journal,
        relocation: RelocationId,
    ) -> Result<RecycledItem<'a, 'stage>> {
        if self.attempted {
            return Err(Error::Invalid(
                "Recycling was already attempted; reconcile its result.".into(),
            ));
        }
        self.stage.verify_source(&self.guard, &self.entry)?;
        check_content(&self.guard, &self.stamp)?;
        let (parent, transfer, bin) = apartment.stage_items(self.stage)?;
        let entry: Vec<u16> = self.entry.as_str().encode_utf16().chain(Some(0)).collect();
        let source: IShellItem =
            unsafe { SHCreateItemFromRelativeName(&parent, PCWSTR(entry.as_ptr()), None) }
                .map_err(shell_error)?;
        check_shell_identity(&source, self.identity, false)?;
        journal.start_recycling(relocation, self.identity, self.stage.identity())?;
        self.attempted = true;
        let item = unsafe { transfer.RecycleItem(&source, &bin, 0) }.map_err(shell_error)?;
        let locator = filesystem_path(&item)?;
        let in_bin = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL)
            .open(&locator)?;
        if file_identity(&in_bin)? != self.identity || file_identity(&self.guard)? != self.identity
        {
            return Err(Error::Invalid(
                "Recycled item identity is unresolved.".into(),
            ));
        }
        check_content(&self.guard, &self.stamp)?;
        let journal_error = journal
            .record_recycled(relocation, self.identity, &locator)
            .err()
            .map(|error| error.to_string());
        Ok(RecycledItem {
            _item: item,
            _apartment: apartment,
            source: self,
            locator,
            journal_error,
        })
    }
}

impl RecycledItem<'_, '_> {
    pub fn locator(&self) -> &Path {
        &self.locator
    }
    pub fn identity(&self) -> StageIdentity {
        self.source.identity
    }
    pub fn recorded(&self) -> bool {
        self.journal_error.is_none()
    }
    pub fn journal_error(&self) -> Option<&str> {
        self.journal_error.as_deref()
    }
}

fn check_content(file: &File, stamp: &Stamp) -> Result<()> {
    let metadata = file.metadata()?;
    validate_file(&metadata)?;
    if crate::windows::file_identity(file)?.key != stamp.identity
        || metadata.len() != stamp.size
        || metadata.modified()? != stamp.modified
    {
        return Err(Error::Invalid(
            "Staged content changed since review.".into(),
        ));
    }
    Ok(())
}

fn file_identity(file: &File) -> std::io::Result<StageIdentity> {
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
        return Err(std::io::Error::last_os_error());
    }
    Ok(StageIdentity {
        volume: id.VolumeSerialNumber,
        file: id.FileId.Identifier,
    })
}

fn shell_item(path: &Path) -> Result<IShellItem> {
    let wide = crate::windows::wide(path);
    unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }
        .map_err(|error| shell_context("parse staged Shell path", error))
}

fn check_shell_identity(item: &IShellItem, expected: StageIdentity, directory: bool) -> Result<()> {
    let path = filesystem_path(item)?;
    let metadata = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT
                | FILE_FLAG_OPEN_NO_RECALL
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        )
        .open(path)?;
    if file_identity(&metadata)? != expected {
        return Err(Error::Invalid(
            "Shell resolved a different filesystem object.".into(),
        ));
    }
    Ok(())
}

fn filesystem_path(item: &IShellItem) -> Result<PathBuf> {
    let text = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.map_err(shell_error)?;
    let path = unsafe { std::ffi::OsString::from_wide(text.as_wide()) };
    unsafe { CoTaskMemFree(Some(text.0.cast())) };
    Ok(path.into())
}

fn shell_error(error: ::windows::core::Error) -> Error {
    std::io::Error::other(format!("Windows recycling: {error}")).into()
}

fn shell_context(action: &str, error: ::windows::core::Error) -> Error {
    std::io::Error::other(format!("Windows recycling could not {action}: {error}")).into()
}
