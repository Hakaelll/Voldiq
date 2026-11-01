#![cfg(windows)]
use std::sync::{Arc, RwLock, atomic::AtomicBool};
use voldiq_core::{
    NodeId,
    cleanup::Plan,
    duplicates::{HashCache, find},
    metadata::{Eligibility, Store},
    scanner,
    windows::cleanup_stage::StageName,
};

#[test]
fn native_recovery_stage_names_are_protected_from_suggestions_hashing_and_manual_previews() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let stage = StageName::generate().unwrap();
    let directory = source.path().join(stage.as_str());
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("recovery.data"), b"retained recovery copy").unwrap();
    std::fs::write(
        source.path().join("ordinary.data"),
        b"retained recovery copy",
    )
    .unwrap();
    let index = scanner::scan(
        source.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let protected = index
        .entries
        .iter()
        .find(|entry| entry.name == "recovery.data")
        .unwrap()
        .id;
    let mut metadata = Store::open(state.path()).unwrap();
    let protections = metadata.protections().unwrap();
    let mask = protections
        .for_index(&index, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(mask.eligibility(protected).unwrap(), Eligibility::Protected);
    assert!(
        Plan::manual(&index, &[protected])
            .unwrap()
            .with_protections(state.path())
            .is_err()
    );
    drop(mask);
    let shared = Arc::new(RwLock::new(index));
    let result = find(
        &shared,
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut HashCache::default(),
        |_| {},
        &protections,
    )
    .unwrap();
    assert!(result.groups.is_empty());
    assert_eq!(result.bytes_read, 0);
    assert_eq!(
        std::fs::read(directory.join("recovery.data")).unwrap(),
        b"retained recovery copy"
    );
}
