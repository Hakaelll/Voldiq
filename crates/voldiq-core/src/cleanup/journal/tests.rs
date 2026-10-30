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
    relocation_migration_and_interruption(&fixture);
}

fn relocation_migration_and_interruption(fixture: &tempfile::TempDir) {
    use crate::windows::cleanup_stage::{StageIdentity, StageName};
    let root = fixture.path().join("relocation-fixture");
    std::fs::create_dir(&root).unwrap();
    let (plan, nodes) = fixture_plan(
        &root,
        &["prepared", "ready", "moving", "staged", "returning"],
    );
    let path = fixture.path().join("legacy.sqlite");
    let legacy = Connection::open(&path).unwrap();
    legacy.execute_batch(SCHEMA).unwrap();
    legacy.execute_batch(&format!("PRAGMA application_id={APPLICATION_ID};
        INSERT INTO operations VALUES(1,'manual','permanent',42,'finished');
        INSERT INTO items VALUES(1,7,'historical.bin',zeroblob(16),zeroblob(8),4,1,0,'succeeded',NULL);")).unwrap();
    drop(legacy);
    let mut journal = Journal::open(&path).unwrap();
    assert_eq!(
        journal
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    let history = journal.items(0, 100).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(
        (
            history[0].path.as_str(),
            history[0].state.as_str(),
            history[0].created
        ),
        ("historical.bin", "succeeded", 42)
    );
    let mut old = Vec::new();
    for (phase, node) in nodes.iter().enumerate() {
        let source = plan.verify(*node).unwrap();
        let name = StageName::generate().unwrap();
        let id = journal
            .prepare_relocation(&plan, *node, &source, &name)
            .unwrap();
        // Journal-only transition data; native stage acquisition is covered by the executable fixture.
        let record = journal.relocations(0, 1).unwrap().remove(0);
        assert_eq!(
            record.path,
            root.canonicalize()
                .unwrap()
                .join(["prepared", "ready", "moving", "staged", "returning"][phase])
                .to_str()
                .unwrap()
        );
        assert!(
            journal
                .prepare_relocation(&plan, *node, &source, &StageName::generate().unwrap())
                .is_err()
        );
        if phase > 0 {
            assert!(
                journal
                    .relocation_stage_ready(
                        id,
                        StageIdentity {
                            volume: record.source.volume.wrapping_add(1),
                            file: [1; 16]
                        }
                    )
                    .is_err()
            );
            journal
                .relocation_stage_ready(
                    id,
                    StageIdentity {
                        volume: record.source.volume,
                        file: [phase as u8; 16],
                    },
                )
                .unwrap();
        }
        if phase > 1 {
            journal.start_relocation(id).unwrap();
            assert!(journal.start_relocation(id).is_err());
        }
        if phase > 2 {
            journal.record_staged(id).unwrap();
        }
        if phase > 3 {
            journal.start_return(id).unwrap();
        }
        old.push(id);
    }
    drop(journal);
    for _ in 0..2 {
        let mut journal = Journal::open(&path).unwrap();
        let records = journal.relocations(0, 100).unwrap();
        assert_eq!(records.len(), 5);
        for record in records {
            assert_eq!(
                record.state,
                if record.node == nodes[0] {
                    "cancelled"
                } else {
                    "unknown"
                }
            );
            assert_eq!(std::fs::read(&record.path).unwrap(), b"fixture content");
        }
        for id in &old {
            assert!(journal.start_relocation(*id).is_err());
        }
        assert_eq!(journal.items(0, 100).unwrap()[0].state, "succeeded");
    }
}
