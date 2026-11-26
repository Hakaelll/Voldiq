use crate::{
    Entry, Error, Kind, NodeId, Result,
    cloud::CloudState,
    index::Index,
    metadata::{Eligibility, Protections},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_IDENTITIES: usize = 250_000;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CandidateRequest {
    pub scope: Option<NodeId>,
    pub min_bytes: Option<String>,
    pub logical_bytes: Option<String>,
    pub offset: usize,
    pub limit: Option<usize>,
}

impl CandidateRequest {
    pub fn valid(&self) -> bool {
        self.offset <= MAX_IDENTITIES
            && self.limit.is_none_or(|limit| (1..=100).contains(&limit))
            && self
                .min_bytes
                .as_deref()
                .is_none_or(|value| bytes(value).is_some())
            && self
                .logical_bytes
                .as_deref()
                .is_none_or(|value| bytes(value).is_some())
    }
}

pub(super) fn bytes(text: &str) -> Option<u64> {
    if text.len() > 20 {
        return None;
    }
    let value = text.parse::<u64>().ok()?;
    (value.to_string() == text).then_some(value)
}

#[derive(Serialize)]
pub struct CandidatePage {
    pub scope: NodeId,
    pub evidence: &'static str,
    pub recorded_complete: bool,
    pub files_without_evidence: usize,
    pub unverified_branches: usize,
    pub next_offset: Option<usize>,
    #[serde(flatten)]
    pub rows: CandidateRows,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CandidateRows {
    Groups {
        total: usize,
        groups: Vec<CandidateGroup>,
    },
    Members {
        logical_bytes: String,
        total: usize,
        members: Vec<CandidateMember>,
    },
}

#[derive(Serialize)]
pub struct CandidateGroup {
    pub logical_bytes: String,
    pub distinct_files: usize,
}

#[derive(Serialize)]
pub struct CandidateMember {
    pub node_id: NodeId,
    pub name: String,
    pub modified: Option<u64>,
}

pub(crate) fn candidate_page(
    index: &Index,
    request: &CandidateRequest,
    page_size: usize,
    protections: &Protections,
    cancel: &AtomicBool,
) -> Result<Option<CandidatePage>> {
    if !request.valid() {
        return Err(Error::Invalid("Invalid duplicate candidate page.".into()));
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let scope = request.scope.unwrap_or(NodeId(0));
    let folder = index.entry(scope)?;
    if folder.kind != Kind::Directory {
        return Err(Error::Invalid("Choose an indexed folder.".into()));
    }
    let mask = match protections.for_index(index, cancel) {
        Ok(mask) => mask,
        Err(_) if cancel.load(Ordering::Relaxed) => return Ok(None),
        Err(error) => return Err(error),
    };
    let minimum = request.min_bytes.as_deref().and_then(bytes).unwrap_or(1);
    let selected_size = request.logical_bytes.as_deref().and_then(bytes);
    let mut seen = HashSet::new();
    let mut groups = BTreeMap::<u64, Vec<&Entry>>::new();
    let mut files_without_evidence = 0;
    for entry in &index.entries {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if !entry.is_metadata_file()
            || entry.logical < minimum
            || !index.in_scope(entry.id, scope)
            || mask.eligibility(entry.id)? == Eligibility::Protected
        {
            continue;
        }
        let matching = selected_size.is_none_or(|size| size == entry.logical);
        let Some(state) = entry.file_state.filter(|_| {
            !entry.incomplete
                && entry.kind == Kind::File
                && entry.cloud == CloudState::NotPlaceholder
                && mask.allows(entry.id)
        }) else {
            files_without_evidence += usize::from(matching);
            continue;
        };
        if seen.insert(state.identity) {
            if seen.len() > MAX_IDENTITIES {
                return Err(Error::Invalid(
                    "Too many candidate files. Choose a narrower folder or raise the minimum size."
                        .into(),
                ));
            }
            if matching {
                groups.entry(entry.logical).or_default().push(entry);
            }
        }
    }
    groups.retain(|_, members| members.len() > 1);
    let limit = request.limit.unwrap_or(25).min(page_size);
    let (total, rows) = if let Some(size) = selected_size {
        let entries = groups.remove(&size).unwrap_or_default();
        let total = entries.len();
        let members = entries
            .into_iter()
            .skip(request.offset)
            .take(limit)
            .map(|entry| CandidateMember {
                node_id: entry.id,
                name: entry.name.clone(),
                modified: entry.modified,
            })
            .collect();
        (
            total,
            CandidateRows::Members {
                logical_bytes: size.to_string(),
                total,
                members,
            },
        )
    } else {
        let total = groups.len();
        let groups = groups
            .into_iter()
            .rev()
            .skip(request.offset)
            .take(limit)
            .map(|(size, members)| CandidateGroup {
                logical_bytes: size.to_string(),
                distinct_files: members.len(),
            })
            .collect();
        (total, CandidateRows::Groups { total, groups })
    };
    let unverified_branches = mask.unverified_branches(scope)?;
    let next = request.offset + limit;
    Ok((!cancel.load(Ordering::Relaxed)).then_some(CandidatePage {
        scope,
        evidence: "recorded_size_only",
        recorded_complete: index
            .observation
            .as_ref()
            .is_some_and(|measurement| measurement.complete)
            && !folder.incomplete,
        files_without_evidence,
        unverified_branches,
        next_offset: (next < total).then_some(next),
        rows,
    }))
}
