use super::Plan;
use crate::{Error, NodeId, Result};
use rusqlite::{Connection, TransactionBehavior, params};
use serde::Serialize;
use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, UNIX_EPOCH},
};
#[cfg(windows)]
pub mod relocation;

const APPLICATION_ID: i64 = 0x56444c43;
static OWNERS: AtomicU64 = AtomicU64::new(1);
const SCHEMA: &str = "
CREATE TABLE operations(id INTEGER PRIMARY KEY AUTOINCREMENT, rule TEXT NOT NULL,
 policy TEXT NOT NULL CHECK(policy='permanent'), created INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('active','finished','interrupted')));
CREATE TABLE items(operation INTEGER NOT NULL REFERENCES operations(id), node INTEGER NOT NULL,
 path TEXT NOT NULL, identity BLOB NOT NULL CHECK(length(identity)=16),
 change_time BLOB NOT NULL CHECK(length(change_time)=8), size INTEGER NOT NULL CHECK(size>=0),
 modified_seconds INTEGER NOT NULL, modified_nanos INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('prepared','in_flight','succeeded','failed','cancelled','unknown')),
 message TEXT, PRIMARY KEY(operation,node));
PRAGMA user_version=1;";

pub struct Journal {
    connection: Connection,
    owner: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct OperationId(i64, u64);

pub enum Outcome {
    Succeeded,
    Failed(String),
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Permanent,
    Recycling,
}

#[derive(Debug, Serialize)]
pub struct ActivityItem {
    pub kind: ActivityKind,
    #[serde(flatten)]
    pub item: Item,
}

#[derive(Debug, Serialize)]
pub struct Item {
    pub operation: i64,
    pub node: NodeId,
    pub path: String,
    pub state: String,
    pub message: Option<String>,
    pub created: i64,
}

impl Journal {
    #[cfg(test)]
    pub(crate) fn reject_result_updates(&self) -> Result<()> {
        self.connection.execute_batch("CREATE TRIGGER reject_result BEFORE UPDATE ON items
            WHEN NEW.state IN ('succeeded','failed') BEGIN SELECT RAISE(ABORT,'Controlled result-write failure'); END;")?;
        Ok(())
    }
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(Error::Invalid(
                "Cleanup journal requires an absolute file path.".into(),
            ));
        }
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_millis(250))?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA locking_mode=EXCLUSIVE; PRAGMA synchronous=FULL;",
        )?;
        let mode: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        if mode != "delete" {
            return Err(Error::Invalid(
                "Cleanup journal requires DELETE journal mode.".into(),
            ));
        }
        let owner = OWNERS
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| Error::Invalid("Cleanup journal owner sequence exhausted.".into()))?;
        // Exclusive locking persists between commits, so another owner cannot recover a live operation.
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Exclusive)?;
        let version: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let application: i64 =
            transaction.query_row("PRAGMA application_id", [], |row| row.get(0))?;
        if version == 0 && application == 0 {
            let tables: i64 = transaction.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )?;
            if tables != 0 {
                return Err(Error::Invalid(
                    "Cleanup journal is not an empty database.".into(),
                ));
            }
            transaction.execute_batch(SCHEMA)?;
            transaction.execute_batch(&format!("PRAGMA application_id={APPLICATION_ID};"))?;
        } else if !matches!(version, 1..=4) || application != APPLICATION_ID {
            return Err(Error::Invalid(
                "Unsupported cleanup journal database.".into(),
            ));
        }
        #[cfg(windows)]
        {
            if version < 2 {
                transaction.execute_batch(relocation::SCHEMA)?;
            }
            if version < 3 {
                transaction.execute_batch(relocation::RECYCLING_MIGRATION)?;
            }
            if version < 4 {
                transaction.execute_batch(relocation::VERSION_MIGRATION)?;
            }
            transaction.execute("UPDATE relocations SET state=CASE state WHEN 'prepared' THEN 'cancelled' ELSE 'unknown' END,
                message='Interrupted; reconcile identities before any explicit recovery.'
                WHERE state IN ('prepared','stage_ready','relocating','staged','returning','recycling','restoring')", [])?;
        }
        transaction.execute("UPDATE items SET state=CASE state WHEN 'prepared' THEN 'cancelled' ELSE 'unknown' END,
            message='Interrupted; not retried automatically.' WHERE state IN ('prepared','in_flight')
            AND operation IN (SELECT id FROM operations WHERE state='active')", [])?;
        transaction.execute(
            "UPDATE operations SET state='interrupted' WHERE state='active'",
            [],
        )?;
        transaction.commit()?;
        Ok(Self { connection, owner })
    }

    /// Records permanent-delete intent only. This does not confirm or execute a deletion.
    pub fn prepare_permanent(&mut self, plan: &Plan) -> Result<OperationId> {
        if plan.files.is_empty() || plan.files.len() > 500 {
            return Err(Error::Invalid("Invalid cleanup plan size.".into()));
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO operations(rule,policy,created,state) VALUES(?1,'permanent',?2,'active')",
            params![plan.preview.rule, integer(super::rules::now()?)?],
        )?;
        let operation = transaction.last_insert_rowid();
        for file in &plan.files {
            let modified = file
                .stamp
                .modified
                .duration_since(UNIX_EPOCH)
                .map_err(|_| {
                    Error::Invalid("Cleanup timestamp is before the Unix epoch.".into())
                })?;
            let identity: Vec<u8> = file
                .stamp
                .identity
                .0
                .to_le_bytes()
                .into_iter()
                .chain(file.stamp.identity.1.to_le_bytes())
                .collect();
            let path = file.path.to_str().ok_or_else(|| {
                Error::Invalid("Cleanup path cannot be represented exactly.".into())
            })?;
            transaction.execute("INSERT INTO items(operation,node,path,identity,change_time,size,modified_seconds,modified_nanos,state)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'prepared')",
                params![operation, file.id.0, path, identity, file.stamp.change_time.to_le_bytes().as_slice(),
                    integer(file.stamp.size)?, integer(modified.as_secs())?, modified.subsec_nanos()])?;
        }
        transaction.commit()?;
        Ok(OperationId(operation, self.owner))
    }

    /// Must commit successfully before the native filesystem operation starts.
    pub fn start_item(&mut self, operation: OperationId, node: NodeId) -> Result<()> {
        self.transition(operation, node, "prepared", "in_flight", None)
    }

    /// If this commit fails after deletion, stop execution; reopening preserves an unknown outcome.
    pub fn complete_item(
        &mut self,
        operation: OperationId,
        node: NodeId,
        outcome: Outcome,
    ) -> Result<()> {
        match outcome {
            Outcome::Succeeded => self.transition(operation, node, "in_flight", "succeeded", None),
            Outcome::Failed(message) => {
                self.transition(operation, node, "in_flight", "failed", Some(&message))
            }
        }
    }

    pub fn cancel_pending(&mut self, operation: OperationId) -> Result<()> {
        self.check_owner(operation)?;
        let transaction = self.connection.transaction()?;
        active(&transaction, operation)?;
        transaction.execute(
            "UPDATE items SET state='cancelled' WHERE operation=?1 AND state='prepared'",
            [operation.0],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn finish(&mut self, operation: OperationId) -> Result<()> {
        self.check_owner(operation)?;
        let transaction = self.connection.transaction()?;
        active(&transaction, operation)?;
        let pending: i64 = transaction.query_row(
            "SELECT count(*) FROM items WHERE operation=?1 AND state IN ('prepared','in_flight')",
            [operation.0],
            |row| row.get(0),
        )?;
        if pending != 0 {
            return Err(Error::Invalid("Cleanup still has unfinished items.".into()));
        }
        transaction.execute(
            "UPDATE operations SET state='finished' WHERE id=?1",
            [operation.0],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn items(&self, offset: usize, limit: usize) -> Result<Vec<Item>> {
        let mut statement = self.connection.prepare(
            "SELECT i.operation,i.node,i.path,i.state,i.message,o.created FROM items i
            JOIN operations o ON o.id=i.operation
            ORDER BY i.operation DESC,i.node LIMIT ?1 OFFSET ?2",
        )?;
        let rows = statement.query_map(
            params![
                limit.clamp(1, 100) as i64,
                i64::try_from(offset)
                    .map_err(|_| Error::Invalid("Invalid journal offset.".into()))?
            ],
            |row| {
                Ok(Item {
                    operation: row.get(0)?,
                    node: NodeId(row.get(1)?),
                    path: row.get(2)?,
                    state: row.get(3)?,
                    message: row.get(4)?,
                    created: row.get(5)?,
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    #[cfg(windows)]
    pub fn activity(&self, offset: usize, limit: usize) -> Result<Vec<ActivityItem>> {
        let mut statement = self.connection.prepare(
            "SELECT 0 AS kind,i.operation AS operation,i.node AS node,i.path AS path,
                i.state AS state,i.message AS message,o.created AS created
             FROM items i JOIN operations o ON o.id=i.operation
             UNION ALL
             SELECT 1,r.id,r.node,r.path,r.state,r.message,r.created FROM relocations r
             ORDER BY created DESC,kind DESC,operation DESC,node ASC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = statement.query_map(
            params![
                limit.clamp(1, 100) as i64,
                i64::try_from(offset)
                    .map_err(|_| Error::Invalid("Invalid activity offset.".into()))?
            ],
            |row| {
                Ok(ActivityItem {
                    kind: if row.get::<_, bool>(0)? {
                        ActivityKind::Recycling
                    } else {
                        ActivityKind::Permanent
                    },
                    item: Item {
                        operation: row.get(1)?,
                        node: NodeId(row.get(2)?),
                        path: row.get(3)?,
                        state: row.get(4)?,
                        message: row.get(5)?,
                        created: row.get(6)?,
                    },
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    fn transition(
        &mut self,
        operation: OperationId,
        node: NodeId,
        before: &str,
        after: &str,
        message: Option<&str>,
    ) -> Result<()> {
        self.check_owner(operation)?;
        let transaction = self.connection.transaction()?;
        active(&transaction, operation)?;
        let changed = transaction.execute(
            "UPDATE items SET state=?1,message=?2 WHERE operation=?3 AND node=?4 AND state=?5",
            params![after, message, operation.0, node.0, before],
        )?;
        if changed != 1 {
            return Err(Error::Invalid(
                "Cleanup item cannot repeat or change this transition.".into(),
            ));
        }
        transaction.commit()?;
        Ok(())
    }

    fn check_owner(&self, operation: OperationId) -> Result<()> {
        if operation.1 != self.owner {
            return Err(Error::Invalid(
                "Cleanup operation belongs to another journal session.".into(),
            ));
        }
        Ok(())
    }
}

fn active(connection: &Connection, operation: OperationId) -> Result<()> {
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM operations WHERE id=?1 AND state='active'",
        [operation.0],
        |row| row.get(0),
    )?;
    if count != 1 {
        return Err(Error::Invalid(
            "Cleanup operation is no longer active.".into(),
        ));
    }
    Ok(())
}

fn integer(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::Invalid("Cleanup journal integer overflow.".into()))
}

#[cfg(all(test, windows))]
mod tests;
