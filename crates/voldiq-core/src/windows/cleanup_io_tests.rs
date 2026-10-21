use super::*;

#[test]
fn acquired_directory_component_keeps_its_name_until_release() {
    let fixture = tempfile::tempdir().unwrap();
    let held = fixture.path().join("held");
    let moved = fixture.path().join("moved");
    std::fs::create_dir(&held).unwrap();
    let mut name = native_root_name(&std::fs::canonicalize(fixture.path()).unwrap());
    let root = native_open(
        &mut name,
        None,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )
    .unwrap();
    let parent = open_child(&root, OsStr::new("held"), true).unwrap();
    assert!(
        std::fs::rename(&held, &moved).is_err(),
        "An acquired cleanup parent was renamed before its child was opened"
    );
    std::fs::write(held.join("sibling.bin"), b"allowed").unwrap();
    drop(parent);
    std::fs::rename(&held, &moved).unwrap();
    assert_eq!(
        std::fs::read(moved.join("sibling.bin")).unwrap(),
        b"allowed"
    );
}
