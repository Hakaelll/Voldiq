//! An index-bound selection is a request for review, never execution authority.
use crate::{Error, Kind, NodeId, Result, index::Index};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};
mod prepare;
pub use prepare::{Environment, Prepared, prepare};

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Manual,
    Duplicates,
    UserTemp,
    OldFiles,
    ProjectTemp,
    BrowserCache {
        application: Option<crate::applications::review::Context>,
    },
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub source: Source,
    pub selection: Vec<NodeId>,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    #[default]
    Logical,
    Allocated,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub metric: Metric,
    pub expression: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetProgress {
    Measured {
        metric: Metric,
        bytes: String,
        selected_bytes: String,
        complete: bool,
        reached: bool,
    },
    Invalid {
        message: &'static str,
    },
}

#[derive(Clone, Serialize)]
pub struct Origin {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub risk: &'static str,
}

impl From<super::rules::Rule> for Origin {
    fn from(rule: super::rules::Rule) -> Self {
        Self {
            id: rule.id,
            label: rule.name,
            description: rule.description,
            risk: rule.risk,
        }
    }
}

impl Source {
    pub fn origin(&self) -> Origin {
        use super::rules::{BROWSER_CACHE, OLD_FILES, PROJECT_TEMP, USER_TEMP};
        match self {
            Self::Manual => Origin {
                id: "manual-files",
                label: "Selected files",
                description: "Explicit selection of regular files.",
                risk: "review",
            },
            Self::Duplicates => Origin {
                id: "duplicate-files",
                label: "Exact duplicate copies",
                description: "Preparation rechecks content and verifies a retained copy.",
                risk: "review",
            },
            Self::UserTemp => USER_TEMP.into(),
            Self::OldFiles => OLD_FILES.into(),
            Self::ProjectTemp => PROJECT_TEMP.into(),
            Self::BrowserCache {
                application: Some(_),
            } => Origin {
                label: "Application HTTP cache",
                ..BROWSER_CACHE.into()
            },
            Self::BrowserCache { application: None } => BROWSER_CACHE.into(),
        }
    }
}

#[derive(Serialize)]
pub struct Item {
    pub id: NodeId,
    pub parent: NodeId,
    pub name: String,
    pub folder: String,
    pub logical: String,
    pub allocated: Option<String>,
    pub hard_link: bool,
}

#[derive(Serialize)]
pub struct GroupSummary {
    pub origin: Origin,
    pub items: Vec<Item>,
}

#[derive(Serialize)]
pub struct Summary {
    pub root: String,
    pub indexed_at: Option<u64>,
    pub incomplete: bool,
    pub root_logical: String,
    pub selected_logical: String,
    pub remaining_logical: Option<String>,
    pub known_allocated: String,
    pub allocation_complete: bool,
    pub hard_link_paths: usize,
    pub repeated_identity_paths: usize,
    pub groups: Vec<GroupSummary>,
    pub target: Option<TargetProgress>,
}

pub(super) fn validate(index: &Index, groups: &[Group], cancel: &AtomicBool) -> Result<()> {
    if index.snapshot_id.is_some() {
        return Err(Error::Invalid(
            "Use a current analysis for cleanup review.".into(),
        ));
    }
    if groups.is_empty() || groups.len() > 16 {
        return Err(Error::Invalid(
            "Review between 1 and 16 source groups.".into(),
        ));
    }
    let mut seen = HashSet::new();
    for group in groups {
        super::check_cancelled(cancel)?;
        if group.selection.is_empty() || group.selection.len() > 500 {
            return Err(Error::Invalid(
                "A review group requires between 1 and 500 files.".into(),
            ));
        }
        if let Source::BrowserCache {
            application: Some(context),
        } = &group.source
            && (index.entry(context.scope)?.kind != Kind::Directory
                || context.application_id.len() != 64
                || !context
                    .application_id
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(Error::Invalid("Invalid application review context.".into()));
        }
        for &id in &group.selection {
            let entry = index.entry(id)?;
            if id == NodeId(0) || entry.kind != Kind::File || entry.incomplete {
                return Err(Error::Invalid(
                    "Add complete regular files to the review tray.".into(),
                ));
            }
            if !seen.insert(id) {
                return Err(Error::Invalid(
                    "A file is already present in another review group.".into(),
                ));
            }
            if seen.len() > 500 {
                return Err(Error::Invalid("Review up to 500 files in total.".into()));
            }
        }
    }
    Ok(())
}

