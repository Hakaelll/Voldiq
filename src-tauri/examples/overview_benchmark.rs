#[path = "../src/overview.rs"]
mod overview;

use std::{
    collections::{BTreeMap, HashMap},
    io,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use voldiq_core::{
    Entry, Kind, NodeId,
    index::{Index, Page, Query},
};

const FILES: u32 = 1_000_000;

fn entry(id: u32, parent: Option<NodeId>, kind: Kind) -> Entry {
    Entry {
        id: NodeId(id),
        parent,
        name: if kind == Kind::File {
            format!(
                "file-{id:08}.{}",
                if id.is_multiple_of(2) { "bin" } else { "rs" }
            )
        } else {
            format!("folder-{id}")
        },
        kind,
        logical: 0,
        allocated: 0,
        allocation_known: true,
        modified: Some(1),
        files: 0,
        incomplete: false,
        hard_link: false,
        file_state: None,
        cloud: voldiq_core::cloud::CloudState::Unknown,
    }
}

fn fixture(shape: &str) -> (Index, NodeId, u64) {
    let directories = match shape {
        "wide-equal" => 16,
        "wide-varied" => 1000,
        "deep-varied" => 64,
        _ => unreachable!(),
    };
    let deep = shape == "deep-varied";
    let mut index = Index {
        root: PathBuf::from("C:\\generated-overview-benchmark"),
        entries: Vec::with_capacity((FILES + directories + 1) as usize),
        issues: Vec::new(),
        error_count: 0,
        native_names: HashMap::new(),
        root_identity: None,
        exclusions: Vec::new(),
        snapshot_id: None,
        captured_caches: Vec::new(),
        captured_applications: None,
        captured_libraries: None,
        observation: None,
        ntfs_checkpoint: None,
    };
    index.entries.push(entry(0, None, Kind::Directory));
    for id in 1..=directories {
        index.entries.push(entry(
            id,
            Some(NodeId(if deep { id - 1 } else { 0 })),
            Kind::Directory,
        ));
    }
    let mut bytes = 0;
    for position in 0..FILES {
        let parent = if deep {
            directories
        } else {
            1 + position % directories
        };
        let mut file = entry(directories + 1 + position, Some(NodeId(parent)), Kind::File);
        file.logical = if shape == "wide-equal" {
            1
        } else {
            (u64::from(position) * 7919) % 1_000_003 + 1
        };
        file.allocated = file.logical;
        file.files = 1;
        bytes += file.logical;
        index.entries[parent as usize].logical += file.logical;
        index.entries[parent as usize].files += 1;
        index.entries.push(file);
    }
    for id in (1..=directories).rev() {
        let parent = index.entries[id as usize].parent.unwrap().0 as usize;
        index.entries[parent].logical += index.entries[id as usize].logical;
        index.entries[parent].files += index.entries[id as usize].files;
        index.entries[id as usize].allocated = index.entries[id as usize].logical;
    }
    index.entries[0].allocated = bytes;
    (index, NodeId(if deep { 32 } else { 0 }), bytes)
}

fn reference(index: &Index, scope: NodeId) -> (Vec<NodeId>, usize, BTreeMap<u32, Vec<NodeId>>) {
    let mut children: Vec<_> = index
        .entries
        .iter()
        .filter(|entry| entry.parent == Some(scope))
        .collect();
    let rank = |a: &&Entry, b: &&Entry| b.logical.cmp(&a.logical).then(a.id.0.cmp(&b.id.0));
    children.sort_unstable_by(rank);
    let total = children.len();
    children.truncate(250);
    let mut branches: BTreeMap<_, Vec<&Entry>> = children
        .iter()
        .filter(|entry| entry.kind == Kind::Directory)
        .take(12)
        .map(|entry| (entry.id.0, Vec::new()))
        .collect();
    for entry in &index.entries {
        if let Some(items) = entry.parent.and_then(|parent| branches.get_mut(&parent.0)) {
            items.push(entry);
        }
    }
    let branches = branches
        .into_iter()
        .map(|(parent, mut entries)| {
            entries.sort_unstable_by(rank);
            entries.truncate(8);
            (parent, entries.into_iter().map(|entry| entry.id).collect())
        })
        .collect();
    (
        children.into_iter().map(|entry| entry.id).collect(),
        total,
        branches,
    )
}

#[cfg(windows)]
fn cpu_ticks() -> io::Result<u64> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentThread, GetThreadTimes},
    };
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    if unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Ok(ticks(kernel) + ticks(user))
}

