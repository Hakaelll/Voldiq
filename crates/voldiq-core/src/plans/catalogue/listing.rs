use super::{
    FILE_COLUMNS, HumanAuthority, State, Store, invalid, read_file, recorded_manifest,
    validate_file,
};
use crate::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferFilter {
    pub text: String,
    pub published_only: bool,
    #[serde(default)]
    pub review_due_only: bool,
}

impl TransferFilter {
    pub(super) fn needle(&self) -> Result<String> {
        if self.text.len() > 800 || self.text.chars().count() > 200 {
            return Err(crate::Error::Invalid(
                "Search is limited to 200 characters.".into(),
            ));
        }
        Ok(self.text.trim().replace('/', "\\").to_lowercase())
    }
}

pub(super) fn matches_file(
    manifest: &super::Manifest,
    file: &super::TransferFile,
    needle: &str,
    filter: &TransferFilter,
    retention: Retention,
) -> Result<bool> {
    if (filter.published_only && file.published_at.is_none())
        || (filter.review_due_only && !retention.due(manifest, file))
    {
        return Ok(false);
    }
    if needle.is_empty() {
        return Ok(true);
    }
    let entry = manifest
        .entries
        .iter()
        .find(|entry| entry.id == file.node)
        .ok_or_else(invalid)?;
    let group = manifest.action.transfer_group(file.node)?;
    let output = group.destination.path.join(&file.output_name);
    let includes = |text: &str| text.replace('/', "\\").to_lowercase().contains(needle);
    Ok(includes(&entry.path) || includes(&output.to_string_lossy()) || includes(&file.output_name))
}

#[derive(Clone, Copy)]
pub(in crate::plans) struct Retention {
    pub days: Option<u16>,
    pub checked_at: i64,
}

impl Retention {
    pub(super) fn read(tx: &rusqlite::Transaction<'_>) -> Result<Self> {
        Ok(Self {
            days: super::vault::retention_days(tx)?,
            checked_at: crate::plans::now()?,
        })
    }

    pub(in crate::plans) fn due(
        self,
        manifest: &super::Manifest,
        file: &super::TransferFile,
    ) -> bool {
        super::vault::review_at(manifest, file, self.days).is_some_and(|at| at <= self.checked_at)
    }
}

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
        review_due_files: usize,
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
        filter: &TransferFilter,
    ) -> Result<TransferPage> {
        human.check_directory(self.directory.identity())?;
        if !(1..=25).contains(&limit) {
            return Err(invalid());
        }
        let needle = filter.needle()?;
        let filtering = filter.published_only || filter.review_due_only || !needle.is_empty();
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
        let retention = Retention::read(&tx)?;
        let mut statement = tx.prepare(
            "SELECT p.rowid,CASE WHEN length(p.id)=64 THEN p.id ELSE '' END FROM agent_plans p JOIN transfer_operations o ON o.plan_id=p.id
            WHERE (?1 IS NULL OR p.rowid<?1) ORDER BY p.rowid DESC LIMIT ?2",
        )?;
        let scan_limit = if filtering { 100 } else { limit };
        let mut rows = statement
            .query_map(params![before, (scan_limit + 1) as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut more = rows.len() > scan_limit;
        rows.truncate(scan_limit);
        let mut next_before = None;
        let mut items = Vec::with_capacity(rows.len());
        let count = rows.len();
        for (index, (row, id)) in rows.into_iter().enumerate() {
            next_before = Some(row.to_string());
            let status = (|| -> Result<Option<TransferSummaryStatus>> {
                let recorded = recorded_manifest(&tx, &id)?;
                let manifest = &recorded.manifest;
                let groups = manifest.action.transfer_groups()?;
                let mut files = tx.prepare(&format!("SELECT {FILE_COLUMNS} FROM transfer_files WHERE plan_id=?1 ORDER BY node LIMIT 501"))?;
                let files = files.query_map([&id], read_file)?.collect::<std::result::Result<Vec<_>,_>>()?;
                if files.len() != manifest.entries.len() { return Err(invalid()); }
                let mut published_files = 0;
                let mut published_bytes = 0_u128;
                let mut review_due_files = 0;
                let mut found = !filtering;
                for file in files {
                    validate_file(&file, manifest)?;
                    if !found {
                        found = matches_file(manifest, &file, &needle, filter, retention)?;
                    }
                    if file.published_at.is_some() {
                        published_files += 1;
                        published_bytes = published_bytes.checked_add(file.verified_bytes.as_deref().ok_or_else(invalid)?.parse::<u128>().map_err(|_| invalid())?).ok_or_else(invalid)?;
                    }
                    if retention.due(manifest, &file) { review_due_files += 1; }
                }
                if !found { return Ok(None); }
                Ok(Some(TransferSummaryStatus::Ready {
                    action: match manifest.action { super::Action::Offload(_) => TransferKind::Offload, super::Action::Restore(_) => TransferKind::Restore, super::Action::Recycle => return Err(invalid()) },
                    state: recorded.state,
                    created: manifest.created,
                    connection_id: manifest.connection_id,
                    files: manifest.entries.len(),
                    published_files,
                    published_bytes: published_bytes.to_string(),
                    review_due_files,
                    source_preview: preview(&manifest.root),
                    destination_preview: preview(groups[0].destination.path.to_str().ok_or_else(invalid)?),
                    destinations: groups.len(),
                }))
            })().unwrap_or(Some(TransferSummaryStatus::Unavailable {
                reason: "Recorded evidence is unavailable or inconsistent. No file operations are authorized by this row.",
            }));
            let Some(status) = status else { continue };
            if filtering && matches!(status, TransferSummaryStatus::Unavailable { .. }) {
                continue;
            }
            items.push(TransferSummary {
                id: if super::hex(&id, 64) {
                    id
                } else {
                    format!("unavailable-{row}")
                },
                status,
            });
            if items.len() == limit {
                more |= index + 1 < count;
                break;
            }
        }
        drop(statement);
        tx.commit()?;
        human.check_directory(self.directory.identity())?;
        Ok(TransferPage {
            items,
            next_before: next_before.filter(|_| more),
        })
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
