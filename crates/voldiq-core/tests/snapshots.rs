use std::{path::Path, sync::atomic::AtomicBool};
use voldiq_core::{
    NodeId, ScanOptions,
    index::{Index, Query},
    scanner::scan,
    snapshots::Store,
};

fn capture(path: &Path) -> Index {
    scan(
        path,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap()
}

#[test]
fn file_change_totals_cover_all_pages_without_counting_parent_sizes() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("nested")).unwrap();
    let mut store = Store::open(&database.path().join("history.db")).unwrap();
    let before = store
        .save(&capture(fixture.path()), &AtomicBool::new(false))
        .unwrap();
    for i in 1..=60 {
        std::fs::write(fixture.path().join(format!("nested/file-{i}")), vec![42; i]).unwrap();
    }
    std::fs::write(fixture.path().join("nested/empty"), []).unwrap();
    let after = store
        .save(&capture(fixture.path()), &AtomicBool::new(false))
        .unwrap();
    let first = store.compare(before, after, 0, 1).unwrap();
    let last = store.compare(before, after, 60, 1).unwrap();
    let outside = store.compare(before, after, 1000, 1).unwrap();
    assert_eq!(first.changes.len(), 1);
    assert!(outside.changes.is_empty());
    assert_eq!(first.file_changes, last.file_changes);
    assert_eq!(first.file_changes, outside.file_changes);
    assert_eq!(first.file_changes.added.files, 61);
    assert_eq!(first.file_changes.added.logical, 1830);
    assert_eq!(first.file_changes.changed.delta, 0);
    assert_eq!(first.file_changes.changed.files, 0);
    assert_eq!(first.file_changes.removed.files, 0);
    assert_eq!(first.delta, 1830);
}

#[test]
fn file_directory_transitions_count_only_regular_file_bytes() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("to-folder"), [1; 4]).unwrap();
    std::fs::create_dir(fixture.path().join("to-file")).unwrap();
    std::fs::write(fixture.path().join("to-file/old-child"), [1; 5]).unwrap();
    let mut store = Store::open(&database.path().join("history.db")).unwrap();
    let before = store
        .save(&capture(fixture.path()), &AtomicBool::new(false))
        .unwrap();
    std::fs::remove_file(fixture.path().join("to-folder")).unwrap();
    std::fs::create_dir(fixture.path().join("to-folder")).unwrap();
    std::fs::write(fixture.path().join("to-folder/new-child"), [1; 7]).unwrap();
    std::fs::remove_file(fixture.path().join("to-file/old-child")).unwrap();
    std::fs::remove_dir(fixture.path().join("to-file")).unwrap();
    std::fs::write(fixture.path().join("to-file"), [1; 9]).unwrap();
    let after = store
        .save(&capture(fixture.path()), &AtomicBool::new(false))
        .unwrap();
    let comparison = store.compare(before, after, 0, 1).unwrap();
    assert_eq!(comparison.file_changes.added.logical, 7);
    assert_eq!(comparison.file_changes.added.files, 1);
    assert_eq!(comparison.file_changes.removed.logical, 5);
    assert_eq!(comparison.file_changes.removed.files, 1);
    assert_eq!(comparison.file_changes.changed.delta, 5);
    assert_eq!(comparison.file_changes.changed.files, 2);
    assert_eq!(comparison.delta, 7);
}

#[test]
fn history_survives_restart_and_compares_actual_directory_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let db = database.path().join("history.db");
    std::fs::create_dir(fixture.path().join("nested")).unwrap();
    std::fs::write(fixture.path().join("nested/growth"), [1; 12]).unwrap();
    std::fs::write(fixture.path().join("removed"), [2; 7]).unwrap();
    let first = capture(fixture.path());
    let mut store = Store::open(&db).unwrap();
    let before = store.save(&first, &AtomicBool::new(false)).unwrap();
    std::fs::write(fixture.path().join("nested/growth"), [3; 33]).unwrap();
    std::fs::remove_file(fixture.path().join("removed")).unwrap();
    std::fs::write(fixture.path().join("added"), [4; 14]).unwrap();
    let second = capture(fixture.path());
    let after = store.save(&second, &AtomicBool::new(false)).unwrap();
    drop(store);
    let store = Store::open(&db).unwrap();
    let history = store.list(0, 10).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, after);
    let loaded = store.load(before).unwrap();
    assert_eq!(loaded.entries[0].logical, 19);
    assert_eq!(
        loaded
            .query(&Query {
                files_only: true,
                ..Query::default()
            })
            .unwrap()
            .total,
        2
    );
    let comparison = store.compare(before, after, 0, 200).unwrap();
    assert!(comparison.reliable);
    assert_eq!(comparison.delta, 28);
    assert_eq!(comparison.file_changes.added.logical, 14);
    assert_eq!(comparison.file_changes.added.files, 1);
    assert_eq!(comparison.file_changes.changed.delta, 21);
    assert_eq!(comparison.file_changes.changed.files, 1);
    assert_eq!(comparison.file_changes.removed.logical, 7);
    assert_eq!(comparison.file_changes.removed.files, 1);
    assert!(
        comparison
            .changes
            .iter()
            .any(|c| c.path.ends_with("removed") && c.kind == "removed" && c.delta == -7)
    );
    assert!(
        comparison
            .changes
            .iter()
            .any(|c| c.path.ends_with("added") && c.kind == "added" && c.delta == 14)
    );
    assert!(
        comparison
            .changes
            .iter()
            .any(|c| c.path.ends_with("nested") && c.directory && c.delta == 21)
    );
    let page = store.compare(before, after, 1, 1).unwrap();
    assert_eq!(page.total, comparison.total);
    assert_eq!(page.changes.len(), 1);
    assert_eq!(page.file_changes, comparison.file_changes);
    assert_eq!(store.load(after).unwrap().entries[0].logical, 47);
}

