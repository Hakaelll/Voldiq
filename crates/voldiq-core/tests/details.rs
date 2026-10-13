use std::sync::atomic::AtomicBool;
use voldiq_core::{NodeId, ScanMode, ScanOptions, scanner::scan};

#[test]
fn deep_details_cancel_without_partial_paths_and_preserve_ancestor_order() {
    let fixture = tempfile::tempdir().unwrap();
    let mut folder = fixture.path().to_path_buf();
    for _ in 0..300 {
        folder.push("d");
        std::fs::create_dir(&folder).unwrap();
    }
    let file = folder.join("résumé.txt");
    std::fs::write(&file, b"fixture").unwrap();
    let index = scan(
        fixture.path(),
        &ScanOptions {
            mode: ScanMode::Directory,
            ..ScanOptions::default()
        },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let id = index
        .entries
        .iter()
        .find(|entry| entry.name == "résumé.txt")
        .unwrap()
        .id;
    let mut checks = 0;
    let details = index
        .details_cancellable(id, || {
            checks += 1;
            false
        })
        .unwrap()
        .unwrap();
    assert!(checks >= 9);
    assert_eq!(
        details.path,
        std::fs::canonicalize(&file).unwrap().to_string_lossy()
    );
    assert!(details.path_exact);
    assert_eq!(details.entry.logical, 7);
    assert_eq!(details.ancestors.len(), 301);
    assert_eq!(details.ancestors[0].id, NodeId(0));
    assert_eq!(
        details.ancestors.last().unwrap().id,
        details.entry.parent.unwrap()
    );
    for ancestors in details.ancestors.windows(2) {
        assert_eq!(ancestors[1].parent, Some(ancestors[0].id));
    }
    let ordinary = index.details(id).unwrap();
    assert_eq!(ordinary.path, details.path);
    assert_eq!(
        ordinary
            .ancestors
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        details
            .ancestors
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    );
    for checkpoint in 1..=checks {
        let mut calls = 0;
        assert!(
            index
                .details_cancellable(id, || {
                    calls += 1;
                    calls == checkpoint
                })
                .unwrap()
                .is_none()
        );
        assert_eq!(calls, checkpoint);
    }
    assert!(
        index
            .details_cancellable(NodeId(u32::MAX), || true)
            .unwrap()
            .is_none()
    );
    let root = index
        .details_cancellable(NodeId(0), || false)
        .unwrap()
        .unwrap();
    assert!(root.ancestors.is_empty());
    assert_eq!(root.path, index.root.to_string_lossy());
}
