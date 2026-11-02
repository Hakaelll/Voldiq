use std::{path::Path, sync::atomic::AtomicBool};
use voldiq_core::{
    ScanOptions,
    index::{Index, Query},
    scanner::scan,
    snapshots::Store,
};

fn capture(path: &Path) -> Index {
    scan(
        path,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap()
}

#[test]
fn history_survives_restart_and_compares_actual_directory_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let db = database.path().join("history.db");
    let loading = AtomicBool::new(false);
    std::fs::create_dir(fixture.path().join("nested")).unwrap();
    std::fs::write(fixture.path().join("nested/growth"), [1; 12]).unwrap();
    std::fs::write(fixture.path().join("removed"), [2; 7]).unwrap();
    std::fs::write(fixture.path().join("UPPER.txt"), [5; 7]).unwrap();
    std::fs::write(fixture.path().join("Résumé.txt"), [6; 7]).unwrap();
    let first = capture(fixture.path());
    let mut store = Store::open(&db).unwrap();
    let before = store.save(&first, &AtomicBool::new(false), &[]).unwrap();
    std::fs::write(fixture.path().join("nested/growth"), [3; 33]).unwrap();
    std::fs::remove_file(fixture.path().join("removed")).unwrap();
    std::fs::write(fixture.path().join("added"), [4; 14]).unwrap();
    let second = capture(fixture.path());
    let after = store.save(&second, &AtomicBool::new(false), &[]).unwrap();
    let preference = store.retention_settings(after).unwrap();
    assert_eq!(preference.keep_latest, None);
    let preference = store
        .set_retention(after, &preference.version, Some(1))
        .unwrap();
    let review = store.preview_retention(after, &loading).unwrap().unwrap();
    assert_eq!(
        review
            .captures
            .iter()
            .map(|capture| capture.id)
            .collect::<Vec<_>>(),
        [before]
    );
    assert_eq!(review.settings.keep_latest, Some(1));
    assert!(store.set_retention(after, "0", Some(2)).is_err());
    let preference = store
        .set_retention(after, &preference.version, None)
        .unwrap();
    assert!(
        store
            .apply_retention(after, &review.fingerprint, &loading)
            .is_err()
    );
    assert_eq!(
        store.retention_settings(after).unwrap().version,
        preference.version
    );
    assert!(store.save(&second, &AtomicBool::new(true), &[]).is_err());
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
    drop(store);
    let store = Store::open(&db).unwrap();
    let history = store.list(0, 10).unwrap();
    assert_eq!(history.len(), 2);
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        store.load(-1, &cancelled).err().unwrap().to_string(),
        "Snapshot loading cancelled."
    );
    assert_eq!(
        store.load(before, &cancelled).err().unwrap().to_string(),
        "Snapshot loading cancelled."
    );
    assert_eq!(history[0].id, after);
    let loaded = store.load(before, &loading).unwrap();
    assert_eq!(loaded.observation, first.observation);
    assert!(
        loaded
            .observation
            .as_ref()
            .is_some_and(|observation| observation.complete)
    );
    assert_eq!(loaded.entries[0].logical, 33);
    let frame = store.visual_capture(before, &loading).unwrap().unwrap();
    assert_eq!(frame.snapshot, before);
    assert_eq!(frame.logical.as_deref(), Some("33"));
    assert_eq!(frame.entries.len(), 4);
    assert_eq!(frame.observation, first.observation);
    assert!(
        store
            .visual_capture(-1, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        loaded
            .query(&Query {
                files_only: true,
                ..Query::default()
            })
            .unwrap()
            .total,
        4
    );
    for (text, expected) in [
        ("GROWTH", vec!["growth"]),
        ("upper", vec!["UPPER.txt"]),
        ("RÉSUMÉ", vec!["Résumé.txt"]),
        ("absent", vec![]),
    ] {
        let query = Query {
            text: text.into(),
            files_only: true,
            ..Query::default()
        };
        for index in [&first, &loaded] {
            let page = index.query(&query).unwrap();
            let cancellable = index.query_cancellable(&query, || false).unwrap().unwrap();
            assert_eq!(page.total, expected.len());
            assert_eq!(cancellable.total, expected.len());
            for result in [page, cancellable] {
                assert_eq!(
                    result
                        .items
                        .iter()
                        .map(|entry| entry.name.as_str())
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
    }
    let comparison = store.compare(before, after, 0, 200).unwrap();
    assert!(comparison.reliable);
    assert!(
        store
            .compare_cancellable(-1, -2, 0, 10, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    assert_eq!(comparison.delta, 28);
    assert_eq!(comparison.file_changes.added.logical, 14);
    assert_eq!(comparison.file_changes.added.files, 1);
    assert_eq!(comparison.file_changes.changed.delta, 21);
    assert_eq!(comparison.file_changes.changed.files, 1);
    assert_eq!(comparison.file_changes.removed.logical, 7);
    assert_eq!(comparison.file_changes.removed.files, 1);
    assert!(
        comparison
            .changes
            .iter()
            .any(|c| c.path.ends_with("removed") && c.kind == "removed" && c.delta == -7)
    );
    assert!(
        comparison
            .changes
            .iter()
            .any(|c| c.path.ends_with("added") && c.kind == "added" && c.delta == 14)
    );
    assert!(
        comparison
            .changes
            .iter()
            .any(|c| c.path.ends_with("nested") && c.directory && c.delta == 21)
    );
    let page = store.compare(before, after, 1, 1).unwrap();
    assert_eq!(page.total, comparison.total);
    assert_eq!(page.changes.len(), 1);
    assert_eq!(page.file_changes, comparison.file_changes);
    assert_eq!(store.load(after, &loading).unwrap().entries[0].logical, 61);
    drop(store);
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch("DROP TABLE snapshot_retention; DROP TRIGGER history_snapshot_insert; DROP TRIGGER history_snapshot_update; DROP TRIGGER history_snapshot_delete; DROP TABLE snapshot_history_revision; DROP INDEX snapshots_observed; ALTER TABLE snapshots DROP COLUMN observed_at; ALTER TABLE snapshots DROP COLUMN observed_complete; ALTER TABLE snapshots DROP COLUMN engine_version; ALTER TABLE snapshots DROP COLUMN accounting_version; ALTER TABLE snapshot_versions DROP COLUMN cloud; DROP TABLE duplicate_hashes; PRAGMA user_version=4;")
        .unwrap();
    drop(connection);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
    assert_eq!(store.load(after, &loading).unwrap().entries[0].logical, 61);
    assert_eq!(store.load(after, &loading).unwrap().observation, None);
    let duplicates = tempfile::tempdir().unwrap();
    for name in ["first", "second"] {
        std::fs::write(duplicates.path().join(name), [7_u8; 16_384]).unwrap();
    }
    let cancel = AtomicBool::new(false);
    let policy_state = tempfile::tempdir().unwrap();
    let protections = voldiq_core::metadata::Store::open(policy_state.path())
        .unwrap()
        .protections()
        .unwrap();
    let index = std::sync::Arc::new(std::sync::RwLock::new(capture(duplicates.path())));
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let cold = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(cold.errors, 0);
    assert_eq!(cold.groups.len(), 1);
    assert_eq!(cold.groups[0].members.len(), 2);
    assert_eq!(
        cold.groups[0].digest,
        blake3::hash(&[7_u8; 16_384]).to_hex().as_str()
    );
    assert_eq!(cold.bytes_read, 49_152);
    assert_eq!(cold.cache_hits, 0);
    store.save_hash_cache(&cache, &cancel).unwrap();
    drop(cache);
    drop(store);
    let mut store = Store::open(&db).unwrap();
    store
        .save_hash_cache(
            &voldiq_core::duplicates::HashCache::default(),
            &AtomicBool::new(true),
        )
        .unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let warm = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(warm.errors, 0);
    assert_eq!(warm.groups.len(), 1);
    assert_eq!(warm.groups[0].digest, cold.groups[0].digest);
    assert_eq!(warm.groups[0].members.len(), 2);
    assert_eq!(warm.bytes_read, 0);
    assert_eq!(warm.cache_hits, 2);
    drop(cache);
    drop(store);
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE duplicate_hashes SET sample=x'00' WHERE identity=(SELECT min(identity) FROM duplicate_hashes);").unwrap();
    drop(connection);
    let mut store = Store::open(&db).unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let repaired = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(repaired.errors, 0);
    assert_eq!(repaired.groups.len(), 1);
    assert_eq!(repaired.groups[0].digest, cold.groups[0].digest);
    assert_eq!(repaired.bytes_read, 24_576);
    assert_eq!(repaired.cache_hits, 1);
    store.save_hash_cache(&cache, &cancel).unwrap();
    let first_path = duplicates.path().join("first");
    let modified = std::fs::metadata(&first_path).unwrap().modified().unwrap();
    std::fs::write(&first_path, [8_u8; 16_384]).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&first_path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    drop(cache);
    drop(store);
    let store = Store::open(&db).unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let changed = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(changed.errors, 0);
    assert!(changed.groups.is_empty());
    assert_eq!(changed.bytes_read, 8192);
    drop(cache);
    drop(store);
    let mut store = Store::open(&db).unwrap();
    store
        .save_hash_cache(&voldiq_core::duplicates::HashCache::default(), &cancel)
        .unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let cleared = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(cleared.errors, 0);
    assert!(cleared.groups.is_empty());
    assert_eq!(cleared.bytes_read, 16_384);
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
    budget_history_uses_measurements_and_real_scoped_image_samples();

    // Continue the historical comparison flow with a folder larger than either map/page budget.
    for n in 0..110 {
        std::fs::create_dir(fixture.path().join(format!("nested/dir{n:03}"))).unwrap();
    }
    for n in 0..170 {
        std::fs::write(fixture.path().join(format!("nested/file{n:03}")), [1]).unwrap();
    }
    let third = store.save(&capture(fixture.path()), &loading, &[]).unwrap();
    let root_pair = store
        .visual_pair(before, third, None, None, false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(root_pair.before.logical.as_deref(), Some("33"));
    assert_eq!(root_pair.after.logical.as_deref(), Some("231"));
    assert_eq!(root_pair.delta.as_deref(), Some("198"));
    assert_eq!(
        root_pair.accounting,
        voldiq_core::snapshots::Accounting::Unknown
    );
    let nested = &root_pair.folders.items[0].path_id;
    let scoped = store
        .visual_pair(before, third, Some(nested), None, false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(scoped.before.logical.as_deref(), Some("12"));
    assert_eq!(scoped.after.logical.as_deref(), Some("203"));
    assert_eq!(scoped.after.child_count, 281);
    assert_eq!(scoped.after.entries.len(), 160);
    assert_eq!(scoped.after.remainder.as_ref().unwrap().logical, "11");
    assert_eq!(scoped.after.remainder.as_ref().unwrap().entries, 121);
    assert_eq!(scoped.folders.items.len(), 100);
    assert!(scoped.folders.previous.is_none());
    let cursor = scoped.folders.next.as_deref().unwrap();
    let next = store
        .visual_pair(before, third, Some(nested), Some(cursor), false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(next.folders.items.len(), 10);
    assert!(next.folders.next.is_none());
    let previous = store
        .visual_pair(
            before,
            third,
            Some(nested),
            next.folders.previous.as_deref(),
            true,
            &loading,
        )
        .unwrap()
        .unwrap();
    assert_eq!(previous.folders.items[0].name, "dir000");
    assert_eq!(previous.folders.items.last().unwrap().name, "dir099");
    let empty = &scoped.folders.items[0].path_id;
    let missing = store
        .visual_pair(before, third, Some(empty), None, false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(missing.before.logical, None);
    assert_eq!(
        serde_json::to_value(&missing).unwrap()["before"]["presence"],
        "not_recorded"
    );
    assert_eq!(missing.after.logical.as_deref(), Some("0"));
    assert!(
        store
            .visual_pair(before, third, Some("01"), None, false, &loading)
            .is_err()
    );
    assert!(
        store
            .visual_pair(before, before, None, None, false, &loading)
            .is_err()
    );
    assert!(
        store
            .visual_pair(-1, -2, None, None, false, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute("UPDATE snapshots SET complete=0 WHERE id=?1", [before])
        .unwrap();
    let partial = store
        .visual_pair(before, third, Some(empty), None, false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&partial).unwrap()["before"]["presence"],
        "not_observed"
    );
    assert!(
        !store
            .compare(before, third, 0, 10)
            .unwrap()
            .coverage_complete
    );
    connection
        .execute(
            "UPDATE snapshots SET accounting_version=99 WHERE id=?1",
            [third],
        )
        .unwrap();
    assert!(
        store
            .visual_pair(before, third, None, None, false, &loading)
            .is_err()
    );
    assert!(store.compare(before, third, 0, 10).is_err());
    let foreign = store
        .save(&capture(duplicates.path()), &loading, &[])
        .unwrap();
    assert!(
        store
            .visual_pair(before, foreign, None, None, false, &loading)
            .is_err()
    );
    connection
        .execute(
            "UPDATE snapshots SET accounting_version=?1 WHERE id=?2",
            rusqlite::params![voldiq_core::index::ACCOUNTING_VERSION, third],
        )
        .unwrap();
    let current = capture(fixture.path());
    for _ in 0..104 {
        store.save(&current, &loading, &[]).unwrap();
    }
    assert!(store.set_retention(third, "0", Some(0)).is_err());
    assert!(store.set_retention(third, "0", Some(10001)).is_err());
    let preference = store.set_retention(third, "0", Some(1)).unwrap();
    connection
        .execute(
            "UPDATE snapshots SET observed_complete=0 WHERE id=?1",
            [after],
        )
        .unwrap();
    let review = store.preview_retention(third, &loading).unwrap().unwrap();
    assert_eq!(review.eligible, 106);
    assert_eq!(review.captures.len(), 100);
    assert!(
        !review
            .captures
            .iter()
            .find(|capture| capture.id == after)
            .unwrap()
            .complete
    );
    let mut concurrent = Store::open(&db).unwrap();
    assert_eq!(
        concurrent.retention_settings(third).unwrap().version,
        preference.version
    );
    let newest = concurrent.save(&current, &loading, &[]).unwrap();
    assert!(
        store
            .apply_retention(third, &review.fingerprint, &loading)
            .is_err()
    );
    let review = store.preview_retention(newest, &loading).unwrap().unwrap();
    assert_eq!(review.eligible, 107);
    assert!(
        store
            .apply_retention(newest, &review.fingerprint, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    connection.execute_batch(&format!("CREATE TRIGGER fail_retention_fixture BEFORE DELETE ON snapshots WHEN OLD.id={after} BEGIN SELECT RAISE(ABORT,'owned fixture failure'); END;")).unwrap();
    assert!(
        store
            .apply_retention(newest, &review.fingerprint, &loading)
            .is_err()
    );
    assert_eq!(store.load(before, &loading).unwrap().entries[0].logical, 33);
    connection
        .execute_batch("DROP TRIGGER fail_retention_fixture;")
        .unwrap();
    let result = store
        .apply_retention(newest, &review.fingerprint, &loading)
        .unwrap()
        .unwrap();
    assert_eq!((result.removed, result.remaining_eligible), (100, 7));
    assert!(
        store
            .apply_retention(newest, &review.fingerprint, &loading)
            .is_err()
    );
    let review = store.preview_retention(newest, &loading).unwrap().unwrap();
    assert_eq!(review.captures.len(), 7);
    assert_eq!(
        store
            .apply_retention(newest, &review.fingerprint, &loading)
            .unwrap()
            .unwrap()
            .remaining_eligible,
        0
    );
    drop(store);
    let store = Store::open(&db).unwrap();
    assert_eq!(store.retention_settings(newest).unwrap().total, 1);
    assert_eq!(
        store.load(newest, &loading).unwrap().entries[0].logical,
        231
    );
    assert_eq!(
        store.load(foreign, &loading).unwrap().entries[0].logical,
        32768
    );
    assert_eq!(
        std::fs::metadata(fixture.path().join("nested/growth"))
            .unwrap()
            .len(),
        33
    );
}

fn budget_history_uses_measurements_and_real_scoped_image_samples() {
    use voldiq_core::{
        NodeId,
        budgets::{Forecast, ForecastReason, Status, Store as Budgets},
    };
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let folder = source.path().join("photos");
    std::fs::create_dir(&folder).unwrap();
    let steady = source.path().join("steady");
    std::fs::create_dir(&steady).unwrap();
    std::fs::write(steady.join("unchanged.bin"), [0; 16]).unwrap();
    std::fs::write(folder.join("a.jpg"), [1; 100]).unwrap();
    std::fs::write(folder.join("b.png"), [2; 200]).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut history = Store::open(&state.path().join("snapshots.db")).unwrap();
    let mut budgets = Budgets::open(state.path()).unwrap();
    let cancel = AtomicBool::new(false);
    let mut ids = Vec::new();
    for day in 0..8 {
        std::fs::write(folder.join("growth.bin"), vec![3; 100 * (day + 1)]).unwrap();
        let mut index = capture(source.path());
        index.observation.as_mut().unwrap().completed_at = now - (7 - day as u64) * 86400;
        ids.push(history.save(&index, &cancel, &[]).unwrap());
    }
    let mut index = history.load(*ids.last().unwrap(), &cancel).unwrap();
    let node = index
        .entries
        .iter()
        .find(|entry| entry.name == "photos")
        .unwrap()
        .id;
    // Saving a limit uses a fresh indexed identity; historical reading may use only recorded evidence.
    let fresh = capture(source.path());
    let fresh_node = fresh
        .entries
        .iter()
        .find(|entry| entry.name == "photos")
        .unwrap()
        .id;
    budgets.save(&fresh, fresh_node, "0", "2KB").unwrap();
    let steady_node = fresh
        .entries
        .iter()
        .find(|entry| entry.name == "steady")
        .unwrap()
        .id;
    budgets.save(&fresh, steady_node, "0", "1KB").unwrap();
    assert!(matches!(
        budgets
            .read(&fresh, steady_node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::NotGrowing
        }
    ));
    let report = budgets
        .read(&index, node, &history, true, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(report.logical_bytes, 1100);
    let sample = report.image_equivalent.unwrap();
    assert_eq!(
        (
            sample.sample_count,
            sample.matched_images,
            sample.mean_logical_bytes,
            sample.approximate_images
        ),
        (2, 2, 150, 7)
    );
    match report.forecast {
        Forecast::Estimate {
            as_of,
            estimated_at,
            earliest_at,
            latest_at,
            samples,
            span_days,
            ..
        } => {
            assert_eq!(as_of, now);
            assert_eq!(estimated_at, now + 9 * 86400);
            assert_eq!(
                (earliest_at, latest_at, samples, span_days),
                (estimated_at, estimated_at, 8, 7)
            );
        }
        _ => panic!("Expected a qualified, measured estimate"),
    }
    index.entries[node.0 as usize].logical = 900;
    assert!(matches!(
        budgets
            .read(&index, node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::Volatile
        }
    ));
    index.entries[node.0 as usize].logical = 1100;
    let far = budgets
        .save(&fresh, fresh_node, "1", "18446744073709551615B")
        .unwrap();
    assert!(matches!(
        budgets
            .read(&index, node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::BeyondHorizon
        }
    ));
    budgets
        .save(&fresh, fresh_node, &far.version, "2KB")
        .unwrap();
    index.observation.as_mut().unwrap().accounting_version = 99;
    assert!(matches!(
        budgets
            .read(&index, node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::Incompatible
        }
    ));
    index.observation.as_mut().unwrap().accounting_version = voldiq_core::index::ACCOUNTING_VERSION;
    index.entries[node.0 as usize].incomplete = true;
    let partial = budgets
        .read(&index, node, &history, true, &cancel)
        .unwrap()
        .unwrap();
    assert!(matches!(partial.status, Status::Unknown));
    assert!(matches!(
        partial.forecast,
        Forecast::Unavailable {
            reason: ForecastReason::Partial
        }
    ));
    index.entries[node.0 as usize].logical = 2500;
    assert!(matches!(
        budgets
            .read(&index, node, &history, false, &cancel)
            .unwrap()
            .unwrap()
            .status,
        Status::Exceeded
    ));
    index.entries[node.0 as usize].logical = 1100;
    index.entries[node.0 as usize].incomplete = false;
    index.observation.as_mut().unwrap().completed_at = now - 2 * 86400;
    assert!(matches!(
        budgets
            .read(&index, node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::Stale
        }
    ));
    index.observation = None;
    assert!(matches!(
        budgets
            .read(&index, node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::Unmeasured
        }
    ));
    assert!(
        budgets
            .read(&index, NodeId(0), &history, true, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    for id in ids.iter().take(7) {
        history.delete(*id).unwrap();
    }
    let mut fresh = capture(source.path());
    fresh.observation.as_mut().unwrap().completed_at = now;
    for _ in 0..7 {
        history.save(&fresh, &cancel, &[]).unwrap();
    }
    assert!(matches!(
        budgets
            .read(&fresh, fresh_node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::InsufficientHistory
        }
    ));
}
