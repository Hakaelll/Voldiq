use std::sync::atomic::{AtomicBool, Ordering};
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
    Ok(Some(Overview {
        directory,
        children,
        types,
        branches,
    }))
}
