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
fn permanent_disposition_removes_only_the_verified_link_and_preserves_readers() {
    use std::io::Read;
    let fixture = tempfile::tempdir().unwrap();
    let selected = fixture.path().join("selected.bin");
    let alias = fixture.path().join("retained.bin");
    let unrelated = fixture.path().join("unrelated.bin");
    std::fs::write(&selected, b"approved").unwrap();
    std::fs::hard_link(&selected, &alias).unwrap();
    std::fs::write(&unrelated, b"unrelated").unwrap();
    let (plan, id) = plan(fixture.path(), "selected.bin");
    assert_eq!(plan.preview().reclaimable, 0);
    let mut reader = std::fs::File::open(&selected).unwrap();
    let verified = plan.verify(id).unwrap();
    let identity = verified.identity().unwrap();
    assert!(std::fs::rename(&selected, fixture.path().join("moved.bin")).is_err());
    assert!(std::fs::write(&selected, b"replacement").is_err());
    verified.remove_permanently().unwrap();
    assert!(!selected.exists());
    assert_eq!(crate::windows::identity(&alias).unwrap().key, identity);
    assert_eq!(std::fs::read(&alias).unwrap(), b"approved");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();
    assert_eq!(content, b"approved");
    std::fs::write(&selected, b"new item").unwrap();
    assert_ne!(crate::windows::identity(&selected).unwrap().key, identity);
    assert_eq!(std::fs::read(&selected).unwrap(), b"new item");
    assert_eq!(std::fs::read(&unrelated).unwrap(), b"unrelated");
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

#[test]
fn last_link_disposition_leaves_an_existing_reader_alive_without_claiming_reclaimed_space() {
    use std::io::Read;
    let fixture = tempfile::tempdir().unwrap();
    let selected = fixture.path().join("selected.bin");
    std::fs::write(&selected, b"retained by reader").unwrap();
    let (plan, id) = plan(fixture.path(), "selected.bin");
    let mut reader = std::fs::File::open(&selected).unwrap();
    plan.verify(id).unwrap().remove_permanently().unwrap();
    assert!(!selected.exists());
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();
    assert_eq!(content, b"retained by reader");
}

#[test]
fn metadata_changed_after_verification_is_rejected_before_disposition() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
    };
    let fixture = tempfile::tempdir().unwrap();
    let selected = fixture.path().join("selected.bin");
    std::fs::write(&selected, b"original").unwrap();
    let attributes = std::fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&selected)
        .unwrap();
    let (plan, id) = plan(fixture.path(), "selected.bin");
    let verified = plan.verify(id).unwrap();
    attributes
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000))
        .unwrap();
    assert!(verified.remove_permanently().is_err());
    assert_eq!(std::fs::read(&selected).unwrap(), b"original");
}
