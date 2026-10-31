//! Durable scan status and an OS-released cross-process scan lease. No job auto-retry.
use crate::{Error, Progress, Result, windows};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    fs::File,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub struct ScanLease {
    _file: File,
    directory_identity: (u64, u64),
    recovered: Cell<bool>,
}

impl ScanLease {
    pub fn acquire(directory: &Path) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("scan.lock"))?;
        file.try_lock()
            .map_err(|_| Error::Invalid("Another Voldiq process owns the active scan.".into()))?;
        Ok(Self {
            _file: file,
            directory_identity: windows::identity(directory)?.key,
            recovered: Cell::new(false),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Completed,
    Cancelled,
    Failed,
    Interrupted,
}

impl JobState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
    fn parse(value: &str) -> Result<Self> {
        match value {
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(Error::Invalid("Invalid persisted scan job state.".into())),
        }
    }
}

#[derive(Serialize)]
pub struct Job {
    pub id: i64,
    pub root: PathBuf,
    pub state: JobState,
    pub started: i64,
    pub finished: Option<i64>,
    pub snapshot_id: Option<i64>,
    pub cancel_requested: bool,
    pub progress: Option<serde_json::Value>,
}

pub struct Jobs {
    connection: Connection,
    directory_identity: (u64, u64),
}

impl Jobs {
    pub fn open(directory: &Path) -> Result<Self> {
        let mut connection = Connection::open(directory.join("jobs.db"))?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: u32 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version == 0 {
            tx.execute_batch("CREATE TABLE scan_jobs(id INTEGER PRIMARY KEY AUTOINCREMENT, owner INTEGER NOT NULL,
                root TEXT NOT NULL, state TEXT NOT NULL, started INTEGER NOT NULL, finished INTEGER,
                snapshot_id INTEGER, cancel_requested INTEGER NOT NULL DEFAULT 0, progress TEXT);
                CREATE INDEX scan_jobs_owner ON scan_jobs(owner,id);
                PRAGMA user_version=1;")?;
        } else if version != 1 {
            return Err(Error::Invalid("Unsupported scan jobs schema.".into()));
        }
        tx.commit()?;
        Ok(Self {
            connection,
            directory_identity: windows::identity(directory)?.key,
        })
    }

    pub fn begin(&mut self, lease: &ScanLease, owner: i64, root: &Path) -> Result<i64> {
        if lease.directory_identity != self.directory_identity {
            return Err(Error::Invalid(
                "Scan lease belongs to another workspace.".into(),
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM scan_jobs WHERE state='running')",
            [],
            |row| row.get(0),
        )?;
        if lease.recovered.get() && active {
            return Err(Error::Invalid(
                "Scan lease already has an active job.".into(),
            ));
        }
        tx.execute(
            "UPDATE scan_jobs SET state='interrupted',finished=?1 WHERE state='running'",
            [now()?],
        )?;
        tx.execute("DELETE FROM scan_jobs WHERE id IN (SELECT id FROM scan_jobs WHERE state!='running' ORDER BY id DESC LIMIT -1 OFFSET 9999)", [])?;
        tx.execute(
            "INSERT INTO scan_jobs(owner,root,state,started) VALUES(?1,?2,'running',?3)",
            params![
                owner,
                serde_json::to_string(root)
                    .map_err(|_| Error::Invalid("Scan root is not representable.".into()))?,
                now()?
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        lease.recovered.set(true);
        Ok(id)
    }

    pub fn progress(&self, owner: i64, id: i64, progress: &Progress) -> Result<bool> {
        let json = serde_json::to_string(progress).map_err(|_| unavailable())?;
        if self.connection.execute(
            "UPDATE scan_jobs SET progress=?1 WHERE id=?2 AND owner=?3 AND state='running'",
            params![json, id, owner],
        )? != 1
        {
            return Err(unavailable());
        }
        self.cancellation_requested(owner, id)
    }

    pub fn cancellation_requested(&self, owner: i64, id: i64) -> Result<bool> {
        self.connection
            .query_row(
                "SELECT cancel_requested FROM scan_jobs WHERE id=?1 AND owner=?2",
                params![id, owner],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(unavailable)
    }

    pub fn cancel(&self, owner: i64, id: i64) -> Result<()> {
        if self.connection.execute(
            "UPDATE scan_jobs SET cancel_requested=1 WHERE id=?1 AND owner=?2 AND state='running'",
            params![id, owner],
        )? != 1
        {
            return Err(unavailable());
        }
        Ok(())
    }

    pub fn finish(
        &self,
        owner: i64,
        id: i64,
        state: JobState,
        snapshot_id: Option<i64>,
    ) -> Result<()> {
        if state == JobState::Running || (state != JobState::Completed && snapshot_id.is_some()) {
            return Err(unavailable());
        }
        if self.connection.execute("UPDATE scan_jobs SET state=?1,finished=?2,snapshot_id=?3 WHERE id=?4 AND owner=?5 AND state='running'",
            params![state.as_str(),now()?,snapshot_id,id,owner])? != 1 { return Err(unavailable()); }
        Ok(())
    }

    pub fn get(&self, owner: i64, id: i64) -> Result<Job> {
        let mut statement = self.connection.prepare("SELECT id,root,state,started,finished,snapshot_id,cancel_requested,progress FROM scan_jobs WHERE id=?1 AND owner=?2")?;
        let mut rows = statement.query(params![id, owner])?;
        decode(rows.next()?.ok_or_else(unavailable)?)
    }

    pub fn list(&self, owner: i64, offset: usize, limit: usize) -> Result<Vec<Job>> {
        let mut statement = self.connection.prepare("SELECT id,root,state,started,finished,snapshot_id,cancel_requested,progress FROM scan_jobs WHERE owner=?1 ORDER BY id DESC LIMIT ?2 OFFSET ?3")?;
        let mut rows = statement.query(params![
            owner,
            limit.clamp(1, 500) as i64,
            i64::try_from(offset).map_err(|_| unavailable())?
        ])?;
        let mut jobs = Vec::new();
        while let Some(row) = rows.next()? {
            jobs.push(decode(row)?);
        }
        Ok(jobs)
    }
}

fn decode(row: &rusqlite::Row<'_>) -> Result<Job> {
    let root: String = row.get(1)?;
    let progress: Option<String> = row.get(7)?;
    if root.len() > 262_144 || progress.as_ref().is_some_and(|json| json.len() > 4096) {
        return Err(unavailable());
    }
    Ok(Job {
        id: row.get(0)?,
        root: serde_json::from_str(&root).map_err(|_| unavailable())?,
        state: JobState::parse(&row.get::<_, String>(2)?)?,
        started: row.get(3)?,
        finished: row.get(4)?,
        snapshot_id: row.get(5)?,
        cancel_requested: row.get(6)?,
        progress: progress
            .map(|json| serde_json::from_str(&json))
            .transpose()
            .map_err(|_| unavailable())?,
    })
}

fn now() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_secs()).ok())
        .ok_or_else(unavailable)
}

fn unavailable() -> Error {
    Error::Invalid("Scan job unavailable for this connection or no longer running.".into())
}
