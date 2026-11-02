use super::{Store, decode, unsigned};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const SCHEMA: &str = "
CREATE TABLE snapshot_history_revision(id INTEGER PRIMARY KEY CHECK(id=1), value INTEGER NOT NULL CHECK(typeof(value)='integer' AND value>=0));
INSERT INTO snapshot_history_revision VALUES(1,0);
CREATE TABLE snapshot_retention(root_id INTEGER PRIMARY KEY REFERENCES snapshot_roots(id) ON DELETE CASCADE,
 version INTEGER NOT NULL CHECK(typeof(version)='integer' AND version>0),
 keep_latest INTEGER CHECK(keep_latest BETWEEN 1 AND 10000));
CREATE TRIGGER history_snapshot_insert AFTER INSERT ON snapshots BEGIN UPDATE snapshot_history_revision SET value=value+1 WHERE id=1; END;
CREATE TRIGGER history_snapshot_update AFTER UPDATE ON snapshots BEGIN UPDATE snapshot_history_revision SET value=value+1 WHERE id=1; END;
CREATE TRIGGER history_snapshot_delete AFTER DELETE ON snapshots BEGIN UPDATE snapshot_history_revision SET value=value+1 WHERE id=1; END;
CREATE TRIGGER history_retention_insert AFTER INSERT ON snapshot_retention BEGIN UPDATE snapshot_history_revision SET value=value+1 WHERE id=1; END;
CREATE TRIGGER history_retention_update AFTER UPDATE ON snapshot_retention BEGIN UPDATE snapshot_history_revision SET value=value+1 WHERE id=1; END;
CREATE TRIGGER history_retention_delete AFTER DELETE ON snapshot_retention BEGIN UPDATE snapshot_history_revision SET value=value+1 WHERE id=1; END;
PRAGMA user_version=8;";

const BATCH_LIMIT: i64 = 100;

#[derive(Serialize)]
pub struct RetentionSettings {
    pub root_key: i64,
    pub root: String,
    pub version: String,
    pub keep_latest: Option<u32>,
    pub total: u64,
}
#[derive(Serialize)]
pub struct RetentionCapture {
    pub id: i64,
    pub captured: u64,
    pub logical: String,
    pub complete: bool,
}
#[derive(Serialize)]
pub struct RetentionPreview {
    pub settings: RetentionSettings,
    pub revision: String,
    pub fingerprint: String,
    pub eligible: u64,
    pub captures: Vec<RetentionCapture>,
}
#[derive(Serialize)]
pub struct RetentionResult {
    pub removed: usize,
    pub remaining_eligible: u64,
}

fn invalid() -> Error {
    Error::Invalid("History or retention preference changed. Reload and review again.".into())
}

fn settings(connection: &Connection, snapshot: i64) -> Result<RetentionSettings> {
    if snapshot <= 0 {
        return Err(invalid());
    }
    let (root, settings) = connection.query_row("SELECT r.path,r.id,COALESCE(t.version,0),t.keep_latest,(SELECT COUNT(*) FROM snapshots h WHERE h.root_id=r.id) FROM snapshots s JOIN snapshot_roots r ON r.id=s.root_id LEFT JOIN snapshot_retention t ON t.root_id=r.id WHERE s.id=?1", [snapshot], |row| Ok((row.get::<_,Vec<u8>>(0)?, RetentionSettings {
        root_key: row.get(1)?, root: String::new(), version: row.get::<_,i64>(2)?.to_string(), keep_latest: row.get(3)?, total: unsigned(row,4)?,
    }))).optional()?.ok_or_else(invalid)?;
    if settings.root_key <= 0
        || settings.version.starts_with('-')
        || settings
            .keep_latest
            .is_some_and(|keep| !(1..=10000).contains(&keep))
    {
        return Err(invalid());
    }
    Ok(RetentionSettings {
        root: decode(&root)?.to_string_lossy().into_owned(),
        ..settings
    })
}

