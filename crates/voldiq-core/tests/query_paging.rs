use std::{collections::HashSet, sync::atomic::AtomicBool};
use voldiq_core::{Kind, NodeId, ScanOptions, index::Query, scanner::scan};

#[test]
fn extensionless_type_selection_matches_only_measured_files_and_preserves_other_filters() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("plain-folder")).unwrap();
    std::fs::create_dir(fixture.path().join("empty-folder")).unwrap();
    for (name, size) in [
        ("README", 20),
        (".gitignore", 10),
        (".env", 5),
        ("plain-folder/LICENSE", 30),
        ("archive.tar.gz", 15),
        ("example.txt", 40),
        ("file.No extension", 43),
    ] {
        std::fs::write(fixture.path().join(name), vec![1; size]).unwrap();
    }
    let index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let types = index.file_types(NodeId(0)).unwrap();
    let missing = types.iter().find(|item| item.extension.is_none()).unwrap();
    assert_eq!((missing.count, missing.bytes), (4, 65));
    assert_eq!(types.len(), 4);
    assert!(
        types
            .iter()
            .any(|item| item.extension.as_deref() == Some("no extension")
                && item.count == 1
                && item.bytes == 43)
    );
    let query = Query {
        without_extension: true,
        sort: "name".into(),
        ..Query::default()
    };
    let page = index.query(&query).unwrap();
    assert_eq!(page.total, 4);
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        [".env", ".gitignore", "LICENSE", "README"]
    );
    assert!(page.items.iter().all(|item| item.kind == Kind::File));
    let cancellable = index.query_cancellable(&query, || false).unwrap().unwrap();
    assert_eq!(
        page.items.iter().map(|item| item.id).collect::<Vec<_>>(),
        cancellable
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        index
            .query(&Query {
                parent: Some(NodeId(0)),
                ..query.clone()
            })
            .unwrap()
            .total,
        3
    );
    let folder = index
        .entries
        .iter()
        .find(|item| item.name == "plain-folder")
        .unwrap()
        .id;
    assert_eq!(
        index
            .query(&Query {
                scope: Some(folder),
                ..query.clone()
            })
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        index
            .query(&Query {
                min_size: 15,
                ..query.clone()
            })
            .unwrap()
            .total,
        2
    );
    assert_eq!(
        index
            .query(&Query {
                text: "read".into(),
                ..query.clone()
            })
            .unwrap()
            .items[0]
            .name,
        "README"
    );
    assert_eq!(
        index
            .query(&Query {
                offset: 2,
                limit: 1,
                ..query.clone()
            })
            .unwrap()
            .items[0]
            .name,
        "LICENSE"
    );
    assert_eq!(
        index
            .query(&Query {
                extension: "txt".into(),
                ..query.clone()
            })
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        index
            .query(&Query {
                kind: Some(Kind::Directory),
                ..query
            })
            .unwrap()
            .total,
        0
    );
    assert_eq!(index.query(&Query::default()).unwrap().total, 9);
}

