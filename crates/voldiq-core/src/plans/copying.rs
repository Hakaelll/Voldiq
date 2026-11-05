//! Retained-handle content copies. Approval, metadata compatibility and original handling are separate.
use super::{Action, ExecutionLease, HumanAuthority, State, Store, TransferUpdate, invalid, now};
use crate::{
    Error, NodeId, Result,
    agents::{AuthorizedPath, PlanContext},
    cleanup::VerifiedFile,
    operations::Lease,
    transfer::{CopiedBytes, Phase, Progress, copy_bytes, metadata::BasicMetadata, verify_bytes},
    windows::{self, cleanup_io, owned_file::OwnedFile},
};
use std::{ffi::OsStr, io::Seek, sync::atomic::AtomicBool};
use windows_sys::Win32::Storage::FileSystem::*;

pub struct TransferWorker<'a> {
    operation: &'a Lease,
    cancel: &'a AtomicBool,
}
impl<'a> TransferWorker<'a> {
    pub fn new(operation: &'a Lease, cancel: &'a AtomicBool) -> Self {
        Self { operation, cancel }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyPublication {
    NotAttempted,
    Uncertain,
    Confirmed,
}

#[derive(Debug, thiserror::Error)]
#[error("{error}")]
/// Failure is not evidence that no partial file exists. Never retry or clean from a pathname.
pub struct CopyFailure {
    #[source]
    error: Error,
    publication: CopyPublication,
    cleanup_error: Option<Box<Error>>,
}
impl CopyFailure {
    fn before(error: Error) -> Self {
        Self {
            error,
            publication: CopyPublication::NotAttempted,
            cleanup_error: None,
        }
    }
    pub fn publication(&self) -> CopyPublication {
        self.publication
    }
    pub fn cleanup_error(&self) -> Option<&Error> {
        self.cleanup_error.as_deref()
    }
}

/// Opaque, nonserializable content evidence for this exact live job. Not full metadata or restore evidence.
pub struct PublishedContent<'a> {
    id: String,
    hash: String,
    nonce: String,
    node: NodeId,
    directory: (u64, u64),
    _operation: &'a Lease,
    _target: AuthorizedPath,
    content: NativePublished,
}
impl PublishedContent<'_> {
    pub fn node(&self) -> NodeId {
        self.node
    }
    pub fn bytes(&self) -> u64 {
        self.content.bytes()
    }
    pub fn blake3(&self) -> String {
        self.content.blake3()
    }
}

pub(super) struct NativePublished {
    source: VerifiedFile,
    destination: OwnedFile,
    copied: CopiedBytes,
    metadata: BasicMetadata,
}
impl NativePublished {
    pub(super) fn bytes(&self) -> u64 {
        self.copied.bytes()
    }
    pub(super) fn blake3(&self) -> String {
        self.copied.digest()
    }
    pub(super) fn check(&self) -> Result<()> {
        self.source.check_stamp()?;
        self.destination.validate()?;
        if BasicMetadata::observe(&self.source.file)? != self.metadata {
            return Err(invalid());
        }
        self.metadata.verify_destination(self.destination.file())?;
        if self.destination.file().metadata()?.len() != self.copied.bytes() {
            return Err(invalid());
        }
        Ok(())
    }
}

pub(super) enum Control {
    Check,
    Record(TransferUpdate),
}

/// Crate-private producer; only the authorized store path below can expose its sealed result.
pub(super) fn copy_verified(
    source: VerifiedFile,
    metadata: BasicMetadata,
    mut destination: OwnedFile,
    name: &OsStr,
    cancel: &AtomicBool,
    mut control: impl FnMut(Control) -> Result<()>,
    mut progress: impl FnMut(Progress),
) -> std::result::Result<NativePublished, CopyFailure> {
    let mut publication = CopyPublication::NotAttempted;
    let result = (|| {
        crate::transfer::check(cancel, &mut || control(Control::Check))?;
        source.check_stamp()?;
        if BasicMetadata::observe(&source.file)? != metadata {
            return Err(invalid());
        }
        destination.validate()?;
        if destination.file().metadata()?.len() != 0 {
            return Err(invalid());
        }
        control(Control::Record(TransferUpdate::Private {
            name: destination.name().into(),
            identity: destination.identity_text()?,
        }))?;
        let expected = source.file.metadata()?.len();
        let copied = {
            let mut reader = &source.file;
            reader.rewind()?;
            let mut writer = destination.file();
            copy_bytes(
                &mut reader,
                &mut writer,
                expected,
                cancel,
                || control(Control::Check),
                &mut progress,
            )?
        };
        destination.file().sync_all()?;
        source.check_stamp()?;
        destination.file().rewind()?;
        verify_bytes(
            &mut destination.file(),
            &copied,
            cancel,
            || control(Control::Check),
            &mut progress,
        )?;
        (&source.file).rewind()?;
        verify_bytes(
            &mut &source.file,
            &copied,
            cancel,
            || control(Control::Check),
            |mut value| {
                value.phase = Phase::SourceRecheck;
                progress(value);
            },
        )
        .map_err(|error| Error::Invalid(format!("Source content recheck failed: {error}")))?;
        source.check_stamp()?;
        destination.validate()?;
        control(Control::Record(TransferUpdate::Verified {
            bytes: copied.bytes(),
            blake3: copied.digest(),
        }))?;
        control(Control::Record(TransferUpdate::PublishIntent))?;
        crate::transfer::check(cancel, &mut || control(Control::Check))?;
        source.check_read_access()?;
        if BasicMetadata::observe(&source.file)? != metadata {
            return Err(invalid());
        }
        publication = CopyPublication::Uncertain;
        destination.publish(name)?;
        publication = CopyPublication::Confirmed;
        crate::transfer::check(cancel, &mut || control(Control::Check))?;
        metadata.apply_and_verify(destination.file())?;
        destination.file().sync_all()?;
        control(Control::Record(TransferUpdate::Published))?;
        Ok(copied)
    })();
    match result {
        Ok(copied) => Ok(NativePublished {
            source,
            destination,
            copied,
            metadata,
        }),
        Err(error) => {
            let cleanup_error = if !destination.is_finished() {
                destination.abort().err().map(Box::new)
            } else {
                None
            };
            Err(CopyFailure {
                error,
                publication,
                cleanup_error,
            })
        }
    }
}

