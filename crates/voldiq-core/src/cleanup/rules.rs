use super::{Plan, PreviewItem};
use crate::{Error, Kind, NodeId, Result, index::Index, windows};
use serde::Serialize;
use std::{
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

const MINIMUM_AGE: u64 = 7 * 24 * 60 * 60;

#[derive(Serialize)]
pub struct Rule {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub discovery: &'static str,
    pub selection: &'static str,
    pub exclusions: &'static str,
    pub risk: &'static str,
    pub permissions: &'static str,
    pub policy: &'static str,
    pub verification: &'static str,
}

pub const USER_TEMP: Rule = Rule {
    id: "old-user-temporary-files",
    name: "User temporary files",
    description: "Indexed .tmp and .temp files unchanged for at least seven days.",
    discovery: "Current user's Windows temporary directory, within the indexed scope.",
    selection: "Explicit selection of complete regular .tmp or .temp files at least seven days old.",
    exclusions: "Other extensions, incomplete files, reparse points, recall-marked content, historical snapshots, configured exclusions and protected locations.",
    risk: "review",
    permissions: "User metadata access for preview; delete permission and unused-file verification for execution.",
    policy: "recycle",
    verification: "Current temp root, scope, age and extension, followed by scanned identity, current metadata and ancestor validation.",
};

#[derive(Serialize)]
pub struct Page {
    pub rule: Rule,
    pub root: String,
    pub indexed: bool,
    pub complete: bool,
    pub items: Vec<PreviewItem>,
    pub total: usize,
    pub logical: u64,
    pub reclaimable: u64,
}

pub fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| Error::Invalid("System clock is before the Unix epoch.".into()))
}

pub fn query(
    index: Option<&Index>,
    temporary: &Path,
    now: u64,
    offset: usize,
    limit: usize,
) -> Result<Page> {
    let temporary = verified_root(temporary)?;
    let mut page = Page {
        rule: USER_TEMP,
        root: temporary.to_string_lossy().into_owned(),
        indexed: false,
        complete: false,
        items: Vec::new(),
        total: 0,
        logical: 0,
        reclaimable: 0,
    };
    let Some(index) = index else {
        return Ok(page);
    };
    if index.snapshot_id.is_some() {
        return Ok(page);
    }
    let Some(scope) = scope(index, &temporary)? else {
        return Ok(page);
    };
    page.indexed = true;
    page.complete = !index.entry(NodeId(0))?.incomplete
        && !index.entry(scope)?.incomplete
        && temporary.starts_with(&index.root);
    let mut candidates: Vec<_> = index
        .entries
        .iter()
        .filter(|entry| eligible(entry, now) && index.in_scope(entry.id, scope))
        .collect();
    candidates.sort_unstable_by_key(|entry| (entry.modified, entry.id.0));
    page.total = candidates.len();
    for entry in &candidates {
        page.logical = page
            .logical
            .checked_add(entry.logical)
            .ok_or_else(|| Error::Invalid("Temporary size overflow.".into()))?;
        page.reclaimable = page
            .reclaimable
            .checked_add(if entry.hard_link || !entry.allocation_known {
                0
            } else {
                entry.allocated
            })
            .ok_or_else(|| Error::Invalid("Temporary allocation overflow.".into()))?;
    }
    for entry in candidates
        .into_iter()
        .skip(offset)
        .take(limit.clamp(1, 100))
    {
        page.items.push(PreviewItem {
            id: entry.id,
            path: index.path(entry.id)?.to_string_lossy().into_owned(),
            logical: entry.logical,
            reclaimable: if entry.hard_link || !entry.allocation_known {
                0
            } else {
                entry.allocated
            },
            hard_link: entry.hard_link,
        });
    }
    Ok(page)
}

pub fn preview(index: &Index, temporary: &Path, now: u64, selection: &[NodeId]) -> Result<Plan> {
    if selection.is_empty() || selection.len() > 500 {
        return Err(Error::Invalid(
            "Select between 1 and 500 temporary files.".into(),
        ));
    }
    let temporary = verified_root(temporary)?;
    let scope = scope(index, &temporary)?
        .ok_or_else(|| Error::Invalid("Scan the user temporary directory first.".into()))?;
    for &id in selection {
        if !eligible(index.entry(id)?, now) || !index.in_scope(id, scope) {
            return Err(Error::Invalid(
                "Selection is outside the temporary-file rule.".into(),
            ));
        }
    }
    let mut plan = Plan::manual(index, selection)?;
    plan.preview.rule = USER_TEMP.id;
    Ok(plan)
}

fn verified_root(path: &Path) -> Result<std::path::PathBuf> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || windows::is_reparse(&metadata) || windows::requires_recall(&metadata) {
        return Err(Error::Invalid(
            "Temporary directory is redirected or unavailable.".into(),
        ));
    }
    let root = std::fs::canonicalize(path)?;
    if root.to_str().is_none() {
        return Err(Error::Invalid(
            "Temporary path cannot be represented exactly.".into(),
        ));
    }
    Ok(root)
}

fn scope(index: &Index, temporary: &Path) -> Result<Option<NodeId>> {
    if index.root.starts_with(temporary) {
        return Ok(Some(NodeId(0)));
    }
    let Ok(relative) = temporary.strip_prefix(&index.root) else {
        return Ok(None);
    };
    let mut scope = NodeId(0);
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(Error::Invalid("Invalid temporary scope.".into()));
        };
        let Some(entry) = index.entries.iter().find(|entry| {
            entry.parent == Some(scope)
                && entry.kind == Kind::Directory
                && index
                    .native_names
                    .get(&entry.id)
                    .map(|native| native.as_os_str())
                    .unwrap_or_else(|| std::ffi::OsStr::new(&entry.name))
                    == name
        }) else {
            return Ok(None);
        };
        scope = entry.id;
    }
    Ok(Some(scope))
}

fn eligible(entry: &crate::Entry, now: u64) -> bool {
    entry.kind == Kind::File
        && !entry.incomplete
        && entry.name.rsplit_once('.').is_some_and(|(_, extension)| {
            extension.eq_ignore_ascii_case("tmp") || extension.eq_ignore_ascii_case("temp")
        })
        && now
            .checked_sub(MINIMUM_AGE)
            .is_some_and(|cutoff| entry.modified.is_some_and(|modified| modified <= cutoff))
}
