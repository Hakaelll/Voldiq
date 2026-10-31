#[path = "../src/overview.rs"]
mod overview;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use voldiq_core::{
    Kind, NodeId, ScanConcurrency, ScanMode, ScanOptions,
    index::{Page, Query},
    scanner::{self, SharedIndex},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

enum Answer {
    Page(Page),
    Overview(Box<overview::Overview>),
}

#[derive(serde::Serialize)]
struct Sample {
    kind: &'static str,
    round: usize,
    method: &'static str,
    published_entries: usize,
    published_files: u64,
    wait_ms: f64,
    query_ms: f64,
    held_ms: f64,
    serialize_ms: f64,
    response_ms: f64,
    json_bytes: usize,
}

fn sample(
    shared: &SharedIndex,
    round: usize,
    method: usize,
) -> std::result::Result<Sample, String> {
    let start = Instant::now();
    let index = shared.read().map_err(|error| error.to_string())?;
    let acquired = Instant::now();
    let published_entries = index.entries.len();
    let published_files = index.entries[0].files;
    let logical = index.entries[0].logical;
    let cancel = AtomicBool::new(false);
    let answer = if method == 2 {
        Answer::Overview(Box::new(
            overview::overview(&index, NodeId(0), &cancel)?
                .ok_or("Unexpected overview cancellation")?,
        ))
    } else {
        let query = if method == 0 {
            Query {
                parent: Some(NodeId(0)),
                limit: 100,
                ..Query::default()
            }
        } else {
            Query {
                text: "file-".into(),
                files_only: true,
                limit: 100,
                ..Query::default()
            }
        };
        Answer::Page(
            index
                .query_cancellable(&query, || false)
                .map_err(|error| error.to_string())?
                .ok_or("Unexpected search cancellation")?,
        )
    };
    let queried = Instant::now();
    assert_eq!(logical, published_files * 128);
    assert_eq!(index.error_count, 0);
    match &answer {
        Answer::Page(page) => {
            assert_eq!(page.items.len(), page.total.min(100));
            if method == 0 {
                assert!(page.total <= 100);
                assert!(
                    page.items
                        .iter()
                        .all(|item| item.kind == Kind::Directory && item.parent == Some(NodeId(0)))
                );
            } else {
                assert_eq!(page.total as u64, published_files);
                assert!(page.items.iter().all(|item| item.kind == Kind::File
                    && item.logical == 128
                    && item.name.starts_with("file-")));
            }
        }
        Answer::Overview(overview) => {
            assert_eq!(overview.directory.entry.logical, logical);
            assert_eq!(overview.directory.entry.files, published_files);
            assert_eq!(overview.types.total_bytes, logical);
            assert_eq!(overview.types.total_files, published_files);
            assert!(overview.children.total <= 100);
            for branch in &overview.branches {
                assert!(branch.items.len() <= 8);
                assert!(
                    branch
                        .items
                        .iter()
                        .all(|item| item.parent == Some(branch.parent))
                );
            }
        }
    }
    drop(index);
    let released = Instant::now();
    let encoded = match &answer {
        Answer::Page(page) => serde_json::to_vec(page),
        Answer::Overview(overview) => serde_json::to_vec(overview),
    }
    .map_err(|error| error.to_string())?;
    let finished = Instant::now();
    let decoded: serde_json::Value =
        serde_json::from_slice(&encoded).map_err(|error| error.to_string())?;
    if method == 1 {
        assert_eq!(decoded["total"].as_u64(), Some(published_files));
    }
    if method == 2 {
        assert_eq!(
            decoded["directory"]["entry"]["logical"].as_u64(),
            Some(logical)
        );
    }
    Ok(Sample {
        kind: "query",
        round,
        method: ["root", "search", "overview"][method],
        published_entries,
        published_files,
        wait_ms: acquired.duration_since(start).as_secs_f64() * 1000.0,
        query_ms: queried.duration_since(acquired).as_secs_f64() * 1000.0,
        held_ms: released.duration_since(acquired).as_secs_f64() * 1000.0,
        serialize_ms: finished.duration_since(released).as_secs_f64() * 1000.0,
        response_ms: finished.duration_since(start).as_secs_f64() * 1000.0,
        json_bytes: encoded.len(),
    })
}

fn queries(
    shared: SharedIndex,
    stop: &AtomicBool,
    round: usize,
) -> std::result::Result<Vec<Sample>, String> {
    let mut samples = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        samples.push(sample(&shared, round, samples.len() % 3)?);
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(samples)
}

fn main() -> Result<()> {
    let count: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "100000".into())
        .parse()?;
    if !(10_000..=100_000).contains(&count) {
        return Err("Use 10,000 to 100,000 owned files.".into());
    }
    let fixture = tempfile::Builder::new()
        .prefix("voldiq-live-query-")
        .tempdir()?;
    check_space(fixture.path(), count)?;
    eprintln!("Owned live-query fixture: {}", fixture.path().display());
    for directory in 0..100 {
        std::fs::create_dir(fixture.path().join(format!("folder-{directory:03}")))?;
    }
    for file in 0..count {
        std::fs::write(
            fixture
                .path()
                .join(format!("folder-{:03}/file-{file:08}.bin", file % 100)),
            [0u8; 128],
        )?;
    }
    let options = ScanOptions {
        mode: ScanMode::Directory,
        concurrency: ScanConcurrency::automatic(),
        ..ScanOptions::default()
    };
    for round in 1..=5 {
        let order = if round % 2 == 1 {
            [false, true]
        } else {
            [true, false]
        };
        for loaded in order {
            let stop = AtomicBool::new(false);
            let cancel = AtomicBool::new(false);
            let (shared, elapsed, samples) = std::thread::scope(|scope| -> Result<_> {
                let mut reader = None;
                let started = Instant::now();
                let scanned = scanner::scan_live(
                    fixture.path(),
                    &options,
                    &cancel,
                    |_| {},
                    |index| {
                        if loaded {
                            reader = Some(scope.spawn(|| queries(index, &stop, round)));
                        }
                    },
                );
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                stop.store(true, Ordering::Relaxed);
                let samples = match reader {
                    Some(reader) => reader.join().map_err(|_| "Live query worker panicked")??,
                    None => Vec::new(),
                };
                Ok((scanned?, elapsed, samples))
            })?;
            let index = shared.read().map_err(|error| error.to_string())?;
            assert_eq!(index.entries.len(), count + 101);
            assert_eq!(index.entries[0].files, count as u64);
            assert_eq!(index.entries[0].logical, count as u64 * 128);
            assert_eq!(index.error_count, 0);
            assert!(!index.entries[0].incomplete);
            assert!(index.entries[0].allocation_known);
            if loaded {
                for method in ["root", "search", "overview"] {
                    assert!(samples.iter().any(|sample| sample.method == method));
                }
            }
            println!(
                "{}",
                serde_json::json!({"kind":"scan","round":round,"loaded":loaded,"files":count,"entries":index.entries.len(),"errors":index.error_count,"scan_ms":elapsed,"queries":samples.len()})
            );
            drop(index);
            for sample in samples {
                println!("{}", serde_json::to_string(&sample)?);
            }
        }
    }
    let path = fixture.path().to_owned();
    fixture.close()?;
    assert!(!path.exists());
    eprintln!("Owned live-query fixture removed: {}", path.display());
    Ok(())
}

#[cfg(windows)]
fn check_space(path: &std::path::Path, count: usize) -> std::io::Result<()> {
    let mut available = 0;
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            voldiq_core::windows::wide(path).as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if available < count as u64 * 8192 + 512 * 1024 * 1024 {
        return Err(std::io::Error::other("Insufficient fixture safety reserve"));
    }
    Ok(())
}
#[cfg(not(windows))]
fn check_space(_: &std::path::Path, _: usize) -> std::io::Result<()> {
    Err(std::io::Error::other("Windows fixture required"))
}
