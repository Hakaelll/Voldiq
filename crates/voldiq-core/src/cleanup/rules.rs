use super::{Plan, PreviewItem, score};
use crate::{Error, Kind, NodeId, Result, index::Index, windows};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MINIMUM_AGE: u64 = 7 * 24 * 60 * 60;
const OLD_FILE_AGE: u64 = 90 * 24 * 60 * 60;
mod scope;
use scope::DirectoryScope;

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    UserTemp,
    OldFiles,
    ProjectTemp,
    BrowserCache,
}

#[derive(Clone, Copy)]
enum RuleSource<'a> {
    UserTemp,
    OldFiles,
    ProjectTemp(&'a [crate::developer::CacheLocation]),
    BrowserCache(&'a Path, Option<NodeId>),
}

impl RuleSource<'_> {
    fn kind(self) -> RuleKind {
        match self {
            Self::UserTemp => RuleKind::UserTemp,
            Self::OldFiles => RuleKind::OldFiles,
            Self::ProjectTemp(_) => RuleKind::ProjectTemp,
            Self::BrowserCache(..) => RuleKind::BrowserCache,
        }
    }
}

impl RuleKind {
    fn metadata(self) -> Rule {
        match self {
            Self::UserTemp => USER_TEMP,
            Self::OldFiles => OLD_FILES,
            Self::ProjectTemp => PROJECT_TEMP,
            Self::BrowserCache => BROWSER_CACHE,
        }
    }
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

pub const BROWSER_CACHE: Rule = Rule {
    id: "old-browser-http-cache-files",
    name: "Browser HTTP cache",
    description: "External Blockfile cache files unchanged for seven days. Close the browser before reviewing; cached pages may need downloading again.",
    discovery: "Default Chrome and Edge profiles under the current user's Windows Local AppData, within the live index.",
    selection: "Explicit selection of complete regular f_ hexadecimal payload files directly inside Profile/Cache/Cache_Data.",
    exclusions: "Cookies, sessions, credentials, preferences, service workers, shared data_ blocks, indexes, Simple Cache streams, nested paths, custom profiles, incomplete files, reparses, recalled content and historical snapshots.",
    risk: "review",
    permissions: "Current user's metadata access; delete access and rejection of writable open files through verified handles.",
    policy: "recycle",
    verification: "Known-folder root, exact browser/profile/cache hierarchy, file shape and age, then scanned identity, current metadata and ancestor verification.",
};

#[derive(Serialize)]
pub struct Page {
    pub rule: Rule,
    pub root: String,
    pub indexed: bool,
    pub complete: bool,
    pub unverified_branches: usize,
    pub items: Vec<Candidate>,
    pub assessed_at: u64,
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

#[derive(Serialize)]
pub struct Candidate {
    #[serde(flatten)]
    pub file: PreviewItem,
    pub priority: score::Assessment,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub offset: usize,
    pub limit: usize,
    #[serde(default)]
    pub weights: score::Weights,
}

pub struct Environment<'a> {
    pub temporary: &'a Path,
    pub local_data: &'a Path,
    pub caches: &'a [crate::developer::CacheLocation],
    pub now: u64,
    pub protections: &'a crate::metadata::Protections,
    pub duplicates: Option<&'a crate::duplicates::Analysis>,
}

pub fn query(
    index: Option<&Index>,
    rule: RuleKind,
    request: Query,
    env: &Environment<'_>,
    cancel: &AtomicBool,
) -> Result<Page> {
    let (root, source) = match rule {
        RuleKind::UserTemp => (Some(env.temporary), RuleSource::UserTemp),
        RuleKind::OldFiles => (
            index.map(|index| index.root.as_path()),
            RuleSource::OldFiles,
        ),
        RuleKind::ProjectTemp => (
            index.map(|index| index.root.as_path()),
            RuleSource::ProjectTemp(env.caches),
        ),
        RuleKind::BrowserCache => (
            Some(env.local_data),
            RuleSource::BrowserCache(env.local_data, None),
        ),
    };
    query_scoped(index, root, request, source, env, cancel)
}

pub fn query_browser_cache_scoped(
    index: &Index,
    scope: NodeId,
    request: Query,
    env: &Environment<'_>,
    cancel: &AtomicBool,
) -> Result<Page> {
    query_scoped(
        Some(index),
        Some(env.local_data),
        request,
        RuleSource::BrowserCache(env.local_data, Some(scope)),
        env,
        cancel,
    )
}

struct Ranked<'a> {
    entry: &'a crate::Entry,
    points: u16,
    exact: bool,
    local_copy: bool,
}

