use std::sync::atomic::AtomicBool;
use voldiq_core::{index::Query, scanner};

#[test]
fn allocation_order_preserves_unknowns_paging_and_logical_filter_meaning() {
    let fixture = tempfile::tempdir().unwrap();
    let evidence = [
        ("large", 400, 4, true),
        ("small", 20, 12, true),
        ("partial", 800, 8, false),
        ("zero", 100, 0, true),
        ("unknown", 900, 0, false),
    ];
    for (name, ..) in evidence {
        std::fs::write(fixture.path().join(name), b"owned").unwrap();
    }
    let mut index = scanner::scan(
        fixture.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    // Synthetic accounting evidence tests ordering independently of the fixture volume's allocation unit.
    for entry in index.entries.iter_mut().skip(1) {
        let (_, logical, allocated, known) = evidence
            .iter()
            .find(|(name, ..)| *name == entry.name)
            .unwrap();
        entry.logical = *logical;
        entry.allocated = *allocated;
        entry.allocation_known = *known;
    }
    let query = Query {
        files_only: true,
        sort: "allocated".into(),
        ..Query::default()
    };
    let page = index.query(&query).unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["small", "partial", "large", "zero", "unknown"]
    );
    assert_eq!(page.largest, 900);
    assert_eq!(page.total, 5);
    assert!(!page.items[1].allocation_known);
    assert!(page.items[3].allocation_known);
    assert!(!page.items[4].allocation_known);
    let page = index
        .query(&Query {
            offset: 1,
            limit: 2,
            ..query.clone()
        })
        .unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["partial", "large"]
    );
    let page = index
        .query(&Query {
            min_size: 100,
            ..query.clone()
        })
        .unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["partial", "large", "zero", "unknown"]
    );
    let logical = index
        .query(&Query {
            files_only: true,
            ..Query::default()
        })
        .unwrap();
    assert_eq!(
        logical
            .items
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["unknown", "partial", "large", "zero", "small"]
    );
    assert!(index.query_cancellable(&query, || true).unwrap().is_none());
}
