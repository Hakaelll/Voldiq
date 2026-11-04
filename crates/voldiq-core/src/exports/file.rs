use super::{
    ReportFormat, ReportSelection, StreamOutcome, StreamProgress, stream_csv, stream_pdf,
    stream_xlsx,
};
use crate::{
    Error, Result,
    agents::{AuthorizedPath, ordinary_path, pin_directory},
    cleanup,
    index::Index,
    metadata::{Protections, Store},
    operations::Lease,
    windows::{self, cleanup_io, owned_file::OwnedFile},
};
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::Storage::FileSystem::*;

/// Human-selected destination only. The caller retains the shared operation lease until return.
/// Existing files are never replaced. Cancellation/error prefixes are never published.
pub fn save_report(
    index: &Index,
    format: ReportFormat,
    request: &ReportSelection,
    destination: &Path,
    lease: &Lease,
    cancelled: &AtomicBool,
    progress: impl FnMut(StreamProgress),
) -> Result<StreamOutcome> {
    save_generated(
        format.extension(),
        destination,
        lease,
        cancelled,
        |writer| match format {
            ReportFormat::Csv => stream_csv(index, request, writer, cancelled, progress),
            ReportFormat::Xlsx => stream_xlsx(index, request, writer, cancelled, progress),
            ReportFormat::Pdf => stream_pdf(index, request, writer, cancelled, progress),
        },
    )
}

pub fn save_svg(
    index: &Index,
    request: &super::SvgRequest,
    destination: &Path,
    lease: &Lease,
    cancelled: &AtomicBool,
    progress: impl FnMut(StreamProgress),
) -> Result<StreamOutcome> {
    save_generated("svg", destination, lease, cancelled, |writer| {
        super::stream_svg(index, request, writer, cancelled, progress)
    })
}

pub fn save_png(
    index: &Index,
    request: &super::PngRequest,
    destination: &Path,
    lease: &Lease,
    cancelled: &AtomicBool,
    progress: impl FnMut(StreamProgress),
) -> Result<StreamOutcome> {
    save_generated("png", destination, lease, cancelled, |writer| {
        super::stream_png(index, request, writer, cancelled, progress)
    })
}

pub fn save_history_gif(
    store: &crate::snapshots::Store,
    request: &super::HistoryGifRequest,
    destination: &Path,
    lease: &Lease,
    cancelled: &AtomicBool,
    progress: impl FnMut(super::GifProgress),
    frames: impl FnMut() -> Result<Option<Vec<u8>>>,
) -> Result<StreamOutcome<super::GifProgress>> {
    save_generated("gif", destination, lease, cancelled, |writer| {
        super::stream_history_gif(store, request, writer, cancelled, progress, frames)
    })
}

pub fn save_history_webm(
    store: &crate::snapshots::Store,
    request: &super::HistoryWebmRequest,
    image: &[u8],
    destination: &Path,
    lease: &Lease,
    cancelled: &AtomicBool,
    progress: impl FnMut(super::WebmProgress),
) -> Result<StreamOutcome<super::WebmProgress>> {
    save_generated("webm", destination, lease, cancelled, |writer| {
        super::stream_history_webm(store, request, image, writer, cancelled, progress)
    })
}

fn save_generated<Progress: Copy>(
    extension: &'static str,
    destination: &Path,
    lease: &Lease,
    cancelled: &AtomicBool,
    encode: impl FnOnce(&mut BufWriter<&File>) -> Result<StreamOutcome<Progress>>,
) -> Result<StreamOutcome<Progress>> {
    let destination = ordinary_path(destination)?;
    if !destination
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
    {
        return Err(Error::Invalid(format!(
            "Choose a new .{extension} report filename."
        )));
    }
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
    .map_err(|error| Error::Invalid(format!("Report parent access failed: {error}")))?;
    let mut temporary = OwnedFile::create(&handle)
        .map_err(|error| Error::Invalid(format!("Report temporary creation failed: {error}")))?;
    let result: Result<StreamOutcome<Progress>> = (|| {
        let mut writer = BufWriter::with_capacity(64 * 1024, temporary.file());
        let outcome = encode(&mut writer)?;
        if let StreamOutcome::Complete(state) = outcome {
            writer.flush()?;
            drop(writer);
            temporary
                .file()
                .sync_all()
                .map_err(|error| Error::Invalid(format!("Report sync failed: {error}")))?;
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
            temporary
                .publish(leaf)
                .map_err(|error| Error::Invalid(format!("Report publication failed: {error}")))?;
        }
        Ok(outcome)
    })();
    if !temporary.is_finished()
        && let Err(cleanup) = temporary.abort()
    {
        return Err(Error::Invalid(format!(
            "Report was not published. Temporary report cleanup is uncertain for {}: {cleanup}. Export result: {}",
            parent.path().join(temporary.name()).display(),
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

fn invalid() -> Error {
    Error::Invalid("Report destination is protected, unavailable or changed.".into())
}
