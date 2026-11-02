use std::sync::atomic::AtomicBool;
use voldiq_core::{
    Kind, NodeId,
    cloud::{CloudFilter, CloudState},
    index::Query,
    scanner,
    snapshots::Store,
};

#[test]
fn cloud_metadata_queries_accounting_and_snapshots_preserve_observation_scope() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let evidence = [
        ("cloud.bin", CloudState::FileRecallRequired, Kind::Link, 120),
        ("cached.txt", CloudState::FileNoRecallFlag, Kind::Link, 200),
        ("noext", CloudState::FileNoRecallFlag, Kind::Link, 0),
        (
            "pending.folder",
            CloudState::DirectoryRecallRequired,
            Kind::Link,
            0,
        ),
        (
            "ready.folder",
            CloudState::DirectoryNoRecallFlag,
            Kind::Link,
            0,
        ),
        ("local.txt", CloudState::NotPlaceholder, Kind::File, 10),
        ("legacy.dat", CloudState::Unknown, Kind::File, 20),
    ];
    for (name, state, ..) in evidence {
        if state.is_directory() {
            std::fs::create_dir(fixture.path().join(name)).unwrap();
        } else {
            std::fs::write(fixture.path().join(name), b"owned").unwrap();
        }
    }
    let cancel = AtomicBool::new(false);
    let mut index = scanner::scan(fixture.path(), &Default::default(), &cancel, |_| {}).unwrap();
    // Synthetic observations isolate metadata behavior from provider installation and hydration.
    for entry in index.entries.iter_mut().skip(1) {
        let (_, cloud, kind, logical) = evidence
            .iter()
            .find(|(name, ..)| *name == entry.name)
            .unwrap();
        entry.cloud = *cloud;
        entry.kind = *kind;
        entry.logical = *logical;
        entry.files = u64::from(entry.is_metadata_file());
        if *kind == Kind::Link {
            entry.incomplete = true;
            entry.allocated = 0;
            entry.allocation_known = false;
        }
    }
    let children = index.entries.iter().skip(1).cloned().collect::<Vec<_>>();
    index.entries[0].logical = 0;
    index.entries[0].allocated = 0;
    index.entries[0].files = 0;
    for child in &children {
        index.accumulate(child);
    }
    assert_eq!(index.entries[0].logical, 350);
    assert_eq!(index.entries[0].files, 5);
    assert!(index.entries[0].incomplete);
    assert!(!index.entries[0].allocation_known);
    let base = Query {
        parent: Some(NodeId(0)),
        files_only: true,
        ..Default::default()
    };
    let names = |query: &Query| {
        index
            .query(query)
            .unwrap()
            .items
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&base),
        [
            "cached.txt",
            "cloud.bin",
            "legacy.dat",
            "local.txt",
            "noext"
        ]
    );
    assert_eq!(
        names(&Query {
            cloud: Some(CloudFilter::Placeholder),
            ..base.clone()
        }),
        ["cached.txt", "cloud.bin", "noext"]
    );
    assert_eq!(
        names(&Query {
            cloud: Some(CloudFilter::RecallRequired),
            ..base.clone()
        }),
        ["cloud.bin"]
    );
    assert_eq!(
        names(&Query {
            cloud: Some(CloudFilter::Unknown),
            ..base.clone()
        }),
        ["legacy.dat"]
    );
    assert_eq!(
        names(&Query {
            cloud: Some(CloudFilter::NotPlaceholder),
            ..base.clone()
        }),
        ["local.txt"]
    );
    assert_eq!(
        names(&Query {
            cloud: Some(CloudFilter::NoRecallFlag),
            without_extension: true,
            ..base.clone()
        }),
        ["noext"]
    );
    let dirs = names(&Query {
        files_only: false,
        kind: Some(Kind::Directory),
        cloud: Some(CloudFilter::Placeholder),
        sort: "name".into(),
        ..base.clone()
    });
    assert_eq!(dirs, ["pending.folder", "ready.folder"]);
    assert_eq!(
        index
            .query(&Query {
                limit: 1,
                offset: 1,
                cloud: Some(CloudFilter::Placeholder),
                ..base.clone()
            })
            .unwrap()
            .items[0]
            .name,
        "cloud.bin"
    );
    assert!(index.query_cancellable(&base, || true).unwrap().is_none());
    let types = index.file_types(NodeId(0)).unwrap();
    assert_eq!((types.total_bytes, types.total_files), (350, 5));
    assert_eq!(
        types
            .items
            .iter()
            .find(|item| item.extension.as_deref() == Some("txt"))
            .map(|item| (item.bytes, item.count)),
        Some((210, 2))
    );
    let cloud_id = index
        .entries
        .iter()
        .find(|entry| entry.name == "cloud.bin")
        .unwrap()
        .id;
    let problem = voldiq_core::cleanup::Plan::manual(&index, &[cloud_id])
        .err()
        .unwrap()
        .to_string();
    assert!(problem.contains("regular files"), "{problem}");
    let path = database.path().join("snapshots.db");
    let mut store = Store::open(&path).unwrap();
    let first = store.save(&index, &cancel, &[]).unwrap();
    index.entries[cloud_id.0 as usize].cloud = CloudState::FileNoRecallFlag;
    let second = store.save(&index, &cancel, &[]).unwrap();
    let comparison = store.compare(first, second, 0, 100).unwrap();
    assert_eq!(comparison.delta, 0);
    assert!(
        comparison
            .changes
            .iter()
            .any(|change| change.path.ends_with("cloud.bin")
                && change.kind == "changed"
                && change.delta == 0)
    );
    drop(store);
    let store = Store::open(&path).unwrap();
    let loaded = store.load(first, &cancel).unwrap();
    for (name, state, kind, logical) in evidence {
        let entry = loaded
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap();
        assert_eq!(
            (entry.cloud, entry.kind, entry.logical),
            (state, kind, logical)
        );
    }
    assert_eq!(
        store
            .load(second, &cancel)
            .unwrap()
            .entries
            .iter()
            .find(|entry| entry.name == "cloud.bin")
            .unwrap()
            .cloud,
        CloudState::FileNoRecallFlag
    );
}

