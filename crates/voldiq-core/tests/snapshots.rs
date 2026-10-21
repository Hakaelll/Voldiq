use std::{path::Path, sync::atomic::AtomicBool};
use voldiq_core::{
    ScanOptions,
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
fn history_survives_restart_and_compares_actual_directory_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let db = database.path().join("history.db");
    std::fs::create_dir(fixture.path().join("nested")).unwrap();
    std::fs::write(fixture.path().join("nested/growth"), [1; 12]).unwrap();
    std::fs::write(fixture.path().join("removed"), [2; 7]).unwrap();
    let first = capture(fixture.path());
    let mut store = Store::open(&db).unwrap();
    let before = store.save(&first, &AtomicBool::new(false), &[]).unwrap();
    std::fs::write(fixture.path().join("nested/growth"), [3; 33]).unwrap();
    std::fs::remove_file(fixture.path().join("removed")).unwrap();
    std::fs::write(fixture.path().join("added"), [4; 14]).unwrap();
    let second = capture(fixture.path());
    let after = store.save(&second, &AtomicBool::new(false), &[]).unwrap();
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
