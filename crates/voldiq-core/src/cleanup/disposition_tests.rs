use super::Plan;
use crate::{NodeId, ScanOptions, scanner};
use std::{path::Path, sync::atomic::AtomicBool};

fn plan(path: &Path, name: &str) -> (Plan, NodeId) {
    let index = scanner::scan(
        path,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let id = index
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .unwrap()
        .id;
    (Plan::manual(&index, &[id]).unwrap(), id)
}

#[test]
fn replacement_before_verification_and_readonly_targets_are_not_removed() {
    let fixture = tempfile::tempdir().unwrap();
    let selected = fixture.path().join("selected.bin");
    let retained = fixture.path().join("original.bin");
    std::fs::write(&selected, b"original").unwrap();
    let (first, id) = plan(fixture.path(), "selected.bin");
    std::fs::rename(&selected, &retained).unwrap();
    std::fs::write(&selected, b"replaced").unwrap();
    assert!(first.verify(id).is_err());
    let original_permissions = std::fs::metadata(&selected).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&selected, permissions.clone()).unwrap();
    let (second, id) = plan(fixture.path(), "selected.bin");
    let rejected = second.verify(id).unwrap().remove_permanently();
    std::fs::set_permissions(&selected, original_permissions).unwrap();
    assert!(rejected.is_err());
    assert_eq!(std::fs::read(&selected).unwrap(), b"replaced");
    assert_eq!(std::fs::read(&retained).unwrap(), b"original");
}