fn preview(
    connection: &Connection,
    snapshot: i64,
    cancel: &AtomicBool,
) -> Result<Option<RetentionPreview>> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let settings = settings(connection, snapshot)?;
    let revision: i64 = connection.query_row(
        "SELECT value FROM snapshot_history_revision WHERE id=1",
        [],
        |row| row.get(0),
    )?;
    let eligible = settings
        .keep_latest
        .map_or(0, |keep| settings.total.saturating_sub(u64::from(keep)));
    let mut captures = Vec::new();
    if eligible > 0 {
        let keep = settings.keep_latest.ok_or_else(invalid)?;
        let mut statement = connection.prepare("SELECT id,captured,logical,(complete AND COALESCE(observed_complete,1)) FROM snapshots WHERE root_id=?1 AND id<(SELECT id FROM snapshots WHERE root_id=?1 ORDER BY id DESC LIMIT 1 OFFSET ?2) ORDER BY id LIMIT ?3")?;
        let mut rows = statement.query(params![settings.root_key, keep - 1, BATCH_LIMIT])?;
        while let Some(row) = rows.next()? {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            captures.push(RetentionCapture {
                id: row.get(0)?,
                captured: unsigned(row, 1)?,
                logical: unsigned(row, 2)?.to_string(),
                complete: row.get(3)?,
            });
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let mut result = RetentionPreview {
        settings,
        revision: revision.to_string(),
        fingerprint: String::new(),
        eligible,
        captures,
    };
    result.fingerprint = blake3::hash(&serde_json::to_vec(&result).map_err(|_| invalid())?)
        .to_hex()
        .to_string();
    Ok(Some(result))
}

pub(super) fn prune(connection: &Connection, cancel: &AtomicBool) -> Result<bool> {
    for sql in [
        "DELETE FROM snapshot_roots WHERE NOT EXISTS(SELECT 1 FROM snapshots s WHERE s.root_id=snapshot_roots.id)",
        "DELETE FROM snapshot_versions WHERE NOT EXISTS (SELECT 1 FROM snapshots s JOIN snapshot_paths p ON p.root_id=s.root_id WHERE p.id=snapshot_versions.path_id AND s.id>=snapshot_versions.start_id AND (snapshot_versions.end_id IS NULL OR s.id<snapshot_versions.end_id))",
    ] {
        if cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        connection.execute(sql, [])?;
    }
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        if connection.execute("DELETE FROM snapshot_paths WHERE NOT EXISTS(SELECT 1 FROM snapshot_versions v WHERE v.path_id=snapshot_paths.id) AND NOT EXISTS(SELECT 1 FROM snapshot_paths child WHERE child.parent_id=snapshot_paths.id)", [])? == 0 { break; }
    }
    Ok(!cancel.load(Ordering::Relaxed))
}

impl Store {
    pub fn retention_settings(&self, snapshot: i64) -> Result<RetentionSettings> {
        let _read = self.history_read()?;
        settings(&self.connection, snapshot)
    }

    pub fn set_retention(
        &mut self,
        snapshot: i64,
        version: &str,
        keep_latest: Option<u32>,
    ) -> Result<RetentionSettings> {
        if keep_latest.is_some_and(|keep| !(1..=10000).contains(&keep)) {
            return Err(Error::Invalid(
                "Keep between 1 and 10,000 newest captures, or choose Keep all.".into(),
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = settings(&transaction, snapshot)?;
        if current.version != version {
            return Err(invalid());
        }
        if current.keep_latest == keep_latest {
            return Ok(current);
        }
        let next = version
            .parse::<i64>()
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(invalid)?;
        transaction.execute("INSERT INTO snapshot_retention(root_id,version,keep_latest) VALUES(?1,?2,?3) ON CONFLICT(root_id) DO UPDATE SET version=excluded.version,keep_latest=excluded.keep_latest", params![current.root_key,next,keep_latest])?;
        let updated = settings(&transaction, snapshot)?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn preview_retention(
        &self,
        snapshot: i64,
        cancel: &AtomicBool,
    ) -> Result<Option<RetentionPreview>> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let _read = self.history_read()?;
        preview(&self.connection, snapshot, cancel)
    }

    pub fn apply_retention(
        &mut self,
        snapshot: i64,
        fingerprint: &str,
        cancel: &AtomicBool,
    ) -> Result<Option<RetentionResult>> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(review) = preview(&transaction, snapshot, cancel)? else {
            return Ok(None);
        };
        if review.fingerprint != fingerprint || review.captures.is_empty() {
            return Err(invalid());
        }
        for capture in &review.captures {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            if transaction.execute(
                "DELETE FROM snapshots WHERE id=?1 AND root_id=?2",
                params![capture.id, review.settings.root_key],
            )? != 1
            {
                return Err(invalid());
            }
        }
        if !prune(&transaction, cancel)? {
            return Ok(None);
        }
        let result = RetentionResult {
            removed: review.captures.len(),
            remaining_eligible: review.eligible - review.captures.len() as u64,
        };
        transaction.commit()?;
        Ok(Some(result))
    }
}