pub fn summarize(
    index: &Index,
    groups: &[Group],
    target: Option<&Target>,
    cancel: &AtomicBool,
) -> Result<Summary> {
    validate(index, groups, cancel)?;
    let target = target.map(|target| {
        crate::search::storage_limit(&target.expression)
            .map(|bytes| (target.metric, u128::from(bytes)))
    });
    let root = index.entry(NodeId(0))?;
    let mut selected = 0u128;
    let volume = index
        .observation
        .as_ref()
        .filter(|observation| observation.accounting_version == crate::index::ACCOUNTING_VERSION)
        .and(index.root_identity)
        .map(|(volume, _)| volume);
    let mut identities: HashMap<(u64, u64), Option<u128>> = HashMap::new();
    let mut linked = HashSet::new();
    let mut missing_identity = false;
    let mut hard_link_paths = 0;
    let mut repeated_identity_paths = 0;
    let mut summaries = Vec::new();
    for group in groups {
        let mut items = Vec::new();
        for &id in &group.selection {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Invalid("Cleanup review cancelled.".into()));
            }
            let entry = index.entry(id)?;
            if let Some(state) = entry.file_state {
                let accounted = volume == Some(state.identity.0);
                let allocation =
                    (accounted && entry.allocation_known).then_some(u128::from(entry.allocated));
                let repeated = identities.insert(state.identity, allocation).is_some();
                repeated_identity_paths += usize::from(repeated);
                if accounted && (repeated || entry.hard_link) {
                    linked.insert(state.identity);
                }
            } else {
                missing_identity = true;
            }
            selected += u128::from(entry.logical);
            hard_link_paths += usize::from(entry.hard_link);
            let parent = entry
                .parent
                .ok_or_else(|| Error::Invalid("Invalid selected-file parent.".into()))?;
            items.push(Item {
                id,
                parent,
                name: entry.name.clone(),
                folder: index
                    .path_cancellable(parent, || cancel.load(Ordering::Relaxed))?
                    .ok_or_else(|| Error::Invalid("Cleanup review cancelled.".into()))?
                    .to_string_lossy()
                    .into_owned(),
                logical: entry.logical.to_string(),
                allocated: None,
                hard_link: entry.hard_link,
            });
        }
        items.sort_by(|a, b| {
            a.folder
                .cmp(&b.folder)
                .then(a.name.cmp(&b.name))
                .then(a.id.0.cmp(&b.id.0))
        });
        summaries.push(GroupSummary {
            origin: group.source.origin(),
            items,
        });
    }
    if !linked.is_empty() {
        // The scanner charges allocation to one indexed path per identity, which can be outside this selection.
        for &identity in &linked {
            identities.insert(identity, Some(0));
        }
        for (position, entry) in index.entries.iter().enumerate() {
            if position % 256 == 0 {
                super::check_cancelled(cancel)?;
            }
            let Some(state) = entry.file_state else {
                continue;
            };
            if !linked.contains(&state.identity) {
                continue;
            }
            if let Some(allocation) = identities.get_mut(&state.identity) {
                if entry.kind != Kind::File || !entry.allocation_known {
                    *allocation = None;
                } else if let Some(total) = allocation {
                    *total += u128::from(entry.allocated);
                }
            }
        }
    }
    for item in summaries.iter_mut().flat_map(|group| &mut group.items) {
        item.allocated = index
            .entry(item.id)?
            .file_state
            .and_then(|state| identities.get(&state.identity).copied().flatten())
            .map(|bytes| bytes.to_string());
    }
    let allocated: u128 = identities.values().filter_map(|value| *value).sum();
    let allocation_complete = !missing_identity && identities.values().all(Option::is_some);
    super::check_cancelled(cancel)?;
    Ok(Summary {
        root: index.root.to_string_lossy().into_owned(),
        indexed_at: index.observation.as_ref().map(|value| value.completed_at),
        incomplete: root.incomplete
            || index.error_count > 0
            || index
                .observation
                .as_ref()
                .is_none_or(|value| !value.complete),
        root_logical: root.logical.to_string(),
        selected_logical: selected.to_string(),
        remaining_logical: u128::from(root.logical)
            .checked_sub(selected)
            .map(|value| value.to_string()),
        known_allocated: allocated.to_string(),
        allocation_complete,
        hard_link_paths,
        repeated_identity_paths,
        groups: summaries,
        target: target.map(|parsed| {
            let (metric, bytes) = match parsed {
                Ok(target) => target,
                Err(error) => {
                    return TargetProgress::Invalid {
                        message: error.message,
                    };
                }
            };
            let (value, complete) = match metric {
                Metric::Logical => (selected, true),
                Metric::Allocated => (allocated, allocation_complete),
            };
            TargetProgress::Measured {
                metric,
                bytes: bytes.to_string(),
                selected_bytes: value.to_string(),
                complete,
                reached: value >= bytes,
            }
        }),
    })
}
