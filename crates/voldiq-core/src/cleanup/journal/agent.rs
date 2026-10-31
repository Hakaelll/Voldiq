use super::Journal;
use crate::{Error, NodeId, Result};
use rusqlite::{Connection, params};
use serde::Serialize;

pub(super) const MIGRATION: &str = "
CREATE TABLE agent_relocations(
 relocation INTEGER PRIMARY KEY REFERENCES relocations(id),
 plan_id TEXT NOT NULL CHECK(length(plan_id)=64), manifest_hash TEXT NOT NULL CHECK(length(manifest_hash)=64),
 connection_id INTEGER NOT NULL CHECK(connection_id>0), execution_nonce TEXT NOT NULL CHECK(length(execution_nonce)=64));
CREATE INDEX agent_relocations_plan ON agent_relocations(plan_id,manifest_hash);
PRAGMA user_version=5;";

/// Audit context only. The live approval and opaque execution ticket remain separate authority.
pub(crate) struct AgentExecutionRef<'a> {
    pub plan_id: &'a str,
    pub manifest_hash: &'a str,
    pub connection_id: i64,
    pub execution_nonce: &'a str,
    pub directory: (u64, u64),
}

#[derive(Serialize)]
pub struct AgentRelocation {
    pub operation: i64,
    pub node: NodeId,
    pub connection_id: i64,
    pub state: String,
}

impl Journal {
    /// Read-only receipts, not execution or restoration authority. Adapters must authorize the manifest first.
    pub fn agent_relocations(
        &self,
        plan_id: &str,
        manifest_hash: &str,
    ) -> Result<Vec<AgentRelocation>> {
        if !hex_id(plan_id) || !hex_id(manifest_hash) {
            return Err(invalid());
        }
        let mut statement = self.connection.prepare("SELECT a.relocation,r.node,a.connection_id,r.state FROM agent_relocations a JOIN relocations r ON r.id=a.relocation WHERE a.plan_id=?1 AND a.manifest_hash=?2 ORDER BY a.relocation LIMIT 501")?;
        let records = statement
            .query_map(params![plan_id, manifest_hash], |row| {
                Ok(AgentRelocation {
                    operation: row.get(0)?,
                    node: NodeId(row.get(1)?),
                    connection_id: row.get(2)?,
                    state: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if records.len() > 500 {
            return Err(invalid());
        }
        Ok(records)
    }
}

pub(super) fn bind(
    connection: &Connection,
    relocation: i64,
    origin: &AgentExecutionRef<'_>,
    directory: (u64, u64),
) -> Result<()> {
    if origin.directory != directory
        || origin.connection_id <= 0
        || !hex_id(origin.plan_id)
        || !hex_id(origin.manifest_hash)
        || !hex_id(origin.execution_nonce)
    {
        return Err(invalid());
    }
    connection.execute("INSERT INTO agent_relocations(relocation,plan_id,manifest_hash,connection_id,execution_nonce) VALUES(?1,?2,?3,?4,?5)",params![relocation,origin.plan_id,origin.manifest_hash,origin.connection_id,origin.execution_nonce])?;
    Ok(())
}

fn hex_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn invalid() -> Error {
    Error::Invalid("Invalid agent journal binding or receipt limit.".into())
}
