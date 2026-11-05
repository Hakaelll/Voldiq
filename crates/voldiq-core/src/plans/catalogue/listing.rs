use super::{
    FILE_COLUMNS, HumanAuthority, State, Store, invalid, read_file, recorded_manifest,
    validate_file,
};
use crate::Result;
use rusqlite::params;
use serde::Serialize;

#[derive(Serialize)]
pub struct TransferPage {
    pub items: Vec<TransferSummary>,
    pub next_before: Option<String>,
}

#[derive(Serialize)]
pub struct TransferSummary {
    pub id: String,
    #[serde(flatten)]
    pub status: TransferSummaryStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferKind {
    Offload,
    Restore,
}

#[derive(Serialize)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub enum TransferSummaryStatus {
    Ready {
        action: TransferKind,
        state: State,
        created: i64,
        connection_id: i64,
        files: usize,
        published_files: usize,
        published_bytes: String,
        source_preview: String,
        destination_preview: String,
        destinations: usize,
    },
    Unavailable {
        reason: &'static str,
    },
}

impl Store {
    /// Human-owned descriptive history. A cursor or catalogue record grants no file authority.
    pub(in crate::plans) fn transfer_operations(
        &self,
        human: &HumanAuthority,
        before: Option<&str>,
        limit: usize,
    ) -> Result<TransferPage> {
        human.check_directory(self.directory.identity())?;
        if !(1..=25).contains(&limit) {
            return Err(invalid());
        }
        let before = before
            .map(|value| {
                if value.len() > 19 {
                    return Err(invalid());
                }
                let parsed = value.parse::<i64>().map_err(|_| invalid())?;
                if parsed <= 0 || parsed.to_string() != value {
                    return Err(invalid());
                }
                Ok(parsed)
            })
            .transpose()?;
        let tx = self.connection.unchecked_transaction()?;
        let mut statement = tx.prepare(
            "SELECT p.rowid,CASE WHEN length(p.id)=64 THEN p.id ELSE '' END FROM agent_plans p JOIN transfer_operations o ON o.plan_id=p.id
            WHERE (?1 IS NULL OR p.rowid<?1) ORDER BY p.rowid DESC LIMIT ?2",
        )?;
        let mut rows = statement
            .query_map(params![before, (limit + 1) as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let next_before = if rows.len() > limit {
            rows.truncate(limit);
            rows.last().map(|(row, _)| row.to_string())
        } else {
            None
        };
        let mut items = Vec::with_capacity(rows.len());
        for (row, id) in rows {
            let status = (|| -> Result<TransferSummaryStatus> {
                let recorded = recorded_manifest(&tx, &id)?;
                let manifest = &recorded.manifest;
                let groups = manifest.action.transfer_groups()?;
                let mut files = tx.prepare(&format!("SELECT {FILE_COLUMNS} FROM transfer_files WHERE plan_id=?1 ORDER BY node LIMIT 501"))?;
                let files = files.query_map([&id], read_file)?.collect::<std::result::Result<Vec<_>,_>>()?;
                if files.len() != manifest.entries.len() { return Err(invalid()); }
                let mut published_files = 0;
                let mut published_bytes = 0_u128;
                for file in files {
                    validate_file(&file, manifest)?;
                    if file.published_at.is_some() {
                        published_files += 1;
                        published_bytes = published_bytes.checked_add(file.verified_bytes.ok_or_else(invalid)?.parse::<u128>().map_err(|_| invalid())?).ok_or_else(invalid)?;
                    }
                }
                Ok(TransferSummaryStatus::Ready {
                    action: match manifest.action { super::Action::Offload(_) => TransferKind::Offload, super::Action::Restore(_) => TransferKind::Restore, super::Action::Recycle => return Err(invalid()) },
                    state: recorded.state,
                    created: manifest.created,
                    connection_id: manifest.connection_id,
                    files: manifest.entries.len(),
                    published_files,
                    published_bytes: published_bytes.to_string(),
                    source_preview: preview(&manifest.root),
                    destination_preview: preview(groups[0].destination.path.to_str().ok_or_else(invalid)?),
                    destinations: groups.len(),
                })
            })().unwrap_or(TransferSummaryStatus::Unavailable {
                reason: "Recorded evidence is unavailable or inconsistent. No file operations are authorized by this row.",
            });
            items.push(TransferSummary {
                id: if super::hex(&id, 64) {
                    id
                } else {
                    format!("unavailable-{row}")
                },
                status,
            });
        }
        drop(statement);
        tx.commit()?;
        human.check_directory(self.directory.identity())?;
        Ok(TransferPage { items, next_before })
    }
}

fn preview(value: &str) -> String {
    let mut chars = value.chars();
    let mut result = chars.by_ref().take(160).collect::<String>();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}
