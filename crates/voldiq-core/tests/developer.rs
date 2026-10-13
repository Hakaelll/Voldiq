use std::sync::atomic::AtomicBool;
use voldiq_core::{
    NodeId, ScanOptions,
    developer::{self, CacheLocation, Category},
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
    assert_eq!(
        page.categories
            .iter()
            .map(|total| (total.category, total.directories, total.logical))
            .collect::<Vec<_>>(),
        [
            (Category::Dependencies, 1, 64),
            (Category::Build, 3, 176),
            (Category::Cache, 1, 8),
        ]
    );
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
    assert_eq!(paged.categories, page.categories);
    assert!(developer::query(&index, NodeId(u32::MAX), 0, 10).is_err());
}

#[test]
fn explicit_cache_roots_use_indexed_sizes_without_classifying_credentials_or_namesakes() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    for (path, size) in [
        ("cargo/registry/cache/pkg.crate", 80),
        ("cargo/git/db/repo/data", 40),
        ("cargo/bin/cargo-tool.exe", 90),
        ("cargo/credentials.toml", 70),
        ("custom npm/node_modules/pkg/bytes", 20),
        ("custom npm/package.json", 2),
        ("unrelated/registry/data", 110),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![1; size]).unwrap();
    }
    let index = scan(
        root,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let caches = [
        CacheLocation {
            path: root.join("cargo/registry").canonicalize().unwrap(),
            tool: "Cargo",
            category: Category::Cache,
        },
        CacheLocation {
            path: root.join("cargo/git").canonicalize().unwrap(),
            tool: "Cargo",
            category: Category::Cache,
        },
        CacheLocation {
            path: root.join("custom npm").canonicalize().unwrap(),
            tool: "npm",
            category: Category::Cache,
        },
    ];
    let page = developer::query_with_caches(&index, NodeId(0), 0, 100, &caches).unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.logical, 142);
    assert!(
        page.items
            .iter()
            .all(|item| item.category == Category::Cache)
    );
    let registry = page.items.iter().find(|item| item.logical == 80).unwrap();
    let scoped = developer::query_with_caches(&index, registry.id, 0, 100, &caches).unwrap();
    assert_eq!(scoped.total, 1);
    assert_eq!(scoped.logical, 80);
    let child = index
        .entries
        .iter()
        .find(|entry| entry.parent == Some(registry.id) && entry.name == "cache")
        .unwrap();
    let child_page = developer::query_with_caches(&index, child.id, 0, 100, &caches).unwrap();
    assert_eq!(child_page.total, 1);
    assert_eq!(child_page.logical, 80);
    assert_eq!(child_page.items[0].id, child.id);
    assert_eq!(child_page.items[0].category, Category::Cache);
    let paged = developer::query_with_caches(&index, NodeId(0), 1, 1, &caches).unwrap();
    assert_eq!(paged.items[0].id, page.items[1].id);
    assert_eq!(paged.total, 3);
    assert_eq!(paged.logical, 142);
    assert!(
        developer::query_with_caches(&index, NodeId(0), 3, 10, &caches)
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn scanning_inside_a_cache_classifies_only_the_measured_subtree_and_keeps_partial_coverage() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("registry");
    let subtree = cache.join("src/pkg");
    std::fs::create_dir_all(&subtree).unwrap();
    std::fs::write(subtree.join("a.rs"), b"source").unwrap();
    std::fs::write(cache.join("outside"), b"unscanned").unwrap();
    let mut index = scan(
        &subtree,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    index.entries[0].incomplete = true;
    let caches = [CacheLocation {
        path: cache.canonicalize().unwrap(),
        tool: "Cargo",
        category: Category::Cache,
    }];
    let page = developer::query_with_caches(&index, NodeId(0), 0, 10, &caches).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.logical, 6);
    assert_eq!(page.items[0].id, NodeId(0));
    assert!(page.items[0].incomplete);
    assert_eq!(page.items[0].category, Category::Cache);
    index.snapshot_id = Some(1);
    let historical = developer::query_with_caches(&index, NodeId(0), 0, 10, &caches).unwrap();
    assert_eq!(historical.total, 0);
    assert_eq!(historical.logical, 0);
}

#[test]
fn overlapping_locations_preserve_the_most_specific_scoped_dependency_category() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    let packages = cache.join("packages");
    let package = packages.join("pkg");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("lib.bin"), [1; 17]).unwrap();
    std::fs::write(cache.join("other"), [1; 5]).unwrap();
    let index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let locations = [
        CacheLocation {
            path: cache.canonicalize().unwrap(),
            tool: "npm",
            category: Category::Cache,
        },
        CacheLocation {
            path: packages.canonicalize().unwrap(),
            tool: "NuGet",
            category: Category::Dependencies,
        },
    ];
    let whole = developer::query_with_caches(&index, NodeId(0), 0, 10, &locations).unwrap();
    assert_eq!(whole.total, 1);
    assert_eq!(whole.logical, 22);
    for name in ["packages", "pkg"] {
        let scope = index
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
            .id;
        let page = developer::query_with_caches(&index, scope, 0, 10, &locations).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.logical, 17);
        assert_eq!(page.items[0].tool, "NuGet");
        assert_eq!(page.items[0].category, Category::Dependencies);
    }
    let subtree = scan(
        &package,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let page = developer::query_with_caches(&subtree, NodeId(0), 0, 10, &locations).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].category, Category::Dependencies);
    let coincident = [
        CacheLocation {
            path: packages.canonicalize().unwrap(),
            tool: "NuGet",
            category: Category::Dependencies,
        },
        CacheLocation {
            path: packages.canonicalize().unwrap(),
            tool: "npm",
            category: Category::Cache,
        },
    ];
    let page = developer::query_with_caches(&subtree, NodeId(0), 0, 10, &coincident).unwrap();
    assert_eq!(page.items[0].tool, "NuGet");
    assert_eq!(page.items[0].category, Category::Dependencies);
}
