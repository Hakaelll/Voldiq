use super::{Plan, PreviewItem};
use crate::{Error, Kind, NodeId, Result, index::Index, windows};
use serde::Serialize;
use std::{
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

const MINIMUM_AGE: u64 = 7 * 24 * 60 * 60;
const OLD_FILE_AGE: u64 = 90 * 24 * 60 * 60;

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    UserTemp,
    OldFiles,
    ProjectTemp,
}

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

pub const PROJECT_TEMP: Rule = Rule {
    id: "old-project-temporary-files",
    name: "Project temporary files",
    description: "Indexed .tmp and .temp files unchanged for at least seven days in recognized build artifacts.",
    discovery: "Indexed Cargo, Node.js, CMake or .NET build artifacts identified by complete project manifest entries.",
    selection: "Explicit selection of complete regular .tmp or .temp files at least seven days old within recognized build scopes.",
    exclusions: "Source directories, recognized installed dependencies, configured global cache locations, other extensions, incomplete files or markers, reparse points, recalled content, historical snapshots, configured exclusions and protected locations.",
    risk: "review",
    permissions: "User metadata access for preview; delete permission and unused-file verification for execution.",
    policy: "recycle",
    verification: "Indexed build classification, age, scanned identity, current metadata, exclusions, protected locations and ancestor validation.",
};

pub const OLD_FILES: Rule = Rule {
    id: "manually-selected-old-files",
    name: "Old files",
    description: "Indexed files unchanged for at least 90 days. Age does not establish that a file is unused.",
    discovery: "Current live scan scope, excluding protected Windows locations.",
    selection: "Explicit manual selection of complete regular files at least 90 days old.",
    exclusions: "Protected Windows locations, configured exclusions, incomplete files, reparse points, recall-marked content and historical snapshots.",
    risk: "review",
    permissions: "User metadata access for preview; delete permission for execution.",
    policy: "recycle",
    verification: "Live scope and age, followed by scanned identity, current metadata, exclusions, protected locations and ancestor validation.",
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
    query_scoped(
        index,
        temporary,
        now,
        offset,
        limit,
        RuleKind::UserTemp,
        &[],
    )
}

pub fn query_old(index: Option<&Index>, now: u64, offset: usize, limit: usize) -> Result<Page> {
    query_current(index, now, offset, limit, RuleKind::OldFiles, &[])
}

pub fn query_project(
    index: Option<&Index>,
    now: u64,
    offset: usize,
    limit: usize,
    caches: &[crate::developer::CacheLocation],
) -> Result<Page> {
    query_current(index, now, offset, limit, RuleKind::ProjectTemp, caches)
}

fn query_current(
    index: Option<&Index>,
    now: u64,
    offset: usize,
    limit: usize,
    kind: RuleKind,
    caches: &[crate::developer::CacheLocation],
) -> Result<Page> {
    match index.filter(|index| index.snapshot_id.is_none()) {
        Some(index) => query_scoped(Some(index), &index.root, now, offset, limit, kind, caches),
        None => Ok(Page {
            rule: match kind {
                RuleKind::ProjectTemp => PROJECT_TEMP,
                RuleKind::OldFiles => OLD_FILES,
                RuleKind::UserTemp => USER_TEMP,
            },
            root: String::new(),
            indexed: false,
            complete: false,
            items: Vec::new(),
            total: 0,
            logical: 0,
            reclaimable: 0,
        }),
    }
}

