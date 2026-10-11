#![cfg(windows)]
use std::{
    io::{Seek, SeekFrom, Write},
    sync::atomic::AtomicBool,
};
use voldiq_core::{Kind, ScanOptions, scanner::scan};

#[test]
fn deep_long_paths_preserve_totals() {
    let root = tempfile::tempdir().unwrap();
    let mut path = std::fs::canonicalize(root.path()).unwrap();
    for i in 0..100 {
        path.push(format!("directory-{i}"));
        std::fs::create_dir(&path).unwrap();
    }
    std::fs::write(path.join("final-é.txt"), [7; 23]).unwrap();
    let index = scan(
        root.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(index.entries[0].logical, 23);
    assert_eq!(index.entries[0].files, 1);
    assert!(
        index
            .path(index.entries.last().unwrap().id)
            .unwrap()
            .exists()
    );
}

#[test]
fn junction_loop_is_recorded_without_traversal() {
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(root.path().join("loop"))
        .arg(root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::write(root.path().join("file"), [1; 9]).unwrap();
    let index = scan(
        root.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(index.entries.len(), 3);
    assert_eq!(index.entries[0].logical, 9);
    assert!(index.entries.iter().any(|entry| entry.kind == Kind::Link));
    assert!(index.entries[0].incomplete);
    assert!(
        scan(
            &root.path().join("loop"),
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err()
    );
}

#[test]
fn sparse_file_uses_actual_allocation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("sparse.bin");
    let mut file = std::fs::File::create(&path).unwrap();
    let status = std::process::Command::new("fsutil.exe")
        .args(["sparse", "setflag"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    file.seek(SeekFrom::Start(1024 * 1024 * 1024)).unwrap();
    file.write_all(&[1]).unwrap();
    drop(file);
    let index = scan(
        root.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(index.entries[0].logical, 1024 * 1024 * 1024 + 1);
    assert!(index.entries[0].allocated < 1024 * 1024);
}
