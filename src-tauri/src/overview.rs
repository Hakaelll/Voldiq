use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};
use voldiq_core::{
    NodeId,
    index::{Details, FileTypes, Index, Page, Query},
};

#[derive(serde::Serialize)]
pub(crate) struct Overview {
    pub(crate) directory: Details,
    pub(crate) children: Page,
    pub(crate) types: FileTypes,
    pub(crate) branches: Vec<MapBranch>,
    pub(crate) map_metrics: Vec<MapMetric>,
}

#[derive(serde::Serialize)]
pub(crate) struct MapMetric {
    pub(crate) id: NodeId,
    pub(crate) latest_modified: Option<u64>,
    pub(crate) modified_known: u64,
    pub(crate) modified_total: u64,
}

impl MapMetric {
    fn include_file(&mut self, modified: Option<u64>) {
        self.latest_modified = self.latest_modified.max(modified);
        self.modified_known += u64::from(modified.is_some());
        self.modified_total += 1;
    }
}

#[derive(serde::Serialize)]
pub(crate) struct MapBranch {
    pub(crate) parent: NodeId,
    pub(crate) items: Vec<voldiq_core::Entry>,
}

pub(crate) fn map_branches(
    index: &Index,
    children: &Page,
    cancelled: &AtomicBool,
) -> Option<Vec<MapBranch>> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let mut groups: std::collections::BTreeMap<u32, Vec<&voldiq_core::Entry>> = children
        .items
        .iter()
        .filter(|entry| entry.kind == voldiq_core::Kind::Directory)
        .take(12)
        .map(|entry| (entry.id.0, Vec::new()))
        .collect();
    if groups.is_empty() {
        return Some(Vec::new());
    }
    for (position, entry) in index.entries.iter().enumerate() {
        if position % 256 == 0 && cancelled.load(Ordering::Relaxed) {
            return None;
        }
        if let Some(items) = entry.parent.and_then(|parent| groups.get_mut(&parent.0)) {
            if items.len() == 8
                && items.last().is_some_and(|last| {
                    entry.logical < last.logical
                        || (entry.logical == last.logical && entry.id.0 >= last.id.0)
                })
            {
                continue;
            }
            items.push(entry);
            items.sort_unstable_by(|a, b| b.logical.cmp(&a.logical).then(a.id.0.cmp(&b.id.0)));
            items.truncate(8);
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    Some(
        groups
            .into_iter()
            .map(|(parent, items)| MapBranch {
                parent: NodeId(parent),
                items: items.into_iter().cloned().collect(),
            })
            .collect(),
    )
}

pub(crate) fn overview(
    index: &Index,
    id: NodeId,
    cancelled: &AtomicBool,
) -> Result<Option<Overview>, String> {
    let Some(children) = index
        .query_cancellable(
            &Query {
                parent: Some(id),
                limit: 250,
                sort: "size".into(),
                ..Query::default()
            },
            || cancelled.load(Ordering::Relaxed),
        )
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let Some(types) = index
        .file_types_cancellable(id, || cancelled.load(Ordering::Relaxed))
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let Some(directory) = index
        .details_cancellable(id, || cancelled.load(Ordering::Relaxed))
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let Some(branches) = map_branches(index, &children, cancelled) else {
        return Ok(None);
    };
    let Some(map_metrics) = map_metrics(index, &directory.entry, &children, &branches, || {
        cancelled.load(Ordering::Relaxed)
    }) else {
        return Ok(None);
    };
    Ok(Some(Overview {
        directory,
        children,
        types,
        branches,
        map_metrics,
    }))
}

fn map_metrics(
    index: &Index,
    scope: &voldiq_core::Entry,
    children: &Page,
    branches: &[MapBranch],
    mut cancelled: impl FnMut() -> bool,
) -> Option<Vec<MapMetric>> {
    if cancelled() {
        return None;
    }
    let mut positions = HashMap::new();
    let mut metrics = Vec::new();
    for entry in std::iter::once(scope)
        .chain(&children.items)
        .chain(branches.iter().flat_map(|branch| &branch.items))
    {
        if positions.contains_key(&entry.id) {
            continue;
        }
        positions.insert(entry.id, metrics.len());
        metrics.push(MapMetric {
            id: entry.id,
            latest_modified: None,
            modified_known: 0,
            modified_total: 0,
        });
    }
    let mut parent_metrics = HashMap::<NodeId, Vec<usize>>::new();
    for (position, entry) in index.entries.iter().enumerate() {
        if position % 256 == 0 && cancelled() {
            return None;
        }
        if entry.kind != voldiq_core::Kind::File {
            continue;
        }
        if let Some(&position) = positions.get(&entry.id) {
            metrics[position].include_file(entry.modified);
        }
        let Some(parent) = entry.parent else {
            continue;
        };
        if !parent_metrics.contains_key(&parent) {
            let mut path = Vec::new();
            let mut ancestors = Vec::new();
            let mut next = Some(parent);
            while let Some(id) = next {
                if path.len() % 256 == 0 && cancelled() {
                    return None;
                }
                if let Some(cached) = parent_metrics.get(&id) {
                    ancestors.extend(cached);
                    break;
                }
                path.push(id);
                next = index.entries[id.0 as usize].parent;
            }
            for (depth, id) in path.into_iter().rev().enumerate() {
                if depth % 256 == 0 && cancelled() {
                    return None;
                }
                if let Some(&position) = positions.get(&id) {
                    ancestors.push(position);
                }
                // Bound cache memory independently of index size while reusing shared ancestry.
                if parent_metrics.len() == 4096 {
                    parent_metrics.clear();
                }
                parent_metrics.insert(id, ancestors.clone());
            }
        }
        for &position in &parent_metrics[&parent] {
            metrics[position].include_file(entry.modified);
        }
    }
    if cancelled() { None } else { Some(metrics) }
}

#[cfg(test)]
#[path = "overview_tests.rs"]
mod tests;