fn query_scoped(
    index: Option<&Index>,
    temporary: &Path,
    now: u64,
    offset: usize,
    limit: usize,
    kind: RuleKind,
    caches: &[crate::developer::CacheLocation],
) -> Result<Page> {
    let temporary = verified_root(temporary)?;
    let mut page = Page {
        rule: match kind {
            RuleKind::UserTemp => USER_TEMP,
            RuleKind::OldFiles => OLD_FILES,
            RuleKind::ProjectTemp => PROJECT_TEMP,
        },
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
    let Some(scope_id) = scope(index, &temporary)? else {
        return Ok(page);
    };
    page.indexed = true;
    page.complete = !index.entry(NodeId(0))?.incomplete
        && !index.entry(scope_id)?.incomplete
        && temporary.starts_with(&index.root);
    let mut protected_scopes = Vec::new();
    #[cfg(windows)]
    if matches!(kind, RuleKind::OldFiles | RuleKind::ProjectTemp) {
        for path in windows::protected_directories()? {
            let path = std::fs::canonicalize(path)?;
            if let Some(id) = scope(index, &path)? {
                protected_scopes.push(id);
            }
        }
    }
    let builds = if matches!(kind, RuleKind::ProjectTemp) {
        crate::developer::project_build_scopes(index, caches)?
    } else {
        Default::default()
    };
    let mut candidates: Vec<_> = index
        .entries
        .iter()
        .filter(|entry| {
            eligible(entry, now, kind)
                && index.in_scope(entry.id, scope_id)
                && (!matches!(kind, RuleKind::ProjectTemp) || builds.contains(index, entry.id))
                && !protected_scopes
                    .iter()
                    .any(|&protected| index.in_scope(entry.id, protected))
        })
        .collect();
    candidates.sort_unstable_by_key(|entry| (entry.modified, entry.id.0));
    page.total = candidates.len();
    for entry in &candidates {
        page.logical = page
            .logical
            .checked_add(entry.logical)
            .ok_or_else(|| Error::Invalid("Cleanup size overflow.".into()))?;
        page.reclaimable = page
            .reclaimable
            .checked_add(if entry.hard_link || !entry.allocation_known {
                0
            } else {
                entry.allocated
            })
            .ok_or_else(|| Error::Invalid("Cleanup allocation overflow.".into()))?;
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
            modified: entry.modified,
        });
    }
    Ok(page)
}

pub fn preview(index: &Index, temporary: &Path, now: u64, selection: &[NodeId]) -> Result<Plan> {
    preview_scoped(index, temporary, now, selection, RuleKind::UserTemp)
}

pub fn preview_old(index: &Index, now: u64, selection: &[NodeId]) -> Result<Plan> {
    preview_scoped(index, &index.root, now, selection, RuleKind::OldFiles)
}

pub fn preview_project(
    index: &Index,
    now: u64,
    selection: &[NodeId],
    caches: &[crate::developer::CacheLocation],
) -> Result<Plan> {
    let builds = crate::developer::project_build_scopes(index, caches)?;
    for &id in selection {
        index.entry(id)?;
        if !builds.contains(index, id) {
            return Err(Error::Invalid(
                "Selection is outside recognized project build artifacts.".into(),
            ));
        }
    }
    preview_scoped(index, &index.root, now, selection, RuleKind::ProjectTemp)
}

fn preview_scoped(
    index: &Index,
    temporary: &Path,
    now: u64,
    selection: &[NodeId],
    kind: RuleKind,
) -> Result<Plan> {
    if selection.is_empty() || selection.len() > 500 {
        return Err(Error::Invalid("Select between 1 and 500 files.".into()));
    }
    let temporary = verified_root(temporary)?;
    let scope = scope(index, &temporary)?
        .ok_or_else(|| Error::Invalid("Scan the rule location first.".into()))?;
    for &id in selection {
        if !eligible(index.entry(id)?, now, kind) || !index.in_scope(id, scope) {
            return Err(Error::Invalid(
                "Selection is outside the cleanup rule.".into(),
            ));
        }
    }
    let mut plan = Plan::manual(index, selection)?;
    plan.preview.rule = match kind {
        RuleKind::UserTemp => USER_TEMP.id,
        RuleKind::OldFiles => OLD_FILES.id,
        RuleKind::ProjectTemp => PROJECT_TEMP.id,
    };
    Ok(plan)
}

fn verified_root(path: &Path) -> Result<std::path::PathBuf> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || windows::is_reparse(&metadata) || windows::requires_recall(&metadata) {
        return Err(Error::Invalid(
            "Rule directory is redirected or unavailable.".into(),
        ));
    }
    let root = std::fs::canonicalize(path)?;
    if root.to_str().is_none() {
        return Err(Error::Invalid(
            "Rule path cannot be represented exactly.".into(),
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
            return Err(Error::Invalid("Invalid cleanup scope.".into()));
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

fn eligible(entry: &crate::Entry, now: u64, kind: RuleKind) -> bool {
    entry.kind == Kind::File
        && !entry.incomplete
        && (matches!(kind, RuleKind::OldFiles)
            || entry.name.rsplit_once('.').is_some_and(|(_, extension)| {
                extension.eq_ignore_ascii_case("tmp") || extension.eq_ignore_ascii_case("temp")
            }))
        && now
            .checked_sub(match kind {
                RuleKind::UserTemp | RuleKind::ProjectTemp => MINIMUM_AGE,
                RuleKind::OldFiles => OLD_FILE_AGE,
            })
            .is_some_and(|cutoff| entry.modified.is_some_and(|modified| modified <= cutoff))
}
