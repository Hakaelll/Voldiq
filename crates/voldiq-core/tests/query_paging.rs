use std::{collections::HashSet, sync::atomic::AtomicBool};
use voldiq_core::{NodeId, ScanOptions, index::Query, scanner::scan};

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
