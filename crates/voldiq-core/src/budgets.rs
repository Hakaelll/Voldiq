//! Logical-byte folder limits and explicitly qualified metadata estimates.
mod forecast;
use crate::{
    Error, NodeId, Result,
    index::{Index, Observation, Query},
    operations::Coordinator,
};
pub use forecast::{Forecast, ForecastReason};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Store {
    connection: Connection,
    directory: Coordinator,
}
#[derive(Serialize)]
pub struct Budget {
    pub id: i64,
    pub path: PathBuf,
    pub expression: String,
    #[serde(serialize_with = "bytes")]
    pub limit_bytes: u64,
    pub version: String,
}
#[derive(Serialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Complete,
    Partial,
    Provisional,
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NoBudget,
    Within,
    AtLimit,
    Exceeded,
    Unknown,
}
#[derive(Serialize)]
pub struct ImageEquivalent {
    pub sample_count: usize,
    pub matched_images: usize,
    #[serde(serialize_with = "bytes")]
    pub mean_logical_bytes: u64,
    #[serde(serialize_with = "bytes")]
    pub approximate_images: u64,
}
#[derive(Serialize)]
pub struct Report {
    pub detailed: bool,
    pub budget: Option<Budget>,
    pub coverage: Coverage,
    pub status: Status,
    #[serde(serialize_with = "bytes")]
    pub logical_bytes: u64,
    pub observed_percent: Option<f64>,
    pub source_snapshot: Option<i64>,
    pub observation: Option<Observation>,
    pub forecast: Forecast,
    pub image_equivalent: Option<ImageEquivalent>,
}
fn bytes<S: serde::Serializer>(value: &u64, serializer: S) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}
fn changed() -> Error {
    Error::Invalid("Folder budget changed or is unavailable. Reload before continuing.".into())
}
fn read_budget(row: &rusqlite::Row<'_>) -> Result<Budget> {
    let id: i64 = row.get(0)?;
    let path: String = row.get(1)?;
    let expression: String = row.get(2)?;
    let raw: Vec<u8> = row.get(3)?;
    let limit_bytes = u64::from_le_bytes(raw.try_into().map_err(|_| changed())?);
    let version: i64 = row.get(4)?;
    if !(1..=9_007_199_254_740_991).contains(&id)
        || version <= 0
        || path.len() > 32767
        || crate::search::storage_limit(&expression)? != limit_bytes
    {
        return Err(changed());
    }
    Ok(Budget {
        id,
        path: path.into(),
        expression,
        limit_bytes,
        version: version.to_string(),
    })
}
impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        let directory = Coordinator::open(directory)?;
        let mut connection = Connection::open(directory.directory().join("budgets.db"))?;
        crate::database::configure(&connection)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: u32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 0 {
            tx.execute_batch("CREATE TABLE folder_budgets(id INTEGER PRIMARY KEY AUTOINCREMENT, identity BLOB NOT NULL UNIQUE, path TEXT NOT NULL, expression TEXT NOT NULL, limit_bytes BLOB NOT NULL CHECK(length(limit_bytes)=8), version INTEGER NOT NULL CHECK(version>0)); PRAGMA user_version=1;")?;
        } else if version != 1 {
            return Err(Error::Invalid("Unsupported folder budget schema.".into()));
        }
        tx.commit()?;
        Ok(Self {
            connection,
            directory,
        })
    }
    fn get(&self, identity: &[u8]) -> Result<Option<Budget>> {
        let mut statement = self.connection.prepare(
            "SELECT id,path,expression,limit_bytes,version FROM folder_budgets WHERE identity=?1",
        )?;
        let mut rows = statement.query([identity])?;
        rows.next()?.map(read_budget).transpose()
    }
    pub fn save(
        &mut self,
        index: &Index,
        node: NodeId,
        version: &str,
        expression: &str,
    ) -> Result<Budget> {
        let limit = crate::search::storage_limit(expression)?;
        let folder = index.pin_folder(node)?;
        let identity = folder.identity_bytes()?;
        let path = folder
            .path()
            .to_str()
            .filter(|path| path.len() <= 32767)
            .ok_or_else(changed)?;
        let _lease = self.directory.acquire()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Option<i64> = tx
            .query_row(
                "SELECT version FROM folder_budgets WHERE identity=?1",
                [&identity],
                |r| r.get(0),
            )
            .optional()?;
        if old.unwrap_or(0).to_string() != version {
            return Err(changed());
        }
        if old.is_none()
            && tx.query_row("SELECT COUNT(*) FROM folder_budgets", [], |r| {
                r.get::<_, i64>(0)
            })? >= 1000
        {
            return Err(Error::Invalid("Save up to 1000 folder budgets.".into()));
        }
        let next = old.unwrap_or(0).checked_add(1).ok_or_else(changed)?;
        tx.execute("INSERT INTO folder_budgets(identity,path,expression,limit_bytes,version) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(identity) DO UPDATE SET path=excluded.path,expression=excluded.expression,limit_bytes=excluded.limit_bytes,version=excluded.version", params![identity,path,expression.trim(),limit.to_le_bytes().as_slice(),next])?;
        tx.commit()?;
        self.get(&identity)?.ok_or_else(changed)
    }
    pub fn remove(&mut self, index: &Index, node: NodeId, id: i64, version: &str) -> Result<()> {
        let folder = index.pin_folder(node)?;
        let _lease = self.directory.acquire()?;
        let expected: i64 = version.parse().map_err(|_| changed())?;
        if expected <= 0
            || expected.to_string() != version
            || self.connection.execute(
                "DELETE FROM folder_budgets WHERE identity=?1 AND id=?2 AND version=?3",
                params![folder.identity_bytes()?, id, expected],
            )? != 1
        {
            return Err(changed());
        }
        Ok(())
    }
    pub fn read(
        &self,
        index: &Index,
        node: NodeId,
        history: &crate::snapshots::Store,
        detailed: bool,
        cancelled: &AtomicBool,
    ) -> Result<Option<Report>> {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let folder = if node != NodeId(0)
            && index.snapshot_id.is_some()
            && index.entry(node)?.file_state.is_none()
        {
            let expected = history
                .indexed_folder_identity(index, node)?
                .ok_or_else(changed)?;
            index.pin_folder_expected(node, expected)?
        } else {
            index.pin_folder(node)?
        };
        let mut budget = self.get(&folder.identity_bytes()?)?;
        if let Some(budget) = &mut budget {
            budget.path = folder.path().to_owned();
        }
        let entry = index.entry(node)?;
        let coverage = if index.observation.is_none() && index.snapshot_id.is_none() {
            Coverage::Provisional
        } else if entry.incomplete
            || index.error_count > 0
            || index.entries[0].incomplete
            || index.observation.as_ref().is_some_and(|o| !o.complete)
        {
            Coverage::Partial
        } else {
            Coverage::Complete
        };
        let status = match budget.as_ref().map(|b| b.limit_bytes) {
            None => Status::NoBudget,
            Some(limit) if entry.logical > limit => Status::Exceeded,
            Some(_) if coverage != Coverage::Complete => Status::Unknown,
            Some(limit) if entry.logical == limit => Status::AtLimit,
            Some(_) => Status::Within,
        };
        let observed_percent = budget
            .as_ref()
            .map(|b| entry.logical as f64 / b.limit_bytes as f64 * 100.0);
        let image_equivalent = if detailed && coverage == Coverage::Complete {
            let Some(sample) = index.query_cancellable(
                &Query {
                    scope: Some(node),
                    expression: "category:image size>0B".into(),
                    sort: "name".into(),
                    limit: 128,
                    ..Query::default()
                },
                || cancelled.load(Ordering::Relaxed),
            )?
            else {
                return Ok(None);
            };
            let count = sample.items.len();
            let sum: u128 = sample.items.iter().map(|e| u128::from(e.logical)).sum();
            if sum == 0 {
                None
            } else {
                Some(ImageEquivalent {
                    sample_count: count,
                    matched_images: sample.total,
                    mean_logical_bytes: u64::try_from(sum / count as u128)
                        .map_err(|_| changed())?,
                    approximate_images: u64::try_from(
                        u128::from(entry.logical) * count as u128 / sum,
                    )
                    .map_err(|_| changed())?,
                })
            }
        } else {
            None
        };
        let forecast = if detailed {
            forecast::estimate(index, node, budget.as_ref(), coverage, history, cancelled)?
        } else {
            Forecast::Unavailable {
                reason: ForecastReason::NotRequested,
            }
        };
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        Ok(Some(Report {
            detailed,
            observed_percent,
            logical_bytes: entry.logical,
            budget,
            coverage,
            status,
            source_snapshot: index.snapshot_id,
            observation: index.observation.clone(),
            forecast,
            image_equivalent,
        }))
    }
}
