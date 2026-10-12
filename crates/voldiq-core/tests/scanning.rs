use std::sync::atomic::AtomicBool;
use voldiq_core::{Kind, NodeId, ScanOptions, ScanPhase, index::Query, scanner::scan};

#[test]
fn scans_real_files_aggregates_and_paginates() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("日本語")).unwrap();
    std::fs::write(dir.path().join("日本語/a.txt"), [1_u8; 17]).unwrap();
    std::fs::write(dir.path().join("b.bin"), [2_u8; 31]).unwrap();
    let mut phase = ScanPhase::Scanning;
    let index = scan(
        dir.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |p| phase = p.phase,
    )
    .unwrap();
    assert_eq!(phase, ScanPhase::Complete);
    assert_eq!(index.entries[0].logical, 48);
    assert_eq!(index.entries[0].files, 2);
    let page = index
        .query(&Query {
            parent: Some(NodeId(0)),
            limit: 1,
            ..Query::default()
        })
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].name, "b.bin");
    let page = index
        .query(&Query {
            extension: "txt".into(),
            ..Query::default()
        })
        .unwrap();
    assert_eq!(page.items[0].logical, 17);
    assert_eq!(index.file_types(NodeId(0)).unwrap().len(), 2);
    assert!(index.path(page.items[0].id).unwrap().exists());
    assert!(index.details(page.items[0].id).unwrap().ancestors.len() == 2);
}

#[test]
fn physical_hard_links_are_counted_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), [3_u8; 8192]).unwrap();
    std::fs::hard_link(dir.path().join("a"), dir.path().join("b")).unwrap();
    let index = scan(
        dir.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(index.entries[0].logical, 16384);
    let entries: Vec<_> = index
        .entries
        .iter()
        .filter(|e| e.kind == Kind::File)
        .collect();
    assert!(entries.iter().all(|e| e.hard_link));
    assert_eq!(entries.iter().filter(|e| e.allocated > 0).count(), 1);
    assert_eq!(index.entries[0].allocated, 8192);
}

#[test]
fn cancellation_exclusions_and_limits_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("excluded")).unwrap();
    std::fs::write(dir.path().join("excluded/a"), [1; 16]).unwrap();
    std::fs::write(dir.path().join("b"), [1; 8]).unwrap();
    let options = ScanOptions {
        exclusions: vec![dir.path().join("excluded").to_string_lossy().into_owned()],
        max_entries: None,
        ..ScanOptions::default()
    };
    let index = scan(dir.path(), &options, &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!(index.entries[0].logical, 8);
    let index = scan(
        dir.path(),
        &ScanOptions::default(),
        &AtomicBool::new(true),
        |_| {},
    )
    .unwrap();
    assert!(index.entries[0].incomplete);
    let index = scan(
        dir.path(),
        &ScanOptions {
            max_entries: Some(1),
            ..ScanOptions::default()
        },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(index.error_count, 1);
    assert!(index.entries[0].incomplete);
}