#[test]
fn legacy_snapshots_migrate_to_unknown_and_invalid_cloud_evidence_is_rejected() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("local"), b"owned").unwrap();
    let cancel = AtomicBool::new(false);
    let index = scanner::scan(fixture.path(), &Default::default(), &cancel, |_| {}).unwrap();
    let path = database.path().join("legacy.db");
    let mut store = Store::open(&path).unwrap();
    let snapshot = store.save(&index, &cancel, &[]).unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch("DROP INDEX snapshots_observed; ALTER TABLE snapshots DROP COLUMN observed_at; ALTER TABLE snapshots DROP COLUMN observed_complete; ALTER TABLE snapshots DROP COLUMN engine_version; ALTER TABLE snapshots DROP COLUMN accounting_version; ALTER TABLE snapshot_versions DROP COLUMN cloud; PRAGMA user_version=5;")
        .unwrap();
    drop(connection);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Store::open(&path).unwrap();
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert!(
        store
            .load(snapshot, &cancel)
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.cloud == CloudState::Unknown)
    );
    let mut store = store;
    let observed = store.save(&index, &cancel, &[]).unwrap();
    assert_eq!(store.compare(snapshot, observed, 0, 100).unwrap().total, 0);
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert!(
        connection
            .execute("UPDATE snapshot_versions SET cloud=2 WHERE kind=1", [])
            .is_err()
    );
    connection
        .execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE snapshot_versions SET cloud=99;")
        .unwrap();
    drop(connection);
    let store = Store::open(&path).unwrap();
    assert!(
        store
            .load(snapshot, &cancel)
            .err()
            .unwrap()
            .to_string()
            .contains("cloud evidence")
    );
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA user_version=8;").unwrap();
    drop(connection);
    assert!(
        Store::open(&path)
            .err()
            .unwrap()
            .to_string()
            .contains("Unsupported snapshot")
    );
}

#[test]
fn owned_native_metadata_observes_regular_files_without_cloud_provider_assumptions() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("empty");
    std::fs::write(&path, b"").unwrap();
    let (metadata, identity) = voldiq_core::windows::metadata_identity(&path).unwrap();
    let expected = if cfg!(windows) {
        CloudState::NotPlaceholder
    } else {
        CloudState::Unknown
    };
    assert_eq!(identity.cloud, expected);
    assert_eq!(metadata.len(), 0);
    let index = scanner::scan(
        fixture.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert!(index.entries.iter().all(|entry| entry.cloud == expected));
    assert_eq!(index.entries[1].kind, Kind::File);
    assert_eq!(index.entries[1].logical, 0);
}
