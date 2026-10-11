#![cfg(windows)]
use std::{
    collections::HashMap,
    fs::File,
    os::windows::ffi::OsStrExt,
    path::Path,
    sync::{Arc, RwLock, atomic::AtomicBool},
};
use voldiq_core::{
    ScanOptions,
    index::Index,
    ntfs::{Record, populate},
    scanner::scan,
    windows,
};

fn metadata_handle(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::*;
    std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

fn fresh(root: &Path, exclusions: Vec<String>) -> Arc<RwLock<Index>> {
    let mut index = scan(
        root,
        &ScanOptions {
            exclusions,
            max_entries: Some(1),
        },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    index.issues.clear();
    index.error_count = 0;
    index.entries[0].incomplete = false;
    Arc::new(RwLock::new(index))
}

fn record(path: &Path) -> Record {
    let identity = windows::identity(path).unwrap();
    Record {
        reference: identity.key.1,
        parent: windows::identity(path.parent().unwrap()).unwrap().key.1,
        usn: 0,
        reason: 0,
        attributes: if path.is_dir() { 0x10 } else { 0 },
        name: path.file_name().unwrap().encode_wide().collect(),
    }
}

#[test]
fn native_metadata_matches_directory_scan_with_excluded_primary_hard_link() {
    let fixture = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    let hidden = root.join("excluded");
    let included = root.join("日本語");
    std::fs::create_dir(&hidden).unwrap();
    std::fs::create_dir(&included).unwrap();
    let original = hidden.join("original.bin");
    let alias = included.join("alias.bin");
    std::fs::write(&original, [3; 128]).unwrap();
    std::fs::hard_link(&original, &alias).unwrap();
    let other = root.join("other.bin");
    std::fs::write(&other, [2; 17]).unwrap();
    let paths = [&hidden, &included, &original, &other];
    let handles: HashMap<_, _> = paths
        .iter()
        .map(|path| (windows::identity(path).unwrap().key.1, path.to_path_buf()))
        .collect();
    let exclusions = vec![hidden.to_string_lossy().into_owned()];
    let shared = fresh(&root, exclusions.clone());
    populate(
        &shared,
        paths.iter().rev().map(|path| record(path)).collect(),
        |id| Ok(metadata_handle(&handles[&id])?),
        100,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let conventional = scan(
        &root,
        &ScanOptions {
            exclusions,
            max_entries: None,
        },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let native = shared.read().unwrap();
    assert_eq!(native.entries[0].logical, 145);
    assert_eq!(native.entries[0].files, 2);
    assert_eq!(
        native.entries[0].allocated,
        conventional.entries[0].allocated
    );
    assert_eq!(native.entries.len(), conventional.entries.len());
    assert_eq!(native.error_count, 0);
    assert!(!native.entries[0].incomplete);
    assert!(
        native
            .entries
            .iter()
            .any(|entry| native.path(entry.id).unwrap() == alias)
    );
}

#[test]
fn malformed_names_fail_before_publication_and_limits_cancel_remain_partial() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("a");
    std::fs::write(&path, [1; 32]).unwrap();
    let mut invalid = record(&path);
    invalid.name = "../outside".encode_utf16().collect();
    let shared = fresh(fixture.path(), vec![]);
    assert!(
        populate(
            &shared,
            vec![invalid],
            |_| unreachable!(),
            100,
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err()
    );
    assert_eq!(shared.read().unwrap().entries.len(), 1);
    populate(
        &shared,
        vec![record(&path)],
        |_| Ok(File::open(&path)?),
        1,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert!(shared.read().unwrap().entries[0].incomplete);
    let shared = fresh(fixture.path(), vec![]);
    populate(
        &shared,
        vec![record(&path)],
        |_| unreachable!(),
        100,
        &AtomicBool::new(true),
        |_| {},
    )
    .unwrap();
    assert!(shared.read().unwrap().entries[0].incomplete);
    assert_eq!(shared.read().unwrap().entries.len(), 1);
}

#[test]
fn all_aliases_count_logical_bytes_but_share_allocation() {
    let fixture = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    let first = root.join("first");
    let second = root.join("second");
    std::fs::write(&first, [1; 128]).unwrap();
    std::fs::hard_link(&first, &second).unwrap();
    let shared = fresh(&root, vec![]);
    populate(
        &shared,
        vec![record(&first)],
        |_| Ok(metadata_handle(&first)?),
        100,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let index = shared.read().unwrap();
    assert_eq!(index.entries[0].files, 2);
    assert_eq!(index.entries[0].logical, 256);
    assert_eq!(
        index.entries[0].allocated,
        windows::identity(&first).unwrap().allocated
    );
    assert_eq!(index.entries.len(), 3);
    assert_eq!(index.error_count, 0);
}

#[test]
fn unresolved_parents_and_changed_identities_are_reported() {
    let fixture = tempfile::tempdir().unwrap();
    let first = fixture.path().join("first");
    let other = fixture.path().join("other");
    std::fs::write(&first, [1; 128]).unwrap();
    std::fs::write(&other, [1; 128]).unwrap();
    let shared = fresh(fixture.path(), vec![]);
    populate(
        &shared,
        vec![record(&first)],
        |_| Ok(metadata_handle(&other)?),
        100,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let index = shared.read().unwrap();
    assert_eq!(index.entries[0].files, 0);
    assert_eq!(index.error_count, 1);
    assert!(index.entries[0].incomplete);
    drop(index);
    let mut orphan = record(&first);
    orphan.parent = u64::MAX;
    let shared = fresh(fixture.path(), vec![]);
    populate(
        &shared,
        vec![orphan],
        |_| Ok(metadata_handle(&first)?),
        100,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let index = shared.read().unwrap();
    assert_eq!(index.entries[0].files, 0);
    assert_eq!(index.error_count, 1);
    assert!(index.entries[0].incomplete);
}

#[test]
fn bounded_volume_scan_keeps_conventional_fallback_available() {
    let volume = windows::volumes()
        .into_iter()
        .find(|volume| {
            volume.filesystem == "NTFS" && volume.accessible && volume.drive_type == "Local"
        })
        .unwrap();
    let mut method = String::new();
    let index = scan(
        Path::new(&volume.path),
        &ScanOptions {
            exclusions: vec![],
            max_entries: Some(1),
        },
        &AtomicBool::new(false),
        |update| method = update.method,
    )
    .unwrap();
    assert_eq!(method, "Directory enumeration");
    assert_eq!(index.entries.len(), 1);
    assert!(index.entries[0].incomplete);
}
