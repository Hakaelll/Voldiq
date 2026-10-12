use super::*;
use crate::{ScanOptions, scanner};

#[test]
fn result_write_failure_preserves_partial_report_and_stops_before_next_file() {
    let fixture = tempfile::tempdir().unwrap();
    for name in ["approved", "pending"] {
        std::fs::write(fixture.path().join(name), b"original").unwrap();
    }
    let index = scanner::scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let ids: Vec<_> = ["approved", "pending"]
        .iter()
        .map(|name| {
            index
                .entries
                .iter()
                .find(|entry| entry.name == *name)
                .unwrap()
                .id
        })
        .collect();
    let plan = Plan::manual(&index, &ids).unwrap();
    let database = fixture.path().join("cleanup.sqlite");
    let mut journal = Journal::open(&database).unwrap();
    journal.reject_result_updates().unwrap();
    let report = permanent(plan, &mut journal, &AtomicBool::new(false), |_| {}).unwrap();
    assert!(report.journal_error.is_some());
    assert_eq!(report.items.len(), 1);
    assert!(report.items[0].removed && !report.items[0].recorded);
    assert!(!fixture.path().join("approved").exists());
    assert_eq!(
        std::fs::read(fixture.path().join("pending")).unwrap(),
        b"original"
    );
    drop(journal);
    let journal = Journal::open(&database).unwrap();
    let records = journal.items(0, 100).unwrap();
    assert!(
        records
            .iter()
            .any(|item| item.node == ids[0] && item.state == "unknown")
    );
    assert!(
        records
            .iter()
            .any(|item| item.node == ids[1] && item.state == "cancelled")
    );
}

#[test]
fn executor_logs_actual_removal_failure_and_cancellation_without_repeating_items() {
    let fixture = tempfile::tempdir().unwrap();
    for name in ["approved", "changed", "pending"] {
        std::fs::write(fixture.path().join(name), b"original").unwrap();
    }
    let index = scanner::scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let ids: Vec<_> = ["approved", "changed", "pending"]
        .iter()
        .map(|name| {
            index
                .entries
                .iter()
                .find(|entry| entry.name == *name)
                .unwrap()
                .id
        })
        .collect();
    let plan = Plan::manual(&index, &ids).unwrap();
    std::fs::write(fixture.path().join("changed"), b"changed").unwrap();
    let cancel = AtomicBool::new(false);
    let database = fixture.path().join("cleanup.sqlite");
    let mut journal = Journal::open(&database).unwrap();
    let report = permanent(plan, &mut journal, &cancel, |completed| {
        if completed == 2 {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();
    assert!(report.cancelled);
    assert_eq!(report.items.len(), 2);
    assert!(report.items[0].removed);
    assert!(!report.items[1].removed && report.items[1].error.is_some());
    assert!(!fixture.path().join("approved").exists());
    assert_eq!(
        std::fs::read(fixture.path().join("changed")).unwrap(),
        b"changed"
    );
    assert_eq!(
        std::fs::read(fixture.path().join("pending")).unwrap(),
        b"original"
    );
    drop(journal);
    let journal = Journal::open(&database).unwrap();
    let results = journal.items(0, 100).unwrap();
    assert!(
        results
            .iter()
            .any(|item| item.node == ids[0] && item.state == "succeeded")
    );
    assert!(
        results
            .iter()
            .any(|item| item.node == ids[1] && item.state == "failed")
    );
    assert!(
        results
            .iter()
            .any(|item| item.node == ids[2] && item.state == "cancelled")
    );
}

#[test]
fn pre_cancelled_plan_and_missing_duplicate_keep_authority_do_not_delete() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("approved");
    std::fs::write(&path, b"original").unwrap();
    let index = scanner::scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let id = index
        .entries
        .iter()
        .find(|entry| entry.name == "approved")
        .unwrap()
        .id;
    let mut journal = Journal::open(&fixture.path().join("cleanup.sqlite")).unwrap();
    let plan = Plan::manual(&index, &[id]).unwrap();
    let report = permanent(plan, &mut journal, &AtomicBool::new(true), |_| {
        panic!("No progress before cancellation")
    })
    .unwrap();
    assert!(report.cancelled && report.items.is_empty());
    let mut plan = Plan::manual(&index, &[id]).unwrap();
    plan.preview
        .retained
        .push(path.to_string_lossy().into_owned());
    assert!(permanent(plan, &mut journal, &AtomicBool::new(false), |_| {}).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"original");
}
