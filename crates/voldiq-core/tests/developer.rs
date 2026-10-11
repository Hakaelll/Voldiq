use std::sync::atomic::AtomicBool;
use voldiq_core::{
    NodeId, ScanOptions,
    developer::{self, Category},
    scanner::scan,
};

#[test]
fn indexed_project_artifacts_are_classified_without_counting_nested_storage_twice() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    for path in [
        "rust/target",
        "node/node_modules/pkg/__pycache__",
        "node/dist",
        "unrelated/target",
        "dotnet/bin",
        "python/__pycache__",
    ] {
        std::fs::create_dir_all(root.join(path)).unwrap();
    }
    for path in ["rust/Cargo.toml", "node/package.json", "dotnet/app.csproj"] {
        std::fs::write(root.join(path), b"{}").unwrap();
    }
    for (path, size) in [
        ("rust/target/artifact", 128),
        ("node/node_modules/pkg/__pycache__/a.pyc", 64),
        ("node/dist/app.js", 32),
        ("unrelated/target/user-data", 256),
        ("dotnet/bin/app.dll", 16),
        ("python/__pycache__/a.pyc", 8),
    ] {
        std::fs::write(root.join(path), vec![1; size]).unwrap();
    }
    let index = scan(
        root,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let page = developer::query(&index, NodeId(0), 0, 100).unwrap();
    assert_eq!(page.total, 5);
    assert_eq!(page.logical, 248);
    assert_eq!(page.items[0].tool, "Cargo");
    assert_eq!(page.items[1].category, Category::Dependencies);
    assert_eq!(
        page.items
            .iter()
            .filter(|item| item.tool == "Python")
            .count(),
        1
    );
    let paged = developer::query(&index, NodeId(0), 2, 2).unwrap();
    assert_eq!(paged.items.len(), 2);
    assert_eq!(paged.items[0].id, page.items[2].id);
    assert_eq!(paged.total, 5);
    assert_eq!(paged.logical, 248);
    assert!(developer::query(&index, NodeId(u32::MAX), 0, 10).is_err());
}