fn query_scoped(
    index: Option<&Index>,
    temporary: Option<&Path>,
    request: Query,
    source: RuleSource<'_>,
    env: &Environment<'_>,
    cancel: &AtomicBool,
) -> Result<Page> {
    let kind = source.kind();
    let now = env.now;
    let model = score::Model::new(request.weights)?;
    let temporary = temporary.map(verified_root).transpose()?;
    let mut page = Page {
        rule: kind.metadata(),
        root: temporary
            .as_deref()
            .map_or_else(String::new, |path| path.to_string_lossy().into_owned()),
        assessed_at: now,
        indexed: false,
        complete: false,
        unverified_branches: 0,
        items: Vec::new(),
        total: 0,
        logical: 0,
        reclaimable: 0,
    };
    let (Some(index), Some(temporary)) = (index, temporary) else {
        return Ok(page);
    };
    if index.snapshot_id.is_some() {
        return Ok(page);
    }
    let Some(binding) = DirectoryScope::bind(index, &temporary)? else {
        return Ok(page);
    };
    let mut scope_id = binding.id;
    if let RuleSource::BrowserCache(_, Some(requested)) = source {
        if index.entry(requested)?.kind != Kind::Directory || !index.in_scope(requested, scope_id) {
            return Err(Error::Invalid(
                "Application cache scope is outside the current browser rule.".into(),
            ));
        }
        scope_id = requested;
        page.root = index.path(scope_id)?.to_string_lossy().into_owned();
    }
    page.indexed = true;
    page.complete = !index.entry(NodeId(0))?.incomplete
        && !index.entry(scope_id)?.incomplete
        && (binding.whole || matches!(source, RuleSource::BrowserCache(_, Some(_))));
    let mask = env.protections.for_index(index, cancel)?;
    page.unverified_branches = mask.unverified_branches(scope_id)?;
    page.complete &= page.unverified_branches == 0;
    let mut protected_scopes = Vec::new();
    #[cfg(windows)]
    if matches!(
        kind,
        RuleKind::OldFiles | RuleKind::ProjectTemp | RuleKind::BrowserCache
    ) {
        for path in windows::protected_directories()? {
            let path = verified_root(&path)?;
            if let Some(id) = scope(index, &path)? {
                protected_scopes.push(id);
            }
        }
    }
    let builds = if let RuleSource::ProjectTemp(caches) = source {
        crate::developer::project_build_scopes(index, caches)?
    } else {
        Default::default()
    };
    let browser_scopes = if let RuleSource::BrowserCache(local_data, _) = source {
        browser_cache_scopes(index, local_data, cancel)?
    } else {
        HashSet::new()
    };
    let mut candidates: Vec<_> = index
        .entries
        .iter()
        .take_while(|_| !cancel.load(Ordering::Relaxed))
        .filter(|entry| {
            eligible(entry, now, kind)
                && mask.allows(entry.id)
                && index.in_scope(entry.id, scope_id)
                && (!matches!(kind, RuleKind::ProjectTemp) || builds.contains(index, entry.id))
                && (!matches!(kind, RuleKind::BrowserCache)
                    || entry.parent.is_some_and(|id| browser_scopes.contains(&id)))
                && !protected_scopes
                    .iter()
                    .any(|&protected| index.in_scope(entry.id, protected))
        })
        .map(|entry| Ranked {
            entry,
            points: 0,
            exact: false,
            local_copy: false,
        })
        .collect();
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Invalid("Application cache query cancelled.".into()));
    }
    if let Some(duplicates) = env
        .duplicates
        .filter(|run| run.check_protections(env.protections).is_ok())
    {
        for group in &duplicates.groups {
            super::check_cancelled(cancel)?;
            let mut identity = None;
            let mut distinct = false;
            let mut independent = [None, None];
            for member in &group.members {
                super::check_cancelled(cancel)?;
                let entry = index.entry(member.id)?;
                if !mask.allows(member.id) || !index.in_scope(member.id, scope_id) {
                    continue;
                }
                let Some(state) = entry.file_state else {
                    continue;
                };
                if let Some(first) = identity {
                    distinct |= first != state.identity;
                } else {
                    identity = Some(state.identity);
                }
                if member.links == 1 && !independent.contains(&Some(state.identity)) {
                    if independent[0].is_none() {
                        independent[0] = Some(state.identity);
                    } else if independent[1].is_none() {
                        independent[1] = Some(state.identity);
                    }
                }
            }
            if !distinct {
                continue;
            }
            for member in &group.members {
                super::check_cancelled(cancel)?;
                if let Ok(at) =
                    candidates.binary_search_by_key(&member.id.0, |item| item.entry.id.0)
                {
                    let item = &mut candidates[at];
                    if let Some(state) = item.entry.file_state {
                        item.exact = true;
                        item.local_copy = independent
                            .iter()
                            .flatten()
                            .any(|peer| *peer != state.identity);
                    }
                }
            }
        }
    }
    let browser = matches!(kind, RuleKind::BrowserCache);
    let facts = |item: &Ranked<'_>| {
        score::Facts::candidate(item.entry, now, browser, item.exact, item.local_copy)
    };
    page.total = candidates.len();
    for item in &mut candidates {
        super::check_cancelled(cancel)?;
        item.points = model.points(facts(item));
        let entry = item.entry;
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
    if request.offset >= candidates.len() {
        return Ok(page);
    }
    let compare = |a: &Ranked<'_>, b: &Ranked<'_>| {
        b.points
            .cmp(&a.points)
            .then(a.entry.id.0.cmp(&b.entry.id.0))
    };
    let mut cancelled = || cancel.load(Ordering::Relaxed);
    if request.offset > 0
        && !crate::index::rank::select(&mut candidates, request.offset, &compare, &mut cancelled)
    {
        super::check_cancelled(cancel)?;
    }
    let rest = &mut candidates[request.offset..];
    let limit = request.limit.clamp(1, 100).min(rest.len());
    if limit < rest.len() && !crate::index::rank::select(rest, limit, &compare, &mut cancelled) {
        super::check_cancelled(cancel)?;
    }
    rest[..limit].sort_unstable_by(compare);
    for item in &rest[..limit] {
        super::check_cancelled(cancel)?;
        let entry = item.entry;
        page.items.push(Candidate {
            priority: model.explain(facts(item)),
            file: PreviewItem {
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
            },
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

pub fn preview_browser_cache(
    index: &Index,
    local_data: &Path,
    now: u64,
    selection: &[NodeId],
) -> Result<Plan> {
    let scopes = browser_cache_scopes(index, local_data, &AtomicBool::new(false))?;
    for &id in selection {
        let entry = index.entry(id)?;
        if !entry.parent.is_some_and(|parent| scopes.contains(&parent)) {
            return Err(Error::Invalid(
                "Selection is outside recognized browser HTTP caches.".into(),
            ));
        }
    }
    preview_scoped(index, local_data, now, selection, RuleKind::BrowserCache)
}

fn browser_cache_scopes(
    index: &Index,
    local_data: &Path,
    cancel: &AtomicBool,
) -> Result<HashSet<NodeId>> {
    let mut scopes = HashSet::new();
    let Some(binding) = DirectoryScope::bind(index, local_data)? else {
        return Ok(scopes);
    };
    for entry in index.entries.iter().filter(|entry| {
        entry.kind == Kind::Directory
            && !entry.incomplete
            && entry.name.eq_ignore_ascii_case("Cache_Data")
    }) {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Invalid("Application cache query cancelled.".into()));
        }
        if !index.in_scope(entry.id, binding.id) {
            continue;
        }
        let path = binding.path(index, entry.id)?;
        let Ok(relative) = path.strip_prefix(binding.root.path()) else {
            continue;
        };
        let parts: Vec<_> = relative
            .components()
            .map(|part| part.as_os_str().to_str())
            .collect();
        let [
            Some(vendor),
            Some(browser),
            Some(data),
            Some(profile),
            Some(cache),
            Some(contents),
        ] = parts.as_slice()
        else {
            continue;
        };
        let supported_browser = (vendor.eq_ignore_ascii_case("Google")
            && browser.eq_ignore_ascii_case("Chrome"))
            || (vendor.eq_ignore_ascii_case("Microsoft") && browser.eq_ignore_ascii_case("Edge"));
        let numbered_profile = profile.strip_prefix("Profile ").is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        });
        if supported_browser
            && data.eq_ignore_ascii_case("User Data")
            && (profile.eq_ignore_ascii_case("Default") || numbered_profile)
            && cache.eq_ignore_ascii_case("Cache")
            && contents.eq_ignore_ascii_case("Cache_Data")
        {
            scopes.insert(entry.id);
        }
    }
    Ok(scopes)
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
    plan.preview.rule = kind.metadata().id;
    Ok(plan)
}

