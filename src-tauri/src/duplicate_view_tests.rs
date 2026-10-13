use super::*;

#[test]
fn group_names_come_from_real_indexed_members_and_empty_groups_are_rejected() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("copía.bin"), [42; 16]).unwrap();
    std::fs::write(fixture.path().join("another-name.bin"), [42; 16]).unwrap();
    let cancel = AtomicBool::new(false);
    let index = scanner::scan(fixture.path(), &ScanOptions::default(), &cancel, |_| {}).unwrap();
    let index = Arc::new(std::sync::RwLock::new(index));
    let analysis = duplicates::find(
        &index,
        NodeId(0),
        0,
        &cancel,
        &mut HashCache::default(),
        |_| {},
    )
    .unwrap();
    assert_eq!(analysis.groups.len(), 1);
    let index = index.read().unwrap();
    let group = &analysis.groups[0];
    let summary = duplicate_group(7, group, &index).unwrap();
    assert!(matches!(
        summary.name.as_str(),
        "copía.bin" | "another-name.bin"
    ));
    assert_eq!(summary.logical, 16);
    assert_eq!(summary.copies, 2);
    assert_eq!(summary.key, 7);
    assert_eq!(summary.name, index.entry(group.members[0].id).unwrap().name);
    let payload = serde_json::to_value(summary).unwrap();
    assert!(payload["name"].is_string());
    let empty = duplicates::Group {
        digest: String::new(),
        logical: 0,
        recoverable: 0,
        members: Vec::new(),
    };
    assert!(duplicate_group(0, &empty, &index).is_err());
}
