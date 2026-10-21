use super::*;
use crate::{ScanOptions, scanner};

fn duplicate_plan(root: &std::path::Path) -> Plan {
    use crate::duplicates::{Analysis, Group, Member};
    for (name, content) in [
        ("keep-a", b"alpha".as_slice()),
        ("a1", b"alpha"),
        ("a2", b"alpha"),
        ("keep-b", b"bravo"),
        ("b1", b"bravo"),
    ] {
        std::fs::write(root.join(name), content).unwrap();
    }
    let index = scanner::scan(
        root,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let id = |name| {
        index
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
            .id
    };
    let groups = [
        (["keep-a", "a1", "a2"].as_slice(), b"alpha".as_slice()),
        (["keep-b", "b1"].as_slice(), b"bravo".as_slice()),
    ]
    .into_iter()
    .map(|(names, content)| Group {
        digest: blake3::hash(content).to_hex().to_string(),
        logical: content.len() as u64,
        recoverable: 0,
        members: names
            .iter()
            .map(|name| Member {
                id: id(*name),
                allocated: 0,
                links: 1,
            })
            .collect(),
    })
    .collect();
    Plan::duplicates(
        &index,
        &Analysis {
            groups,
            ..Analysis::default()
        },
        &[id("a1"), id("a2"), id("b1")],
        &AtomicBool::new(false),
    )
    .unwrap()
}

#[test]
fn changed_keeper_aborts_before_intent_and_duplicate_cancellation_preserves_pending_files() {
    let fixture = tempfile::tempdir().unwrap();
    let plan = duplicate_plan(fixture.path());
    std::fs::write(fixture.path().join("keep-b"), b"altered").unwrap();
    let mut journal = Journal::open(&fixture.path().join("cleanup.sqlite")).unwrap();
    assert!(
        permanent(plan, &mut journal, &AtomicBool::new(false), |_| panic!(
            "No removal allowed"
        ))
        .is_err()
    );
    assert!(journal.items(0, 100).unwrap().is_empty());
    for name in ["a1", "a2", "b1"] {
        assert!(fixture.path().join(name).exists());
    }
    // Failure while acquiring the second keeper releases the first keeper too.
    std::fs::rename(
        fixture.path().join("keep-a"),
        fixture.path().join("released-a"),
    )
    .unwrap();
    let plan = duplicate_plan(fixture.path());
    let cancel = AtomicBool::new(false);
    let report = permanent(plan, &mut journal, &cancel, |completed| {
        if completed == 1 {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();
    assert!(report.cancelled && report.items.len() == 1 && report.items[0].removed);
    assert!(fixture.path().join("a2").exists() && fixture.path().join("b1").exists());
    let rows = journal.items(0, 100).unwrap();
    assert_eq!(
        rows.iter().filter(|item| item.state == "cancelled").count(),
        2
    );
    assert_eq!(
        std::fs::read(fixture.path().join("keep-a")).unwrap(),
        b"alpha"
    );
    assert_eq!(
        std::fs::read(fixture.path().join("keep-b")).unwrap(),
        b"bravo"
    );
}

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
