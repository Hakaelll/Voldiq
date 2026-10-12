#![cfg(windows)]
use std::sync::atomic::AtomicBool;
use voldiq_core::{NodeId, ScanOptions, cleanup::Plan, scanner::scan};

#[test]
fn preview_uses_real_sizes_and_verification_pins_identity_against_mutation() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("a.bin");
    std::fs::write(&path, [1; 128]).unwrap();
    let index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let plan = Plan::manual(&index, &[NodeId(1)]).unwrap();
    assert_eq!(plan.preview().logical, 128);
    let verified = plan.verify(NodeId(1)).unwrap();
    assert_eq!(
        verified.identity().unwrap(),
        voldiq_core::windows::identity(&path).unwrap().key
    );
    assert!(verified.pinned_parents() >= 2);
    assert!(std::fs::rename(&path, fixture.path().join("renamed")).is_err());
    assert!(std::fs::write(&path, [2; 128]).is_err());
    drop(verified);
    std::fs::write(&path, [2; 128]).unwrap();
    assert!(plan.verify(NodeId(1)).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), [2; 128]);
}

#[test]
fn changed_names_exclusions_historical_indices_and_allocation_are_guarded() {
    let fixture = tempfile::tempdir().unwrap();
    let original = fixture.path().join("original");
    let alias = fixture.path().join("alias");
    std::fs::write(&original, [1; 32]).unwrap();
    std::fs::hard_link(&original, &alias).unwrap();
    let mut index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let id = index
        .entries
        .iter()
        .find(|entry| entry.name == "original")
        .unwrap()
        .id;
    let plan = Plan::manual(&index, &[id]).unwrap();
    assert_eq!(plan.preview().reclaimable, 0);
    assert!(plan.preview().items[0].hard_link);
    std::fs::remove_file(&original).unwrap();
    std::fs::write(&original, [1; 32]).unwrap();
    assert!(plan.verify(id).is_err());
    index
        .exclusions
        .push(std::fs::canonicalize(fixture.path()).unwrap());
    assert!(Plan::manual(&index, &[id]).is_err());
    index.exclusions.clear();
    index.snapshot_id = Some(1);
    assert!(Plan::manual(&index, &[id]).is_err());
    index.snapshot_id = None;
    assert!(Plan::manual(&index, &[NodeId(0)]).is_err());
    assert!(Plan::manual(&index, &[id, id]).is_err());
}

#[test]
fn replacements_before_preview_cannot_reuse_scanned_size_and_timestamp() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("a");
    std::fs::write(&path, [1; 32]).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    std::fs::rename(&path, fixture.path().join("old")).unwrap();
    std::fs::write(&path, [1; 32]).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert!(Plan::manual(&index, &[NodeId(1)]).is_err());
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 32);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
}

#[test]
fn native_protected_locations_are_absolute_existing_directories() {
    let protected = voldiq_core::windows::protected_directories().unwrap();
    assert_eq!(protected.len(), 4);
    assert!(
        protected
            .iter()
            .all(|path| path.is_absolute() && path.is_dir())
    );
}

#[test]
fn changed_parent_junction_is_rejected_before_opening_its_child() {
    let fixture = tempfile::tempdir().unwrap();
    let parent = fixture.path().join("parent");
    let retained = fixture.path().join("retained");
    let target = fixture.path().join("target");
    std::fs::create_dir(&parent).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::write(parent.join("a.bin"), b"original").unwrap();
    std::fs::write(target.join("a.bin"), b"replacement").unwrap();
    let index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let id = index
        .entries
        .iter()
        .find(|entry| {
            index.path(entry.id).unwrap() == std::fs::canonicalize(parent.join("a.bin")).unwrap()
        })
        .unwrap()
        .id;
    let plan = Plan::manual(&index, &[id]).unwrap();
    std::fs::rename(&parent, &retained).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&parent)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = plan.verify(id);
    assert!(
        matches!(result, Err(voldiq_core::Error::Invalid(ref problem)) if problem == "Parent directory changed since preview.")
    );
    std::fs::remove_dir(&parent).unwrap();
    assert_eq!(std::fs::read(retained.join("a.bin")).unwrap(), b"original");
    assert_eq!(std::fs::read(target.join("a.bin")).unwrap(), b"replacement");
}
