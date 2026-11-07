use super::*;
use crate::{ScanOptions, scanner};

fn duplicate_plan(root: &std::path::Path, state: &std::path::Path) -> Plan {
    use crate::cleanup::tray::{
        self, Environment, Group as TrayGroup, Metric, Source, Target, TargetProgress,
    };
    use crate::duplicates::{Analysis, Group, Member};
    for (name, content) in [
        ("keep-a", b"alpha".as_slice()),
        ("a1", b"alpha"),
        ("a2", b"alpha"),
        ("keep-b", b"bravo"),
        ("b1", b"bravo"),
        ("older.tmp", b"old-data"),
    ] {
        std::fs::write(root.join(name), content).unwrap();
    }
    let now = crate::cleanup::rules::now().unwrap();
    for (name, days) in [("a1", 100), ("older.tmp", 300)] {
        std::fs::OpenOptions::new()
            .write(true)
            .open(root.join(name))
            .unwrap()
            .set_modified(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(now - days * 86400),
            )
            .unwrap();
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
    let protections = crate::metadata::Store::open(state)
        .unwrap()
        .protections()
        .unwrap();
    let analysis = Analysis {
        groups,
        protection_fingerprint: protections.fingerprint().to_owned(),
        ..Default::default()
    };
    let env = Environment {
        temporary: root,
        local_data: root,
        caches: &[],
        state,
        now,
        duplicates: Some(&analysis),
    };
    let duplicates = TrayGroup {
        source: Source::Duplicates,
        selection: vec![id("a1"), id("a2"), id("b1")],
    };
    let cancel = AtomicBool::new(false);
    use crate::cleanup::{rules, score};
    let mut rules_env = rules::Environment {
        temporary: root,
        local_data: root,
        caches: &[],
        now,
        protections: &protections,
        duplicates: Some(&analysis),
    };
    let mut request = rules::Query {
        offset: 0,
        limit: 1,
        weights: score::Weights::default(),
    };
    let first = rules::query(
        Some(&index),
        rules::RuleKind::OldFiles,
        request,
        &rules_env,
        &cancel,
    )
    .unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.items[0].file.id, id("a1"));
    assert_eq!(
        (
            first.items[0].priority.points,
            first.items[0].priority.coverage
        ),
        (4405, 80)
    );
    request.offset = 1;
    let second = rules::query(
        Some(&index),
        rules::RuleKind::OldFiles,
        request,
        &rules_env,
        &cancel,
    )
    .unwrap();
    assert_eq!(second.items[0].file.id, id("older.tmp"));
    request.offset = 0;
    request.weights.age = 100;
    request.weights.duplication = 0;
    request.weights.recovery = 0;
    let reweighted = rules::query(
        Some(&index),
        rules::RuleKind::OldFiles,
        request,
        &rules_env,
        &cancel,
    )
    .unwrap();
    assert_eq!(reweighted.items[0].file.id, id("older.tmp"));
    request.weights.age = 101;
    assert!(
        rules::query(
            Some(&index),
            rules::RuleKind::OldFiles,
            request,
            &rules_env,
            &cancel
        )
        .is_err()
    );
    request.weights = score::Weights::default();
    rules_env.duplicates = None;
    let missing = rules::query(
        Some(&index),
        rules::RuleKind::OldFiles,
        request,
        &rules_env,
        &cancel,
    )
    .unwrap();
    assert_eq!(missing.items[0].file.id, id("older.tmp"));
    assert_eq!(missing.items[0].priority.coverage, 55);
    let conflict = tray::prepare(
        &index,
        &[
            duplicates.clone(),
            TrayGroup {
                source: Source::Manual,
                selection: vec![id("keep-a")],
            },
        ],
        &env,
        &cancel,
    )
    .err()
    .unwrap();
    assert!(conflict.to_string().contains("retained duplicate copy"));
    let groups = [
        duplicates,
        TrayGroup {
            source: Source::OldFiles,
            selection: vec![id("older.tmp")],
        },
    ];
    let review = tray::summarize(
        &index,
        &groups,
        Some(&Target {
            metric: Metric::Logical,
            expression: "24B".into(),
        }),
        &cancel,
    )
    .unwrap();
    assert_eq!(review.selected_logical, "23");
    assert_eq!(review.groups[0].items.len(), 3);
    assert_eq!(review.groups[1].items[0].name, "older.tmp");
    assert_eq!(
        review.remaining_logical.unwrap().parse::<u64>().unwrap() + 23,
        index.entries[0].logical
    );
    assert!(
        matches!(review.target, Some(TargetProgress::Measured { ref selected_bytes, reached: false, complete: true, .. }) if selected_bytes == "23")
    );
    let invalid = tray::summarize(
        &index,
        &groups,
        Some(&Target {
            metric: Metric::Logical,
            expression: "1 cloud".into(),
        }),
        &cancel,
    )
    .unwrap();
    assert!(matches!(
        invalid.target,
        Some(TargetProgress::Invalid { .. })
    ));
    assert_eq!(invalid.groups.len(), 2);
    let prepared = tray::prepare(&index, &groups, &env, &cancel).unwrap();
    assert_eq!(prepared.origins.len(), 2);
    assert_eq!(prepared.plan.preview().items.len(), 4);
    assert_eq!(prepared.plan.preview().retained.len(), 2);
    assert_eq!(std::fs::read(root.join("older.tmp")).unwrap(), b"old-data");
    prepared.plan
}

#[test]
fn changed_keeper_aborts_before_intent_and_duplicate_cancellation_preserves_pending_files() {
    let fixture = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let plan = duplicate_plan(fixture.path(), state.path());
    std::fs::write(fixture.path().join("keep-b"), b"altered").unwrap();
    let mut journal = Journal::open(&fixture.path().join("cleanup.sqlite")).unwrap();
    assert!(
        permanent(plan, &mut journal, &AtomicBool::new(false), |_| panic!(
            "No removal allowed"
        ))
        .is_err()
    );
    assert!(journal.items(0, 100).unwrap().is_empty());
    for name in ["a1", "a2", "b1", "older.tmp"] {
        assert!(fixture.path().join(name).exists());
    }
    // Failure while acquiring the second keeper releases the first keeper too.
    std::fs::rename(
        fixture.path().join("keep-a"),
        fixture.path().join("released-a"),
    )
    .unwrap();
    let plan = duplicate_plan(fixture.path(), state.path());
    let cancel = AtomicBool::new(false);
    let report = permanent(plan, &mut journal, &cancel, |completed| {
        if completed == 1 {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();
    assert!(report.cancelled && report.items.len() == 1 && report.items[0].removed);
    assert!(fixture.path().join("a2").exists() && fixture.path().join("b1").exists());
    assert_eq!(
        std::fs::read(fixture.path().join("older.tmp")).unwrap(),
        b"old-data"
    );
    let rows = journal.items(0, 100).unwrap();
    assert_eq!(
        rows.iter().filter(|item| item.state == "cancelled").count(),
        3
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