impl Store {
    /// Requires an existing consumed offload grant. This does not approve, finish or modify originals.
    /// Offload admission remains disabled until metadata, recovery and original handling are integrated.
    pub fn copy_transfer_file<'a>(
        &mut self,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        lease: &ExecutionLease,
        node: NodeId,
        worker: TransferWorker<'a>,
        progress: impl FnMut(Progress),
    ) -> std::result::Result<PublishedContent<'a>, CopyFailure> {
        let prepared = (|| {
            self.copy_checkpoint(context, human, lease, &worker)?;
            let stored = self.read(context, &lease.id)?;
            if stored.view.hash != lease.hash || stored.view.state != State::Executing {
                return Err(invalid());
            }
            let Action::Offload(action) = &stored.view.manifest.action else {
                return Err(invalid());
            };
            let entry = stored
                .view
                .manifest
                .entries
                .iter()
                .find(|entry| entry.id == node)
                .ok_or_else(invalid)?;
            let _source_scope = context.path(std::path::Path::new(&entry.path))?;
            let metadata = action.file_metadata(node)?.clone();
            let target = action.copy_destination(
                context,
                self.directory.path(),
                std::path::Path::new(&stored.view.manifest.root),
                entry,
            )?;
            stored.record.validate(lease.directory)?;
            let plan = stored
                .record
                .into_plan()?
                .with_protections(self.directory.path())?;
            let source = plan.copy_source(node)?;
            let handle = windows::open_metadata(target.path())?;
            if windows::file_identity(&handle)?.key != target.identity() {
                return Err(invalid());
            }
            let parent = cleanup_io::reopen_directory(
                &handle,
                FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | FILE_ADD_FILE | FILE_TRAVERSE,
            )?;
            self.copy_checkpoint(context, human, lease, &worker)?;
            let destination = OwnedFile::create(&parent)?;
            let name = action
                .outputs
                .iter()
                .find(|output| output.id == node)
                .ok_or_else(invalid)?
                .name
                .clone();
            Ok((source, metadata, destination, target, name))
        })()
        .map_err(CopyFailure::before)?;
        let (source, metadata, destination, target, name) = prepared;
        let content = copy_verified(
            source,
            metadata,
            destination,
            OsStr::new(&name),
            worker.cancel,
            |control| match control {
                Control::Check => self.copy_checkpoint(context, human, lease, &worker),
                Control::Record(update) => {
                    self.record_transfer(context, human, lease, node, update)
                }
            },
            progress,
        )?;
        Ok(PublishedContent {
            id: lease.id.clone(),
            hash: lease.hash.clone(),
            nonce: lease.nonce.clone(),
            node,
            directory: lease.directory,
            _operation: worker.operation,
            _target: target,
            content,
        })
    }

    /// Rechecks the retained source and destination bytes. This grants no original-handling authority.
    pub fn revalidate_published_content(
        &self,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        lease: &ExecutionLease,
        content: &PublishedContent<'_>,
        worker: TransferWorker<'_>,
        mut progress: impl FnMut(Progress),
    ) -> Result<()> {
        self.copy_checkpoint(context, human, lease, &worker)?;
        if content.id != lease.id
            || content.hash != lease.hash
            || content.nonce != lease.nonce
            || content.directory != lease.directory
        {
            return Err(invalid());
        }
        content.content.check()?;
        content.content.source.check_read_access()?;
        (&content.content.source.file).rewind()?;
        verify_bytes(
            &mut &content.content.source.file,
            &content.content.copied,
            worker.cancel,
            || self.copy_checkpoint(context, human, lease, &worker),
            |mut value| {
                value.phase = Phase::SourceRecheck;
                progress(value);
            },
        )?;
        content.content.destination.file().rewind()?;
        verify_bytes(
            &mut content.content.destination.file(),
            &content.content.copied,
            worker.cancel,
            || self.copy_checkpoint(context, human, lease, &worker),
            &mut progress,
        )?;
        content.content.check()?;
        self.copy_checkpoint(context, human, lease, &worker)
    }

    fn copy_checkpoint(
        &self,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        lease: &ExecutionLease,
        worker: &TransferWorker<'_>,
    ) -> Result<()> {
        crate::transfer::check(worker.cancel, &mut || {
            if worker.operation.directory_identity() != lease.directory
                || !matches!(
                    self.check_execution(context, human, lease, now)?,
                    super::store::ExecutionControl::Proceed
                )
            {
                return Err(invalid());
            }
            Ok(())
        })
    }
}
