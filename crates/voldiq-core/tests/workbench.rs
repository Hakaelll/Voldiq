use std::sync::atomic::AtomicBool;
use voldiq_core::{
    NodeId,
    index::Query,
    scanner,
    workbench::{RecordRef, Representation, Scope, Store, View, ViewSpec},
};

#[test]
fn saved_views_reopen_with_all_filters_without_transient_analysis_ids_or_page_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let initial = store.catalog().unwrap();
    let spec = ViewSpec {
        scope: Scope::AnalysisRoot,
        representation: Representation::Voronoi,
        query: Query {
            expression: "category:video size>1GiB".into(),
            text: "archive".into(),
            sort: "name".into(),
            ..Query::default()
        },
        ..ViewSpec::default()
    };
    let saved = store
        .save_view(None, &initial.revision, "Video archives", spec)
        .unwrap();
    assert!(
        store
            .save_view(None, &saved.revision, "video archives", ViewSpec::default())
            .is_err()
    );
    assert_ne!(saved.revision, initial.revision);
    let reference = RecordRef {
        id: saved.views[0].id,
        revision: saved.revision.clone(),
    };
    drop(store);
    let store = Store::open(directory.path()).unwrap();
    let view = store.view(&reference).unwrap();
    assert_eq!(view.name, "Video archives");
    assert_eq!(view.spec.representation, Representation::Voronoi);
    assert_eq!(view.spec.scope, Scope::AnalysisRoot);
    let query = view.spec.query;
    assert_eq!(query.scope, None);
    assert_eq!(query.parent, None);
    assert_eq!(query.expression, "category:video size>1GiB");
    assert_eq!(query.text, "archive");
    assert_eq!(query.sort, "name");
    assert!(query.compile().is_ok());
    assert_eq!(query.offset, 0);
    assert_eq!(query.limit, 100);
}

#[test]
fn stale_catalogues_cannot_replace_delete_or_load_a_different_version() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = Store::open(directory.path()).unwrap();
    let mut second = Store::open(directory.path()).unwrap();
    let revision = first.catalog().unwrap().revision;
    let created = first
        .save_view(None, &revision, "One", ViewSpec::default())
        .unwrap();
    assert!(
        second
            .save_view(None, &revision, "Lost update", ViewSpec::default())
            .is_err()
    );
    let reference = RecordRef {
        id: created.views[0].id,
        revision: created.revision.clone(),
    };
    let updated = second
        .save_view(
            Some(reference.id),
            &reference.revision,
            "Renamed",
            ViewSpec::default(),
        )
        .unwrap();
    assert!(first.view(&reference).is_err());
    assert!(first.remove_view(&reference).is_err());
    let removed = first
        .remove_view(&RecordRef {
            id: reference.id,
            revision: updated.revision,
        })
        .unwrap();
    assert!(removed.views.is_empty());
    assert!(second.catalog().unwrap().views.is_empty());
}

#[test]
fn validation_preserves_the_catalogue_and_existing_operations_exclude_writes() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let revision = store.catalog().unwrap().revision;
    for spec in [
        ViewSpec {
            query: Query {
                parent: Some(NodeId(1)),
                ..Query::default()
            },
            ..ViewSpec::default()
        },
        ViewSpec {
            query: Query {
                offset: 1,
                ..Query::default()
            },
            ..ViewSpec::default()
        },
        ViewSpec {
            query: Query {
                expression: "app:secret".into(),
                ..Query::default()
            },
            ..ViewSpec::default()
        },
        ViewSpec {
            depth: 7,
            ..ViewSpec::default()
        },
        ViewSpec {
            view: View::Overview,
            representation: Representation::Grid,
            ..ViewSpec::default()
        },
        ViewSpec {
            query: Query {
                modified_before: Some(1),
                ..Query::default()
            },
            ..ViewSpec::default()
        },
    ] {
        assert!(store.save_view(None, &revision, "Invalid", spec).is_err());
    }
    assert!(
        store
            .save_view(None, &revision, " ", ViewSpec::default())
            .is_err()
    );
    assert!(
        store
            .save_view(None, &revision, &"é".repeat(65), ViewSpec::default())
            .is_err()
    );
    assert_eq!(store.catalog().unwrap().revision, revision);
    let coordinator = voldiq_core::operations::Coordinator::open(directory.path()).unwrap();
    let lease = coordinator.acquire().unwrap();
    assert!(
        store
            .save_view(None, &revision, "Busy", ViewSpec::default())
            .is_err()
    );
    drop(lease);
    assert_eq!(
        store
            .save_view(None, &revision, "Allowed", ViewSpec::default())
            .unwrap()
            .views
            .len(),
        1
    );
}

#[test]
fn favorites_verify_indexed_and_full_current_identity_and_never_follow_replacement_paths() {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("favorite");
    std::fs::create_dir(&folder).unwrap();
    let index = scanner::scan(
        root.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let node = index
        .entries
        .iter()
        .find(|entry| entry.name == "favorite")
        .unwrap()
        .id;
    let mut store = Store::open(state.path()).unwrap();
    let saved = store
        .add_favorite(&store.catalog().unwrap().revision, &index, node)
        .unwrap();
    let repeated = store.add_favorite(&saved.revision, &index, node).unwrap();
    assert_eq!(repeated.favorites.len(), 1);
    assert_eq!(repeated.favorites[0].id, saved.favorites[0].id);
    let reference = RecordRef {
        id: repeated.favorites[0].id,
        revision: repeated.revision.clone(),
    };
    let guard = store.pin_favorite(&reference).unwrap();
    assert_eq!(
        std::fs::canonicalize(guard.path()).unwrap(),
        std::fs::canonicalize(&folder).unwrap()
    );
    #[cfg(windows)]
    assert!(std::fs::rename(&folder, root.path().join("moved")).is_err());
    drop(guard);
    std::fs::rename(&folder, root.path().join("moved")).unwrap();
    assert!(store.pin_favorite(&reference).is_err());
    std::fs::create_dir(&folder).unwrap();
    assert!(store.pin_favorite(&reference).is_err());
    assert!(
        store
            .add_favorite(&repeated.revision, &index, node)
            .is_err()
    );
    assert_eq!(
        store.remove_favorite(&reference).unwrap().favorites.len(),
        0
    );
}

#[test]
fn catalogue_limits_and_unknown_schema_never_reset_existing_records() {
    let state = tempfile::tempdir().unwrap();
    let mut store = Store::open(state.path()).unwrap();
    let mut revision = store.catalog().unwrap().revision;
    for i in 0..100 {
        revision = store
            .save_view(None, &revision, &format!("View {i}"), ViewSpec::default())
            .unwrap()
            .revision;
    }
    assert!(
        store
            .save_view(None, &revision, "Overflow", ViewSpec::default())
            .is_err()
    );
    assert_eq!(store.catalog().unwrap().views.len(), 100);
    drop(store);
    let database = rusqlite::Connection::open(state.path().join("workbench.db")).unwrap();
    database.execute_batch("PRAGMA user_version=999;").unwrap();
    assert!(Store::open(state.path()).is_err());
    assert_eq!(
        database
            .query_row("SELECT COUNT(*) FROM saved_views", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        100
    );
}
