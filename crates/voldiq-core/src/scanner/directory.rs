use super::{SharedIndex, issue, modified};
use crate::{Entry, Error, Kind, NodeId, Progress, Result, ScanPhase, windows};
use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Instant,
};

struct Input {
    path: PathBuf,
    name: OsString,
}

struct Measured {
    input: Input,
    entry: Option<Entry>,
    identity: Option<windows::FileIdentity>,
    error: Option<String>,
}

struct Pool<'a> {
    control: Option<&'a super::ScanControl>,
    profile: crate::ScanProfile,
    workers: usize,
    capacity: usize,
    jobs: mpsc::SyncSender<(usize, Input)>,
    results: mpsc::Receiver<(usize, Option<Measured>)>,
}

impl Pool<'_> {
    fn measure(&self, inputs: Vec<Input>) -> Result<Vec<Option<Measured>>> {
        let count = inputs.len();
        for (slot, input) in inputs.into_iter().enumerate() {
            self.jobs
                .send((slot, input))
                .map_err(|_| Error::Invalid("Scan worker stopped.".into()))?;
        }
        let mut measured: Vec<_> = (0..count).map(|_| None).collect();
        for _ in 0..count {
            let (slot, item) = self
                .results
                .recv()
                .map_err(|_| Error::Invalid("Scan worker stopped.".into()))?;
            measured[slot] = item;
        }
        Ok(measured)
    }
}

pub(super) fn enumerate(
    shared: &SharedIndex,
    exclusions: &[PathBuf],
    limit: usize,
    options: &crate::ScanOptions,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(Progress),
    start: Instant,
) -> Result<()> {
    let root = shared
        .read()
        .map_err(|e| Error::Invalid(e.to_string()))?
        .root
        .clone();
    let concurrency = if cancel.load(Ordering::Relaxed) {
        crate::ScanConcurrency::default()
    } else {
        options.concurrency
    };
    let resources = options.profile.resources(concurrency, &root);
    let workers = resources.workers;
    let batch_size = resources.batch_entries;
    let (jobs_tx, jobs_rx) = mpsc::sync_channel::<(usize, Input)>(batch_size);
    let (results_tx, results_rx) = mpsc::sync_channel(batch_size);
    let jobs_rx = Mutex::new(jobs_rx);
    std::thread::scope(|scope| {
        if workers > 1 {
            for _ in 0..workers {
                let jobs = &jobs_rx;
                let results = results_tx.clone();
                scope.spawn(move || {
                    loop {
                        let job = match jobs.lock() {
                            Ok(receiver) => receiver.recv(),
                            Err(_) => return,
                        };
                        let Ok((slot, input)) = job else { return };
                        let measured = if cancel.load(Ordering::Relaxed) {
                            None
                        } else {
                            Some(measure(input))
                        };
                        if results.send((slot, measured)).is_err() {
                            return;
                        }
                    }
                });
            }
        }
        drop(results_tx);
        let pool = Pool {
            control: options.control.as_deref(),
            profile: resources.profile,
            workers,
            capacity: batch_size,
            jobs: jobs_tx,
            results: results_rx,
        };
        walk(shared, exclusions, limit, &pool, cancel, progress, start)
    })
}

fn walk(
    shared: &SharedIndex,
    exclusions: &[PathBuf],
    limit: usize,
    pool: &Pool<'_>,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(Progress),
    start: Instant,
) -> Result<()> {
    let batch_size = if pool.workers > 1 { pool.capacity } else { 1 };
    let (root, root_identity) = {
        let index = shared.read().map_err(|e| Error::Invalid(e.to_string()))?;
        (index.root.clone(), index.root_identity)
    };
    let mut stack = vec![(root.clone(), NodeId(0))];
    let mut physical = HashSet::new();
    let mut last_report = start;
    progress(update(shared, ScanPhase::Scanning, start)?);
    'scan: while let Some((path, parent)) = stack.pop() {
        if !checkpoint(pool, cancel)? {
            break;
        }
        let expected = shared
            .read()
            .map_err(|e| Error::Invalid(e.to_string()))?
            .entries[parent.0 as usize]
            .file_state
            .as_ref()
            .map(|state| state.identity);
        // Keep native ancestors pinned across enumeration, worker batches and paused waits.
        let _pins = match super::pin::directory(&path, &root, root_identity, expected) {
            Ok(pins) => pins,
            Err(error) => {
                record_issue(shared, parent, &path, error.to_string())?;
                continue;
            }
        };
        let mut directory = match std::fs::read_dir(&path) {
            Ok(directory) => directory,
            Err(error) => {
                record_issue(shared, parent, &path, error.to_string())?;
                continue;
            }
        };
        loop {
            if !checkpoint(pool, cancel)? || !pool.profile.checkpoint(cancel) {
                break 'scan;
            }
            let remaining = limit.saturating_sub(
                shared
                    .read()
                    .map_err(|e| Error::Invalid(e.to_string()))?
                    .entries
                    .len(),
            );
            let mut inputs = Vec::new();
            let mut exhausted = false;
            while inputs.len() < batch_size.min(remaining.max(1)) {
                if cancel.load(Ordering::Relaxed) {
                    break 'scan;
                }
                match directory.next() {
                    None => {
                        exhausted = true;
                        break;
                    }
                    Some(Err(error)) => record_issue(shared, parent, &path, error.to_string())?,
                    Some(Ok(item)) => {
                        let child = item.path();
                        if !exclusions
                            .iter()
                            .any(|excluded| child.starts_with(excluded))
                        {
                            if remaining == 0 {
                                record_issue(
                                    shared,
                                    parent,
                                    &path,
                                    format!(
                                        "Index limit of {limit} entries reached. Narrow the scan scope."
                                    ),
                                )?;
                                break 'scan;
                            }
                            inputs.push(Input {
                                path: child,
                                name: item.file_name(),
                            });
                        }
                    }
                }
            }
            let measured = match pool.workers {
                2.. => pool.measure(inputs)?,
                _ => inputs
                    .into_iter()
                    .map(|input| {
                        if cancel.load(Ordering::Relaxed) {
                            None
                        } else {
                            Some(measure(input))
                        }
                    })
                    .collect(),
            };
            for item in measured.into_iter().flatten() {
                if cancel.load(Ordering::Relaxed) {
                    break 'scan;
                }
                let mut index = shared.write().map_err(|e| Error::Invalid(e.to_string()))?;
                if let Some(error) = item.error {
                    issue(&mut index, parent, &item.input.path, error);
                }
                let Some(mut entry) = item.entry else {
                    continue;
                };
                entry.parent = Some(parent);
                entry.id = NodeId(index.entries.len() as u32);
                if let Some(identity) = item.identity
                    && physical.insert(identity.key)
                {
                    entry.allocated = identity.allocated;
                }
                if item.input.name.to_str().is_none() {
                    index.native_names.insert(entry.id, item.input.name);
                }
                if entry.kind == Kind::Directory {
                    stack.push((item.input.path, entry.id));
                }
                index.accumulate(&entry);
                index.entries.push(entry);
                let report = index.entries.len() == 2 || last_report.elapsed().as_millis() >= 100;
                drop(index);
                if report {
                    progress(update(shared, ScanPhase::Scanning, start)?);
                    last_report = Instant::now();
                }
            }
            if exhausted {
                break;
            }
        }
    }
    let phase = if cancel.load(Ordering::Relaxed) {
        shared
            .write()
            .map_err(|e| Error::Invalid(e.to_string()))?
            .entries[0]
            .incomplete = true;
        ScanPhase::Cancelled
    } else {
        ScanPhase::Complete
    };
    progress(update(shared, phase, start)?);
    Ok(())
}

