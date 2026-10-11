use std::{sync::atomic::AtomicBool, time::Instant};
use voldiq_core::{NodeId, ScanOptions, index::Query, scanner::scan};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "10000".into())
        .parse()?;
    if count > 500_000 {
        return Err("Fixture limit is 500,000 files".into());
    }
    let root = tempfile::tempdir()?;
    for i in 0..100 {
        std::fs::create_dir(root.path().join(format!("folder-{i:03}")))?;
    }
    for i in 0..count {
        std::fs::write(
            root.path()
                .join(format!("folder-{:03}/file-{i:08}.bin", i % 100)),
            [i as u8; 128],
        )?;
    }
    println!("run,files,entries,errors,scan_ms,root_page_ms,global_search_ms,node_struct_bytes");
    for run in 1..=3 {
        let start = Instant::now();
        let index = scan(
            root.path(),
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if index.entries[0].files != count as u64
            || index.entries[0].logical != (count * 128) as u64
            || index.error_count != 0
        {
            return Err("Fixture totals did not match the expected values".into());
        }
        let start = Instant::now();
        let page = index.query(&Query {
            parent: Some(NodeId(0)),
            limit: 100,
            ..Query::default()
        })?;
        let query = start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(page.total, 100);
        let start = Instant::now();
        let _ = index.query(&Query {
            text: "file-0000".into(),
            files_only: true,
            limit: 100,
            ..Query::default()
        })?;
        let search = start.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{run},{count},{},{},{elapsed:.3},{query:.3},{search:.3},{}",
            index.entries.len(),
            index.error_count,
            std::mem::size_of::<voldiq_core::Entry>()
        );
    }
    Ok(())
}
