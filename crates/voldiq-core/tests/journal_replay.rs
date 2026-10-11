#![cfg(windows)]
use std::{
    collections::HashMap,
    fs::File,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};
use voldiq_core::{
    Error, Kind, ScanOptions,
    index::Index,
    ntfs::{Checkpoint, ReadState, Record, Records, populate, replay},
    scanner::scan,
    windows,
};

fn paths(root: &Path) -> Vec<PathBuf> {
    let mut result = vec![root.to_owned()];
    let mut stack = vec![root.to_owned()];
    while let Some(parent) = stack.pop() {
        for child in std::fs::read_dir(parent).unwrap() {
            let path = child.unwrap().path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            result.push(path);
        }
    }
    result
}
fn handle(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::*;
    File::options()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}
fn record(path: &Path, usn: i64) -> Record {
    Record {
        reference: windows::identity(path).unwrap().key.1,
        parent: windows::identity(path.parent().unwrap()).unwrap().key.1,
        usn,
        reason: 0,
        attributes: if path.is_dir() { 0x10 } else { 0 },
        name: path.file_name().unwrap().encode_wide().collect(),
    }
}
fn baseline(root: &Path, exclusions: Vec<String>) -> Index {
    let mut index = scan(
        root,
        &ScanOptions {
            exclusions,
            ..ScanOptions::default()
        },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    index.entries.truncate(1);
    index.entries[0].logical = 0;
    index.entries[0].allocated = 0;
    index.entries[0].files = 0;
    index.native_names.clear();
    let files = paths(&index.root);
    let handles: HashMap<_, _> = files
        .iter()
        .map(|path| (windows::identity(path).unwrap().key.1, path))
        .collect();
    let shared = Arc::new(RwLock::new(index));
    let mut seen = std::collections::HashSet::new();
    populate(
        &shared,
        files
            .iter()
            .skip(1)
            .map(|path| record(path, 0))
            .filter(|record| seen.insert(record.reference))
            .collect(),
        |id| Ok(handle(handles[&id])?),
        1000,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let mut index = Arc::try_unwrap(shared).ok().unwrap().into_inner().unwrap();
    index.ntfs_checkpoint = Some(Checkpoint {
        volume: index.root_identity.unwrap().0,
        journal: 7,
        next: 100,
    });
    index
}
fn changes(index: &Index, records: Vec<Record>) -> Records {
    Records {
        items: records,
        state: ReadState::Complete(Checkpoint {
            next: 200,
            ..index.ntfs_checkpoint.unwrap()
        }),
    }
}
fn current_handles(root: &Path) -> HashMap<u64, PathBuf> {
    paths(root)
        .into_iter()
        .map(|path| (windows::identity(&path).unwrap().key.1, path))
        .collect()
}
fn open(handles: &HashMap<u64, PathBuf>, reference: u64) -> voldiq_core::Result<File> {
    match handles.get(&reference) {
        Some(path) => Ok(handle(path)?),
        None => Err(std::io::Error::from_raw_os_error(2).into()),
    }
}
fn parity(index: &Index) {
    let conventional = scan(
        &index.root,
        &ScanOptions {
            exclusions: index
                .exclusions
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            ..ScanOptions::default()
        },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let rows = |index: &Index| {
        let mut rows: Vec<_> = index
            .entries
            .iter()
            .map(|entry| {
                (
                    index.path(entry.id).unwrap(),
                    entry.kind == Kind::Directory,
                    entry.logical,
                    entry.files,
                )
            })
            .collect();
        rows.sort();
        rows
    };
    assert_eq!(rows(index), rows(&conventional));
    assert_eq!(
        index.entries[0].allocated,
        conventional.entries[0].allocated
    );
}

#[test]
fn journal_replay_applies_size_delete_file_rename_and_directory_move_without_rereading_unchanged_files()
 {
    let fixture = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    std::fs::create_dir(root.join("source")).unwrap();
    std::fs::create_dir(root.join("destination")).unwrap();
    std::fs::write(root.join("source/unchanged"), [1; 17]).unwrap();
    std::fs::write(root.join("growth"), [1; 10]).unwrap();
    std::fs::write(root.join("deleted"), [1; 12]).unwrap();
    std::fs::write(root.join("rename"), [1; 9]).unwrap();
    let index = baseline(&root, Vec::new());
    let old_total = index.entries[0].logical;
    let deleted = record(&root.join("deleted"), 100);
    std::fs::remove_file(root.join("deleted")).unwrap();
    std::fs::write(root.join("growth"), [2; 42]).unwrap();
    std::fs::rename(root.join("rename"), root.join("destination/renamed")).unwrap();
    std::fs::rename(root.join("source"), root.join("destination/moved")).unwrap();
    std::fs::write(root.join("destination/new"), [2; 23]).unwrap();
    let records = vec![
        deleted,
        record(&root.join("growth"), 101),
        record(&root.join("destination/renamed"), 102),
        record(&root.join("destination/moved"), 103),
        record(&root.join("destination/new"), 104),
    ];
    let handles = current_handles(&root);
    let unchanged = windows::identity(&root.join("destination/moved/unchanged"))
        .unwrap()
        .key
        .1;
    let mut opened = Vec::new();
    let result = replay(
        &index,
        changes(&index, records),
        |id| {
            opened.push(id);
            open(&handles, id)
        },
        1000,
        &AtomicBool::new(false),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(opened.len(), 5);
    assert!(!opened.contains(&unchanged));
    assert_eq!(index.entries[0].logical, old_total);
    assert_eq!(index.ntfs_checkpoint.unwrap().next, 100);
    assert_eq!(result.ntfs_checkpoint.unwrap().next, 200);
    parity(&result);
}

#[test]
fn journal_replay_preserves_hard_link_allocation_when_a_directory_moves_into_exclusions() {
    let fixture = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    std::fs::create_dir(root.join("visible")).unwrap();
    std::fs::create_dir(root.join("excluded")).unwrap();
    std::fs::write(root.join("visible/original"), [1; 123]).unwrap();
    std::fs::hard_link(root.join("visible/original"), root.join("alias")).unwrap();
    let index = baseline(
        &root,
        vec![root.join("excluded").to_string_lossy().into_owned()],
    );
    std::fs::rename(root.join("visible"), root.join("excluded/moved")).unwrap();
    let handles = current_handles(&root);
    let result = replay(
        &index,
        changes(&index, vec![record(&root.join("excluded/moved"), 100)]),
        |id| open(&handles, id),
        1000,
        &AtomicBool::new(false),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(result.entries[0].logical, 123);
    assert_eq!(result.entries[0].files, 1);
    assert_eq!(result.entries[0].allocated, index.entries[0].allocated);
    parity(&result);
}

#[test]
fn journal_replay_does_not_publish_partial_cancelled_changed_or_unknown_subtree_results() {
    let fixture = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    std::fs::write(root.join("file"), [1; 20]).unwrap();
    let index = baseline(&root, Vec::new());
    let handles = current_handles(&root);
    let cancel = AtomicBool::new(true);
    assert!(
        replay(
            &index,
            changes(&index, Vec::new()),
            |id| open(&handles, id),
            1000,
            &cancel,
            |_| Ok(())
        )
        .is_err()
    );
    cancel.store(false, Ordering::Relaxed);
    let mut gap = changes(&index, Vec::new());
    gap.state = ReadState::Limited;
    assert!(
        replay(
            &index,
            gap,
            |id| open(&handles, id),
            1000,
            &cancel,
            |_| Ok(())
        )
        .is_err()
    );
    let mut reset = changes(&index, Vec::new());
    reset.state = ReadState::Complete(Checkpoint {
        journal: 8,
        ..index.ntfs_checkpoint.unwrap()
    });
    assert!(
        replay(
            &index,
            reset,
            |id| open(&handles, id),
            1000,
            &cancel,
            |_| Ok(())
        )
        .is_err()
    );
    assert!(
        replay(
            &index,
            changes(&index, Vec::new()),
            |id| open(&handles, id),
            1000,
            &cancel,
            |_| Err(Error::Invalid("journal advanced".into()))
        )
        .is_err()
    );
    assert!(
        replay(
            &index,
            changes(&index, Vec::new()),
            |id| open(&handles, id),
            1,
            &cancel,
            |_| Ok(())
        )
        .is_err()
    );
    std::fs::create_dir(root.join("new-folder")).unwrap();
    let handles = current_handles(&root);
    assert!(
        replay(
            &index,
            changes(&index, vec![record(&root.join("new-folder"), 100)]),
            |id| open(&handles, id),
            1000,
            &cancel,
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(index.entries[0].logical, 20);
    assert_eq!(index.ntfs_checkpoint.unwrap().next, 100);
}

#[test]
fn deleting_a_directory_prunes_its_subtree_and_missing_checkpoints_rescan_real_files() {
    let fixture = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    std::fs::create_dir(root.join("removed")).unwrap();
    std::fs::write(root.join("removed/child"), [1; 31]).unwrap();
    std::fs::write(root.join("remaining"), [1; 17]).unwrap();
    let index = baseline(&root, Vec::new());
    let deleted = record(&root.join("removed"), 100);
    std::fs::remove_file(root.join("removed/child")).unwrap();
    std::fs::remove_dir(root.join("removed")).unwrap();
    let handles = current_handles(&root);
    let result = replay(
        &index,
        changes(&index, vec![deleted]),
        |id| open(&handles, id),
        1000,
        &AtomicBool::new(false),
        |_| Ok(()),
    )
    .unwrap();
    parity(&result);
    assert_eq!(result.entries[0].logical, 17);
    let previous = Arc::new(RwLock::new(result));
    previous.write().unwrap().ntfs_checkpoint = None;
    std::fs::write(root.join("remaining"), [2; 44]).unwrap();
    let mut published = None;
    let refreshed = voldiq_core::scanner::refresh_or_scan(
        &root,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
        |value| published = Some(value),
        Some(&previous),
    )
    .unwrap();
    assert!(Arc::ptr_eq(&published.unwrap(), &refreshed));
    assert!(!Arc::ptr_eq(&previous, &refreshed));
    assert_eq!(refreshed.read().unwrap().entries[0].logical, 44);
    assert_eq!(previous.read().unwrap().entries[0].logical, 17);
    previous.write().unwrap().ntfs_checkpoint = index.ntfs_checkpoint;
    std::fs::write(root.join("remaining"), [3; 60]).unwrap();
    let mut methods = Vec::new();
    let fallback = voldiq_core::scanner::refresh_or_scan(
        &root,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |update| methods.push(update.method),
        |_| {},
        Some(&previous),
    )
    .unwrap();
    assert_eq!(methods.first().unwrap(), "USN Journal");
    assert!(
        methods
            .iter()
            .any(|method| method == "Directory enumeration")
    );
    assert_eq!(fallback.read().unwrap().entries[0].logical, 60);
    assert_eq!(previous.read().unwrap().entries[0].logical, 17);
    let cancel = AtomicBool::new(false);
    let mut published = false;
    let mut terminal = None;
    let retained = voldiq_core::scanner::refresh_or_scan(
        &root,
        &ScanOptions::default(),
        &cancel,
        |update| {
            if update.phase == voldiq_core::ScanPhase::Scanning {
                cancel.store(true, Ordering::Relaxed);
            } else {
                terminal = Some(update);
            }
        },
        |_| published = true,
        Some(&previous),
    )
    .unwrap();
    assert!(Arc::ptr_eq(&previous, &retained));
    assert!(!published);
    let terminal = terminal.unwrap();
    assert_eq!(terminal.phase, voldiq_core::ScanPhase::Cancelled);
    assert!(terminal.previous);
    assert_eq!(terminal.method, "USN Journal");
    assert_eq!(terminal.logical, 17);
}
