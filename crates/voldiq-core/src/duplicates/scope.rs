use super::{candidates::bytes, verification::VerifiedGroup};
use crate::{Error, NodeId, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct ScopeRequest {
    pub scope: Option<NodeId>,
    pub min_bytes: Option<String>,
}

impl ScopeRequest {
    pub fn valid(&self) -> bool {
        self.min_bytes
            .as_deref()
            .is_none_or(|value| bytes(value).is_some())
    }

    pub(crate) fn folder(&self) -> NodeId {
        self.scope.unwrap_or(NodeId(0))
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ResultRequest {
    pub group: Option<usize>,
    pub offset: usize,
    pub limit: Option<usize>,
}

impl ResultRequest {
    pub fn valid(&self) -> bool {
        self.offset <= 250_000
            && self.group.is_none_or(|group| group > 0 && group <= 125_000)
            && self.limit.is_none_or(|limit| (1..=100).contains(&limit))
    }
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Report {
    pub observed_at: i64,
    pub compared_files: usize,
    pub aliases_omitted: usize,
    pub unavailable_files: usize,
    pub unverified_branches: usize,
    pub recorded_complete: bool,
    pub bytes_read: String,
    pub groups: Vec<VerifiedGroup>,
}

impl Report {
    pub(crate) fn page(
        self,
        request: &ResultRequest,
        page_size: usize,
    ) -> Result<serde_json::Value> {
        use serde_json::json;
        if !request.valid() {
            return Err(Error::Invalid("Invalid duplicate analysis page.".into()));
        }
        let limit = request.limit.unwrap_or(25).min(page_size);
        let mut value = json!({
            "observed_at":self.observed_at,
            "compared_files":self.compared_files,
            "aliases_omitted":self.aliases_omitted,
            "unavailable_files":self.unavailable_files,
            "unverified_branches":self.unverified_branches,
            "recorded_complete":self.recorded_complete,
            "bytes_read":self.bytes_read
        });
        let total = if let Some(group) = request.group {
            let group = self
                .groups
                .get(group - 1)
                .ok_or_else(|| Error::Invalid("Unknown duplicate group.".into()))?;
            value["kind"] = json!("members");
            value["group"] = json!(request.group);
            value["logical_bytes"] = json!(group.logical_bytes);
            value["node_ids"] = json!(
                group
                    .node_ids
                    .iter()
                    .skip(request.offset)
                    .take(limit)
                    .collect::<Vec<_>>()
            );
            group.node_ids.len()
        } else {
            let total = self.groups.len();
            value["kind"] = json!("groups");
            value["groups"] = json!(
                self.groups
                    .into_iter()
                    .enumerate()
                    .skip(request.offset)
                    .take(limit)
                    .map(|(position, group)| json!({
                        "group":position+1, "logical_bytes":group.logical_bytes,
                        "distinct_files":group.node_ids.len()
                    }))
                    .collect::<Vec<_>>()
            );
            total
        };
        let next = request.offset + limit;
        value["total"] = json!(total);
        value["next_offset"] = json!((next < total).then_some(next));
        Ok(value)
    }
}

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub(crate) use native::{AnalysisProgress, analyze};
