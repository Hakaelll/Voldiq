use super::*;
use std::collections::HashMap;
use std::path::Path;
use voldiq_core::{Entry, Kind};

fn entry(id: u32, parent: Option<u32>, kind: Kind, modified: Option<u64>) -> Entry {
    Entry {
        id: NodeId(id),
        parent: parent.map(NodeId),
        name: format!("item-{id}"),
        kind,
        logical: 1,
        allocated: 1,
        allocation_known: true,
        modified,
        files: u64::from(kind == Kind::File),
        incomplete: false,
        hard_link: false,
        file_state: None,
        cloud: voldiq_core::cloud::CloudState::Unknown,
    }
}

fn index(root: &Path, entries: Vec<Entry>) -> Index {
    Index {
        root: root.to_owned(),
        entries,
        issues: Vec::new(),
        error_count: 0,
        native_names: HashMap::new(),
        root_identity: None,
        exclusions: Vec::new(),
        snapshot_id: None,
        captured_caches: Vec::new(),
        observation: None,
        ntfs_checkpoint: None,
    }
}

#[test]
fn overview_reports_descendant_dates_and_coverage_only_for_displayed_nodes() {
    let root = tempfile::tempdir().unwrap();
    let index = index(
        root.path(),
        vec![
            entry(0, None, Kind::Directory, Some(10000)),
            entry(1, Some(0), Kind::Directory, Some(10000)),
            entry(2, Some(0), Kind::Directory, Some(10000)),
            entry(3, Some(1), Kind::Directory, Some(10000)),
            entry(4, Some(1), Kind::Directory, Some(10000)),
            entry(5, Some(1), Kind::File, Some(70)),
            entry(6, Some(1), Kind::File, None),
            entry(7, Some(1), Kind::Link, Some(10000)),
            entry(8, Some(3), Kind::Directory, Some(10000)),
            entry(9, Some(3), Kind::File, Some(90)),
            entry(10, Some(8), Kind::File, Some(200)),
            entry(11, Some(8), Kind::File, None),
            entry(12, Some(3), Kind::Directory, None),
            entry(13, Some(12), Kind::File, Some(300)),
            entry(14, Some(2), Kind::File, Some(9999)),
        ],
    );
    let overview = overview(&index, NodeId(1), None, None, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    let actual: std::collections::BTreeMap<_, _> = overview
        .map_metrics
        .iter()
        .map(|metric| {
            (
                metric.id.0,
                (
                    metric.latest_modified,
                    metric.modified_known,
                    metric.modified_total,
                ),
            )
        })
        .collect();
    assert_eq!(
        actual,
        std::collections::BTreeMap::from([
            (1, (Some(300), 4, 6)),
            (3, (Some(300), 3, 4)),
            (4, (None, 0, 0)),
            (5, (Some(70), 1, 1)),
            (6, (None, 0, 1)),
            (7, (None, 0, 0)),
            (8, (Some(200), 1, 2)),
            (9, (Some(90), 1, 1)),
            (12, (Some(300), 1, 1)),
        ])
    );
    let json = serde_json::to_value(&overview).unwrap();
    assert_eq!(json["map_metrics"][0]["id"], 1);
    assert_eq!(json["map_metrics"][0]["latest_modified"], 300);
    assert_eq!(json["map_metrics"][0]["modified_known"], 4);
    assert_eq!(json["map_metrics"][0]["modified_total"], 6);
    assert!(
        json["map_metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|metric| metric["id"] == 4)
            .unwrap()["latest_modified"]
            .is_null()
    );
}

#[test]
fn metric_cache_eviction_keeps_all_descendant_files() {
    let root = tempfile::tempdir().unwrap();
    let mut entries = vec![entry(0, None, Kind::Directory, Some(10000))];
    for number in 0..4100 {
        let parent = entries.len() as u32;
        entries.push(entry(parent, Some(0), Kind::Directory, None));
        entries.push(entry(parent + 1, Some(parent), Kind::File, Some(number)));
    }
    let index = index(root.path(), entries);
    let empty_page = Page {
        items: Vec::new(),
        total: 0,
        largest: 0,
    };
    let metrics = map_metrics(
        &index,
        &index.entries[0],
        &empty_page,
        &[],
        None,
        None,
        || false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].latest_modified, Some(4099));
    assert_eq!(metrics[0].modified_known, 4100);
    assert_eq!(metrics[0].modified_total, 4100);
}

#[test]
fn metric_ancestry_walk_can_be_cancelled() {
    let root = tempfile::tempdir().unwrap();
    let mut entries = vec![entry(0, None, Kind::Directory, None)];
    for id in 1..1024 {
        entries.push(entry(id, Some(id - 1), Kind::Directory, None));
    }
    entries.push(entry(1024, Some(1023), Kind::File, Some(1)));
    let index = index(root.path(), entries);
    let empty_page = Page {
        items: Vec::new(),
        total: 0,
        largest: 0,
    };
    let mut checks = 0;
    let metrics = map_metrics(
        &index,
        &index.entries[0],
        &empty_page,
        &[],
        None,
        None,
        || {
            checks += 1;
            checks == 9
        },
    )
    .unwrap();
    assert!(metrics.is_none());
    assert_eq!(checks, 9);
    assert!(
        overview(&index, NodeId(0), None, None, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
}

#[test]
fn cloud_overview_counts_matching_files_beyond_its_bounded_unfiltered_frontier() {
    use voldiq_core::cloud::{CloudFilter, CloudState};
    let root = tempfile::tempdir().unwrap();
    let mut entries = vec![entry(0, None, Kind::Directory, None)];
    entries[0].logical = 250010;
    entries[0].files = 260;
    entries[0].allocation_known = false;
    entries[0].incomplete = true;
    for id in 1..=260 {
        let mut file = entry(id, Some(0), Kind::File, Some(u64::from(id)));
        if id <= 250 {
            file.logical = 1000;
            file.cloud = CloudState::NotPlaceholder;
        } else {
            file.kind = Kind::Link;
            file.cloud = CloudState::FileRecallRequired;
            file.allocation_known = false;
            file.allocated = 0;
            file.incomplete = true;
        }
        entries.push(file);
    }
    let index = index(root.path(), entries);
    let result = overview(
        &index,
        NodeId(0),
        Some(CloudFilter::Placeholder),
        None,
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(result.children.total, 260);
    assert_eq!(result.children.items.len(), 250);
    assert_eq!(result.directory.entry.logical, 250010);
    assert_eq!(result.map_metrics.len(), 251);
    assert_eq!(
        (
            result.map_metrics[0].logical,
            result.map_metrics[0].files,
            result.map_metrics[0].latest_modified
        ),
        (10, 10, Some(260))
    );
    assert!(!result.map_metrics[0].allocation_known);
    assert!(
        result.map_metrics[1..]
            .iter()
            .all(|metric| metric.logical == 0 && metric.files == 0)
    );
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(json["cloud"], "placeholder");
    assert_eq!(json["directory"]["entry"]["logical"], 250010);
    assert_eq!(json["map_metrics"][0]["logical"], 10);
    let query = Query {
        expression: "type:file size<=1B".into(),
        ..Default::default()
    };
    let matching = overview(
        &index,
        NodeId(0),
        None,
        Some(&query),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(matching.directory.entry.logical, 250010);
    assert_eq!(matching.map_metrics[0].logical, 250010);
    assert_eq!(
        matching.map_metrics[0]
            .matches
            .as_ref()
            .unwrap()
            .descendants,
        10
    );
    assert!(
        matching.map_metrics[1..]
            .iter()
            .all(|metric| !metric.matches.as_ref().unwrap().own
                && metric.matches.as_ref().unwrap().descendants == 0)
    );
}
