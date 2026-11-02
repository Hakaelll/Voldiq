use super::{Checkpoint, ReadState, Records, VolumeReader, indexing::read_entry};
use crate::{Entry, Error, Kind, NodeId, Result, index::Index, windows};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ffi::OsString,
    fs::File,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

pub fn refresh(index: &Index, limit: usize, cancel: &AtomicBool) -> Result<Index> {
    cancelled(cancel)?;
    let checkpoint = index.ntfs_checkpoint.ok_or_else(resync)?;
    if index.root.parent().is_some()
        || windows::identity(&index.root)?.key != index.root_identity.ok_or_else(resync)?
    {
        return Err(resync());
    }
    let reader = VolumeReader::open(&index.root)?;
    let changes = reader.changes(checkpoint, limit, cancel)?;
    replay(
        index,
        changes,
        |reference| reader.file(reference),
        limit,
        cancel,
        |next| {
            let journal = reader.journal()?;
            if !journal.can_resume(reader.serial, next)
                || journal.next != next.next
                || windows::identity(&index.root)?.key != index.root_identity.ok_or_else(resync)?
            {
                return Err(resync());
            }
            Ok(())
        },
    )
}

struct Directory {
    entry: Entry,
    name: OsString,
    parent: u64,
    current_path: Option<PathBuf>,
}

