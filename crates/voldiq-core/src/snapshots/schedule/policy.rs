#[cfg(windows)]
use super::decode;
use super::{Claim, encode, invalid_schedule};
use crate::{
    Error, Kind, NodeId, Result,
    cleanup::record::{self, Record},
    index::{Index, Query},
    metadata,
};
#[cfg(windows)]
use crate::{cleanup::Plan, operations::Lease};
#[cfg(windows)]
use rusqlite::OptionalExtension;
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub(in crate::snapshots) const SCHEMA: &str = "
ALTER TABLE scan_schedules ADD COLUMN cleanup_policy TEXT;
ALTER TABLE scheduled_runs ADD COLUMN cleanup_revision INTEGER;
ALTER TABLE scheduled_runs ADD COLUMN cleanup_status TEXT CHECK(cleanup_status IN ('ready','empty','unavailable','cooldown'));
ALTER TABLE scheduled_runs ADD COLUMN cleanup_files INTEGER;
ALTER TABLE scheduled_runs ADD COLUMN cleanup_logical TEXT;
ALTER TABLE scheduled_runs ADD COLUMN cleanup_record TEXT;
ALTER TABLE scheduled_runs ADD COLUMN cleanup_hash TEXT;
ALTER TABLE scheduled_runs ADD COLUMN cleanup_consumed INTEGER;
PRAGMA user_version=13;";

