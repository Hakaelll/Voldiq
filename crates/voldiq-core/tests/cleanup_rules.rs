use std::{
    sync::atomic::AtomicBool,
    time::{Duration, UNIX_EPOCH},
};
use voldiq_core::{ScanOptions, cleanup::rules, scanner::scan};

#[test]
fn temporary_rule_requires_indexed_scope_extension_age_and_explicit_current_selection() {
    let fixture = tempfile::tempdir().unwrap();
    let temporary = fixture.path().join("Temp");
    std::fs::create_dir(&temporary).unwrap();
    let now = 2_000_000_000;
    let old = now - 8 * 24 * 60 * 60;
    for (name, modified) in [
        ("old.tmp", old),
        ("old.TEMP", old + 1),
        ("recent.tmp", now),
        ("data.sqlite", old),
        ("config.json", old),
    ] {
        let path = temporary.join(name);
        std::fs::write(&path, [1; 24]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(modified))
            .unwrap();
    }
    let outside = fixture.path().join("outside.tmp");
    std::fs::write(&outside, [1; 24]).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&outside)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(old))
        .unwrap();
    let mut index = scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let absent = rules::query(None, &temporary, now, 0, 100).unwrap();
    assert!(!absent.indexed);
    let page = rules::query(Some(&index), &temporary, now, 0, 1).unwrap();
    assert!(page.indexed && page.complete);
    assert_eq!(page.total, 2);
    assert_eq!(page.logical, 48);
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].path.ends_with("old.tmp"));
    let next = rules::query(Some(&index), &temporary, now, 1, 1).unwrap();
    assert!(next.items[0].path.ends_with("old.TEMP"));
    let id = page.items[0].id;
    let preview = rules::preview(&index, &temporary, now, &[id]).unwrap();
    assert_eq!(preview.preview().rule, rules::USER_TEMP.id);
    assert_eq!(preview.preview().items.len(), 1);
    let outside_id = index
        .entries
        .iter()
        .find(|e| e.name == "outside.tmp")
        .unwrap()
        .id;
    let recent = index
        .entries
        .iter()
        .find(|e| e.name == "recent.tmp")
        .unwrap()
        .id;
    assert!(rules::preview(&index, &temporary, now, &[outside_id]).is_err());
    assert!(rules::preview(&index, &temporary, now, &[recent]).is_err());
    assert_eq!(
        rules::query(Some(&index), &temporary, 0, 0, 50)
            .unwrap()
            .total,
        0
    );
    index.snapshot_id = Some(1);
    let historical = rules::query(Some(&index), &temporary, now, 0, 50).unwrap();
    assert!(!historical.indexed && historical.items.is_empty());
    assert!(rules::preview(&index, &temporary, now, &[id]).is_err());
    index.snapshot_id = None;
    index.entries[0].incomplete = true;
    let cancelled = rules::query(Some(&index), &temporary, now, 0, 50).unwrap();
    assert!(cancelled.indexed && !cancelled.complete);
    assert_eq!(cancelled.total, 2);
    index.entries[0].incomplete = false;
    std::fs::write(temporary.join("old.tmp"), [2; 24]).unwrap();
    assert!(rules::preview(&index, &temporary, now, &[id]).is_err());
}

#[test]
fn excluded_or_unscanned_temporary_roots_do_not_report_a_complete_empty_scan() {
    let fixture = tempfile::tempdir().unwrap();
    let temporary = fixture.path().join("Temp");
    std::fs::create_dir(&temporary).unwrap();
    let index = scan(
        fixture.path(),
        &ScanOptions {
            exclusions: vec![temporary.to_string_lossy().into_owned()],
            ..ScanOptions::default()
        },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let page = rules::query(Some(&index), &temporary, 2_000_000_000, 0, 50).unwrap();
    assert!(!page.indexed && !page.complete);
    let other = tempfile::tempdir().unwrap();
    assert!(
        !rules::query(Some(&index), other.path(), 2_000_000_000, 0, 50)
            .unwrap()
            .indexed
    );
    let narrow = scan(
        &temporary,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let nested = temporary.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let subset = scan(
        &nested,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let partial = rules::query(Some(&subset), &temporary, 2_000_000_000, 0, 50).unwrap();
    assert!(partial.indexed && !partial.complete);
    assert!(
        rules::query(Some(&narrow), &temporary, 2_000_000_000, 0, 50)
            .unwrap()
            .indexed
    );
}
