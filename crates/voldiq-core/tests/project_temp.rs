use std::{
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, UNIX_EPOCH},
};
use voldiq_core::{
    NodeId, ScanOptions,
    cleanup::rules,
    developer::{CacheLocation, Category},
    scanner::scan,
};

fn write(root: &Path, path: &str, size: usize, modified: u64) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, vec![1_u8; size]).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(modified))
        .unwrap();
}

#[test]
fn project_temporaries_require_build_classification_age_and_explicit_current_selection() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let now = 2_000_000_000;
    let cutoff = now - 7 * 24 * 60 * 60;
    for marker in [
        "rust/Cargo.toml",
        "node/package.json",
        "cmake/CMakeLists.txt",
        "dotnet/app.csproj",
        "global/crate/Cargo.toml",
        "node/node_modules/pkg/Cargo.toml",
        "node/dist/vendor/package.json",
        "node/dist/cache/crate/Cargo.toml",
    ] {
        write(root, marker, 2, cutoff);
    }
    for (path, size) in [
        ("rust/target/sub/old.tmp", 11),
        ("node/dist/old.TEMP", 13),
        ("cmake/build/old.tmp", 16),
        ("dotnet/obj/old.tmp", 23),
        ("dotnet/bin/old.temp", 27),
        ("rust/src/source.tmp", 31),
        ("node/node_modules/pkg/target/dependency.tmp", 32),
        ("global/crate/target/cached.tmp", 33),
        ("node/dist/vendor/node_modules/dependency.tmp", 32),
        ("node/dist/cache/crate/target/cached.tmp", 33),
        ("node/dist/cache/old.tmp", 33),
        ("unrelated/target/old.tmp", 34),
        ("root.tmp", 35),
        ("node/dist/app.js", 36),
        ("rust/target/excluded/old.tmp", 37),
    ] {
        write(root, path, size, cutoff);
    }
    write(root, "node/dist/recent.tmp", 38, cutoff + 1);
    std::fs::hard_link(
        root.join("rust/target/sub/old.tmp"),
        root.join("rust/target/alias.tmp"),
    )
    .unwrap();
    let cache = std::fs::canonicalize(root.join("global")).unwrap();
    let caches = [
        CacheLocation {
            path: cache.clone(),
            tool: "Cargo",
            category: Category::Cache,
        },
        CacheLocation {
            path: std::fs::canonicalize(root.join("node/dist/cache")).unwrap(),
            tool: "npm",
            category: Category::Cache,
        },
    ];
    let mut index = scan(
        root,
        &ScanOptions {
            exclusions: vec![root.join("rust/target/excluded").to_str().unwrap().into()],
            ..ScanOptions::default()
        },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let page = rules::query_project(Some(&index), now, 0, 100, &caches).unwrap();
    assert!(page.indexed && page.complete);
    assert_eq!((page.total, page.logical), (6, 101));
    assert_eq!(page.rule.id, rules::PROJECT_TEMP.id);
    assert!(
        page.items
            .iter()
            .all(|item| !item.path.contains("node_modules") && !item.path.contains("global"))
    );
    assert!(
        page.items
            .iter()
            .filter(|item| item.hard_link)
            .all(|item| item.reclaimable == 0)
    );
    let id = page.items[0].id;
    let plan = rules::preview_project(&index, now, &[id], &caches).unwrap();
    assert_eq!(plan.preview().rule, rules::PROJECT_TEMP.id);
    #[cfg(windows)]
    assert_eq!(
        plan.verify(id).unwrap().identity().unwrap(),
        index.entry(id).unwrap().file_state.unwrap().identity
    );
    let find = |suffix: &str| {
        index
            .entries
            .iter()
            .find(|entry| index.path(entry.id).unwrap().ends_with(suffix))
            .unwrap()
            .id
    };
    for suffix in [
        "root.tmp",
        "rust/src/source.tmp",
        "node/node_modules/pkg/target/dependency.tmp",
        "global/crate/target/cached.tmp",
        "node/dist/vendor/node_modules/dependency.tmp",
        "node/dist/cache/crate/target/cached.tmp",
        "node/dist/cache/old.tmp",
        "node/dist/recent.tmp",
        "node/dist/app.js",
    ] {
        assert!(rules::preview_project(&index, now, &[find(suffix)], &caches).is_err());
    }
    assert!(rules::preview_project(&index, now, &[NodeId(u32::MAX)], &caches).is_err());
    assert!(rules::preview_project(&index, now, &[id, id], &caches).is_err());
    assert!(rules::preview_project(&index, now, &[], &caches).is_err());
    assert_eq!(
        rules::query_project(Some(&index), 0, 0, 50, &caches)
            .unwrap()
            .total,
        0
    );
    assert!(
        !rules::query_project(None, now, 0, 50, &caches)
            .unwrap()
            .indexed
    );
    let marker = find("rust/Cargo.toml");
    index.entries[marker.0 as usize].incomplete = true;
    assert_eq!(
        rules::query_project(Some(&index), now, 0, 50, &caches)
            .unwrap()
            .total,
        4
    );
    index.entries[marker.0 as usize].incomplete = false;
    index.entries[0].incomplete = true;
    assert!(
        !rules::query_project(Some(&index), now, 0, 50, &caches)
            .unwrap()
            .complete
    );
    index.snapshot_id = Some(1);
    assert!(
        !rules::query_project(Some(&index), now, 0, 50, &caches)
            .unwrap()
            .indexed
    );
    assert!(rules::preview_project(&index, now, &[id], &caches).is_err());
    index.snapshot_id = None;
    std::fs::write(index.path(id).unwrap(), b"changed").unwrap();
    #[cfg(windows)]
    assert!(plan.verify(id).is_err());
    assert!(rules::preview_project(&index, now, &[id], &caches).is_err());
    assert_eq!(
        std::fs::read(root.join("rust/src/source.tmp")).unwrap(),
        vec![1; 31]
    );
    let cached_index = scan(
        &cache,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(
        rules::query_project(Some(&cached_index), now, 0, 50, &caches)
            .unwrap()
            .total,
        0
    );
}

#[test]
fn build_discovery_is_not_limited_to_the_developer_views_first_page() {
    let fixture = tempfile::tempdir().unwrap();
    let now = 2_000_000_000;
    let cutoff = now - 7 * 24 * 60 * 60;
    for project in 0..106 {
        write(
            fixture.path(),
            &format!("project-{project}/Cargo.toml"),
            2,
            cutoff,
        );
        write(
            fixture.path(),
            &format!("project-{project}/target/{project}.tmp"),
            3,
            cutoff,
        );
    }
    let index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let first = rules::query_project(Some(&index), now, 0, 100, &[]).unwrap();
    let next = rules::query_project(Some(&index), now, 100, 100, &[]).unwrap();
    assert_eq!(
        (
            first.total,
            first.logical,
            first.items.len(),
            next.items.len()
        ),
        (106, 318, 100, 6)
    );
    assert!(
        next.items
            .iter()
            .all(|item| !first.items.iter().any(|first| first.id == item.id))
    );
    let plan = rules::preview_project(&index, now, &[next.items[0].id], &[]).unwrap();
    assert_eq!(plan.preview().logical, 3);
}
