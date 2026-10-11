use std::{path::PathBuf, sync::atomic::AtomicBool, time::Instant};
use voldiq_core::{NodeId, ScanOptions, index::Query, scanner::scan};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("Usage: cargo run --release -p voldiq-core --example scan -- <directory>")?;
    let start = Instant::now();
    let index = scan(
        &path,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )?;
    let scan_ms = start.elapsed().as_secs_f64() * 1000.0;
    let query_start = Instant::now();
    let page = index.query(&Query {
        parent: Some(NodeId(0)),
        limit: 100,
        ..Query::default()
    })?;
    let query_ms = query_start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "method,entries,files,logical_bytes,allocated_bytes,errors,scan_ms,root_query_ms,root_children,node_struct_bytes"
    );
    println!(
        "directory,{},{},{},{},{},{:.3},{:.3},{},{}",
        index.entries.len(),
        index.entries[0].files,
        index.entries[0].logical,
        index.entries[0].allocated,
        index.error_count,
        scan_ms,
        query_ms,
        page.total,
        std::mem::size_of::<voldiq_core::Entry>()
    );
    Ok(())
}
