use crate::{Error, Kind, NodeId, Result, index::Index, metadata::Protections};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Request {
    pub scope: Option<NodeId>,
    pub after: Option<NodeId>,
    pub limit: Option<usize>,
}

impl Request {
    pub fn valid(&self) -> bool {
        self.limit.is_none_or(|limit| (1..=100).contains(&limit))
    }
}

#[derive(Serialize)]
pub struct Page {
    scope: NodeId,
    rule: &'static str,
    rule_version: u32,
    risk: &'static str,
    #[serde(flatten)]
    status: Status,
}

#[derive(Serialize)]
#[serde(tag = "availability", rename_all = "snake_case")]
enum Status {
    #[cfg(not(windows))]
    Unsupported,
    #[cfg(windows)]
    ScanObservationUnavailable,
    #[cfg(windows)]
    Observed {
        scan_observed_at: u64,
        scan_complete: bool,
        checked_at: u64,
        unverified_folders: usize,
        next_after: Option<NodeId>,
        items: Vec<Folder>,
    },
}

#[cfg(windows)]
#[derive(Serialize)]
struct Folder {
    node_id: NodeId,
    relative_path: String,
}

pub fn query(
    index: &Index,
    request: &Request,
    page_size: usize,
    protections: &Protections,
    cancel: &AtomicBool,
) -> Result<Option<Page>> {
    if !request.valid() || page_size == 0 {
        return Err(Error::Invalid("Invalid empty folder page.".into()));
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let scope = request.scope.unwrap_or(NodeId(0));
    if index.entry(scope)?.kind != Kind::Directory
        || request
            .after
            .is_some_and(|after| index.entry(after).is_err() || !index.in_scope(after, scope))
    {
        return Err(Error::Invalid(
            "Choose an indexed directory and a cursor within it.".into(),
        ));
    }
    let status = observe(index, scope, request, page_size, protections, cancel);
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some(Page {
        scope,
        rule: "empty-folders",
        rule_version: 1,
        risk: "informational",
        status: status?,
    }))
}

#[cfg(windows)]
fn observe(
    index: &Index,
    scope: NodeId,
    request: &Request,
    page_size: usize,
    protections: &Protections,
    cancel: &AtomicBool,
) -> Result<Status> {
    use crate::metadata::{Eligibility, path_key};
    use std::collections::HashSet;

    // Native probes are bounded even when the current filesystem no longer matches the index.
    const MAX_PROBES: usize = 128;
    let Some(observation) = &index.observation else {
        return Ok(Status::ScanObservationUnavailable);
    };
    let _scope = index.pin_folder(scope)?;
    let scope_path = index.path(scope)?;
    let mask = protections.for_index(index, cancel)?;
    let excluded_parents: HashSet<_> = index
        .exclusions
        .iter()
        .filter_map(|path| path.parent())
        .map(path_key)
        .collect();
    let mut has_children = vec![false; index.entries.len()];
    for entry in &index.entries {
        super::check_cancelled(cancel)?;
        if let Some(parent) = entry.parent {
            has_children[parent.0 as usize] = true;
        }
    }
    let limit = request.limit.unwrap_or(page_size).min(page_size).min(100);
    let mut items = Vec::with_capacity(limit);
    let mut last = None;
    let mut next_after = None;
    let mut unverified_folders = 0;
    let mut probes = 0;
    let start = request.after.map_or(0, |cursor| cursor.0 as usize + 1);
    for entry in &index.entries[start..] {
        super::check_cancelled(cancel)?;
        if entry.id == scope
            || entry.kind != Kind::Directory
            || has_children[entry.id.0 as usize]
            || !index.in_scope(entry.id, scope)
            || mask.eligibility(entry.id)? == Eligibility::Protected
        {
            continue;
        }
        if probes == MAX_PROBES || items.len() == limit {
            next_after = last;
            break;
        }
        probes += 1;
        last = Some(entry.id);
        let path = index.path(entry.id)?;
        if entry.incomplete || !mask.allows(entry.id) || excluded_parents.contains(&path_key(&path))
        {
            unverified_folders += 1;
            continue;
        }
        match checked_empty(index, entry.id) {
            Ok(true) => {
                let relative_path = path
                    .strip_prefix(&scope_path)
                    .map_err(|_| Error::Invalid("Invalid indexed folder location.".into()))?;
                items.push(Folder {
                    node_id: entry.id,
                    relative_path: relative_path.to_string_lossy().into_owned(),
                });
            }
            Ok(false) => {}
            Err(_) => unverified_folders += 1,
        }
    }
    Ok(Status::Observed {
        scan_observed_at: observation.completed_at,
        scan_complete: observation.complete,
        checked_at: super::rules::now()?,
        unverified_folders,
        next_after,
        items,
    })
}

#[cfg(windows)]
fn checked_empty(index: &Index, node: NodeId) -> Result<bool> {
    use crate::windows;
    let folder = index.pin_folder(node)?;
    let directory = folder.directory_handle()?;
    let before = windows::file_identity(directory)?;
    let empty = windows::cleanup_stage::directory_is_empty(directory)?;
    if empty && windows::file_identity(directory)?.change_time != before.change_time {
        return Err(Error::Invalid(
            "Directory changed during enumeration.".into(),
        ));
    }
    Ok(empty)
}

#[cfg(not(windows))]
fn observe(
    _: &Index,
    _: NodeId,
    _: &Request,
    _: usize,
    _: &Protections,
    _: &AtomicBool,
) -> Result<Status> {
    Ok(Status::Unsupported)
}
