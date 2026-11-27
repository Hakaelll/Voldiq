use std::sync::atomic::AtomicBool;
use voldiq_core::snapshots::{DiaryDetail, SnapshotTier, Store, TrendRequest, schedule::time_now};

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
    let store = Store::open(&database).unwrap();
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
}
