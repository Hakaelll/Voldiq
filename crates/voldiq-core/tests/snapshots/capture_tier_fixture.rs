use std::sync::atomic::AtomicBool;
use voldiq_core::snapshots::{
    DashboardTrend, DiaryDetail, SnapshotTier, Store, TrendRequest, schedule::time_now,
};

pub(super) fn verify() {
    let fixture = tempfile::tempdir().unwrap();
    let folder = fixture.path().join("Measured folder");
    let database = fixture.path().join("history.db");
    std::fs::create_dir(&folder).unwrap();
    let cancel = AtomicBool::new(false);
    let observed = u64::try_from(time_now().unwrap()).unwrap() - 240;
    let capture = |when| {
        let mut index = super::capture(&folder);
        index.observation.as_mut().unwrap().completed_at = when;
        index
    };
    let path_rows = |store: &Store| {
        assert!(store.disk_bytes().unwrap() > 0);
        let sqlite = rusqlite::Connection::open(&database).unwrap();
        let count = |query| {
            sqlite
                .query_row(query, [], |row| row.get::<_, i64>(0))
                .unwrap()
        };
        (
            count("SELECT COUNT(*) FROM snapshot_paths"),
            count("SELECT COUNT(*) FROM snapshot_versions"),
        )
    };

    std::fs::write(folder.join("kept.txt"), [1u8; 2]).unwrap();
    std::fs::write(folder.join("old.txt"), [2u8; 10]).unwrap();
    let mut store = Store::open(&database).unwrap();
    let first = store.save(&capture(observed), &cancel, &[]).unwrap();
    let stored_rows = path_rows(&store);
    assert_eq!(stored_rows, (3, 3));
    drop(store);

    // Reopen the same path as a pre-tier database with detailed evidence.
    let previous = rusqlite::Connection::open(&database).unwrap();
    previous
        .execute_batch(
            "ALTER TABLE snapshots DROP COLUMN tier;
             ALTER TABLE snapshots DROP COLUMN indexed_depth;
             ALTER TABLE snapshots DROP COLUMN allocation_complete;
             ALTER TABLE snapshots DROP COLUMN exclusion_count;
             PRAGMA user_version=14;",
        )
        .unwrap();
    drop(previous);
    let mut store = Store::open(&database).unwrap();
    assert_eq!(store.tier(first).unwrap(), SnapshotTier::Detailed);
    let legacy = store.summary(first).unwrap().unwrap();
    assert_eq!(legacy.indexed_depth, None);
    assert_eq!(legacy.allocation_complete, None);
    assert_eq!(store.load(first, &cancel).unwrap().entries[0].logical, 12);

    std::fs::remove_file(folder.join("old.txt")).unwrap();
    std::fs::write(folder.join("summary-source.txt"), [3u8; 13]).unwrap();
    let measured = capture(observed + 60);
    assert!(
        store
            .save_summary(&measured, &AtomicBool::new(true))
            .is_err()
    );
    assert_eq!(store.list(0, 10).unwrap().len(), 1);
    let summary = store.save_summary(&measured, &cancel).unwrap();
    assert_eq!(path_rows(&store), stored_rows);
    assert_eq!(store.scope(summary).unwrap().root, measured.root);
    let header = store.summary(summary).unwrap().unwrap();
    assert_eq!(header.tier, SnapshotTier::Summary);
    assert_eq!((header.logical, header.files, header.entries), (15, 2, 3));
    assert_eq!(header.indexed_depth, Some(1));
    assert_eq!(header.exclusion_count, Some(0));
    assert_eq!(
        header.allocation_complete,
        Some(measured.entries[0].allocation_known)
    );
    assert_eq!(header.observation.unwrap().completed_at, observed + 60);
    assert!(store.checkpoint(summary).unwrap().is_none());
    assert!(store.load(summary, &cancel).is_err());
    assert!(store.compare(first, summary, 0, 10).is_err());
    assert!(store.visual_capture(summary, &cancel).is_err());
    assert!(
        store
            .visual_pair(first, summary, None, None, false, &cancel)
            .is_err()
    );
    assert!(store.calendar(summary, "2026-01", &cancel).is_err());
    assert!(
        store
            .calendar_entries(summary, "2026-01", 1, 0, 10, &cancel)
            .is_err()
    );
    assert!(store.trend_changes(first, summary, &cancel).is_err());

    std::fs::remove_file(folder.join("summary-source.txt")).unwrap();
    std::fs::write(folder.join("replacement.txt"), [4u8; 19]).unwrap();
    let last = store.save(&capture(observed + 120), &cancel, &[]).unwrap();
    let compare = store.compare(first, last, 0, 10).unwrap();
    assert_eq!(compare.delta, 9);
    assert_eq!(compare.file_changes.added.logical, 19);
    assert_eq!(compare.file_changes.removed.logical, 10);
    assert!(
        store
            .load(first, &cancel)
            .unwrap()
            .entries
            .iter()
            .any(|e| e.name == "old.txt")
    );
    let newest = store.load(last, &cancel).unwrap();
    assert!(newest.entries.iter().any(|e| e.name == "replacement.txt"));
    assert!(
        newest
            .entries
            .iter()
            .all(|e| e.name != "summary-source.txt")
    );
    let diary = store.diary(last, 0, 10, 5, &cancel).unwrap().unwrap();
    assert!(matches!(&diary.events[0].detail,
        DiaryDetail::Aggregate { delta, coverage_complete: true, .. } if delta == "6"));
    assert!(matches!(&diary.events[1].detail,
        DiaryDetail::Aggregate { delta, coverage_complete: true, .. } if delta == "3"));
    let trend = store
        .trend(
            TrendRequest {
                snapshot: last,
                days: 7,
            },
            &cancel,
        )
        .unwrap()
        .unwrap();
    assert_eq!(trend.folder.points[1].tier, SnapshotTier::Summary);
    assert_eq!(trend.folder.points[1].logical, "15");
    assert_eq!(trend.folder.points[1].delta.as_deref(), Some("3"));

    let historical = store.load(first, &cancel).unwrap();
    assert!(matches!(
        store.dashboard_trend(&historical, &cancel).unwrap(),
        Some(DashboardTrend::Recorded { summary })
            if summary.capture.id == first && summary.latest.as_ref().is_some_and(|point| point.snapshot == first && point.delta.is_none())
    ));
    let mut live = capture(observed + 150);
    live.exclusions.push(folder.join("excluded"));
    let other_group = store.save_summary(&live, &cancel).unwrap();
    assert!(matches!(
        store.dashboard_trend(&live, &cancel).unwrap(),
        Some(DashboardTrend::Recorded { summary }) if summary.capture.id == other_group
    ));
    live.exclusions.clear();
    assert!(matches!(
        store.dashboard_trend(&live, &cancel).unwrap(),
        Some(DashboardTrend::Recorded { summary })
            if summary.capture.id == last && summary.latest.as_ref().is_some_and(|point| point.delta.as_deref() == Some("6"))
    ));
    let native_identity = live.root_identity.unwrap();
    live.root_identity = Some((native_identity.0, native_identity.1 ^ 1));
    assert!(matches!(
        store.dashboard_trend(&live, &cancel).unwrap(),
        Some(DashboardTrend::Unavailable)
    ));
    assert!(
        store
            .dashboard_trend(&live, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    store.delete(other_group).unwrap();
    // Two saves share one measurement instant. A later capture must not replace the anchor's counter.
    {
        let sqlite = rusqlite::Connection::open(&database).unwrap();
        for (id, at, free) in [
            (first, observed, 90),
            (summary, observed + 60, 70),
            (last, observed + 60, 99),
        ] {
            sqlite.execute(
                "INSERT OR REPLACE INTO snapshot_volume_samples(snapshot_id,guid,serial,measured_at,capacity,free)
                VALUES (?1,'fixture-volume','fixture-serial',?2,100,?3)",
                (id, i64::try_from(at).unwrap(), free),
            ).unwrap();
        }
    }
    let through = store
        .trend_at(
            TrendRequest {
                snapshot: summary,
                days: 30,
            },
            &cancel,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        through
            .folder
            .points
            .iter()
            .map(|point| point.snapshot)
            .collect::<Vec<_>>(),
        [first, summary]
    );
    assert_eq!(through.volume.points.last().unwrap().free, "70");
    assert!(through.folder.forecast.is_none());

    let settings = store.retention_settings(last).unwrap();
    store
        .set_retention(last, &settings.version, Some(2))
        .unwrap();
    let review = store.preview_retention(last, &cancel).unwrap().unwrap();
    assert_eq!(review.captures.len(), 1);
    assert_eq!(
        (review.captures[0].id, review.captures[0].tier),
        (first, SnapshotTier::Detailed)
    );
    assert_eq!(
        store
            .apply_retention(last, &review.fingerprint, &cancel)
            .unwrap()
            .unwrap()
            .removed,
        1
    );
    assert_eq!(path_rows(&store), stored_rows);
    assert_eq!(store.tier(summary).unwrap(), SnapshotTier::Summary);
    assert_eq!(store.load(last, &cancel).unwrap().entries[0].logical, 21);
    assert!(matches!(
        store.dashboard_trend(&historical, &cancel).unwrap(),
        Some(DashboardTrend::Unavailable)
    ));
    assert!(
        store
            .trend_at(
                TrendRequest {
                    snapshot: first,
                    days: 30
                },
                &cancel
            )
            .err()
            .is_some_and(|error| error.to_string().contains("no longer retained"))
    );

    let newest_summary = store
        .save_summary(&capture(observed + 180), &cancel)
        .unwrap();
    let settings = store.retention_settings(newest_summary).unwrap();
    store
        .set_retention(newest_summary, &settings.version, Some(1))
        .unwrap();
    let review = store
        .preview_retention(newest_summary, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(
        review
            .captures
            .iter()
            .map(|row| (row.id, row.tier))
            .collect::<Vec<_>>(),
        [
            (summary, SnapshotTier::Summary),
            (last, SnapshotTier::Detailed)
        ]
    );
    assert!(
        store
            .apply_retention(newest_summary, &review.fingerprint, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    assert_eq!(path_rows(&store), stored_rows);
    store
        .apply_retention(newest_summary, &review.fingerprint, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(path_rows(&store), (0, 0));
    drop(store);
    let mut store = Store::open(&database).unwrap();
    assert_eq!(store.summary(newest_summary).unwrap().unwrap().logical, 21);
    assert_eq!(
        store
            .trend(
                TrendRequest {
                    snapshot: newest_summary,
                    days: 7
                },
                &cancel
            )
            .unwrap()
            .unwrap()
            .folder
            .points
            .len(),
        1
    );

    // A later save at the same earlier observation cannot replace a pinned pair.
    let linked_root = fixture.path().join("Linked changes");
    let linked_child = linked_root.join("Child");
    std::fs::create_dir_all(&linked_child).unwrap();
    let save_at = |store: &mut Store, when, bytes| {
        std::fs::write(linked_child.join("changed.bin"), vec![1u8; bytes]).unwrap();
        let mut index = super::capture(&linked_root);
        index.observation.as_mut().unwrap().completed_at = when;
        store.save(&index, &cancel, &[]).unwrap()
    };
    let before = save_at(&mut store, observed, 5);
    let after = save_at(&mut store, observed + 60, 11);
    let duplicate = save_at(&mut store, observed, 20);
    let pinned = store
        .trend_at(
            TrendRequest {
                snapshot: after,
                days: 30,
            },
            &cancel,
        )
        .unwrap()
        .unwrap();
    assert_eq!(pinned.folder.points[1].previous, Some(before));
    let changes = store
        .trend_changes_at(before, after, after, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!((changes.before, changes.after), (before, after));
    assert_eq!(changes.grew.len(), 1);
    assert_eq!(
        (&*changes.grew[0].name, &*changes.grew[0].delta),
        ("Child", "6")
    );
    assert!(store.trend_changes(before, after, &cancel).is_err());
    assert_eq!(
        store
            .trend_changes(duplicate, after, &cancel)
            .unwrap()
            .unwrap()
            .shrank[0]
            .delta,
        "-9"
    );
    assert!(
        store
            .trend_changes_at(duplicate, after, after, &cancel)
            .is_err()
    );
    let other_anchor = store
        .save_summary(&capture(observed + 210), &cancel)
        .unwrap();
    assert!(
        store
            .trend_changes_at(duplicate, after, other_anchor, &cancel)
            .is_err()
    );
}
