use std::sync::{Arc, RwLock, atomic::AtomicBool};
use voldiq_core::{ScanMode, ScanOptions, ntfs::Checkpoint, scanner};

#[test]
fn directory_mode_builds_fresh_rows_without_replaying_an_existing_checkpoint() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("old"), b"old").unwrap();
    let options = ScanOptions {
        mode: ScanMode::Directory,
        ..ScanOptions::default()
    };
    let mut previous =
        scanner::scan(fixture.path(), &options, &AtomicBool::new(false), |_| {}).unwrap();
    previous.ntfs_checkpoint = Some(Checkpoint {
        volume: previous.root_identity.unwrap().0,
        journal: u64::MAX,
        next: i64::MAX,
    });
    let previous = Arc::new(RwLock::new(previous));
    std::fs::remove_file(fixture.path().join("old")).unwrap();
    std::fs::write(fixture.path().join("new"), b"measured content").unwrap();
    let mut methods = Vec::new();
    let mut published = None;
    let current = scanner::refresh_or_scan(
        fixture.path(),
        &options,
        &AtomicBool::new(false),
        |update| methods.push(update.method),
        |index| published = Some(index),
        Some(&previous),
    )
    .unwrap();
    assert!(!Arc::ptr_eq(&current, &previous));
    assert!(Arc::ptr_eq(&current, &published.unwrap()));
    assert!(
        !methods.is_empty()
            && methods
                .iter()
                .all(|method| method == "Directory enumeration")
    );
    let current = current.read().unwrap();
    assert_eq!(current.ntfs_checkpoint, None);
    assert_eq!(current.entries[0].logical, 16);
    assert!(current.entries.iter().any(|entry| entry.name == "new"));
    assert!(!current.entries.iter().any(|entry| entry.name == "old"));
    assert!(
        previous
            .read()
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.name == "old")
    );
}

#[test]
fn directory_mode_keeps_exclusions_limits_and_cancellation() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("excluded")).unwrap();
    std::fs::write(fixture.path().join("excluded/hidden"), b"hidden").unwrap();
    std::fs::write(fixture.path().join("visible"), b"visible").unwrap();
    let mut options = ScanOptions {
        mode: ScanMode::Directory,
        exclusions: vec![
            fixture
                .path()
                .join("excluded")
                .to_string_lossy()
                .into_owned(),
        ],
        ..ScanOptions::default()
    };
    let index = scanner::scan(fixture.path(), &options, &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!(index.entries[0].logical, 7);
    options.max_entries = Some(1);
    let limited = scanner::scan(fixture.path(), &options, &AtomicBool::new(false), |_| {}).unwrap();
    assert!(limited.entries[0].incomplete && limited.entries.len() == 1);
    options.max_entries = None;
    let cancelled =
        scanner::scan(fixture.path(), &options, &AtomicBool::new(true), |_| {}).unwrap();
    assert!(cancelled.entries[0].incomplete && cancelled.entries.len() == 1);
}

#[cfg(windows)]
#[test]
fn directory_mode_never_resumes_a_saved_journal_or_reads_its_root() {
    let fixture = tempfile::tempdir().unwrap();
    let store = voldiq_core::snapshots::Store::open(&fixture.path().join("snapshots.db")).unwrap();
    let options = ScanOptions {
        mode: ScanMode::Directory,
        ..ScanOptions::default()
    };
    assert!(
        store
            .resume_latest(
                &fixture.path().join("nonexistent"),
                &options,
                &AtomicBool::new(false)
            )
            .unwrap()
            .is_none()
    );
}