const MAX_DRAFT_BYTES: usize = 2 * 1024 * 1024;
const REVIEW_WINDOW: i64 = 24 * 60 * 60;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupPolicy {
    pub expression: String,
    pub max_files: u16,
    pub max_size: String,
    pub cooldown_hours: u16,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRef {
    pub schedule: i64,
    pub run: i64,
    pub revision: i64,
}

#[derive(Serialize)]
pub struct CleanupDraft {
    pub run: i64,
    pub status: String,
    pub files: Option<u16>,
    pub logical: Option<String>,
}

pub(super) struct PreparedDraft {
    record: String,
    hash: String,
    files: u16,
    logical: String,
}

#[derive(Deserialize, Serialize)]
struct DraftEvidence {
    record: Record,
    protection: String,
}

impl CleanupPolicy {
    fn query(&self) -> Query {
        Query {
            expression: self.expression.clone(),
            files_only: true,
            ..Default::default()
        }
    }

    pub(super) fn validate(&self) -> Result<u64> {
        if !(1..=500).contains(&self.max_files) || !(1..=168).contains(&self.cooldown_hours) {
            return Err(Error::Invalid(
                "Use 1–500 files and a cooldown of 1–168 hours.".into(),
            ));
        }
        if !self.query().cleanup_policy_is_restrictive()? {
            return Err(Error::Invalid(
                "Add a name, path, extension, category or modified-before restriction.".into(),
            ));
        }
        Ok(crate::search::storage_limit(&self.max_size)?)
    }
}

pub(super) fn prepare(
    policy: &CleanupPolicy,
    index: &Index,
    profile: &Path,
    cancel: &AtomicBool,
) -> Result<Option<PreparedDraft>> {
    let limit = policy.validate()?;
    if !index
        .observation
        .as_ref()
        .is_some_and(|observation| observation.complete)
    {
        return Err(Error::Invalid(
            "Cleanup policies require a complete scan.".into(),
        ));
    }
    let protections = metadata::Store::open(profile)?.protections()?;
    let mask = protections.for_index(index, cancel)?;
    let query = policy.query();
    let compiled = query.compile()?;
    let mut cancelled = || cancel.load(Ordering::Relaxed);
    let matches = index
        .prepare_query(&compiled, None, &mut cancelled)?
        .ok_or_else(invalid_schedule)?;
    let mut budget = crate::search::GLOB_WORK_LIMIT;
    let mut selection = Vec::<NodeId>::new();
    let mut identities = HashSet::new();
    let mut total = 0;
    for entry in &index.entries {
        if cancelled() {
            return Err(invalid_schedule());
        }
        if selection.len() >= usize::from(policy.max_files) {
            break;
        }
        let Some(state) = entry.file_state else {
            continue;
        };
        if entry.kind != Kind::File
            || entry.incomplete
            || entry.hard_link
            || entry.cloud.is_placeholder()
            || entry.logical > limit - total
            || !mask.allows(entry.id)
            || matches.matches(entry.id, &mut budget, &mut cancelled)? != Some(true)
            || record::sensitive(&index.path(entry.id)?)
            || !identities.insert(state.identity)
        {
            continue;
        }
        total += entry.logical;
        selection.push(entry.id);
    }
    if selection.is_empty() {
        return Ok(None);
    }
    let record = Record::capture(index, &selection)?;
    record.check_protections(&protections)?;
    if record.has_shared_files() {
        return Err(invalid_schedule());
    }
    let record = encode(&DraftEvidence {
        record,
        protection: protections.fingerprint().to_owned(),
    })?;
    if record.len() > MAX_DRAFT_BYTES {
        return Err(invalid_schedule());
    }
    let hash = blake3::hash(record.as_bytes()).to_hex().to_string();
    Ok(Some(PreparedDraft {
        record,
        hash,
        files: u16::try_from(selection.len()).map_err(|_| invalid_schedule())?,
        logical: total.to_string(),
    }))
}

pub(super) fn summary(
    row: &Row<'_>,
    revision: i64,
    enabled: bool,
) -> rusqlite::Result<Option<CleanupDraft>> {
    let Some(mut status) = row.get::<_, Option<String>>(9)? else {
        return Ok(None);
    };
    if status == "ready" {
        if row.get::<_, Option<i64>>(13)?.is_some() {
            status = "submitted".into();
        } else if !enabled
            || row.get::<_, i64>(12)? != revision
            || row.get::<_, Option<i64>>(3)?.is_none()
            || !row.get::<_, bool>(14)?
        {
            status = "invalidated".into();
        } else if row.get::<_, Option<i64>>(1)?.is_some_and(|finished| {
            super::time_now().is_ok_and(|now| now.saturating_sub(finished) >= REVIEW_WINDOW)
        }) {
            status = "expired".into();
        }
    }
    Ok(Some(CleanupDraft {
        run: row.get(8)?,
        status,
        files: row.get(10)?,
        logical: row.get(11)?,
    }))
}

impl Claim {
    pub(super) fn publish_cleanup(&self, tx: &Connection) -> Result<bool> {
        let Some(draft) = &self.draft else {
            return Ok(false);
        };
        if self.cleanup_on_cooldown {
            tx.execute("UPDATE scheduled_runs SET cleanup_status='cooldown',cleanup_revision=?2 WHERE id=?1", params![self.id, self.revision])?;
            return Ok(false);
        }
        match draft {
            Ok(Some(draft)) => {
                tx.execute("UPDATE scheduled_runs SET cleanup_record=NULL WHERE schedule_id=?1 AND id<?2 AND cleanup_consumed IS NULL", params![self.schedule, self.id])?;
                tx.execute("UPDATE scheduled_runs SET cleanup_status='ready',cleanup_revision=?2,cleanup_files=?3,cleanup_logical=?4,cleanup_record=?5,cleanup_hash=?6 WHERE id=?1",
                    params![self.id, self.revision, draft.files, draft.logical, draft.record, draft.hash])?;
                Ok(true)
            }
            draft => {
                let status = if draft.is_ok() {
                    "empty"
                } else {
                    "unavailable"
                };
                tx.execute(
                    "UPDATE scheduled_runs SET cleanup_status=?2,cleanup_revision=?3 WHERE id=?1",
                    params![self.id, status, self.revision],
                )?;
                Ok(false)
            }
        }
    }
}

impl super::super::Store {
    #[cfg(windows)]
    pub fn review_scheduled_cleanup(
        &self,
        lease: &Lease,
        reference: DraftRef,
    ) -> Result<(Plan, String)> {
        self.check_schedule_lease(lease)?;
        let sql = format!("SELECT r.cleanup_record,r.cleanup_hash {ACTIVE_DRAFT}");
        let (record, hash): (String, String) = self
            .connection
            .query_row(
                &sql,
                params![
                    reference.run,
                    reference.schedule,
                    reference.revision,
                    super::time_now()?,
                    REVIEW_WINDOW
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(invalid_schedule)?;
        if record.len() > MAX_DRAFT_BYTES
            || blake3::hash(record.as_bytes()).to_hex().as_str() != hash
        {
            return Err(invalid_schedule());
        }
        let evidence: DraftEvidence = decode(&record)?;
        let protections = metadata::Store::open(lease.directory())?.protections()?;
        if protections.fingerprint() != evidence.protection {
            return Err(invalid_schedule());
        }
        evidence.record.validate(lease.directory_identity())?;
        evidence.record.check_protections(&protections)?;
        let plan = evidence
            .record
            .into_plan()?
            .with_protections(lease.directory())?;
        Ok((plan, hash))
    }

    #[cfg(windows)]
    pub fn consume_scheduled_cleanup(
        &self,
        lease: &Lease,
        reference: DraftRef,
        hash: &str,
    ) -> Result<()> {
        let (_, current) = self.review_scheduled_cleanup(lease, reference)?;
        if current != hash {
            return Err(invalid_schedule());
        }
        let now = super::time_now()?;
        let sql = format!(
            "UPDATE scheduled_runs SET cleanup_consumed=?4,cleanup_record=NULL WHERE id=(SELECT r.id {ACTIVE_DRAFT} AND r.cleanup_hash=?6)"
        );
        if self.connection.execute(
            &sql,
            params![
                reference.run,
                reference.schedule,
                reference.revision,
                now,
                REVIEW_WINDOW,
                hash
            ],
        )? != 1
        {
            return Err(invalid_schedule());
        }
        Ok(())
    }
}

#[cfg(windows)]
const ACTIVE_DRAFT: &str = "FROM scheduled_runs r JOIN scan_schedules s ON s.id=r.schedule_id
  WHERE r.id=?1 AND s.id=?2 AND s.revision=?3 AND r.cleanup_revision=s.revision
    AND s.enabled AND s.cleanup_policy IS NOT NULL AND r.snapshot IS NOT NULL
    AND r.cleanup_status='ready' AND r.cleanup_consumed IS NULL AND r.cleanup_record IS NOT NULL
    AND r.finished<=?4 AND r.finished>?4-?5";
