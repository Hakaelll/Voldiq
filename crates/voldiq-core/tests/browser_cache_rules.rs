use std::{
    sync::atomic::AtomicBool,
    time::{Duration, UNIX_EPOCH},
};
use voldiq_core::{NodeId, ScanOptions, cleanup::rules, scanner::scan};

#[test]
fn browser_cache_rule_limits_age_scope_and_current_file_authority() {
    let fixture = tempfile::tempdir().unwrap();
    let now = 2_000_000_000;
    let cutoff = now - 7 * 24 * 60 * 60;
    let chrome = "Google/Chrome/User Data/Default/Cache/Cache_Data";
    let edge = "Microsoft/Edge/User Data/Profile 2/Cache/Cache_Data";
    let cases = [
        (format!("{chrome}/f_000001"), cutoff - 1),
        (format!("{edge}/f_000002"), cutoff),
        (format!("{chrome}/f_000003"), cutoff + 1),
        (format!("{chrome}/index"), cutoff),
        (format!("{chrome}/data_0"), cutoff),
        (format!("{chrome}/0000000000000000_0"), cutoff),
        (format!("{chrome}/nested/f_000004"), cutoff),
        ("Google/Chrome/User Data/Default/Cookies".into(), cutoff),
        (
            "Google/Chrome/User Data/Default/Service Worker/CacheStorage/f_000005".into(),
            cutoff,
        ),
        (
            "Microsoft/Edge/User Data/Custom/Cache/Cache_Data/f_000006".into(),
            cutoff,
        ),
        (
            "Google/Other/User Data/Default/Cache/Cache_Data/f_000007".into(),
            cutoff,
        ),
        (format!("{chrome}/f_abc"), cutoff),
        (format!("{chrome}/f_00000z"), cutoff),
    ];
    for (path, modified) in &cases {
        let path = fixture.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, [1; 17]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(*modified))
            .unwrap();
    }
    let mut index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let missing = rules::query_browser_cache(None, fixture.path(), now, 0, 50).unwrap();
    assert!(!missing.indexed);
    let first = rules::query_browser_cache(Some(&index), fixture.path(), now, 0, 1).unwrap();
    assert!(first.indexed && first.complete);
    assert_eq!((first.total, first.logical, first.items.len()), (2, 34, 1));
    assert!(first.items[0].path.ends_with("f_000001"));
    let second = rules::query_browser_cache(Some(&index), fixture.path(), now, 1, 1).unwrap();
    assert!(second.items[0].path.ends_with("f_000002"));
    let id = first.items[0].id;
    let plan = rules::preview_browser_cache(&index, fixture.path(), now, &[id]).unwrap();
    assert_eq!(plan.preview().rule, rules::BROWSER_CACHE.id);
    let outside = index
        .entries
        .iter()
        .find(|entry| entry.name == "f_000004")
        .unwrap()
        .id;
    assert!(rules::preview_browser_cache(&index, fixture.path(), now, &[outside]).is_err());
    assert!(
        rules::preview_browser_cache(&index, fixture.path(), now, &[NodeId(u32::MAX)]).is_err()
    );
    assert!(rules::preview_browser_cache(&index, fixture.path(), now, &[id, id]).is_err());
    assert!(rules::preview_browser_cache(&index, fixture.path(), now, &[]).is_err());
    assert_eq!(
        rules::query_browser_cache(Some(&index), fixture.path(), 0, 0, 50)
            .unwrap()
            .total,
        0
    );
    index.snapshot_id = Some(1);
    assert!(
        !rules::query_browser_cache(Some(&index), fixture.path(), now, 0, 50)
            .unwrap()
            .indexed
    );
    assert!(rules::preview_browser_cache(&index, fixture.path(), now, &[id]).is_err());
    index.snapshot_id = None;
    std::fs::write(fixture.path().join(format!("{chrome}/f_000001")), [2; 17]).unwrap();
    assert!(rules::preview_browser_cache(&index, fixture.path(), now, &[id]).is_err());
    assert_eq!(
        std::fs::read(
            fixture
                .path()
                .join("Google/Chrome/User Data/Default/Cookies")
        )
        .unwrap(),
        [1; 17]
    );
    assert_eq!(
        std::fs::read(fixture.path().join(format!("{chrome}/index"))).unwrap(),
        [1; 17]
    );
}

#[cfg(windows)]
#[test]
fn browser_cache_removal_rejects_open_writer_and_preserves_other_data() {
    use voldiq_core::cleanup::{execute::permanent, journal::Journal};
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture
        .path()
        .join("Microsoft/Edge/User Data/Default/Cache/Cache_Data");
    std::fs::create_dir_all(&cache).unwrap();
    let payload = cache.join("f_abcdef");
    let now = 2_000_000_000;
    std::fs::write(&payload, b"cached response").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&payload)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(now - 8 * 24 * 60 * 60))
        .unwrap();
    std::fs::write(cache.join("data_0"), b"shared blocks").unwrap();
    std::fs::write(cache.join("index"), b"cache index").unwrap();
    let index = scan(
        &cache,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let page = rules::query_browser_cache(Some(&index), fixture.path(), now, 0, 50).unwrap();
    assert_eq!((page.total, page.items.len()), (1, 1));
    let id = page.items[0].id;
    let plan = rules::preview_browser_cache(&index, fixture.path(), now, &[id]).unwrap();
    let writer = std::fs::OpenOptions::new()
        .write(true)
        .open(&payload)
        .unwrap();
    assert!(plan.verify(id).is_err());
    drop(writer);
    let mut journal = Journal::open(&fixture.path().join("cleanup.sqlite")).unwrap();
    let report = permanent(plan, &mut journal, &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!(report.items.len(), 1);
    assert!(report.items[0].removed && report.items[0].recorded && report.items[0].error.is_none());
    assert!(report.journal_error.is_none() && !report.cancelled);
    assert!(!payload.exists());
    assert_eq!(
        std::fs::read(cache.join("data_0")).unwrap(),
        b"shared blocks"
    );
    assert_eq!(std::fs::read(cache.join("index")).unwrap(), b"cache index");
}

#[test]
fn browser_cache_requires_canonical_nonzero_28_bit_external_filenames() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture
        .path()
        .join("Google/Chrome/User Data/Default/Cache/Cache_Data");
    std::fs::create_dir_all(&cache).unwrap();
    let now = 2_000_000_000;
    for name in [
        "f_000001",
        "f_ffffff",
        "f_1000000",
        "f_fffffff",
        "f_000000",
        "f_0000001",
        "f_00000001",
        "f_10000000",
        "f_ABCDEF",
        "f_+00001",
    ] {
        let path = cache.join(name);
        std::fs::write(&path, [1]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(now - 8 * 24 * 60 * 60))
            .unwrap();
    }
    let index = scan(
        &cache,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let page = rules::query_browser_cache(Some(&index), fixture.path(), now, 0, 50).unwrap();
    let mut names: Vec<_> = page
        .items
        .iter()
        .map(|item| {
            std::path::Path::new(&item.path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
        })
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["f_000001", "f_1000000", "f_ffffff", "f_fffffff"]);
    assert_eq!((page.total, page.logical), (4, 4));
}
