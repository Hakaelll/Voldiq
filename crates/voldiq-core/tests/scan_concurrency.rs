use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};
use voldiq_core::{Kind, ScanConcurrency, ScanMode, ScanOptions, ScanPhase, scanner};

fn options(workers: u8) -> ScanOptions {
    ScanOptions {
        mode: ScanMode::Directory,
        concurrency: ScanConcurrency::try_from(workers).unwrap(),
        ..ScanOptions::default()
    }
}

#[test]
fn bounded_parallel_metadata_keeps_paths_sizes_identities_exclusions_and_hard_links() {
    let fixture = tempfile::tempdir().unwrap();
    for directory in 0..12 {
        let path = fixture.path().join(format!("目录-{directory}"));
        std::fs::create_dir(&path).unwrap();
        for file in 0..32 {
            std::fs::write(path.join(format!("{file}.bin")), vec![file as u8; file + 1]).unwrap();
        }
    }
    std::fs::hard_link(
        fixture.path().join("目录-0/0.bin"),
        fixture.path().join("alias.bin"),
    )
    .unwrap();
    let excluded = std::fs::canonicalize(fixture.path().join("目录-11")).unwrap();
    let mut expected = None;
    for concurrency in [
        ScanConcurrency::try_from(1).unwrap(),
        ScanConcurrency::try_from(2).unwrap(),
        ScanConcurrency::try_from(4).unwrap(),
        ScanConcurrency::try_from(8).unwrap(),
        ScanConcurrency::automatic(),
    ] {
        let mut options = options(1);
        options.concurrency = concurrency;
        options.exclusions = vec![excluded.to_str().unwrap().into()];
        let index =
            scanner::scan(fixture.path(), &options, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(index.entries[0].files, 353);
        assert_eq!(index.entries[0].logical, 11 * 528 + 1);
        assert!(!index.entries[0].incomplete);
        assert_eq!(index.error_count, 0);
        let rows: BTreeMap<_, _> = index
            .entries
            .iter()
            .map(|entry| {
                let path = index.path(entry.id).unwrap();
                assert!(!path.starts_with(&excluded));
                (
                    path,
                    (
                        entry.kind,
                        entry.logical,
                        entry.allocated,
                        entry.files,
                        entry.hard_link,
                        entry.file_state,
                    ),
                )
            })
            .collect();
        if let Some(expected) = &expected {
            assert_eq!(&rows, expected);
        } else {
            expected = Some(rows);
        }
    }
}

#[test]
fn parallel_cancellation_keeps_live_queries_consistent_and_limits_remain_exact() {
    let fixture = tempfile::tempdir().unwrap();
    for file in 0..300 {
        std::fs::write(fixture.path().join(format!("{file}.bin")), [0_u8; 9]).unwrap();
    }
    let mut pre_cancelled_options = options(1);
    pre_cancelled_options.concurrency = ScanConcurrency::automatic();
    let mut phase = ScanPhase::Scanning;
    let cancelled = scanner::scan(
        fixture.path(),
        &pre_cancelled_options,
        &AtomicBool::new(true),
        |update| phase = update.phase,
    )
    .unwrap();
    assert_eq!(cancelled.entries.len(), 1);
    assert_eq!(cancelled.entries[0].files, 0);
    assert!(cancelled.entries[0].incomplete);
    assert_eq!(phase, ScanPhase::Cancelled);
    for concurrency in [
        ScanConcurrency::try_from(2).unwrap(),
        ScanConcurrency::try_from(8).unwrap(),
        ScanConcurrency::automatic(),
    ] {
        let mut options = options(1);
        options.concurrency = concurrency;
        options.max_entries = Some(19);
        let limited =
            scanner::scan(fixture.path(), &options, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(limited.entries.len(), 19);
        assert_eq!(limited.entries[0].logical, 18 * 9);
        assert!(limited.entries[0].incomplete);
        assert!(
            limited
                .issues
                .iter()
                .any(|issue| issue.message.contains("Index limit"))
        );
        options.max_entries = None;
        let cancel = AtomicBool::new(false);
        let published = RefCell::new(None::<scanner::SharedIndex>);
        let mut phase = ScanPhase::Scanning;
        let shared = scanner::scan_live(
            fixture.path(),
            &options,
            &cancel,
            |update| {
                phase = update.phase;
                let holder = published.borrow();
                let index = holder.as_ref().unwrap().read().unwrap();
                assert_eq!(update.logical, index.entries[0].logical);
                assert_eq!(
                    update.logical,
                    index
                        .entries
                        .iter()
                        .filter(|entry| entry.kind == Kind::File)
                        .map(|entry| entry.logical)
                        .sum::<u64>()
                );
                if update.phase == ScanPhase::Scanning && update.entries > 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            |shared| *published.borrow_mut() = Some(shared),
        )
        .unwrap();
        assert_eq!(phase, ScanPhase::Cancelled);
        let index = shared.read().unwrap();
        assert_eq!(index.entries.len(), 2);
        assert_eq!(index.entries[0].logical, 9);
        assert!(index.entries[0].incomplete);
        let cancelled =
            scanner::scan(fixture.path(), &options, &AtomicBool::new(true), |_| {}).unwrap();
        assert_eq!(cancelled.entries.len(), 1);
        assert!(cancelled.entries[0].incomplete);
    }
}

#[test]
fn vanished_pending_directory_marks_partial_coverage_without_stopping_the_pool() {
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("gone");
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("file.bin");
    std::fs::write(&file, b"removed before traversal").unwrap();
    let expected_path = std::fs::canonicalize(&directory).unwrap();
    let index = scanner::scan(
        fixture.path(),
        &options(8),
        &AtomicBool::new(false),
        |update| {
            if update.phase == ScanPhase::Scanning && update.entries == 2 {
                std::fs::remove_file(&file).unwrap();
                std::fs::remove_dir(&directory).unwrap();
            }
        },
    )
    .unwrap();
    assert_eq!(index.entries.len(), 2);
    assert!(index.entries[0].incomplete && index.entries[1].incomplete);
    assert_eq!(index.error_count, 1);
    assert_eq!(index.issues[0].path, expected_path.to_str().unwrap());
}
