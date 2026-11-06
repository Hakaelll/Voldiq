use std::{
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use voldiq_core::{
    Entry, Kind, NodeId,
    index::{Index, Query},
};

fn entry(id: u32, parent: Option<NodeId>, kind: Kind, logical: u64) -> Entry {
    Entry {
        file_state: None,
        cloud: voldiq_core::cloud::CloudState::Unknown,
        id: NodeId(id),
        parent,
        name: format!("file-{:08}", 1_100_003_u32.wrapping_mul(id) % 1_000_003),
        kind,
        logical,
        allocated: logical,
        allocation_known: true,
        modified: Some(u64::from(id % 100)),
        files: u64::from(kind == Kind::File),
        incomplete: false,
        hard_link: false,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count = 1_000_000_u32;
    let mut index = Index {
        root: PathBuf::from("C:\\generated-query-benchmark"),
        entries: Vec::with_capacity(count as usize + 1001),
        issues: Vec::new(),
        error_count: 0,
        native_names: HashMap::new(),
        root_identity: None,
        exclusions: Vec::new(),
        snapshot_id: None,
        captured_caches: Vec::new(),
        captured_applications: None,
        observation: None,
        ntfs_checkpoint: None,
    };
    index.entries.push(entry(0, None, Kind::Directory, 0));
    for id in 1..=1000 {
        index.entries.push(entry(
            id,
            Some(NodeId(0)),
            Kind::Directory,
            u64::from(id) * 1000,
        ));
    }
    for id in 1001..=count + 1000 {
        index.entries.push(entry(
            id,
            Some(NodeId(1 + id % 1000)),
            Kind::File,
            u64::from(id.wrapping_mul(7919) % count),
        ));
    }
    eprintln!("Synthetic in-memory index; no filesystem scan or native UI is measured.");
    let search_total = index
        .entries
        .iter()
        .filter(|entry| entry.kind == Kind::File && entry.name.contains("00000"))
        .count();
    println!("run,query,method,milliseconds,total,returned");
    let cancellation = AtomicBool::new(false);
    for run in 1..=5 {
        for (label, query) in [
            (
                "root-size",
                Query {
                    parent: Some(NodeId(0)),
                    ..Query::default()
                },
            ),
            (
                "global-size",
                Query {
                    scope: Some(NodeId(0)),
                    files_only: true,
                    ..Query::default()
                },
            ),
            (
                "global-name",
                Query {
                    files_only: true,
                    sort: "name".into(),
                    offset: 500_000,
                    ..Query::default()
                },
            ),
            (
                "global-modified",
                Query {
                    files_only: true,
                    sort: "modified".into(),
                    ..Query::default()
                },
            ),
            (
                "name-search",
                Query {
                    text: "00000".into(),
                    files_only: true,
                    ..Query::default()
                },
            ),
        ] {
            let methods = if run % 2 == 1 {
                ["standard", "cancellable"]
            } else {
                ["cancellable", "standard"]
            };
            let mut previous: Option<voldiq_core::index::Page> = None;
            for method in methods {
                let start = Instant::now();
                let page = if method == "standard" {
                    index.query(&query)?
                } else {
                    index
                        .query_cancellable(&query, || cancellation.load(Ordering::Relaxed))?
                        .ok_or("Unexpected cancellation")?
                };
                let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
                let expected = match label {
                    "root-size" => 1000,
                    "name-search" => search_total,
                    _ => count as usize,
                };
                assert_eq!(page.total, expected);
                assert_eq!(
                    page.items.len(),
                    expected.saturating_sub(query.offset).min(100)
                );
                if let Some(previous) = &previous {
                    assert_eq!(
                        page.items.iter().map(|entry| entry.id).collect::<Vec<_>>(),
                        previous
                            .items
                            .iter()
                            .map(|entry| entry.id)
                            .collect::<Vec<_>>()
                    );
                }
                println!(
                    "{run},{label},{method},{milliseconds:.3},{},{}",
                    page.total,
                    page.items.len()
                );
                previous = Some(page);
            }
        }
    }
    Ok(())
}