#[test]
fn cancelled_query_returns_no_page_and_releases_the_shared_index() {
    use std::sync::{Arc, Barrier, RwLock, atomic::Ordering};
    let fixture = tempfile::tempdir().unwrap();
    for file in 0..1500 {
        std::fs::write(fixture.path().join(format!("file-{file:04}.bin")), [1]).unwrap();
    }
    let index = Arc::new(RwLock::new(
        scan(
            fixture.path(),
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap(),
    ));
    assert!(
        index
            .read()
            .unwrap()
            .query_cancellable(
                &Query {
                    scope: Some(NodeId(u32::MAX)),
                    ..Query::default()
                },
                || true
            )
            .unwrap()
            .is_none()
    );
    let reached = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let cancel = Arc::new(AtomicBool::new(false));
    let worker = {
        let (index, reached, release, cancel) = (
            index.clone(),
            reached.clone(),
            release.clone(),
            cancel.clone(),
        );
        std::thread::spawn(move || {
            let mut checks = 0;
            index
                .read()
                .unwrap()
                .query_cancellable(
                    &Query {
                        files_only: true,
                        ..Query::default()
                    },
                    || {
                        checks += 1;
                        if checks == 3 {
                            reached.wait();
                            release.wait();
                        }
                        cancel.load(Ordering::Relaxed)
                    },
                )
                .unwrap()
                .is_none()
        })
    };
    reached.wait();
    assert!(index.try_write().is_err());
    cancel.store(true, Ordering::Relaxed);
    release.wait();
    assert!(worker.join().unwrap());
    assert!(index.try_write().is_ok());
    let guard = index.read().unwrap();
    let completed = guard
        .query_cancellable(
            &Query {
                files_only: true,
                offset: 700,
                limit: 37,
                sort: "name".into(),
                ..Query::default()
            },
            || false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(completed.total, 1500);
    assert_eq!(completed.items.len(), 37);
    assert_eq!(completed.items[0].name, "file-0700.bin");
    assert_eq!(completed.items[36].name, "file-0736.bin");
}

#[test]
fn kind_filter_ranks_real_folders_and_preserves_scopes_and_paging() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(fixture.path().join("big/nested")).unwrap();
    std::fs::create_dir(fixture.path().join("small")).unwrap();
    for (path, bytes) in [
        ("big/a.bin", 30),
        ("big/nested/b.bin", 10),
        ("small/c.bin", 5),
        ("huge.bin", 100),
    ] {
        std::fs::write(fixture.path().join(path), vec![1; bytes]).unwrap();
    }
    let index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let folders = Query {
        kind: Some(Kind::Directory),
        limit: 1,
        ..Query::default()
    };
    let mut ranked = Vec::new();
    for offset in 0..3 {
        let page = index
            .query(&Query {
                offset,
                ..folders.clone()
            })
            .unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].kind, Kind::Directory);
        ranked.push(page.items[0].clone());
    }
    assert_eq!(
        ranked
            .iter()
            .map(|e| (e.name.as_str(), e.logical))
            .collect::<Vec<_>>(),
        [("big", 40), ("nested", 10), ("small", 5)]
    );
    assert_eq!(
        index
            .query(&Query {
                min_size: 11,
                ..folders.clone()
            })
            .unwrap()
            .total,
        1
    );
    let scoped = index
        .query(&Query {
            scope: Some(ranked[0].id),
            limit: 100,
            ..folders.clone()
        })
        .unwrap();
    assert_eq!(
        scoped
            .items
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["big", "nested"]
    );
    assert_eq!(
        index
            .query(&Query {
                text: "NEST".into(),
                ..folders.clone()
            })
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        index
            .query(&Query {
                files_only: true,
                ..folders
            })
            .unwrap()
            .total,
        0
    );
    let files = index
        .query(&Query {
            kind: Some(Kind::File),
            ..Query::default()
        })
        .unwrap();
    assert_eq!(files.total, 4);
    assert_eq!(files.items[0].name, "huge.bin");
    assert!(files.items.iter().all(|e| e.kind == Kind::File));
    assert_eq!(index.query(&Query::default()).unwrap().total, 7);
}

#[test]
fn name_sort_preserves_ascii_and_unicode_lowercase_order() {
    let fixture = tempfile::tempdir().unwrap();
    for name in ["Éclair", "Ångström", "zebra", "ALPHA", "alpine"] {
        std::fs::write(fixture.path().join(name), [1]).unwrap();
    }
    let index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let query = Query {
        sort: "name".into(),
        files_only: true,
        ..Query::default()
    };
    assert_eq!(
        index
            .query(&query)
            .unwrap()
            .items
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["ALPHA", "alpine", "zebra", "Ångström", "Éclair"]
    );
}

#[test]
fn ranked_pages_are_complete_stable_and_filtered_on_real_files() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("nested")).unwrap();
    for i in 0..1037 {
        std::fs::write(
            fixture.path().join(format!("nested/File-{i:04}.bin")),
            vec![1; (i * 37) % 101],
        )
        .unwrap();
    }
    std::fs::write(fixture.path().join("root.TXT"), [1; 3]).unwrap();
    let index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    for sort in ["size", "name", "modified"] {
        let mut all = Vec::new();
        for offset in (0..1038).step_by(73) {
            let query = Query {
                files_only: true,
                sort: sort.into(),
                offset,
                limit: 73,
                ..Query::default()
            };
            let page = index.query(&query).unwrap();
            assert_eq!(page.total, 1038);
            assert_eq!(
                page.items.iter().map(|e| e.id).collect::<Vec<_>>(),
                index
                    .query(&query)
                    .unwrap()
                    .items
                    .iter()
                    .map(|e| e.id)
                    .collect::<Vec<_>>()
            );
            all.extend(page.items);
        }
        assert_eq!(all.len(), 1038);
        assert_eq!(all.iter().map(|e| e.id).collect::<HashSet<_>>().len(), 1038);
        for pair in all.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            let order = match sort {
                "name" => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                "modified" => b.modified.cmp(&a.modified),
                _ => b.logical.cmp(&a.logical),
            };
            assert!(order.is_lt() || (order.is_eq() && a.id.0 < b.id.0));
        }
    }
    let filtered = index
        .query(&Query {
            text: "fILE-00".into(),
            extension: ".BIN".into(),
            files_only: true,
            ..Query::default()
        })
        .unwrap();
    assert_eq!(filtered.total, 100);
    assert_eq!(filtered.items.len(), 100);
    assert_eq!(
        index
            .query(&Query {
                parent: Some(NodeId(0)),
                ..Query::default()
            })
            .unwrap()
            .total,
        2
    );
    assert!(
        index
            .query(&Query {
                offset: usize::MAX,
                ..Query::default()
            })
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        index
            .query(&Query {
                limit: usize::MAX,
                ..Query::default()
            })
            .unwrap()
            .items
            .len(),
        500
    );
}