#[cfg(not(windows))]
fn cpu_ticks() -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows thread counters required.",
    ))
}

fn measure<T>(work: impl FnOnce() -> T) -> io::Result<(T, f64, f64)> {
    let before = cpu_ticks()?;
    let start = Instant::now();
    let result = std::hint::black_box(work());
    let wall = start.elapsed().as_secs_f64() * 1000.0;
    let cpu = (cpu_ticks()? - before) as f64 / 10_000.0;
    Ok((result, wall, cpu))
}

fn check_branches(actual: &[overview::MapBranch], expected: &BTreeMap<u32, Vec<NodeId>>) {
    let actual: BTreeMap<_, Vec<_>> = actual
        .iter()
        .map(|branch| {
            (
                branch.parent.0,
                branch.items.iter().map(|entry| entry.id).collect(),
            )
        })
        .collect();
    assert_eq!(&actual, expected);
}

fn check_extension_groups() -> Result<(), Box<dyn std::error::Error>> {
    let mut index = Index {
        root: PathBuf::from("C:\\generated-extension-check"),
        entries: vec![entry(0, None, Kind::Directory)],
        issues: Vec::new(),
        error_count: 0,
        native_names: HashMap::new(),
        root_identity: None,
        exclusions: Vec::new(),
        snapshot_id: None,
        captured_caches: Vec::new(),
        captured_applications: None,
        captured_libraries: None,
        observation: None,
        ntfs_checkpoint: None,
    };
    for (position, name) in ["one.TXT", "two.txt", "three.Ä", "four.ä", "README", "file."]
        .iter()
        .enumerate()
    {
        let mut file = entry(position as u32 + 1, Some(NodeId(0)), Kind::File);
        file.name = (*name).into();
        file.logical = position as u64 + 1;
        index.entries.push(file);
    }
    let expected = BTreeMap::from([
        (None, (5, 1)),
        (Some(String::new()), (6, 1)),
        (Some("txt".into()), (3, 2)),
        (Some("ä".into()), (7, 2)),
    ]);
    for groups in [
        index.file_types(NodeId(0))?,
        index
            .file_types_cancellable(NodeId(0), || false)?
            .ok_or("Unexpected cancellation")?,
    ] {
        assert_eq!((groups.total_bytes, groups.total_files), (21, 6));
        assert_eq!(
            groups
                .items
                .into_iter()
                .map(|group| (group.extension, (group.bytes, group.count)))
                .collect::<BTreeMap<_, _>>(),
            expected
        );
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    check_extension_groups()?;
    let diagnostic = std::env::args().nth(1).as_deref() == Some("--diagnostic");
    if std::env::args().len() > if diagnostic { 2 } else { 1 } {
        return Err("Unexpected benchmark arguments".into());
    }
    eprintln!(
        "Synthetic in-memory index; production overview module. No scan, IPC, Canvas or application latency measured."
    );
    println!("shape,run,phase,method,wall_ms,thread_cpu_ms,files,returned,bytes");
    let cancelled = AtomicBool::new(false);
    for shape in ["wide-equal", "wide-varied", "deep-varied"] {
        let (index, scope, expected_bytes) = fixture(shape);
        let (expected_children, expected_total, expected_branches) = reference(&index, scope);
        let expected_count: usize = expected_branches.values().map(Vec::len).sum();
        let warm = overview::overview(&index, scope, None, None, &cancelled)
            .map_err(io::Error::other)?
            .ok_or("Unexpected cancellation")?;
        assert_eq!(warm.types.total_files, u64::from(FILES));
        let children = Page {
            items: expected_children
                .iter()
                .map(|id| index.entries[id.0 as usize].clone())
                .collect(),
            total: expected_total,
            largest: 0,
        };
        for run in 1..=5 {
            if diagnostic {
                let (page, wall, cpu) = measure(|| {
                    index.query_cancellable(
                        &Query {
                            parent: Some(scope),
                            limit: 250,
                            ..Query::default()
                        },
                        || cancelled.load(Ordering::Relaxed),
                    )
                })?;
                assert_eq!(
                    page?
                        .ok_or("Unexpected cancellation")?
                        .items
                        .iter()
                        .map(|entry| entry.id)
                        .collect::<Vec<_>>(),
                    expected_children
                );
                println!(
                    "{shape},{run},diagnostic,children,{wall:.3},{cpu:.3},{FILES},{},0",
                    expected_children.len()
                );
                let (types, wall, cpu) = measure(|| {
                    index.file_types_cancellable(scope, || cancelled.load(Ordering::Relaxed))
                })?;
                let types = types?.ok_or("Unexpected cancellation")?;
                assert_eq!(
                    (types.total_files, types.total_bytes),
                    (u64::from(FILES), expected_bytes)
                );
                println!(
                    "{shape},{run},diagnostic,file-types,{wall:.3},{cpu:.3},{FILES},{},0",
                    types.items.len()
                );
                let (details, wall, cpu) = measure(|| {
                    index.details_cancellable(scope, || cancelled.load(Ordering::Relaxed))
                })?;
                assert_eq!(
                    details?.ok_or("Unexpected cancellation")?.entry.logical,
                    expected_bytes
                );
                println!("{shape},{run},diagnostic,details,{wall:.3},{cpu:.3},{FILES},1,0");
                continue;
            }
            let methods = if run % 2 == 1 {
                ["overview", "branches"]
            } else {
                ["branches", "overview"]
            };
            for method in methods {
                if method == "branches" {
                    let (branches, wall, cpu) =
                        measure(|| overview::map_branches(&index, &children, &cancelled))?;
                    let branches = branches.ok_or("Unexpected cancellation")?;
                    check_branches(&branches, &expected_branches);
                    println!(
                        "{shape},{run},measurement,branches,{wall:.3},{cpu:.3},{FILES},{expected_count},0"
                    );
                } else {
                    let (result, wall, cpu) =
                        measure(|| overview::overview(&index, scope, None, None, &cancelled))?;
                    let result = result
                        .map_err(io::Error::other)?
                        .ok_or("Unexpected cancellation")?;
                    assert_eq!(result.directory.entry.logical, expected_bytes);
                    assert_eq!(
                        (result.types.total_files, result.types.total_bytes),
                        (u64::from(FILES), expected_bytes)
                    );
                    assert_eq!(result.children.total, expected_total);
                    assert_eq!(
                        result
                            .children
                            .items
                            .iter()
                            .map(|entry| entry.id)
                            .collect::<Vec<_>>(),
                        expected_children
                    );
                    check_branches(&result.branches, &expected_branches);
                    println!(
                        "{shape},{run},measurement,overview,{wall:.3},{cpu:.3},{FILES},{},0",
                        result.children.items.len() + expected_count
                    );
                    let (json, wall, cpu) = measure(|| serde_json::to_vec(&result))?;
                    let json = json?;
                    let decoded: serde_json::Value = serde_json::from_slice(&json)?;
                    assert_eq!(decoded["types"]["total_files"], FILES);
                    assert_eq!(
                        decoded["children"]["items"].as_array().unwrap().len(),
                        expected_children.len()
                    );
                    println!(
                        "{shape},{run},measurement,serialize,{wall:.3},{cpu:.3},{FILES},{},{}",
                        result.children.items.len() + expected_count,
                        json.len()
                    );
                }
            }
        }
    }
    Ok(())
}
