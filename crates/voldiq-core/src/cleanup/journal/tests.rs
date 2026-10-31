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
    recycling_receipts_and_migration(&fixture);
    handle_bound_return_preserves_occupied_names(&fixture);
    activity_keeps_both_operation_kinds_and_uncertain_results(&fixture);
    version3_receipts_do_not_gain_invented_versions(&fixture);
    historical_restoration_requires_fresh_authority(&fixture);
}

fn version3_receipts_do_not_gain_invented_versions(fixture: &tempfile::TempDir) {
    let path = fixture.path().join("version3.sqlite");
    let survivor = fixture.path().join("version3-survivor.bin");
    std::fs::write(&survivor, b"untouched").unwrap();
    let legacy = Connection::open(&path).unwrap();
    legacy.execute_batch(SCHEMA).unwrap();
    legacy.execute_batch(super::relocation::SCHEMA).unwrap();
    legacy
        .execute_batch(super::relocation::RECYCLING_MIGRATION)
        .unwrap();
    let bin: Vec<u8> = "C:\\legacy-bin"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    legacy.execute_batch(&format!("PRAGMA application_id={APPLICATION_ID};
        INSERT INTO operations VALUES(1,'manual','permanent',42,'finished');
        INSERT INTO items VALUES(1,7,'historical.bin',zeroblob(16),zeroblob(8),4,1,0,'succeeded',NULL);")).unwrap();
    legacy.execute("INSERT INTO relocations(id,node,path,source_volume,source_file,parent_file,change_time,size,modified_seconds,modified_nanos,stage_name,stage_file,state,created,bin_path)
        VALUES(27,1,'C:\\legacy-original.bin',zeroblob(8),zeroblob(16),zeroblob(16),zeroblob(8),4,1,0,'voldiq-stage-00000000000000000000000000000001',zeroblob(16),'recycled',1,?1)",params![bin]).unwrap();
    legacy
        .execute_batch("UPDATE sqlite_sequence SET seq=90 WHERE name='relocations';")
        .unwrap();
    drop(legacy);
    for _ in 0..2 {
        let journal = Journal::open(&path).unwrap();
        assert_eq!(
            journal
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert_eq!(
            journal
                .connection
                .query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='relocations'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            90
        );
        let records = journal.relocations(0, 100).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, 27);
        assert_eq!(records[0].state, "recycled");
        assert_eq!(
            records[0].bin_path.as_deref(),
            Some(Path::new("C:\\legacy-bin"))
        );
        assert!(records[0].recycled_version.is_none());
        assert_eq!(journal.items(0, 100).unwrap()[0].state, "succeeded");
        assert_eq!(std::fs::read(&survivor).unwrap(), b"untouched");
    }
    let journal = Journal::open(&path).unwrap();
    assert!(
        journal
            .connection
            .execute("UPDATE relocations SET recycled_change_time=x'010203'", [])
            .is_err()
    );
    journal
        .connection
        .execute(
            "UPDATE relocations SET recycled_change_time=zeroblob(8),modified_nanos=1000000000",
            [],
        )
        .unwrap();
    assert!(journal.relocations(0, 100).is_err());
    journal
        .connection
        .execute(
            "UPDATE relocations SET modified_nanos=0,modified_seconds=-1",
            [],
        )
        .unwrap();
    assert!(journal.relocations(0, 100).is_err());
    journal
        .connection
        .execute(
            "UPDATE relocations SET modified_seconds=9223372036854775807",
            [],
        )
        .unwrap();
    assert!(journal.relocations(0, 100).is_err());
    assert_eq!(std::fs::read(survivor).unwrap(), b"untouched");
}

fn activity_keeps_both_operation_kinds_and_uncertain_results(fixture: &tempfile::TempDir) {
    let path = fixture.path().join("activity.sqlite");
    let survivor = fixture.path().join("activity-survivor.bin");
    std::fs::write(&survivor, b"untouched").unwrap();
    let journal = Journal::open(&path).unwrap();
    journal.connection.execute_batch("INSERT INTO operations VALUES(1,'manual','permanent',10,'finished');
        INSERT INTO items VALUES(1,7,'removed.bin',zeroblob(16),zeroblob(8),4,1,0,'succeeded',NULL);
        INSERT INTO relocations(id,node,path,source_volume,source_file,parent_file,change_time,size,modified_seconds,modified_nanos,stage_name,state,message,created)
        VALUES(1,7,'uncertain.bin',zeroblob(8),zeroblob(16),zeroblob(16),zeroblob(8),4,1,0,'voldiq-stage-00000000000000000000000000000001','unknown','Review required',20);").unwrap();
    let native = |value: &str| {
        value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    journal.connection.execute("INSERT INTO relocations(id,node,path,source_volume,source_file,parent_file,change_time,size,modified_seconds,modified_nanos,stage_name,stage_file,state,created,bin_path,restore_path)
        VALUES(2,8,'restored.bin',zeroblob(8),zeroblob(16),zeroblob(16),zeroblob(8),4,1,0,'voldiq-stage-00000000000000000000000000000002',zeroblob(16),'restored',30,?1,?2)",
        params![native("C:\\fixture-bin"),native("C:\\fixture-destination")]).unwrap();
    drop(journal);
    let journal = Journal::open(&path).unwrap();
    let first = journal.activity(0, 2).unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].kind, ActivityKind::Recycling);
    assert_eq!(first[0].item.path, "restored.bin");
    assert_eq!(first[0].item.state, "restored");
    assert_eq!(first[1].item.operation, 1);
    assert_eq!(first[1].item.node, NodeId(7));
    assert_eq!(first[1].item.state, "unknown");
    assert_eq!(first[1].item.message.as_deref(), Some("Review required"));
    let second = journal.activity(2, 2).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].kind, ActivityKind::Permanent);
    assert_eq!(second[0].item.operation, 1);
    assert_eq!(second[0].item.node, NodeId(7));
    assert_eq!(second[0].item.path, "removed.bin");
    assert_eq!(second[0].item.state, "succeeded");
    assert!(journal.activity(3, 2).unwrap().is_empty());
    assert_eq!(std::fs::read(survivor).unwrap(), b"untouched");
}

fn handle_bound_return_preserves_occupied_names(fixture: &tempfile::TempDir) {
    use crate::cleanup::recycle::file_identity as identity;
    use crate::windows::cleanup_stage::{StageEntry, restore_to_parent};
    let root = fixture.path().join("native-return");
    std::fs::create_dir(&root).unwrap();
    let (plan, ids) = fixture_plan(&root, &["owned.bin"]);
    let source = plan.verify(ids[0]).unwrap();
    let original = identity(&source.file).unwrap();
    let parent = source.stage_parent().unwrap();
    let occupied = StageEntry::parse("occupied.bin").unwrap();
    std::fs::write(root.join("occupied.bin"), b"replacement").unwrap();
    assert_eq!(
        restore_to_parent(&source.file, &parent, &occupied)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        std::fs::read(root.join("owned.bin")).unwrap(),
        b"fixture content"
    );
    assert_eq!(
        std::fs::read(root.join("occupied.bin")).unwrap(),
        b"replacement"
    );
    let returned = StageEntry::parse("returned.bin").unwrap();
    restore_to_parent(&source.file, &parent, &returned).unwrap();
    assert!(!root.join("owned.bin").exists());
    assert_eq!(
        std::fs::read(root.join("returned.bin")).unwrap(),
        b"fixture content"
    );
    assert_eq!(
        identity(&std::fs::File::open(root.join("returned.bin")).unwrap()).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read(root.join("occupied.bin")).unwrap(),
        b"replacement"
    );
    assert!(std::fs::rename(root.join("returned.bin"), root.join("changed.bin")).is_err());
    drop(source);
    std::fs::rename(root.join("returned.bin"), root.join("changed.bin")).unwrap();
    assert_eq!(
        std::fs::read(root.join("changed.bin")).unwrap(),
        b"fixture content"
    );
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
        4
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

fn recycling_receipts_and_migration(fixture: &tempfile::TempDir) {
    use super::relocation::RecycledVersion;
    use crate::windows::cleanup_stage::{StageIdentity, StageName};
    use std::os::windows::ffi::OsStringExt;
    let root = fixture.path().join("receipt-fixture");
    std::fs::create_dir(&root).unwrap();
    let (plan, nodes) = fixture_plan(
        &root,
        &[
            "in-flight",
            "recycled",
            "restoring",
            "restored",
            "write-failed",
        ],
    );
    let path = fixture.path().join("version2.sqlite");
    let legacy = Connection::open(&path).unwrap();
    legacy.execute_batch(SCHEMA).unwrap();
    legacy.execute_batch(super::relocation::SCHEMA).unwrap();
    legacy.execute_batch(&format!("PRAGMA application_id={APPLICATION_ID};
      INSERT INTO relocations VALUES(19,1,'C:\\legacy.bin',zeroblob(8),zeroblob(16),zeroblob(16),zeroblob(8),4,1,0,'voldiq-stage-00000000000000000000000000000001',zeroblob(16),'returned','preserved',1);
      INSERT INTO relocations SELECT 50,node,path,source_volume,source_file,parent_file,change_time,size,modified_seconds,modified_nanos,'voldiq-stage-00000000000000000000000000000002',stage_file,state,message,created FROM relocations WHERE id=19;
      DELETE FROM relocations WHERE id=50;")).unwrap();
    drop(legacy);
    let mut journal = Journal::open(&path).unwrap();
    let history = journal.relocations(0, 100).unwrap();
    assert_eq!(
        (
            history[0].id,
            history[0].state.as_str(),
            history[0].message.as_deref()
        ),
        (19, "returned", Some("preserved"))
    );
    assert!(history[0].bin_path.is_none());
    assert!(history[0].recycled_version.is_none());
    let mut units: Vec<u16> = r"C:\$Recycle.Bin\owned-".encode_utf16().collect();
    units.push(0xd800);
    let bin: std::path::PathBuf = std::ffi::OsString::from_wide(&units).into();
    let destination = root.join("explicit-destination");
    let mut sessions = Vec::new();
    for (phase, node) in nodes.iter().enumerate() {
        let source = plan.verify(*node).unwrap();
        let metadata = source.file.metadata().unwrap();
        let version = RecycledVersion {
            change_time: 0xfedc_ba98_7654_3210 + phase as u64,
            size: metadata.len(),
            modified: metadata.modified().unwrap(),
        };
        let id = journal
            .prepare_relocation(&plan, *node, &source, &StageName::generate().unwrap())
            .unwrap();
        let record = journal.relocations(0, 1).unwrap().remove(0);
        assert!(record.id > 50);
        let stage = StageIdentity {
            volume: record.source.volume,
            file: [(phase + 32) as u8; 16],
        };
        journal.relocation_stage_ready(id, stage).unwrap();
        journal.start_relocation(id).unwrap();
        journal.record_staged(id).unwrap();
        assert!(
            journal
                .start_recycling(
                    id,
                    StageIdentity {
                        volume: stage.volume,
                        file: [0; 16]
                    },
                    stage
                )
                .is_err()
        );
        journal.start_recycling(id, record.source, stage).unwrap();
        assert!(journal.start_recycling(id, record.source, stage).is_err());
        if phase == 0 {
            sessions.push(id);
            continue;
        }
        assert!(
            journal
                .record_recycled(id, record.source, Path::new("relative"), version)
                .is_err()
        );
        if phase == 4 {
            journal
                .connection
                .execute_batch("PRAGMA query_only=ON;")
                .unwrap();
            assert!(
                journal
                    .record_recycled(id, record.source, &bin, version)
                    .is_err()
            );
            journal
                .connection
                .execute_batch("PRAGMA query_only=OFF;")
                .unwrap();
            sessions.push(id);
            continue;
        }
        assert!(
            journal
                .record_recycled(
                    id,
                    record.source,
                    &bin,
                    RecycledVersion {
                        size: version.size + 1,
                        ..version
                    }
                )
                .is_err()
        );
        assert!(
            journal
                .record_recycled(
                    id,
                    record.source,
                    &bin,
                    RecycledVersion {
                        modified: version.modified + std::time::Duration::from_secs(1),
                        ..version
                    }
                )
                .is_err()
        );
        journal
            .record_recycled(id, record.source, &bin, version)
            .unwrap();
        if phase >= 2 {
            let mut wrong_source = record.source;
            wrong_source.file[0] ^= 1;
            assert!(
                journal
                    .start_restore(id, wrong_source, &destination)
                    .is_err()
            );
            journal
                .start_restore(id, record.source, &destination)
                .unwrap();
            if phase == 3 {
                assert!(
                    journal
                        .record_restored(id, record.source, &root.join("wrong-destination"))
                        .is_err()
                );
                journal
                    .record_restored(id, record.source, &destination)
                    .unwrap();
            }
        }
        sessions.push(id);
    }
    drop(journal);
    let mut journal = Journal::open(&path).unwrap();
    let records = journal.relocations(0, 100).unwrap();
    for (phase, node) in nodes.iter().enumerate() {
        let record = records
            .iter()
            .find(|record| record.node == *node && record.id > 50)
            .unwrap();
        assert_eq!(
            record.state,
            ["unknown", "recycled", "unknown", "restored", "unknown"][phase]
        );
        assert_eq!(
            record.bin_path,
            if (1..=3).contains(&phase) {
                Some(bin.clone())
            } else {
                None
            }
        );
        assert_eq!(
            record.restore_path,
            if (2..=3).contains(&phase) {
                Some(destination.clone())
            } else {
                None
            }
        );
        assert_eq!(std::fs::read(&record.path).unwrap(), b"fixture content");
        if (1..=3).contains(&phase) {
            let version = record.recycled_version.unwrap();
            assert_eq!(version.change_time, 0xfedc_ba98_7654_3210 + phase as u64);
            assert_eq!(version.size, 15);
            assert_eq!(
                version.modified,
                std::fs::metadata(&record.path).unwrap().modified().unwrap()
            );
        } else {
            assert!(record.recycled_version.is_none());
        }
        assert!(
            journal
                .start_restore(sessions[phase], record.source, &destination)
                .is_err()
        );
    }
    assert!(bin.to_str().is_none());
}

fn historical_restoration_requires_fresh_authority(fixture: &tempfile::TempDir) {
    use crate::cleanup::{journal::relocation::RecycledVersion, recycle::RestorationReview};
    use crate::windows::cleanup_stage::{StageEntry, StageName, restore_to_parent};
    use std::os::windows::fs::OpenOptionsExt;
    let root = fixture.path().join("historical-review");
    std::fs::create_dir(&root).unwrap();
    let original = root.join("original");
    let holding = root.join("owned-holding");
    std::fs::create_dir(&original).unwrap();
    std::fs::create_dir(&holding).unwrap();
    let (plan, ids) = fixture_plan(&original, &["owned.bin"]);
    let database = root.join("history.sqlite");
    let mut journal = Journal::open(&database).unwrap();
    let source = plan.verify(ids[0]).unwrap();
    let id = journal
        .prepare_relocation(&plan, ids[0], &source, &StageName::generate().unwrap())
        .unwrap();
    let record = journal.relocation(id.value()).unwrap().unwrap();
    assert!(journal.relocation(0).is_err());
    assert!(journal.relocation(i64::MAX).unwrap().is_none());
    assert!(RestorationReview::prepare(&journal, id.value()).is_err());
    let stage = record.parent;
    journal.relocation_stage_ready(id, stage).unwrap();
    journal.start_relocation(id).unwrap();
    journal.record_staged(id).unwrap();
    journal.start_recycling(id, record.source, stage).unwrap();
    // An owned holding directory exercises restart authority without invoking Shell or UAC.
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS)
        .open(&holding)
        .unwrap();
    restore_to_parent(
        &source.file,
        &directory,
        &StageEntry::parse("held.bin").unwrap(),
    )
    .unwrap();
    let metadata = source.file.metadata().unwrap();
    let version = RecycledVersion {
        change_time: crate::windows::file_identity(&source.file)
            .unwrap()
            .change_time,
        size: metadata.len(),
        modified: metadata.modified().unwrap(),
    };
    let locator = holding.join("held.bin");
    journal
        .record_recycled(id, record.source, &locator, version)
        .unwrap();
    drop(source);
    drop(directory);
    drop(journal);
    let mut journal = Journal::open(&database).unwrap();
    let destination = std::path::PathBuf::from(&record.path);
    assert!(
        journal
            .activity(0, 100)
            .unwrap()
            .iter()
            .any(|item| item.kind == ActivityKind::Recycling
                && item.item.operation == id.value()
                && item.can_restore)
    );
    let previous_parent = root.join("previous-original");
    std::fs::rename(&original, &previous_parent).unwrap();
    std::fs::create_dir(&original).unwrap();
    assert!(RestorationReview::prepare(&journal, id.value()).is_err());
    assert!(locator.exists());
    std::fs::remove_dir(&original).unwrap();
    std::fs::rename(&previous_parent, &original).unwrap();
    let review = RestorationReview::prepare(&journal, id.value()).unwrap();
    assert_eq!(review.destination(), destination);
    assert_eq!(review.size(), 15);
    assert!(std::fs::write(&locator, b"changed").is_err());
    std::fs::write(&destination, b"replacement").unwrap();
    assert!(review.restore(&mut journal).is_err());
    assert_eq!(std::fs::read(&destination).unwrap(), b"replacement");
    assert_eq!(std::fs::read(&locator).unwrap(), b"fixture content");
    assert_eq!(
        journal.relocation(id.value()).unwrap().unwrap().state,
        "recycled"
    );
    std::fs::remove_file(&destination).unwrap();
    let stale = RestorationReview::prepare(&journal, id.value()).unwrap();
    drop(journal);
    let mut journal = Journal::open(&database).unwrap();
    assert!(stale.restore(&mut journal).is_err());
    assert!(locator.exists());
    let review = RestorationReview::prepare(&journal, id.value()).unwrap();
    journal
        .connection
        .execute_batch("PRAGMA query_only=ON")
        .unwrap();
    assert!(review.restore(&mut journal).is_err());
    assert!(locator.exists());
    assert!(!destination.exists());
    journal
        .connection
        .execute_batch("PRAGMA query_only=OFF")
        .unwrap();
    let review = RestorationReview::prepare(&journal, id.value()).unwrap();
    let restored = review.restore(&mut journal).unwrap();
    assert!(restored.recorded());
    assert_eq!(restored.identity(), record.source);
    assert_eq!(restored.path(), destination);
    assert_eq!(std::fs::read(&destination).unwrap(), b"fixture content");
    assert!(!locator.exists());
    assert!(RestorationReview::prepare(&journal, id.value()).is_err());
    drop(journal);
    let journal = Journal::open(&database).unwrap();
    assert_eq!(
        journal.relocation(id.value()).unwrap().unwrap().state,
        "restored"
    );
    assert!(
        journal
            .activity(0, 100)
            .unwrap()
            .iter()
            .all(|item| !item.can_restore)
    );
    assert!(RestorationReview::prepare(&journal, id.value()).is_err());
}
