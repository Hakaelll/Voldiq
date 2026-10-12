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
fn durable_results_distinguish_failure_cancellation_and_success() {
    let fixture = tempfile::tempdir().unwrap();
    let (plan, ids) = fixture_plan(
        fixture.path(),
        &["success.bin", "failure.bin", "cancel.bin"],
    );
    let database = fixture.path().join("cleanup.sqlite");
    let mut journal = Journal::open(&database).unwrap();
    let operation = journal.prepare_permanent(&plan).unwrap();
    assert!(journal.finish(operation).is_err());
    journal.start_item(operation, ids[0]).unwrap();
    assert!(journal.start_item(operation, ids[0]).is_err());
    plan.verify(ids[0]).unwrap().remove_permanently().unwrap();
    journal
        .complete_item(operation, ids[0], Outcome::Succeeded)
        .unwrap();
    assert!(
        journal
            .complete_item(operation, ids[0], Outcome::Succeeded)
            .is_err()
    );
    journal.start_item(operation, ids[1]).unwrap();
    let failure = fixture.path().join("failure.bin");
    let original = std::fs::metadata(&failure).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&failure, readonly).unwrap();
    let error = plan
        .verify(ids[1])
        .err()
        .expect("Changed file must fail verification");
    std::fs::set_permissions(&failure, original).unwrap();
    journal
        .complete_item(operation, ids[1], Outcome::Failed(error.to_string()))
        .unwrap();
    journal.cancel_pending(operation).unwrap();
    journal.finish(operation).unwrap();
    assert!(journal.start_item(operation, ids[2]).is_err());
    let exact: Vec<u8> = journal
        .connection
        .query_row(
            "SELECT identity FROM items WHERE node=?1",
            [ids[0].0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(&exact[..8], &plan.files[0].stamp.identity.0.to_le_bytes());
    assert_eq!(&exact[8..], &plan.files[0].stamp.identity.1.to_le_bytes());
    drop(journal);
    let journal = Journal::open(&database).unwrap();
    let results = journal.items(0, 100).unwrap();
    assert_eq!(results.len(), 3);
    assert!(
        results
            .iter()
            .any(|item| item.node == ids[0] && item.state == "succeeded")
    );
    assert!(
        results
            .iter()
            .any(|item| item.node == ids[1] && item.state == "failed" && item.message.is_some())
    );
    assert!(
        results
            .iter()
            .any(|item| item.node == ids[2] && item.state == "cancelled")
    );
    assert_eq!(journal.items(0, 1).unwrap().len(), 1);
    assert_eq!(journal.items(1, 1).unwrap().len(), 1);
    assert!(journal.items(3, 100).unwrap().is_empty());
    assert!(!fixture.path().join("success.bin").exists());
    assert_eq!(std::fs::read(failure).unwrap(), b"fixture content");
    assert_eq!(
        std::fs::read(fixture.path().join("cancel.bin")).unwrap(),
        b"fixture content"
    );
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

#[test]
fn another_owner_cannot_recover_live_work_and_failed_intent_never_starts() {
    let fixture = tempfile::tempdir().unwrap();
    let (plan, ids) = fixture_plan(fixture.path(), &["pending.bin"]);
    let database = fixture.path().join("cleanup.sqlite");
    let mut journal = Journal::open(&database).unwrap();
    let operation = journal.prepare_permanent(&plan).unwrap();
    assert!(Journal::open(&database).is_err());
    let mut other = Journal::open(&fixture.path().join("other.sqlite")).unwrap();
    let other_operation = other.prepare_permanent(&plan).unwrap();
    assert!(other.start_item(operation, ids[0]).is_err());
    assert!(journal.start_item(other_operation, ids[0]).is_err());
    assert_eq!(journal.items(0, 1).unwrap()[0].state, "prepared");
    journal
        .connection
        .execute_batch("PRAGMA query_only=ON;")
        .unwrap();
    assert!(journal.start_item(operation, ids[0]).is_err());
    assert_eq!(journal.items(0, 1).unwrap()[0].state, "prepared");
    assert_eq!(
        std::fs::read(fixture.path().join("pending.bin")).unwrap(),
        b"fixture content"
    );
}

#[test]
fn invalid_plan_rolls_back_and_foreign_or_future_databases_are_rejected() {
    assert!(Journal::open(Path::new(":memory:")).is_err());
    let fixture = tempfile::tempdir().unwrap();
    let (mut plan, _) = fixture_plan(fixture.path(), &["pending.bin"]);
    let database = fixture.path().join("cleanup.sqlite");
    let mut journal = Journal::open(&database).unwrap();
    plan.files[0].stamp.size = u64::MAX;
    assert!(journal.prepare_permanent(&plan).is_err());
    assert!(journal.items(0, 100).unwrap().is_empty());
    let count: i64 = journal
        .connection
        .query_row("SELECT count(*) FROM operations", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    drop(journal);
    let connection = Connection::open(&database).unwrap();
    connection.execute_batch("PRAGMA user_version=99;").unwrap();
    drop(connection);
    assert!(Journal::open(&database).is_err());
    let foreign = fixture.path().join("foreign.sqlite");
    let connection = Connection::open(&foreign).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE personal(value TEXT); INSERT INTO personal VALUES('preserve');",
        )
        .unwrap();
    drop(connection);
    assert!(Journal::open(&foreign).is_err());
    let connection = Connection::open(&foreign).unwrap();
    let value: String = connection
        .query_row("SELECT value FROM personal", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "preserve");
}
