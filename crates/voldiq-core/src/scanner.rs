use crate::{
    Entry, Error, Issue, Kind, NodeId, Progress, Result, ScanOptions, ScanPhase, index::Index,
    windows,
};
use std::{
    collections::HashSet,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Instant, UNIX_EPOCH},
};

pub fn scan(
    root: &Path,
    options: &ScanOptions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<Index> {
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
    let exclusions: Vec<_> = options
        .exclusions
        .iter()
        .map(std::fs::canonicalize)
        .collect::<std::io::Result<_>>()?;
    let mut index = Index {
        root: root.clone(),
        entries: vec![Entry {
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
    };
    let start = Instant::now();
    let mut last_report = start;
    let mut stack = vec![(root, NodeId(0))];
    let mut physical = HashSet::new();
    let (mut files, mut bytes) = (0, 0);
    let limit = options
        .max_entries
        .unwrap_or(5_000_000)
        .clamp(1, u32::MAX as usize);
    'scan: while let Some((path, parent)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let directory = match std::fs::read_dir(&path) {
            Ok(d) => d,
            Err(error) => {
                issue(&mut index, parent, &path, error.to_string());
                continue;
            }
        };
        for item in directory {
            if cancel.load(Ordering::Relaxed) {
                break 'scan;
            }
            if index.entries.len() >= limit {
                issue(
                    &mut index,
                    parent,
                    &path,
                    format!("Index limit of {limit} entries reached. Narrow the scan scope."),
                );
                break 'scan;
            }
            let item = match item {
                Ok(item) => item,
                Err(error) => {
                    issue(&mut index, parent, &path, error.to_string());
                    continue;
                }
            };
            let path = item.path();
            if exclusions.iter().any(|excluded| path.starts_with(excluded)) {
                continue;
            }
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(error) => {
                    issue(&mut index, parent, &path, error.to_string());
                    continue;
                }
            };
            let kind = if windows::is_reparse(&metadata) {
                Kind::Link
            } else if metadata.is_dir() {
                Kind::Directory
            } else if metadata.is_file() {
                Kind::File
            } else {
                continue;
            };
            let mut entry = Entry {
                id: NodeId(index.entries.len() as u32),
                parent: Some(parent),
                name: item.file_name().to_string_lossy().into_owned(),
                kind,
                logical: 0,
                allocated: 0,
                allocation_known: true,
                modified: modified(&metadata),
                files: 0,
                incomplete: false,
                hard_link: false,
            };
            if kind == Kind::File {
                entry.logical = metadata.len();
                entry.files = 1;
                files += 1;
                bytes += entry.logical;
                match windows::identity(&path) {
                    Ok(identity) => {
                        entry.hard_link = identity.links > 1;
                        if physical.insert(identity.key) {
                            entry.allocated = identity.allocated;
                        }
                    }
                    Err(error) => {
                        entry.allocation_known = false;
                        entry.incomplete = true;
                        issue(
                            &mut index,
                            parent,
                            &path,
                            format!("Physical allocation unavailable: {error}"),
                        );
                    }
                }
            }
            if kind == Kind::Link {
                entry.incomplete = true;
                entry.allocation_known = false;
            }
            if kind == Kind::Directory {
                stack.push((path, entry.id));
            }
            index.entries.push(entry);
            if last_report.elapsed().as_millis() >= 100 {
                progress(Progress {
                    phase: ScanPhase::Scanning,
                    entries: index.entries.len(),
                    files,
                    logical: bytes,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    errors: index.error_count,
                    method: "Directory enumeration".into(),
                });
                last_report = Instant::now();
            }
        }
    }
    let phase = if cancel.load(Ordering::Relaxed) {
        index.entries[0].incomplete = true;
        ScanPhase::Cancelled
    } else {
        ScanPhase::Complete
    };
    index.aggregate();
    progress(Progress {
        phase,
        entries: index.entries.len(),
        files,
        logical: bytes,
        elapsed_ms: start.elapsed().as_millis() as u64,
        errors: index.error_count,
        method: "Directory enumeration".into(),
    });
    Ok(index)
}

fn modified(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn issue(index: &mut Index, parent: NodeId, path: &Path, message: String) {
    index.error_count += 1;
    index.entries[parent.0 as usize].incomplete = true;
    if index.issues.len() < 1000 {
        index.issues.push(Issue {
            path: path.to_string_lossy().into_owned(),
            message,
        });
    }
}
