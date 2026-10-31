use std::{sync::atomic::AtomicBool, time::Instant};
use voldiq_core::{ScanOptions, scanner::scan, snapshots::Store};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let database = tempfile::tempdir()?;
    for directory in 0..100 {
        std::fs::create_dir(root.path().join(format!("folder-{directory:03}")))?;
    }
    for file in 0..10_000 {
        std::fs::write(
            root.path()
                .join(format!("folder-{:03}/file-{file:05}", file % 100)),
            [1; 128],
        )?;
    }
    let cancel = AtomicBool::new(false);
    let first = scan(root.path(), &ScanOptions::default(), &cancel, |_| {})?;
    assert_eq!(first.entries[0].logical, 1_280_000);
    assert_eq!(first.error_count, 0);
    let path = database.path().join("snapshots.db");
    let mut store = Store::open(&path)?;
    println!("run,save_ms,load_ms,compare_ms,changes,versions,storage_bytes");
    let mut baseline = 0;
    for run in 1..=3 {
        if run == 3 {
            for file in 0..10 {
                std::fs::write(
                    root.path()
                        .join(format!("folder-{:03}/file-{file:05}", file % 100)),
                    [2; 256],
                )?;
            }
        }
        let modified;
        let index = if run == 3 {
            modified = scan(root.path(), &ScanOptions::default(), &cancel, |_| {})?;
            assert_eq!(modified.error_count, 0);
            &modified
        } else {
            &first
        };
        let start = Instant::now();
        let id = store.save(index, &cancel, &[])?;
        let save_ms = start.elapsed().as_secs_f64() * 1000.0;
        if run == 1 {
            baseline = id;
        }
        let start = Instant::now();
        let loaded = store.load(id, &cancel)?;
        let load_ms = start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(loaded.entries[0].logical, index.entries[0].logical);
        let start = Instant::now();
        let comparison = store.compare(baseline, id, 0, 200)?;
        let compare_ms = start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(comparison.delta, if run == 3 { 1280 } else { 0 });
        let connection = rusqlite::Connection::open(&path)?;
        let versions: i64 =
            connection.query_row("SELECT COUNT(*) FROM snapshot_versions", [], |r| r.get(0))?;
        if run == 2 {
            assert_eq!(versions, first.entries.len() as i64);
        }
        println!(
            "{run},{save_ms:.3},{load_ms:.3},{compare_ms:.3},{},{versions},{}",
            comparison.total,
            store.disk_bytes()?
        );
    }
    Ok(())
}
