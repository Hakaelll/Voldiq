#![cfg(windows)]
use voldiq_core::windows;

#[test]
fn enumerates_native_aliases_and_resizes_for_long_paths() {
    let fixture = tempfile::tempdir().unwrap();
    let original = fixture.path().join("original.bin");
    std::fs::write(&original, [1; 128]).unwrap();
    let mut nested = fixture.path().join("日本語");
    for i in 0..15 {
        nested.push(format!("long-directory-component-{i:02}"));
    }
    std::fs::create_dir_all(&nested).unwrap();
    let alias = nested.join("alias.bin");
    std::fs::hard_link(&original, &alias).unwrap();
    let names = windows::hard_link_names(&original).unwrap();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&std::fs::canonicalize(&original).unwrap()));
    assert!(names.contains(&std::fs::canonicalize(&alias).unwrap()));
    let identity = windows::identity(&std::fs::canonicalize(&original).unwrap()).unwrap();
    for name in names {
        assert_eq!(windows::identity(&name).unwrap().key, identity.key);
    }
    let file = std::fs::File::open(&original).unwrap();
    assert_eq!(
        windows::final_path(&file).unwrap(),
        std::fs::canonicalize(&original).unwrap()
    );
}
