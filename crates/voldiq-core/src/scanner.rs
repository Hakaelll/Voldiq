mod directory;
mod profile;
use crate::{
    Entry, Error, Issue, Kind, NodeId, Progress, Result, ScanOptions, ScanPhase, index::Index,
    windows,
};
pub use profile::{ScanProfile, ScanResources};
use std::{
    path::Path,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Instant, UNIX_EPOCH},
};

pub type SharedIndex = Arc<RwLock<Index>>;

pub fn refresh_or_scan(
    root: &Path,
    options: &ScanOptions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
    publish: impl FnOnce(SharedIndex),
    previous: Option<&SharedIndex>,
) -> Result<SharedIndex> {
    #[cfg(windows)]
    if options.mode == crate::ScanMode::Auto
        && let Some(previous) = previous
    {
        let index = previous.read().map_err(|e| Error::Invalid(e.to_string()))?;
        let mut exclusions: Vec<_> = options
            .exclusions
            .iter()
            .map(std::fs::canonicalize)
            .collect::<std::io::Result<_>>()?;
        exclusions.sort();
        exclusions.dedup();
        if index.ntfs_checkpoint.is_some()
            && index.root == std::fs::canonicalize(root)?
            && index.exclusions == exclusions
        {
            let start = Instant::now();
            progress(Progress {
                previous: true,
                phase: ScanPhase::Scanning,
                entries: 0,
                files: 0,
                logical: 0,
                errors: 0,
                elapsed_ms: 0,
                method: "USN Journal".into(),
            });
            let limit = options
                .max_entries
                .unwrap_or(5_000_000)
                .clamp(1, u32::MAX as usize);
            if let Ok(mut updated) = crate::ntfs::refresh(&index, limit, cancel, options.profile) {
                updated.record_observation(cancel.load(Ordering::Relaxed));
                let update = Progress {
                    previous: false,
                    phase: ScanPhase::Complete,
                    entries: updated.entries.len(),
                    files: updated.entries[0].files,
                    logical: updated.entries[0].logical,
                    errors: updated.error_count,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    method: "USN Journal".into(),
                };
                drop(index);
                let shared = Arc::new(RwLock::new(updated));
                publish(shared.clone());
                progress(update);
                return Ok(shared);
            }
            if cancel.load(Ordering::Relaxed) {
                let update = Progress {
                    previous: true,
                    phase: ScanPhase::Cancelled,
                    entries: index.entries.len(),
                    files: index.entries[0].files,
                    logical: index.entries[0].logical,
                    errors: index.error_count,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    method: "USN Journal".into(),
                };
                drop(index);
                progress(update);
                return Ok(previous.clone());
            }
        }
    }
    #[cfg(not(windows))]
    let _ = previous;
    scan_live(root, options, cancel, progress, publish)
}

pub fn scan(
    root: &Path,
    options: &ScanOptions,
    cancel: &AtomicBool,
    progress: impl FnMut(Progress),
) -> Result<Index> {
    let index = scan_live(root, options, cancel, progress, |_| {})?;
    Arc::try_unwrap(index)
        .map_err(|_| Error::Invalid("Scan index is still in use.".into()))?
        .into_inner()
        .map_err(|e| Error::Invalid(e.to_string()))
}

pub fn scan_live(
    root: &Path,
    options: &ScanOptions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
    publish: impl FnOnce(SharedIndex),
) -> Result<SharedIndex> {
    if windows::is_reparse(&std::fs::symlink_metadata(root)?) {
        return Err(Error::Invalid(
            "Choose a directory that is not a reparse point.".into(),
        ));
    }
    let root = std::fs::canonicalize(root)?;
    let metadata = std::fs::symlink_metadata(&root)?;
    if !metadata.is_dir() || windows::is_reparse(&metadata) {
        return Err(Error::Invalid(
            "Choose a directory that is not a reparse point.".into(),
        ));
    }
    let mut exclusions: Vec<_> = options
        .exclusions
        .iter()
        .map(std::fs::canonicalize)
        .collect::<std::io::Result<_>>()?;
    exclusions.sort();
    exclusions.dedup();
    let root_identity = windows::identity(&root).ok();
    let shared = Arc::new(RwLock::new(Index {
        root: root.clone(),
        entries: vec![Entry {
            cloud: root_identity
                .as_ref()
                .map_or(crate::cloud::CloudState::Unknown, |identity| identity.cloud),
            file_state: root_identity.as_ref().map(|identity| crate::FileState {
                identity: identity.key,
                change_time: identity.change_time,
            }),
            id: NodeId(0),
            parent: None,
            name: root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.to_string_lossy().into_owned()),
            kind: Kind::Directory,
            logical: 0,
            allocated: 0,
            allocation_known: true,
            modified: modified(&metadata),
            files: 0,
            incomplete: false,
            hard_link: false,
        }],
        issues: Vec::new(),
        error_count: 0,
        native_names: Default::default(),
        root_identity: root_identity.map(|identity| identity.key),
        exclusions: exclusions.clone(),
        snapshot_id: None,
        captured_caches: Vec::new(),
        observation: None,
        ntfs_checkpoint: None,
    }));
    publish(shared.clone());
    let start = Instant::now();
    let limit = options
        .max_entries
        .unwrap_or(5_000_000)
        .clamp(1, u32::MAX as usize);
    #[cfg(windows)]
    if options.mode == crate::ScanMode::Auto
        && crate::ntfs::try_scan(
            &shared,
            limit,
            cancel,
            &mut progress,
            start,
            options.profile,
        )?
    {
        shared
            .write()
            .map_err(|e| Error::Invalid(e.to_string()))?
            .record_observation(cancel.load(Ordering::Relaxed));
        return Ok(shared);
    }
    let concurrency = if cancel.load(Ordering::Relaxed) {
        crate::ScanConcurrency::default()
    } else {
        options.concurrency
    };
    directory::enumerate(
        &shared,
        &exclusions,
        limit,
        options.profile.resources(concurrency, &root),
        cancel,
        &mut progress,
        start,
    )?;
    shared
        .write()
        .map_err(|e| Error::Invalid(e.to_string()))?
        .record_observation(cancel.load(Ordering::Relaxed));
    Ok(shared)
}
pub(crate) fn modified(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

pub(crate) fn issue(index: &mut Index, parent: NodeId, path: &Path, message: String) {
    index.error_count += 1;
    let mut next = Some(parent);
    while let Some(id) = next {
        let entry = &mut index.entries[id.0 as usize];
        entry.incomplete = true;
        next = entry.parent;
    }
    if index.issues.len() < 1000 {
        index.issues.push(Issue {
            path: path.to_string_lossy().into_owned(),
            message,
        });
    }
}
