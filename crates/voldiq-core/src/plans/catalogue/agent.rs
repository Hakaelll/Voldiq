use super::{
    FILE_COLUMNS, RecordedManifest, Store, TransferFile, TransferKind, TransferPhase, hex, invalid,
    read_file, recorded_manifest, validate_file,
};
use crate::{
    NodeId, Result,
    agents::{Capability, PlanContext, denied},
    plans::{Action, State},
};
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_PAGE: usize = 25;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTransfers {
    pub before: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

impl AgentTransfers {
    pub fn valid(&self) -> bool {
        (1..=MAX_PAGE).contains(&self.limit) && self.before.as_deref().is_none_or(|id| hex(id, 64))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTransferFiles {
    pub plan_id: String,
    pub after: Option<NodeId>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub include_hashes: bool,
}

impl AgentTransferFiles {
    pub fn valid(&self) -> bool {
        (1..=MAX_PAGE).contains(&self.limit) && hex(&self.plan_id, 64)
    }
}

fn default_limit() -> usize {
    MAX_PAGE
}

#[derive(Serialize)]
pub struct AgentTransferPage {
    pub items: Vec<AgentTransfer>,
    pub next_before: Option<String>,
}

#[derive(Serialize)]
pub struct AgentTransfer {
    pub plan_id: String,
    pub action: TransferKind,
    pub state: State,
    pub created: i64,
    pub destination_observed_at: u64,
    pub files: usize,
    pub published_files: usize,
    pub published_bytes: String,
}

#[derive(Serialize)]
pub struct AgentTransferFilePage {
    pub plan_id: String,
    pub manifest_hash: String,
    pub state: State,
    pub destination_observed_at: u64,
    pub currently_connected: Option<bool>,
    pub hashes_included: bool,
    pub files: Vec<AgentTransferFile>,
    pub next_after: Option<NodeId>,
}

#[derive(Serialize)]
pub struct AgentTransferFile {
    pub node: NodeId,
    pub output_name: String,
    pub destination_preview: String,
    pub phase: TransferPhase,
    pub content_blake3: Option<String>,
    pub verified_bytes: Option<String>,
    pub verified_at: Option<i64>,
    pub published_at: Option<i64>,
    pub interrupted_at: Option<i64>,
}

impl Store {
    /// An unchanged owner/credential/policy binding authorizes these recorded observations.
    /// Reading them does not open the source, output or an offline destination.
    pub(crate) fn agent_transfers(
        &self,
        context: &PlanContext<'_>,
        request: AgentTransfers,
        cancel: &AtomicBool,
    ) -> Result<Option<AgentTransferPage>> {
        if !request.valid() {
            return Err(invalid());
        }
        context.check_directory(self.directory.identity())?;
        let (policy, binding) = context.bound_policy(Capability::Metadata)?;
        let limit = request.limit.min(policy.limits().page_size);
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let tx = self.connection.unchecked_transaction()?;
        let before = request
            .before
            .as_deref()
            .map(|id| owned_transfer(&tx, context.owner(), &binding, id))
            .transpose()?;
        let ids = {
            let mut rows = tx.prepare("SELECT p.id FROM agent_plans p JOIN transfer_operations o ON o.plan_id=p.id AND o.owner=p.owner AND o.hash=p.hash WHERE p.owner=?1 AND p.binding=?2 AND length(p.id)=64 AND (?3 IS NULL OR p.rowid<?3) ORDER BY p.rowid DESC LIMIT ?4")?;
            rows.query_map(
                params![context.owner(), binding, before, (limit + 1) as i64],
                |row| row.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut items = Vec::with_capacity(limit);
        for id in ids.iter().take(limit) {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let recorded = recorded_manifest(&tx, id)?;
            let mut published = (0_usize, 0_u128);
            let mut count = 0;
            let mut files = tx.prepare(&format!(
                "SELECT {FILE_COLUMNS} FROM transfer_files WHERE plan_id=?1 LIMIT 501"
            ))?;
            for file in files.query_map([id], read_file)? {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                let file = file?;
                validate_file(&file, &recorded.manifest)?;
                count += 1;
                if file.published_at.is_some() {
                    published.0 += 1;
                    let bytes = file
                        .verified_bytes
                        .as_deref()
                        .ok_or_else(invalid)?
                        .parse::<u64>()
                        .map_err(|_| invalid())?;
                    published.1 += u128::from(bytes);
                }
            }
            if count != recorded.manifest.entries.len() {
                return Err(invalid());
            }
            items.push(AgentTransfer {
                plan_id: id.clone(),
                action: match recorded.manifest.action {
                    Action::Offload(_) => TransferKind::Offload,
                    Action::Restore(_) => TransferKind::Restore,
                    Action::Recycle => return Err(invalid()),
                },
                state: recorded.state,
                created: recorded.manifest.created,
                destination_observed_at: recorded.observed,
                files: count,
                published_files: published.0,
                published_bytes: published.1.to_string(),
            });
        }
        tx.commit()?;
        if context.binding()? != binding {
            return Err(denied());
        }
        let next_before = ids
            .get(limit)
            .and_then(|_| items.last().map(|item| item.plan_id.clone()));
        Ok((!cancel.load(Ordering::Relaxed)).then_some(AgentTransferPage { items, next_before }))
    }

    pub(crate) fn agent_transfer_files(
        &self,
        context: &PlanContext<'_>,
        request: AgentTransferFiles,
        cancel: &AtomicBool,
    ) -> Result<Option<AgentTransferFilePage>> {
        if !request.valid() {
            return Err(invalid());
        }
        context.check_directory(self.directory.identity())?;
        let (policy, binding) = context.bound_policy(Capability::Metadata)?;
        if request.include_hashes && !policy.capabilities().contains(&Capability::ReadContent) {
            return Err(denied());
        }
        let limit = request.limit.min(policy.limits().page_size);
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let tx = self.connection.unchecked_transaction()?;
        owned_transfer(&tx, context.owner(), &binding, &request.plan_id)?;
        let RecordedManifest {
            manifest,
            hash,
            state,
            observed,
        } = recorded_manifest(&tx, &request.plan_id)?;
        super::check_recorded_file_count(&tx, &manifest)?;
        let mut files = Vec::with_capacity(limit);
        let next_after = {
            let mut statement = tx.prepare(&format!("SELECT {FILE_COLUMNS} FROM transfer_files WHERE plan_id=?1 AND (?2 IS NULL OR node>?2) ORDER BY node LIMIT ?3"))?;
            let rows = statement.query_map(
                params![
                    request.plan_id,
                    request.after.map(|node| node.0),
                    (limit + 1) as i64
                ],
                read_file,
            )?;
            let mut next = None;
            for file in rows {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                let file = file?;
                validate_file(&file, &manifest)?;
                if files.len() == limit {
                    next = files.last().map(|file: &AgentTransferFile| file.node);
                    break;
                }
                let target = &manifest.action.transfer_group(file.node)?.destination.path;
                let destination_preview =
                    super::listing::preview(target.to_str().ok_or_else(invalid)?);
                files.push(public_file(
                    file,
                    destination_preview,
                    request.include_hashes,
                ));
            }
            next
        };
        tx.commit()?;
        if context.binding()? != binding {
            return Err(denied());
        }
        Ok(
            (!cancel.load(Ordering::Relaxed)).then_some(AgentTransferFilePage {
                plan_id: request.plan_id,
                manifest_hash: hash,
                state,
                destination_observed_at: observed,
                currently_connected: None,
                hashes_included: request.include_hashes,
                files,
                next_after,
            }),
        )
    }
}

fn owned_transfer(tx: &Transaction<'_>, owner: i64, binding: &str, id: &str) -> Result<i64> {
    Ok(tx.query_row("SELECT p.rowid FROM agent_plans p JOIN transfer_operations o ON o.plan_id=p.id AND o.owner=p.owner AND o.hash=p.hash WHERE p.id=?1 AND p.owner=?2 AND p.binding=?3", params![id, owner, binding], |row| row.get(0))?)
}

fn public_file(
    file: TransferFile,
    destination_preview: String,
    include_hashes: bool,
) -> AgentTransferFile {
    AgentTransferFile {
        node: file.node,
        output_name: file.output_name,
        destination_preview,
        phase: file.phase,
        content_blake3: if include_hashes {
            file.content_blake3
        } else {
            None
        },
        verified_bytes: file.verified_bytes,
        verified_at: file.verified_at,
        published_at: file.published_at,
        interrupted_at: file.interrupted_at,
    }
}
