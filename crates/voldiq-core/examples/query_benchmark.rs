use std::{collections::HashMap, path::PathBuf, time::Instant};
use voldiq_core::{
    Entry, Kind, NodeId,
    index::{Index, Query},
};

fn entry(id: u32, parent: Option<NodeId>, kind: Kind, logical: u64) -> Entry {
    Entry {
        file_state: None,
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
    println!("Synthetic in-memory index; no filesystem scan or native UI is measured.");
    println!("run,query,milliseconds,total,returned");
    for run in 1..=3 {
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
            let start = Instant::now();
            let page = index.query(&query)?;
            let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
            assert!(page.items.len() <= 100);
            assert_eq!(
                page.total,
                match label {
                    "root-size" => 1000,
                    "name-search" => page.total,
                    _ => count as usize,
                }
            );
            println!(
                "{run},{label},{milliseconds:.3},{},{}",
                page.total,
                page.items.len()
            );
        }
    }
    Ok(())
}