#[test]
fn unchanged_captures_share_versions_and_cancelled_capture_rolls_back() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let db = database.path().join("history.db");
    std::fs::write(fixture.path().join("file"), [1; 20]).unwrap();
    let mut index = capture(fixture.path());
    let mut store = Store::open(&db).unwrap();
    let first = store.save(&index, &AtomicBool::new(false)).unwrap();
    let second = store.save(&index, &AtomicBool::new(false)).unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    let versions: i64 = connection
        .query_row("SELECT COUNT(*) FROM snapshot_versions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(versions, index.entries.len() as i64);
    assert_eq!(store.compare(first, second, 0, 20).unwrap().total, 0);
    index.entries[0].logical += 30;
    assert!(store.save(&index, &AtomicBool::new(true)).is_err());
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
    assert_eq!(store.load(second).unwrap().entries[0].logical, 20);
    index.entries[0].logical = u64::MAX;
    assert!(store.save(&index, &AtomicBool::new(false)).is_err());
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
}

#[test]
fn deleting_captures_preserves_remaining_history_and_future_capture() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("file"), [1; 10]).unwrap();
    let mut store = Store::open(&database.path().join("history.db")).unwrap();
    let mut ids = Vec::new();
    for size in [10, 20, 30] {
        std::fs::write(fixture.path().join("file"), vec![1; size]).unwrap();
        ids.push(
            store
                .save(&capture(fixture.path()), &AtomicBool::new(false))
                .unwrap(),
        );
    }
    store.delete(ids[1]).unwrap();
    assert_eq!(store.compare(ids[0], ids[2], 0, 20).unwrap().delta, 20);
    store.delete(ids[2]).unwrap();
    assert_eq!(store.load(ids[0]).unwrap().entries[0].logical, 10);
    let fourth = store
        .save(&capture(fixture.path()), &AtomicBool::new(false))
        .unwrap();
    assert!(fourth > ids[2]);
    assert_eq!(store.load(fourth).unwrap().entries[0].logical, 30);
    assert_eq!(store.compare(ids[0], fourth, 0, 20).unwrap().delta, 20);
    store.delete(ids[0]).unwrap();
    store.compact().unwrap();
    assert!(store.disk_bytes().unwrap() > 0);
    assert_eq!(store.load(fourth).unwrap().entries[0].logical, 30);
    store.delete(fourth).unwrap();
    assert!(store.list(0, 10).unwrap().is_empty());
}

#[test]
fn incomplete_capture_does_not_assert_deletions_and_coverage_must_match() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("excluded")).unwrap();
    std::fs::write(fixture.path().join("file"), [1; 10]).unwrap();
    let full = capture(fixture.path());
    let mut store = Store::open(&database.path().join("history.db")).unwrap();
    let first = store.save(&full, &AtomicBool::new(false)).unwrap();
    let partial = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(true),
        |_| {},
    )
    .unwrap();
    let second = store.save(&partial, &AtomicBool::new(false)).unwrap();
    let comparison = store.compare(first, second, 0, 20).unwrap();
    assert!(!comparison.reliable);
    assert_eq!(comparison.file_changes.removed.files, 0);
    assert_eq!(comparison.file_changes.unobserved.files, 1);
    assert_eq!(comparison.file_changes.unobserved.logical, 10);
    let reverse = store.compare(second, first, 0, 20).unwrap();
    assert_eq!(reverse.file_changes.added.files, 0);
    assert_eq!(reverse.file_changes.newly_observed.files, 1);
    assert_eq!(reverse.file_changes.newly_observed.logical, 10);
    assert!(comparison.changes.iter().any(|c| c.kind == "unobserved"));
    assert!(!comparison.changes.iter().any(|c| c.kind == "removed"));
    assert!(
        store
            .compare(second, first, 0, 20)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.kind == "newly_observed")
    );
    let excluded = scan(
        fixture.path(),
        &ScanOptions {
            exclusions: vec![
                fixture
                    .path()
                    .join("excluded")
                    .to_string_lossy()
                    .into_owned(),
            ],
            ..ScanOptions::default()
        },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let third = store.save(&excluded, &AtomicBool::new(false)).unwrap();
    assert!(store.compare(first, third, 0, 20).is_err());
    assert_eq!(store.load(third).unwrap().exclusions, excluded.exclusions);
}

#[cfg(windows)]
#[test]
fn native_utf16_names_survive_persistence_and_invalid_components_are_rejected() {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let native = OsString::from_wide(&[0x0061, 0xd800, 0x0062]);
    let original = fixture.path().join(&native);
    std::fs::write(&original, [1; 10]).unwrap();
    let index = capture(fixture.path());
    let db = database.path().join("history.db");
    let mut store = Store::open(&db).unwrap();
    let id = store.save(&index, &AtomicBool::new(false)).unwrap();
    drop(store);
    let store = Store::open(&db).unwrap();
    let loaded = store.load(id).unwrap();
    assert_eq!(
        loaded.path(NodeId(1)).unwrap(),
        std::fs::canonicalize(original).unwrap()
    );
    assert!(!loaded.details(NodeId(1)).unwrap().path_exact);
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE snapshot_paths SET name=?1 WHERE parent_id!=0",
            [vec![46_u8, 0, 46, 0]],
        )
        .unwrap();
    assert!(store.load(id).is_err());
}