fn verified_root(path: &Path) -> Result<std::path::PathBuf> {
    let root = crate::agents::pin_directory(path)?.path().to_owned();
    if root.to_str().is_none() {
        return Err(Error::Invalid(
            "Rule path cannot be represented exactly.".into(),
        ));
    }
    Ok(root)
}

fn scope(index: &Index, temporary: &Path) -> Result<Option<NodeId>> {
    Ok(DirectoryScope::bind(index, temporary)?.map(|scope| scope.id))
}

fn eligible(entry: &crate::Entry, now: u64, kind: RuleKind) -> bool {
    entry.kind == Kind::File
        && !entry.incomplete
        && match kind {
            RuleKind::OldFiles => true,
            RuleKind::BrowserCache => entry.name.strip_prefix("f_").is_some_and(|hash| {
                let Ok(number) = u32::from_str_radix(hash, 16) else {
                    return false;
                };
                (1..=0x0fff_ffff).contains(&number) && hash == format!("{number:06x}")
            }),
            _ => entry.name.rsplit_once('.').is_some_and(|(_, extension)| {
                extension.eq_ignore_ascii_case("tmp") || extension.eq_ignore_ascii_case("temp")
            }),
        }
        && now
            .checked_sub(match kind {
                RuleKind::UserTemp | RuleKind::ProjectTemp | RuleKind::BrowserCache => MINIMUM_AGE,
                RuleKind::OldFiles => OLD_FILE_AGE,
            })
            .is_some_and(|cutoff| entry.modified.is_some_and(|modified| modified <= cutoff))
}
