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
    let loading = AtomicBool::new(false);
    std::fs::create_dir(fixture.path().join("nested")).unwrap();
    std::fs::write(fixture.path().join("nested/growth"), [1; 12]).unwrap();
    std::fs::write(fixture.path().join("removed"), [2; 7]).unwrap();
    std::fs::write(fixture.path().join("UPPER.txt"), [5; 7]).unwrap();
    std::fs::write(fixture.path().join("Résumé.txt"), [6; 7]).unwrap();
    let first = capture(fixture.path());
    let mut store = Store::open(&db).unwrap();
    let before = store.save(&first, &AtomicBool::new(false), &[]).unwrap();
    std::fs::write(fixture.path().join("nested/growth"), [3; 33]).unwrap();
    std::fs::remove_file(fixture.path().join("removed")).unwrap();
    std::fs::write(fixture.path().join("added"), [4; 14]).unwrap();
    let second = capture(fixture.path());
    let after = store.save(&second, &AtomicBool::new(false), &[]).unwrap();
    assert!(store.save(&second, &AtomicBool::new(true), &[]).is_err());
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
    drop(store);
    let store = Store::open(&db).unwrap();
    let history = store.list(0, 10).unwrap();
    assert_eq!(history.len(), 2);
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        store.load(-1, &cancelled).err().unwrap().to_string(),
        "Snapshot loading cancelled."
    );
    assert_eq!(
        store.load(before, &cancelled).err().unwrap().to_string(),
        "Snapshot loading cancelled."
    );
    assert_eq!(history[0].id, after);
    let loaded = store.load(before, &loading).unwrap();
    assert_eq!(loaded.entries[0].logical, 33);
    assert_eq!(
        loaded
            .query(&Query {
                files_only: true,
                ..Query::default()
            })
            .unwrap()
            .total,
        4
    );
    for (text, expected) in [
        ("GROWTH", vec!["growth"]),
        ("upper", vec!["UPPER.txt"]),
        ("RÉSUMÉ", vec!["Résumé.txt"]),
        ("absent", vec![]),
    ] {
        let query = Query {
            text: text.into(),
            files_only: true,
            ..Query::default()
        };
        for index in [&first, &loaded] {
            let page = index.query(&query).unwrap();
            let cancellable = index.query_cancellable(&query, || false).unwrap().unwrap();
            assert_eq!(page.total, expected.len());
            assert_eq!(cancellable.total, expected.len());
            for result in [page, cancellable] {
                assert_eq!(
                    result
                        .items
                        .iter()
                        .map(|entry| entry.name.as_str())
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
    }
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
    assert_eq!(store.load(after, &loading).unwrap().entries[0].logical, 61);
    drop(store);
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch("ALTER TABLE snapshot_versions DROP COLUMN cloud; DROP TABLE duplicate_hashes; PRAGMA user_version=4;")
        .unwrap();
    drop(connection);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
    assert_eq!(store.load(after, &loading).unwrap().entries[0].logical, 61);
    let duplicates = tempfile::tempdir().unwrap();
    for name in ["first", "second"] {
        std::fs::write(duplicates.path().join(name), [7_u8; 16_384]).unwrap();
    }
    let cancel = AtomicBool::new(false);
    let policy_state = tempfile::tempdir().unwrap();
    let protections = voldiq_core::metadata::Store::open(policy_state.path())
        .unwrap()
        .protections()
        .unwrap();
    let index = std::sync::Arc::new(std::sync::RwLock::new(capture(duplicates.path())));
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let cold = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(cold.errors, 0);
    assert_eq!(cold.groups.len(), 1);
    assert_eq!(cold.groups[0].members.len(), 2);
    assert_eq!(
        cold.groups[0].digest,
        blake3::hash(&[7_u8; 16_384]).to_hex().as_str()
    );
    assert_eq!(cold.bytes_read, 49_152);
    assert_eq!(cold.cache_hits, 0);
    store.save_hash_cache(&cache, &cancel).unwrap();
    drop(cache);
    drop(store);
    let mut store = Store::open(&db).unwrap();
    store
        .save_hash_cache(
            &voldiq_core::duplicates::HashCache::default(),
            &AtomicBool::new(true),
        )
        .unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let warm = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(warm.errors, 0);
    assert_eq!(warm.groups.len(), 1);
    assert_eq!(warm.groups[0].digest, cold.groups[0].digest);
    assert_eq!(warm.groups[0].members.len(), 2);
    assert_eq!(warm.bytes_read, 0);
    assert_eq!(warm.cache_hits, 2);
    drop(cache);
    drop(store);
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE duplicate_hashes SET sample=x'00' WHERE identity=(SELECT min(identity) FROM duplicate_hashes);").unwrap();
    drop(connection);
    let mut store = Store::open(&db).unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let repaired = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(repaired.errors, 0);
    assert_eq!(repaired.groups.len(), 1);
    assert_eq!(repaired.groups[0].digest, cold.groups[0].digest);
    assert_eq!(repaired.bytes_read, 24_576);
    assert_eq!(repaired.cache_hits, 1);
    store.save_hash_cache(&cache, &cancel).unwrap();
    let first_path = duplicates.path().join("first");
    let modified = std::fs::metadata(&first_path).unwrap().modified().unwrap();
    std::fs::write(&first_path, [8_u8; 16_384]).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&first_path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    drop(cache);
    drop(store);
    let store = Store::open(&db).unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let changed = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(changed.errors, 0);
    assert!(changed.groups.is_empty());
    assert_eq!(changed.bytes_read, 8192);
    drop(cache);
    drop(store);
    let mut store = Store::open(&db).unwrap();
    store
        .save_hash_cache(&voldiq_core::duplicates::HashCache::default(), &cancel)
        .unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let cleared = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(cleared.errors, 0);
    assert!(cleared.groups.is_empty());
    assert_eq!(cleared.bytes_read, 16_384);
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
}
