use super::*;
use crate::metadata::Protections;
use std::path::PathBuf;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecordedRequest {
    pub scope: Option<NodeId>,
    pub offset: usize,
    pub limit: Option<usize>,
    pub weights: score::Weights,
}

impl RecordedRequest {
    pub fn valid(&self) -> bool {
        self.offset <= u32::MAX as usize
            && self.limit.is_none_or(|limit| (1..=100).contains(&limit))
            && score::Model::new(self.weights).is_ok()
    }
}

#[derive(Serialize)]
pub struct RecordedPage {
    snapshot_id: i64,
    recorded_at: Option<u64>,
    scope: NodeId,
    rule: RuleKind,
    rule_id: &'static str,
    rule_version: u32,
    description: &'static str,
    risk: &'static str,
    regeneration: Option<&'static str>,
    evidence: &'static str,
    limitations: &'static str,
    #[serde(flatten)]
    status: Status,
}

#[derive(Serialize)]
#[serde(tag = "availability", rename_all = "snake_case")]
enum Status {
    ObservationTimeUnavailable,
    RuleLocationUnavailable,
    RuleLocationNotIndexed,
    Recorded {
        rule_scope: NodeId,
        recorded_complete: bool,
        unverified_branches: usize,
        unverified_files: usize,
        total: usize,
        logical_bytes: String,
        next_offset: Option<usize>,
        items: Vec<Item>,
    },
}

#[derive(Serialize)]
struct Item {
    node_id: NodeId,
    name: String,
    modified: Option<u64>,
    logical_bytes: String,
    recorded_allocation_charge_bytes: Option<String>,
    hard_link_alias: bool,
    priority: score::Assessment,
}

pub(crate) fn query_snapshot(
    index: &Index,
    rule: RuleKind,
    request: &RecordedRequest,
    page_size: usize,
    protections: &Protections,
    cancel: &AtomicBool,
) -> Result<RecordedPage> {
    let snapshot_id = index
        .snapshot_id
        .filter(|_| request.valid())
        .ok_or_else(|| {
            Error::Invalid("Use a saved analysis and bounded cleanup candidate options.".into())
        })?;
    let scope = request.scope.unwrap_or(NodeId(0));
    let _folder = index.pin_folder(scope)?;
    let recipe = rule.metadata();
    let mut page = RecordedPage {
        snapshot_id,
        recorded_at: index
            .observation
            .as_ref()
            .map(|observation| observation.completed_at),
        scope,
        rule,
        rule_id: recipe.id,
        rule_version: 1,
        description: recipe.description,
        risk: recipe.risk,
        regeneration: matches!(rule, RuleKind::BrowserCache)
            .then_some("The browser can download cached HTTP content again when it is available."),
        evidence: "recorded_metadata",
        limitations: "Age and allocation come from the saved scan. Current file use, verified recoverable bytes and a recovery copy are unavailable. Cleanup requires live revalidation and separate human approval.",
        status: Status::ObservationTimeUnavailable,
    };
    let Some(observation) = &index.observation else {
        return Ok(page);
    };
    super::super::check_cancelled(cancel)?;
    let temporary;
    let local_data;
    let (location, source) = match rule {
        RuleKind::UserTemp => {
            temporary = std::env::temp_dir();
            (temporary.as_path(), RuleSource::UserTemp)
        }
        RuleKind::OldFiles => (index.root.as_path(), RuleSource::OldFiles),
        RuleKind::ProjectTemp => (index.root.as_path(), RuleSource::ProjectTemp(&[])),
        RuleKind::BrowserCache => {
            let Some(location) = browser_location() else {
                page.status = Status::RuleLocationUnavailable;
                return Ok(page);
            };
            local_data = location;
            (
                local_data.as_path(),
                RuleSource::BrowserCache(local_data.as_path(), None),
            )
        }
    };
    let pinned = match crate::agents::pin_directory(location) {
        Ok(pinned) => pinned,
        Err(_) if matches!(rule, RuleKind::UserTemp | RuleKind::BrowserCache) => {
            page.status = Status::RuleLocationUnavailable;
            return Ok(page);
        }
        Err(error) => return Err(error),
    };
    page.status = Status::RuleLocationNotIndexed;
    let Some(binding) = DirectoryScope::bind_pinned(index, pinned)? else {
        return Ok(page);
    };
    let rule_scope = if index.in_scope(scope, binding.id) {
        scope
    } else if index.in_scope(binding.id, scope) {
        binding.id
    } else {
        return Ok(page);
    };
    let _rule_folder = index.pin_folder(rule_scope)?;
    let ranked = rank_candidates(
        index,
        rule_scope,
        Query {
            offset: request.offset,
            limit: request.limit.unwrap_or(page_size).min(page_size),
            weights: request.weights,
        },
        source,
        &Environment {
            temporary: location,
            local_data: location,
            caches: &[],
            now: observation.completed_at,
            protections,
            duplicates: None,
        },
        cancel,
    )?;
    let returned = request.offset + ranked.items.len();
    page.status = Status::Recorded {
        rule_scope,
        recorded_complete: observation.complete
            && !index.entry(rule_scope)?.incomplete
            && ranked.unverified_branches == 0
            && ranked.unverified_files == 0,
        unverified_branches: ranked.unverified_branches,
        unverified_files: ranked.unverified_files,
        total: ranked.total,
        logical_bytes: ranked.logical.to_string(),
        next_offset: (returned < ranked.total).then_some(returned),
        items: ranked
            .items
            .into_iter()
            .map(|(entry, priority)| Item {
                node_id: entry.id,
                name: entry.name.clone(),
                modified: entry.modified,
                logical_bytes: entry.logical.to_string(),
                recorded_allocation_charge_bytes: recorded_charge(entry)
                    .map(|bytes| bytes.to_string()),
                hard_link_alias: entry.hard_link,
                priority,
            })
            .collect(),
    };
    Ok(page)
}

fn browser_location() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows::local_app_data().ok()
    }
    #[cfg(not(windows))]
    {
        None
    }
}
