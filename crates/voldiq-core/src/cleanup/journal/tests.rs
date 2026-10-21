use super::*;
use crate::{ScanOptions, scanner};
use std::sync::atomic::AtomicBool;

fn fixture_plan(root: &Path, names: &[&str]) -> (Plan, Vec<NodeId>) {
    for name in names {
        std::fs::write(root.join(name), b"fixture content").unwrap();
    }
    let index = scanner::scan(
        root,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let ids: Vec<_> = names
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
    (Plan::manual(&index, &ids).unwrap(), ids)
}

#[test]
fn interruption_after_deletion_never_retries_a_replacement_or_claims_success() {
    let fixture = tempfile::tempdir().unwrap();
    let (plan, ids) = fixture_plan(fixture.path(), &["started.bin", "pending.bin"]);
    let database = fixture.path().join("cleanup.sqlite");
    let mut journal = Journal::open(&database).unwrap();
    let operation = journal.prepare_permanent(&plan).unwrap();
    journal.start_item(operation, ids[0]).unwrap();
    plan.verify(ids[0]).unwrap().remove_permanently().unwrap();
    journal
        .connection
        .execute_batch("PRAGMA query_only=ON;")
        .unwrap();
    assert!(
        journal
            .complete_item(operation, ids[0], Outcome::Succeeded)
            .is_err()
    );
    drop(journal);
    std::fs::write(fixture.path().join("started.bin"), b"replacement").unwrap();
    for _ in 0..2 {
        let mut journal = Journal::open(&database).unwrap();
        let results = journal.items(0, 100).unwrap();
        assert!(
            results
                .iter()
                .any(|item| item.node == ids[0] && item.state == "unknown")
        );
        assert!(
            results
                .iter()
                .any(|item| item.node == ids[1] && item.state == "cancelled")
        );
        assert!(journal.start_item(operation, ids[0]).is_err());
        assert_eq!(
            std::fs::read(fixture.path().join("started.bin")).unwrap(),
            b"replacement"
        );
        assert_eq!(
            std::fs::read(fixture.path().join("pending.bin")).unwrap(),
            b"fixture content"
        );
    }
}