fn update(shared: &SharedIndex, phase: ScanPhase, start: Instant) -> Result<Progress> {
    let index = shared.read().map_err(|e| Error::Invalid(e.to_string()))?;
    Ok(Progress {
        previous: false,
        phase,
        entries: index.entries.len(),
        files: index.entries[0].files,
        logical: index.entries[0].logical,
        elapsed_ms: start.elapsed().as_millis() as u64,
        errors: index.error_count,
        method: "Directory enumeration".into(),
    })
}

fn record_issue(shared: &SharedIndex, parent: NodeId, path: &Path, message: String) -> Result<()> {
    let mut index = shared.write().map_err(|e| Error::Invalid(e.to_string()))?;
    issue(&mut index, parent, path, message);
    Ok(())
}

fn measure(input: Input) -> Measured {
    let mut measured = Measured {
        input,
        entry: None,
        identity: None,
        error: None,
    };
    let metadata = match std::fs::symlink_metadata(&measured.input.path) {
        Ok(metadata) => metadata,
        Err(error) => {
            measured.error = Some(error.to_string());
            return measured;
        }
    };
    let kind = if windows::is_reparse(&metadata) {
        Kind::Link
    } else if metadata.is_dir() {
        Kind::Directory
    } else if metadata.is_file() {
        Kind::File
    } else {
        return measured;
    };
    let mut entry = Entry {
        cloud: crate::cloud::CloudState::Unknown,
        file_state: None,
        id: NodeId(0),
        parent: None,
        name: measured.input.name.to_string_lossy().into_owned(),
        kind,
        logical: 0,
        allocated: 0,
        allocation_known: true,
        modified: modified(&metadata),
        files: 0,
        incomplete: false,
        hard_link: false,
    };
    if matches!(kind, Kind::File | Kind::Directory) {
        if kind == Kind::File {
            entry.logical = metadata.len();
            entry.files = 1;
        }
        match windows::metadata_identity(&measured.input.path) {
            Ok((current, identity))
                if !windows::is_reparse(&current)
                    && ((kind == Kind::File && current.is_file())
                        || (kind == Kind::Directory && current.is_dir())) =>
            {
                entry.modified = modified(&current);
                entry.cloud = identity.cloud;
                entry.file_state = Some(crate::FileState {
                    identity: identity.key,
                    change_time: identity.change_time,
                });
                if kind == Kind::File {
                    entry.logical = current.len();
                    entry.hard_link = identity.links > 1;
                    measured.identity = Some(identity);
                }
            }
            result => {
                let error = match result {
                    Err(error) => error.to_string(),
                    _ => "Entry changed during scanning.".into(),
                };
                entry.allocation_known = false;
                entry.incomplete = true;
                measured.error = Some(format!("Native metadata unavailable: {error}"));
            }
        }
    }
    if kind == Kind::Link {
        entry.incomplete = true;
        entry.allocation_known = false;
        #[cfg(windows)]
        if let Ok((current, identity)) = windows::metadata_identity(&measured.input.path)
            && windows::is_reparse(&current)
        {
            entry.cloud = identity.cloud;
            entry.modified = modified(&current);
            entry.file_state = Some(crate::FileState {
                identity: identity.key,
                change_time: identity.change_time,
            });
            if entry.cloud.is_file() {
                entry.logical = current.len();
                entry.files = 1;
                entry.hard_link = identity.links > 1;
            }
        }
    }
    measured.entry = Some(entry);
    measured
}

fn checkpoint(pool: &Pool<'_>, cancel: &AtomicBool) -> Result<bool> {
    match pool.control {
        Some(control) => control.checkpoint(cancel),
        None => Ok(!cancel.load(Ordering::Relaxed)),
    }
}
