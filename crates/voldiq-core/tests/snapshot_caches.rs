use std::{path::Path, sync::atomic::AtomicBool};
use voldiq_core::{
    NodeId, ScanOptions,
    developer::{self, CacheLocation, Category},
    scanner::scan,
    snapshots::Store,
};

fn capture(path: &Path) -> voldiq_core::index::Index {
    scan(
        path,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap()
}

#[test]
fn captured_cache_categories_survive_restart_and_ignore_current_configuration() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    for name in [
        "profile/npm-cache",
        "profile/packages",
        "namesake/npm-cache",
        "project/target",
    ] {
        std::fs::create_dir_all(fixture.path().join(name)).unwrap();
    }
    for (name, bytes) in [
        ("profile/npm-cache/data", 256),
        ("profile/packages/data", 96),
        ("namesake/npm-cache/data", 1024),
        ("project/target/data", 32),
    ] {
        std::fs::write(fixture.path().join(name), vec![1; bytes]).unwrap();
    }
    std::fs::write(fixture.path().join("project/Cargo.toml"), []).unwrap();
    let index = capture(fixture.path());
    let caches = [
        CacheLocation {
            path: fixture
                .path()
                .join("profile/npm-cache")
                .canonicalize()
                .unwrap(),
            tool: "npm",
            category: Category::Cache,
        },
        CacheLocation {
            path: fixture
                .path()
                .join("profile/packages")
                .canonicalize()
                .unwrap(),
            tool: "NuGet",
            category: Category::Dependencies,
        },
    ];
    let path = database.path().join("history.db");
    let mut store = Store::open(&path).unwrap();
    let first = store
        .save(&index, &AtomicBool::new(false), &caches)
        .unwrap();
    let without_caches = store.save(&index, &AtomicBool::new(false), &[]).unwrap();
    assert!(store.save(&index, &AtomicBool::new(true), &caches).is_err());
    assert_eq!(store.list(0, 100).unwrap().len(), 2);
    drop(store);
    for directory in ["profile/npm-cache", "profile/packages"] {
        std::fs::remove_file(fixture.path().join(directory).join("data")).unwrap();
        std::fs::remove_dir(fixture.path().join(directory)).unwrap();
    }
    std::fs::remove_dir(fixture.path().join("profile")).unwrap();
    let mut store = Store::open(&path).unwrap();
    let saved = store.load(first).unwrap();
    assert!(
        store
            .save(&saved, &AtomicBool::new(false), &caches)
            .is_err()
    );
    let changed = [CacheLocation {
        path: fixture
            .path()
            .join("namesake/npm-cache")
            .canonicalize()
            .unwrap(),
        tool: "npm",
        category: Category::Cache,
    }];
    let page =
        developer::query_with_caches(&saved, &developer::Query::default(), &changed).unwrap();
    assert_eq!((page.total, page.logical), (3, 384));
    assert_eq!(
        page.items
            .iter()
            .map(|item| (item.tool, item.category, item.logical))
            .collect::<Vec<_>>(),
        [
            ("npm", Category::Cache, 256),
            ("NuGet", Category::Dependencies, 96),
            ("Cargo", Category::Build, 32)
        ]
    );
    let old = developer::query(
        &store.load(without_caches).unwrap(),
        &developer::Query::default(),
    )
    .unwrap();
    assert_eq!((old.total, old.logical), (1, 32));
    assert_eq!(
        store.compare(first, without_caches, 0, 50).unwrap().delta,
        0
    );
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM snapshot_caches", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    store.delete(first).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM snapshot_caches", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(store.load(without_caches).is_ok());
}

#[test]
fn captured_subtree_keeps_the_most_specific_category_without_saving_outside_paths() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(fixture.path().join("store/links/project")).unwrap();
    std::fs::write(fixture.path().join("store/links/project/data"), [1; 24]).unwrap();
    let index = capture(&fixture.path().join("store/links/project"));
    let caches = [
        CacheLocation {
            path: fixture.path().join("store").canonicalize().unwrap(),
            tool: "npm",
            category: Category::Cache,
        },
        CacheLocation {
            path: fixture.path().join("store/links").canonicalize().unwrap(),
            tool: "pnpm",
            category: Category::Dependencies,
        },
    ];
    let mut store = Store::open(&database.path().join("history.db")).unwrap();
    let id = store
        .save(&index, &AtomicBool::new(false), &caches)
        .unwrap();
    let saved = store.load(id).unwrap();
    assert_eq!(saved.captured_caches.len(), 1);
    assert_eq!(saved.captured_caches[0].id, NodeId(0));
    let page = developer::query(&saved, &developer::Query::default()).unwrap();
    assert_eq!((page.total, page.logical), (1, 24));
    assert_eq!(
        (page.items[0].tool, page.items[0].category),
        ("pnpm", Category::Dependencies)
    );
}

#[test]
fn version_three_migration_preserves_captures_without_inventing_cache_evidence() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("file"), [1; 17]).unwrap();
    let path = database.path().join("history.db");
    let mut store = Store::open(&path).unwrap();
    let id = store
        .save(&capture(fixture.path()), &AtomicBool::new(false), &[])
        .unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch("DROP TABLE snapshot_caches; PRAGMA user_version=3;")
        .unwrap();
    drop(connection);
    let store = Store::open(&path).unwrap();
    let saved = store.load(id).unwrap();
    assert_eq!(saved.entries[0].logical, 17);
    assert!(saved.captured_caches.is_empty());
    assert_eq!(store.list(0, 100).unwrap().len(), 1);
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        4
    );
}

#[test]
fn invalid_saved_cache_metadata_is_rejected_before_querying() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("cache")).unwrap();
    std::fs::write(fixture.path().join("cache/data"), [1; 8]).unwrap();
    let path = database.path().join("history.db");
    let mut store = Store::open(&path).unwrap();
    let id = store
        .save(
            &capture(fixture.path()),
            &AtomicBool::new(false),
            &[CacheLocation {
                path: fixture.path().join("cache").canonicalize().unwrap(),
                tool: "npm",
                category: Category::Cache,
            }],
        )
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE snapshot_caches SET tool='unsupported' WHERE snapshot_id=?1",
            [id],
        )
        .unwrap();
    assert!(store.load(id).is_err());
    connection.execute("UPDATE snapshot_caches SET tool='npm',path_id=(SELECT path_id FROM snapshot_versions WHERE kind=1 LIMIT 1) WHERE snapshot_id=?1", [id]).unwrap();
    assert!(store.load(id).is_err());
    assert_eq!(
        std::fs::read(fixture.path().join("cache/data")).unwrap(),
        [1; 8]
    );
}
