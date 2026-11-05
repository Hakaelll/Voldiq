use super::{ReadState, Record, VolumeReader};
use crate::{
    Entry, Error, Kind, NodeId, Progress, Result, ScanPhase,
    scanner::{SharedIndex, issue, modified},
    windows,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ffi::OsString,
    fs::File,
    os::windows::ffi::OsStringExt,
    path::{Component, Path},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

pub(crate) fn try_scan(
    shared: &SharedIndex,
    limit: usize,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(Progress),
    start: Instant,
    profile: crate::ScanProfile,
) -> Result<bool> {
    let root = shared
        .read()
        .map_err(|e| Error::Invalid(e.to_string()))?
        .root
        .clone();
    if root.parent().is_some() {
        return Ok(false);
    }
    report(shared, ScanPhase::Scanning, start, progress)?;
    // Capability errors occur before any non-root ID is published.
    let prepared = (|| {
        let reader = VolumeReader::open(&root)?.with_profile(profile);
        let records = reader.enumerate(limit, cancel)?;
        Ok::<_, Error>((reader, records))
    })();
    let Ok((reader, records)) = prepared else {
        return Ok(false);
    };
    let checkpoint = match records.state {
        ReadState::Complete(checkpoint) => checkpoint,
        ReadState::Limited => return Ok(false),
        ReadState::Cancelled => {
            shared
                .write()
                .map_err(|e| Error::Invalid(e.to_string()))?
                .entries[0]
                .incomplete = true;
            report(shared, ScanPhase::Cancelled, start, progress)?;
            return Ok(true);
        }
    };
    if let Err(error) = populate(
        shared,
        records.items,
        |reference| {
            if !profile.checkpoint(cancel) {
                return Err(Error::Invalid("NTFS scan cancelled.".into()));
            }
            reader.file(reference)
        },
        limit,
        cancel,
        &mut *progress,
    ) {
        if shared
            .read()
            .map_err(|e| Error::Invalid(e.to_string()))?
            .entries
            .len()
            == 1
        {
            return Ok(false);
        }
        record_error(shared, NodeId(0), &root, error.to_string())?;
        report(shared, ScanPhase::Failed, start, progress)?;
        return Ok(true);
    }
    if !cancel.load(Ordering::Relaxed) {
        let consistent = reader.journal().map(|journal| {
            journal.can_resume(reader.serial, checkpoint) && journal.next == checkpoint.next
        });
        if !matches!(consistent, Ok(true)) {
            let mut index = shared.write().map_err(|e| Error::Invalid(e.to_string()))?;
            issue(
                &mut index,
                NodeId(0),
                &root,
                "Volume changed during NTFS indexing. Rescan required.".into(),
            );
        } else {
            let mut index = shared.write().map_err(|e| Error::Invalid(e.to_string()))?;
            if !index.entries[0].incomplete && index.error_count == 0 {
                index.ntfs_checkpoint = Some(checkpoint);
            }
        }
    }
    let phase = if cancel.load(Ordering::Relaxed) {
        let mut index = shared.write().map_err(|e| Error::Invalid(e.to_string()))?;
        index.ntfs_checkpoint = None;
        index.entries[0].incomplete = true;
        ScanPhase::Cancelled
    } else {
        ScanPhase::Complete
    };
    report(shared, phase, start, progress)?;
    Ok(true)
}

fn report(
    shared: &SharedIndex,
    phase: ScanPhase,
    start: Instant,
    progress: &mut impl FnMut(Progress),
) -> Result<()> {
    let index = shared.read().map_err(|e| Error::Invalid(e.to_string()))?;
    let update = Progress {
        previous: false,
        phase,
        entries: index.entries.len(),
        files: index.entries[0].files,
        logical: index.entries[0].logical,
        errors: index.error_count,
        elapsed_ms: start.elapsed().as_millis() as u64,
        method: "NTFS metadata".into(),
    };
    drop(index);
    progress(update);
    Ok(())
}

/// Builds a live index from native records and metadata handles. Content is never read.
pub fn populate(
    shared: &SharedIndex,
    records: Vec<Record>,
    mut open: impl FnMut(u64) -> Result<File>,
    limit: usize,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<()> {
    let start = Instant::now();
    let mut last_report = start;
    let (root, root_key, exclusions) = {
        let index = shared.read().map_err(|e| Error::Invalid(e.to_string()))?;
        if index.entries.len() != 1 {
            return Err(Error::Invalid(
                "NTFS population requires a fresh index.".into(),
            ));
        }
        (
            index.root.clone(),
            index
                .root_identity
                .ok_or_else(|| Error::Invalid("Root identity unavailable.".into()))?,
            index.exclusions.clone(),
        )
    };
    let mut children: HashMap<u64, Vec<&Record>> = HashMap::new();
    let mut seen = HashSet::new();
    for record in &records {
        if record.reference == root_key.1 {
            continue;
        }
        let name = OsString::from_wide(&record.name);
        if !seen.insert(record.reference)
            || name.is_empty()
            || name.as_encoded_bytes().contains(&0)
            || !matches!(
                Path::new(&name).components().collect::<Vec<_>>().as_slice(),
                [Component::Normal(_)]
            )
        {
            return Err(Error::Invalid("Invalid NTFS hierarchy record.".into()));
        }
        if record.reference != root_key.1 {
            children.entry(record.parent).or_default().push(record);
        }
    }
    let mut directories = HashMap::from([(root_key.1, NodeId(0))]);
    let mut queue = VecDeque::from([root_key.1]);
    let mut physical = HashSet::new();
    let mut names = HashSet::new();
    let mut visited = HashSet::from([root_key.1]);
    let mut pruned = HashSet::new();
    'directories: while let Some(reference) = queue.pop_front() {
        let parent = directories[&reference];
        let Some(children) = children.get(&reference) else {
            continue;
        };
        for record in children
            .iter()
            .filter(|record| record.attributes & 0x10 != 0)
        {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if !visited.insert(record.reference) {
                return Err(Error::Invalid("Cyclic NTFS hierarchy.".into()));
            }
            let name = OsString::from_wide(&record.name);
            let path = shared
                .read()
                .map_err(|e| Error::Invalid(e.to_string()))?
                .path(parent)?
                .join(&name);
            if exclusions.iter().any(|excluded| path.starts_with(excluded)) {
                pruned.insert(record.reference);
                continue;
            }
            match open(record.reference).and_then(|file| {
                if windows::identity(&path)?.key != (root_key.0, record.reference) {
                    return Err(Error::Invalid("Directory changed during indexing.".into()));
                }
                read_entry(
                    &file,
                    root_key.0,
                    record.reference,
                    parent,
                    &name,
                    &mut physical,
                )
            }) {
                Ok(entry) => {
                    let kind = entry.kind;
                    if let Some(id) = insert(shared, entry, name, limit)? {
                        if kind == Kind::Directory {
                            directories.insert(record.reference, id);
                            queue.push_back(record.reference);
                        } else {
                            pruned.insert(record.reference);
                        }
                    } else {
                        break 'directories;
                    }
                }
                Err(error) => {
                    pruned.insert(record.reference);
                    record_error(shared, parent, &path, error.to_string())?;
                }
            }
            if last_report.elapsed().as_millis() >= 100 {
                report(shared, ScanPhase::Scanning, start, &mut progress)?;
                last_report = Instant::now();
            }
        }
    }
    let mut pruning: Vec<_> = pruned.iter().copied().collect();
    while let Some(reference) = pruning.pop() {
        if let Some(descendants) = children.get(&reference) {
            for record in descendants {
                if pruned.insert(record.reference) {
                    pruning.push(record.reference);
                }
            }
        }
    }
    if !cancel.load(Ordering::Relaxed) {
        for record in records.iter().filter(|record| {
            record.attributes & 0x10 != 0
                && !visited.contains(&record.reference)
                && !pruned.contains(&record.reference)
        }) {
            record_error(
                shared,
                NodeId(0),
                &root,
                format!("Unresolved NTFS directory {}.", record.reference),
            )?;
        }
    }
    for record in records
        .iter()
        .filter(|record| record.attributes & 0x10 == 0)
    {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if shared
            .read()
            .map_err(|e| Error::Invalid(e.to_string()))?
            .entries
            .len()
            >= limit
        {
            record_error(
                shared,
                NodeId(0),
                &root,
                format!("Index limit of {limit} entries reached."),
            )?;
            break;
        }
        // A file may have another name beneath an included parent, even when its MFT name is excluded.
        let result = (|| {
            let file = open(record.reference)?;
            let identity = windows::file_identity(&file)?;
            let aliases = if identity.links > 1 {
                windows::hard_link_names(&windows::final_path(&file)?)?
            } else {
                match directories.get(&record.parent) {
                    Some(parent) => vec![
                        shared
                            .read()
                            .map_err(|e| Error::Invalid(e.to_string()))?
                            .path(*parent)?
                            .join(OsString::from_wide(&record.name)),
                    ],
                    None if pruned.contains(&record.parent) => return Ok(()),
                    None => return Err(Error::Invalid("Unresolved NTFS file parent.".into())),
                }
            };
            for path in aliases {
                if !path.starts_with(&root)
                    || exclusions.iter().any(|excluded| path.starts_with(excluded))
                    || !names.insert(path.clone())
                {
                    continue;
                }
                let Some(parent_path) = path.parent() else {
                    continue;
                };
                let parent_key = windows::identity(parent_path)?.key;
                let Some(&parent) = directories
                    .get(&parent_key.1)
                    .filter(|_| parent_key.0 == root_key.0)
                else {
                    continue;
                };
                let name = path
                    .file_name()
                    .ok_or_else(|| Error::Invalid("Missing native filename.".into()))?
                    .to_owned();
                if shared
                    .read()
                    .map_err(|e| Error::Invalid(e.to_string()))?
                    .path(parent)?
                    != parent_path
                {
                    return Err(Error::Invalid(
                        "Parent name changed during indexing.".into(),
                    ));
                }
                if windows::identity(&path)?.key != identity.key {
                    return Err(Error::Invalid("File name changed during indexing.".into()));
                }
                let entry = read_entry(
                    &file,
                    root_key.0,
                    record.reference,
                    parent,
                    &name,
                    &mut physical,
                )?;
                if insert(shared, entry, name, limit)?.is_none() {
                    break;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            record_error(shared, NodeId(0), &root, error.to_string())?;
        }
        if last_report.elapsed().as_millis() >= 100 {
            report(shared, ScanPhase::Scanning, start, &mut progress)?;
            last_report = Instant::now();
        }
    }
    if cancel.load(Ordering::Relaxed) {
        shared
            .write()
            .map_err(|e| Error::Invalid(e.to_string()))?
            .entries[0]
            .incomplete = true;
    }
    Ok(())
}

pub(super) fn read_entry(
    file: &File,
    volume: u64,
    reference: u64,
    parent: NodeId,
    name: &std::ffi::OsStr,
    physical: &mut HashSet<(u64, u64)>,
) -> Result<Entry> {
    let before = windows::file_identity(file)?;
    let metadata = file.metadata()?;
    let identity = windows::file_identity(file)?;
    if identity.key != (volume, reference)
        || before.key != identity.key
        || before.change_time != identity.change_time
    {
        return Err(Error::Invalid("NTFS file identity changed.".into()));
    }
    let kind = if windows::is_reparse(&metadata) {
        Kind::Link
    } else if metadata.is_dir() {
        Kind::Directory
    } else {
        Kind::File
    };
    Ok(Entry {
        cloud: identity.cloud,
        file_state: Some(crate::FileState {
            identity: identity.key,
            change_time: identity.change_time,
        }),
        id: NodeId(0),
        parent: Some(parent),
        name: name.to_string_lossy().into_owned(),
        kind,
        logical: if kind == Kind::File || identity.cloud.is_file() {
            metadata.len()
        } else {
            0
        },
        allocated: if kind == Kind::File && physical.insert(identity.key) {
            identity.allocated
        } else {
            0
        },
        allocation_known: kind != Kind::Link,
        modified: modified(&metadata),
        files: u64::from(kind == Kind::File || identity.cloud.is_file()),
        incomplete: kind == Kind::Link,
        hard_link: identity.links > 1,
    })
}

fn insert(
    shared: &SharedIndex,
    mut entry: Entry,
    native: OsString,
    limit: usize,
) -> Result<Option<NodeId>> {
    let mut index = shared.write().map_err(|e| Error::Invalid(e.to_string()))?;
    if index.entries.len() >= limit {
        let root = index.root.clone();
        if !index.entries[0].incomplete {
            issue(
                &mut index,
                NodeId(0),
                &root,
                format!("Index limit of {limit} entries reached."),
            );
        }
        return Ok(None);
    }
    entry.id = NodeId(index.entries.len() as u32);
    if native.to_str().is_none() {
        index.native_names.insert(entry.id, native);
    }
    let id = entry.id;
    index.accumulate(&entry);
    index.entries.push(entry);
    Ok(Some(id))
}

fn record_error(shared: &SharedIndex, parent: NodeId, path: &Path, message: String) -> Result<()> {
    let mut index = shared.write().map_err(|e| Error::Invalid(e.to_string()))?;
    issue(&mut index, parent, path, message);
    Ok(())
}
