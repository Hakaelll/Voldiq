use voldiq_core::metadata::Store;

#[test]
fn notes_are_literal_and_protection_survives_reopen_rename_and_prefix_siblings() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let folder = source.path().join("project");
    let sibling = source.path().join("project-other");
    std::fs::create_dir(&folder).unwrap();
    std::fs::create_dir(&sibling).unwrap();
    std::fs::write(folder.join("selected.data"), b"fixture").unwrap();
    std::fs::write(sibling.join("other.data"), b"other").unwrap();
    let mut notes = Store::open(state.path()).unwrap();
    let original = notes.protections().unwrap().fingerprint().to_owned();
    let note = notes
        .save_note(&folder, "<script>literal data</script>\nKeep source files.")
        .unwrap();
    assert!(!note.protected);
    assert_eq!(notes.protections().unwrap().fingerprint(), original);
    let protected = notes.protect_folder(&folder).unwrap();
    assert!(protected.protected);
    assert_ne!(notes.protections().unwrap().fingerprint(), original);
    assert!(
        notes
            .protections()
            .unwrap()
            .check_path(&folder.join("selected.data"))
            .is_err()
    );
    assert!(
        notes
            .protections()
            .unwrap()
            .check_path(&sibling.join("other.data"))
            .is_ok()
    );
    assert!(
        notes
            .protections()
            .unwrap()
            .check_path(&state.path().join("metadata.db"))
            .is_err()
    );
    drop(notes);
    let moved = source.path().join("renamed-project");
    std::fs::rename(&folder, &moved).unwrap();
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("new.data"), b"new").unwrap();
    let mut notes = Store::open(state.path()).unwrap();
    assert_eq!(
        notes.get(&moved).unwrap().note,
        "<script>literal data</script>\nKeep source files."
    );
    assert!(
        notes
            .protections()
            .unwrap()
            .check_path(&moved.join("selected.data"))
            .is_err()
    );
    assert!(
        notes
            .protections()
            .unwrap()
            .check_path(&folder.join("new.data"))
            .is_err()
    );
    assert!(notes.remove_protection(protected.id.unwrap(), "0").is_err());
    notes
        .remove_protection(protected.id.unwrap(), &protected.version)
        .unwrap();
    assert!(
        notes
            .protections()
            .unwrap()
            .check_path(&moved.join("selected.data"))
            .is_ok()
    );
    assert!(
        notes
            .protections()
            .unwrap()
            .check_path(&folder.join("new.data"))
            .is_ok()
    );
    assert!(!notes.get(&moved).unwrap().protected);
    assert_eq!(
        std::fs::read(moved.join("selected.data")).unwrap(),
        b"fixture"
    );
}

#[test]
fn metadata_changes_wait_for_managed_operations_and_do_not_partially_commit() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let mut notes = Store::open(state.path()).unwrap();
    let before = notes.protections().unwrap().fingerprint().to_owned();
    let lease = voldiq_core::operations::Lease::acquire(state.path()).unwrap();
    assert!(notes.save_note(source.path(), "deferred").is_err());
    assert!(notes.protect_folder(source.path()).is_err());
    assert_eq!(notes.protections().unwrap().fingerprint(), before);
    assert_eq!(notes.get(source.path()).unwrap().note, "");
    drop(lease);
    assert!(notes.save_note(source.path(), &"x".repeat(8193)).is_err());
    assert_eq!(notes.get(source.path()).unwrap().note, "");
    notes.protect_folder(source.path()).unwrap();
    let protected = notes.get(source.path()).unwrap();
    notes.save_note(source.path(), "Updated note").unwrap();
    assert!(
        notes
            .remove_protection(protected.id.unwrap(), &protected.version)
            .is_err()
    );
    let fresh = notes.get(source.path()).unwrap();
    assert!(fresh.protected);
    notes
        .remove_protection(fresh.id.unwrap(), &fresh.version)
        .unwrap();
    assert!(!notes.get(source.path()).unwrap().protected);
}

#[test]
fn manual_previews_bind_protections_and_note_edits_do_not_change_cleanup_policy() {
    use std::sync::atomic::AtomicBool;
    use voldiq_core::cleanup::Plan;
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("selected.data"), b"fixture").unwrap();
    let index = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let id = index
        .entries
        .iter()
        .find(|entry| entry.name == "selected.data")
        .unwrap()
        .id;
    let plan = Plan::manual(&index, &[id])
        .unwrap()
        .with_protections(state.path())
        .unwrap();
    let mut metadata = Store::open(state.path()).unwrap();
    metadata.save_note(source.path(), "A literal note").unwrap();
    plan.validate_protections().unwrap();
    metadata.protect_folder(source.path()).unwrap();
    assert!(plan.validate_protections().is_err());
    assert!(
        Plan::manual(&index, &[id])
            .unwrap()
            .with_protections(state.path())
            .is_err()
    );
    let protected = metadata.get(source.path()).unwrap();
    metadata
        .remove_protection(protected.id.unwrap(), &protected.version)
        .unwrap();
    // Removing protection does not restore the authority of an earlier preview.
    assert!(plan.validate_protections().is_err());
    Plan::manual(&index, &[id])
        .unwrap()
        .with_protections(state.path())
        .unwrap();
    assert_eq!(
        std::fs::read(source.path().join("selected.data")).unwrap(),
        b"fixture"
    );
}

#[test]
fn protection_pages_are_bounded_and_missing_folders_can_be_unprotected_by_reviewed_version() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let mut metadata = Store::open(state.path()).unwrap();
    for name in ["first", "second", "third"] {
        let folder = source.path().join(name);
        std::fs::create_dir(&folder).unwrap();
        metadata.protect_folder(&folder).unwrap();
    }
    let first = metadata.protected_folders(0, 1).unwrap();
    assert_eq!(first.len(), 2); // One additional row signals another page.
    let second = metadata.protected_folders(1, 1).unwrap();
    assert_eq!(second[0].id, first[1].id);
    assert!(metadata.protected_folders(0, 0).is_err());
    assert!(metadata.protected_folders(0, 101).is_err());
    let folder = &first[0];
    std::fs::remove_dir(&folder.path).unwrap();
    metadata
        .remove_protection(folder.id.unwrap(), &folder.version)
        .unwrap();
    assert!(
        metadata
            .protection(folder.id.unwrap(), &folder.version)
            .is_err()
    );
    assert_eq!(metadata.protected_folders(0, 25).unwrap().len(), 2);
}