/// Rebuilds rows from a complete journal batch, rereading only changed identities.
/// The final verifier must confirm unchanged journal bounds before publication.
pub fn replay(
    index: &Index,
    changes: Records,
    mut open: impl FnMut(u64) -> Result<File>,
    limit: usize,
    cancel: &AtomicBool,
    verify: impl FnOnce(Checkpoint) -> Result<()>,
) -> Result<Index> {
    cancelled(cancel)?;
    let before = index.ntfs_checkpoint.ok_or_else(resync)?;
    let root_key = index.root_identity.ok_or_else(resync)?;
    let ReadState::Complete(next) = changes.state else {
        return Err(resync());
    };
    let root_entry = index.entry(NodeId(0))?;
    if index.snapshot_id.is_some()
        || index.error_count != 0
        || root_entry.incomplete
        || before.volume != root_key.0
        || next.volume != before.volume
        || next.journal != before.journal
        || before.next < 0
        || next.next < before.next
    {
        return Err(resync());
    }
    let mut changed = HashSet::new();
    let mut last = before.next - 1;
    for record in changes.items {
        cancelled(cancel)?;
        if record.usn <= last || record.usn < before.next || record.usn >= next.next {
            return Err(resync());
        }
        last = record.usn;
        changed.insert(record.reference);
    }
    let mut directories = HashMap::new();
    let mut allocated = HashMap::<u64, u64>::new();
    let references: Vec<_> = index
        .entries
        .iter()
        .map(|entry| {
            cancelled(cancel)?;
            if entry.id == NodeId(0) {
                Ok(root_key.1)
            } else {
                let state = entry.file_state.ok_or_else(resync)?;
                if state.identity.0 != root_key.0 || entry.incomplete {
                    return Err(resync());
                }
                Ok(state.identity.1)
            }
        })
        .collect::<Result<_>>()?;
    for entry in index.entries.iter().skip(1) {
        cancelled(cancel)?;
        let reference = references[entry.id.0 as usize];
        let parent = references[entry.parent.ok_or_else(resync)?.0 as usize];
        if entry.kind == Kind::Directory {
            directories.insert(
                reference,
                Directory {
                    entry: entry.clone(),
                    name: native_name(index, entry),
                    parent,
                    current_path: None,
                },
            );
        } else if entry.kind == Kind::File {
            allocated
                .entry(reference)
                .and_modify(|value| *value = (*value).max(entry.allocated))
                .or_insert(entry.allocated);
        } else {
            return Err(resync());
        }
    }
    let mut updated_files = Vec::new();
    let mut updated_root = None;
    let mut physical = HashSet::new();
    let mut ordered: Vec<_> = changed.iter().copied().collect();
    ordered.sort_unstable();
    for reference in ordered {
        cancelled(cancel)?;
        let file = match open(reference) {
            Ok(file) => file,
            Err(Error::Io(error)) if matches!(error.raw_os_error(), Some(2 | 3)) => {
                directories.remove(&reference);
                continue;
            }
            Err(error) => return Err(error),
        };
        let path = windows::final_path(&file)?;
        let name = if reference == root_key.1 {
            OsString::from(&root_entry.name)
        } else {
            path.file_name().ok_or_else(resync)?.to_owned()
        };
        let entry = read_entry(
            &file,
            root_key.0,
            reference,
            NodeId(0),
            &name,
            &mut physical,
        )?;
        if entry.kind == Kind::Link {
            return Err(resync());
        }
        if reference == root_key.1 {
            if entry.kind != Kind::Directory {
                return Err(resync());
            }
            updated_root = Some(entry);
            continue;
        }
        let parent = windows::identity(path.parent().ok_or_else(resync)?)?.key;
        if parent.0 != root_key.0 || !path.starts_with(&index.root) {
            return Err(resync());
        }
        if entry.kind == Kind::Directory {
            // New included subtrees need enumeration; the caller performs a full resynchronization.
            if !directories.contains_key(&reference) && !excluded(index, &path) {
                return Err(resync());
            }
            directories.insert(
                reference,
                Directory {
                    entry,
                    name,
                    parent: parent.1,
                    current_path: Some(path),
                },
            );
        } else {
            allocated.insert(reference, entry.allocated);
            let paths = if entry.hard_link {
                windows::hard_link_names(&path)?
            } else {
                vec![path]
            };
            updated_files.push((entry, paths));
        }
    }
    let mut root = updated_root.unwrap_or_else(|| root_entry.clone());
    root.parent = None;
    root.id = NodeId(0);
    root.logical = 0;
    root.allocated = 0;
    root.files = 0;
    let mut result = Index {
        root: index.root.clone(),
        entries: vec![root],
        issues: Vec::new(),
        error_count: 0,
        native_names: HashMap::new(),
        root_identity: Some(root_key),
        exclusions: index.exclusions.clone(),
        snapshot_id: None,
        captured_caches: Vec::new(),
        observation: None,
        ntfs_checkpoint: None,
    };
    let mut children = HashMap::<u64, Vec<u64>>::new();
    for (&reference, directory) in &directories {
        children
            .entry(directory.parent)
            .or_default()
            .push(reference);
    }
    for children in children.values_mut() {
        children.sort_unstable();
    }
    let mut parents = HashMap::from([(root_key.1, NodeId(0))]);
    let mut queue = VecDeque::from([root_key.1]);
    while let Some(reference) = queue.pop_front() {
        cancelled(cancel)?;
        let Some(children) = children.get(&reference) else {
            continue;
        };
        for &child in children {
            let directory = &directories[&child];
            let parent = parents[&reference];
            let path = result.path(parent)?.join(&directory.name);
            if excluded(index, &path) {
                continue;
            }
            if directory
                .current_path
                .as_ref()
                .is_some_and(|actual| actual != &path)
            {
                return Err(resync());
            }
            let mut entry = directory.entry.clone();
            entry.parent = Some(parent);
            entry.logical = 0;
            entry.allocated = 0;
            entry.files = 0;
            let id = push(&mut result, entry, directory.name.clone(), limit)?;
            if parents.insert(child, id).is_some() {
                return Err(resync());
            }
            queue.push_back(child);
        }
    }
    for (&reference, directory) in &directories {
        if directory
            .current_path
            .as_ref()
            .is_some_and(|path| !excluded(index, path))
            && !parents.contains_key(&reference)
        {
            return Err(resync());
        }
    }
    let mut physical = HashSet::new();
    for entry in index
        .entries
        .iter()
        .filter(|entry| entry.kind == Kind::File)
    {
        cancelled(cancel)?;
        let reference = references[entry.id.0 as usize];
        if changed.contains(&reference) {
            continue;
        }
        let Some(&parent) = parents.get(&references[entry.parent.ok_or_else(resync)?.0 as usize])
        else {
            continue;
        };
        let name = native_name(index, entry);
        if excluded(index, &result.path(parent)?.join(&name)) {
            continue;
        }
        let mut entry = entry.clone();
        entry.parent = Some(parent);
        entry.allocated = if physical.insert(reference) {
            allocated[&reference]
        } else {
            0
        };
        push(&mut result, entry, name, limit)?;
    }
    for (entry, paths) in updated_files {
        for path in paths {
            cancelled(cancel)?;
            if !path.starts_with(&index.root) || excluded(index, &path) {
                continue;
            }
            let key = windows::identity(path.parent().ok_or_else(resync)?)?.key;
            let parent = *parents
                .get(&key.1)
                .filter(|_| key.0 == root_key.0)
                .ok_or_else(resync)?;
            let name = path.file_name().ok_or_else(resync)?.to_owned();
            if result.path(parent)?.join(&name) != path
                || windows::identity(&path)?.key != entry.file_state.ok_or_else(resync)?.identity
            {
                return Err(resync());
            }
            let mut entry = entry.clone();
            entry.parent = Some(parent);
            entry.allocated = if physical.insert(entry.file_state.ok_or_else(resync)?.identity.1) {
                entry.allocated
            } else {
                0
            };
            push(&mut result, entry, name, limit)?;
        }
    }
    cancelled(cancel)?;
    verify(next)?;
    cancelled(cancel)?;
    result.ntfs_checkpoint = Some(next);
    Ok(result)
}

fn native_name(index: &Index, entry: &Entry) -> OsString {
    index
        .native_names
        .get(&entry.id)
        .cloned()
        .unwrap_or_else(|| OsString::from(&entry.name))
}
fn excluded(index: &Index, path: &std::path::Path) -> bool {
    index
        .exclusions
        .iter()
        .any(|excluded| path.starts_with(excluded))
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Invalid("NTFS refresh cancelled.".into()))
    } else {
        Ok(())
    }
}
fn resync() -> Error {
    Error::Invalid("NTFS refresh requires full resynchronization.".into())
}
fn push(index: &mut Index, mut entry: Entry, name: OsString, limit: usize) -> Result<NodeId> {
    if index.entries.len() >= limit.clamp(1, u32::MAX as usize) {
        return Err(resync());
    }
    entry.id = NodeId(u32::try_from(index.entries.len()).map_err(|_| resync())?);
    if name.to_str().is_none() {
        index.native_names.insert(entry.id, name);
    }
    let id = entry.id;
    index.accumulate(&entry);
    index.entries.push(entry);
    Ok(id)
}
