use std::sync::atomic::AtomicBool;
use voldiq_core::{ScanOptions, ntfs::Checkpoint, scanner::scan, snapshots::Store};

#[test]
fn journal_checkpoint_survives_restart_and_is_bound_to_its_capture() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let db = database.path().join("history.db");
    std::fs::write(fixture.path().join("file"), [1; 20]).unwrap();
    let mut index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let checkpoint = Checkpoint {
        volume: index.root_identity.unwrap().0,
        journal: u64::MAX,
        next: i64::MAX,
    };
    let mut store = Store::open(&db).unwrap();
    let conventional = store.save(&index, &AtomicBool::new(false), &[]).unwrap();
    assert_eq!(store.checkpoint(conventional).unwrap(), None);
    index.ntfs_checkpoint = Some(checkpoint);
    let id = store.save(&index, &AtomicBool::new(false), &[]).unwrap();
    drop(store);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(store.checkpoint(id).unwrap(), Some(checkpoint));
    let historical = store.load(id).unwrap();
    assert_eq!(historical.snapshot_id, Some(id));
    assert_eq!(historical.ntfs_checkpoint, None);
    assert!(store.save(&index, &AtomicBool::new(true), &[]).is_err());
    assert_eq!(store.list(0, 100).unwrap().len(), 2);
    store.delete(id).unwrap();
    assert_eq!(store.checkpoint(id).unwrap(), None);
    let connection = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM snapshot_journals", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn invalid_checkpoint_evidence_cannot_be_committed_or_loaded() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let db = database.path().join("history.db");
    let mut index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let mut store = Store::open(&db).unwrap();
    let checkpoint = Checkpoint {
        volume: index.root_identity.unwrap().0,
        journal: 42,
        next: 100,
    };
    index.ntfs_checkpoint = Some(Checkpoint {
        volume: checkpoint.volume ^ 1,
        ..checkpoint
    });
    assert!(store.save(&index, &AtomicBool::new(false), &[]).is_err());
    index.ntfs_checkpoint = Some(Checkpoint {
        next: -1,
        ..checkpoint
    });
    assert!(store.save(&index, &AtomicBool::new(false), &[]).is_err());
    index.ntfs_checkpoint = Some(checkpoint);
    index.entries[0].incomplete = true;
    assert!(store.save(&index, &AtomicBool::new(false), &[]).is_err());
    index.entries[0].incomplete = false;
    index.error_count = 1;
    assert!(store.save(&index, &AtomicBool::new(false), &[]).is_err());
    index.error_count = 0;
    index.snapshot_id = Some(1);
    assert!(store.save(&index, &AtomicBool::new(false), &[]).is_err());
    index.snapshot_id = None;
    assert!(store.list(0, 100).unwrap().is_empty());
    let id = store.save(&index, &AtomicBool::new(false), &[]).unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE snapshot_journals SET volume=?1 WHERE snapshot_id=?2",
            rusqlite::params![(checkpoint.volume ^ 1).to_le_bytes().as_slice(), id],
        )
        .unwrap();
    assert!(store.checkpoint(id).is_err());
}

#[test]
fn version_one_history_migrates_without_losing_captures() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let db = database.path().join("history.db");
    std::fs::write(fixture.path().join("file"), [1; 11]).unwrap();
    let index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let mut store = Store::open(&db).unwrap();
    let id = store.save(&index, &AtomicBool::new(false), &[]).unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch("ALTER TABLE snapshot_versions DROP COLUMN native_state; DROP TABLE snapshot_journals; DROP TABLE snapshot_caches; PRAGMA user_version=1;")
        .unwrap();
    drop(connection);
    let store = Store::open(&db).unwrap();
    assert_eq!(store.load(id).unwrap().entries[0].logical, 11);
    assert_eq!(store.checkpoint(id).unwrap(), None);
    assert_eq!(store.list(0, 10).unwrap().len(), 1);
    drop(store);
    let connection = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
}

#[test]
fn native_identity_replacement_creates_a_new_version_even_with_matching_size_and_modified_time() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let db = database.path().join("history.db");
    let file = fixture.path().join("file");
    std::fs::write(&file, [1; 20]).unwrap();
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    let before = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let mut store = Store::open(&db).unwrap();
    let first = store.save(&before, &AtomicBool::new(false), &[]).unwrap();
    let replacement = fixture.path().join("replacement");
    std::fs::write(&replacement, [2; 20]).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    std::fs::remove_file(&file).unwrap();
    std::fs::rename(&replacement, &file).unwrap();
    let after = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_ne!(
        before.entries[1].file_state.unwrap().identity,
        after.entries[1].file_state.unwrap().identity
    );
    let second = store.save(&after, &AtomicBool::new(false), &[]).unwrap();
    let comparison = store.compare(first, second, 0, 100).unwrap();
    assert_eq!(comparison.delta, 0);
    assert!(
        comparison
            .changes
            .iter()
            .any(|change| change.path.ends_with("file")
                && change.kind == "changed"
                && change.delta == 0)
    );
    assert!(
        store
            .load(first)
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.file_state.is_none())
    );
}
