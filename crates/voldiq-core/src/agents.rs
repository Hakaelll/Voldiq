//! Human provisioning is separate from agent authentication and per-call authorization.
mod scope;
pub(crate) use scope::pin_directory;

use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
pub use scope::AuthorizedPath;
use scope::Root;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Metadata,
    Scan,
    ReadContent,
    PreparePlans,
    RequestApproval,
    ExecuteApproved,
    Export,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub page_size: usize,
    pub scan_entries: usize,
    pub plan_files: usize,
    pub plan_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            page_size: 100,
            scan_entries: 1_000_000,
            plan_files: 100,
            plan_bytes: 10_000_000_000,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    roots: Vec<Root>,
    exclusions: Vec<Root>,
    capabilities: Vec<Capability>,
    limits: Limits,
}

impl Policy {
    pub fn new(
        roots: Vec<PathBuf>,
        exclusions: Vec<PathBuf>,
        capabilities: Vec<Capability>,
        limits: Limits,
    ) -> Result<Self> {
        let policy = Self {
            roots: roots
                .iter()
                .map(|path| Root::capture(path))
                .collect::<Result<_>>()?,
            exclusions: exclusions
                .iter()
                .map(|path| Root::capture(path))
                .collect::<Result<_>>()?,
            capabilities,
            limits,
        };
        policy.validate()?;
        Ok(policy)
    }

    fn validate(&self) -> Result<()> {
        if self.roots.is_empty()
            || self.roots.len() > 32
            || self.exclusions.len() > 256
            || self.capabilities.is_empty()
            || self.capabilities.len() > 7
            || !(1..=500).contains(&self.limits.page_size)
            || !(1..=5_000_000).contains(&self.limits.scan_entries)
            || !(1..=500).contains(&self.limits.plan_files)
            || self.limits.plan_bytes == 0
        {
            return Err(Error::Invalid(
                "Agent policy requires explicit roots, capabilities and bounded limits.".into(),
            ));
        }
        for (position, root) in self.roots.iter().enumerate() {
            root.validate()?;
            if self
                .roots
                .iter()
                .skip(position + 1)
                .any(|other| root.contains(&other.path) || other.contains(&root.path))
            {
                return Err(Error::Invalid("Agent roots must not overlap.".into()));
            }
        }
        for excluded in &self.exclusions {
            excluded.validate()?;
            if !self.roots.iter().any(|root| root.contains(&excluded.path)) {
                return Err(Error::Invalid(
                    "Agent exclusions must belong to an allowed root.".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }
}

// Deliberately neither Debug nor Serialize: only the human provisioning response reveals it.
pub struct Credential(String);

impl Credential {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

pub struct Provisioned {
    pub id: i64,
    pub credential: Credential,
}

#[derive(Clone)]
pub struct Principal {
    id: i64,
    credential_hash: String,
    record_activity: bool,
}

impl Principal {
    pub fn id(&self) -> i64 {
        self.id
    }
}

#[derive(Serialize)]
pub struct ConnectionInfo {
    pub id: i64,
    pub name: String,
    pub policy: Policy,
    pub revoked: bool,
    pub created: i64,
    pub last_used: Option<i64>,
}

pub struct Registry {
    connection: Connection,
    directory: AuthorizedPath,
}

impl Registry {
    pub fn plan_context<'a>(
        &'a self,
        principal: &'a Principal,
        capability: Capability,
    ) -> Result<PlanContext<'a>> {
        self.policy(principal, capability)?;
        Ok(PlanContext {
            registry: self,
            principal,
            capability,
        })
    }

    pub fn open(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let directory = pin_directory(&std::fs::canonicalize(parent)?)?;
        let name = path.file_name().ok_or_else(denied)?;
        let mut connection = Connection::open(directory.path().join(name))?;
        crate::database::configure(&connection)?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: u32 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version == 0 {
            tx.execute_batch("CREATE TABLE agents(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                credential_hash TEXT NOT NULL UNIQUE, policy TEXT NOT NULL, revoked INTEGER NOT NULL DEFAULT 0,
                created INTEGER NOT NULL, last_used INTEGER);
                PRAGMA user_version=1;")?;
        } else if version != 1 {
            return Err(Error::Invalid("Unsupported agent registry schema.".into()));
        }
        tx.commit()?;
        Ok(Self {
            connection,
            directory,
        })
    }

    /// Call only from the human desktop channel, never from CLI/MCP tool dispatch.
    pub fn provision(&mut self, name: &str, policy: Policy) -> Result<Provisioned> {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err(Error::Invalid(
                "Connection name must contain 1–128 bytes without control characters.".into(),
            ));
        }
        policy.validate()?;
        let credential = credential()?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let count: i64 = tx.query_row("SELECT count(*) FROM agents", [], |row| row.get(0))?;
        if count >= 500 {
            return Err(Error::Invalid("Connection registry limit reached.".into()));
        }
        tx.execute(
            "INSERT INTO agents(name,credential_hash,policy,created) VALUES(?1,?2,?3,?4)",
            params![
                name,
                digest(credential.expose()),
                serde_json::to_string(&policy).map_err(|_| denied())?,
                now()?
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Provisioned { id, credential })
    }

    pub fn authenticate(&mut self, credential: &str) -> Result<Principal> {
        if credential.len() != 64 || !credential.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(denied());
        }
        let hash = digest(credential);
        let id = self
            .connection
            .query_row(
                "SELECT id FROM agents WHERE credential_hash=?1 AND revoked=0",
                [&hash],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(denied)?;
        self.connection.execute(
            "UPDATE agents SET last_used=?1 WHERE id=?2",
            params![now()?, id],
        )?;
        Ok(Principal {
            id,
            credential_hash: hash,
            record_activity: true,
        })
    }

    /// Re-reads credential and policy on every invocation, including already connected clients.
    pub fn policy(&self, principal: &Principal, capability: Capability) -> Result<Policy> {
        let json: String = self
            .connection
            .query_row(
                "SELECT policy FROM agents WHERE id=?1 AND credential_hash=?2 AND revoked=0",
                params![principal.id, principal.credential_hash],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(denied)?;
        if json.len() > 262_144 {
            return Err(denied());
        }
        let policy: Policy = serde_json::from_str(&json).map_err(|_| denied())?;
        policy.validate().map_err(|_| denied())?;
        if !policy.capabilities.contains(&capability) {
            return Err(denied());
        }
        if principal.record_activity {
            self.connection.execute(
                "UPDATE agents SET last_used=?1 WHERE id=?2",
                params![now()?, principal.id],
            )?;
        }
        Ok(policy)
    }

    pub(crate) fn human_principal(&self, id: i64) -> Result<Principal> {
        let hash = self
            .connection
            .query_row(
                "SELECT credential_hash FROM agents WHERE id=?1 AND revoked=0",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(denied)?;
        Ok(Principal {
            id,
            credential_hash: hash,
            record_activity: false,
        })
    }

    pub fn authorize(
        &self,
        principal: &Principal,
        capability: Capability,
        path: &Path,
    ) -> Result<AuthorizedPath> {
        let policy = self.policy(principal, capability)?;
        scope::authorize(&policy.roots, &policy.exclusions, path).map_err(|_| denied())
    }

    pub fn scan_permit(&self, principal: &Principal, path: &Path) -> Result<ScanPermit> {
        let policy = self.policy(principal, Capability::Scan)?;
        let root =
            scope::authorize(&policy.roots, &policy.exclusions, path).map_err(|_| denied())?;
        let mut exclusions = Vec::new();
        for excluded in &policy.exclusions {
            if excluded.path.starts_with(root.path()) {
                exclusions.push(excluded.reopen().map_err(|_| denied())?);
            }
        }
        let options = crate::ScanOptions {
            exclusions: exclusions
                .iter()
                .map(|path| path.path().to_string_lossy().into_owned())
                .collect(),
            max_entries: Some(policy.limits.scan_entries),
            concurrency: crate::ScanConcurrency::automatic(),
            ..Default::default()
        };
        Ok(ScanPermit {
            root,
            options,
            _exclusions: exclusions,
        })
    }

    pub fn authorize_index(
        &self,
        principal: &Principal,
        index: &crate::index::Index,
    ) -> Result<AuthorizedPath> {
        self.authorize_snapshot_scope(
            principal,
            &index.root,
            index.root_identity.ok_or_else(denied)?,
            &index.exclusions,
        )
    }

    pub fn authorize_snapshot(
        &self,
        principal: &Principal,
        snapshot: &crate::snapshots::Scope,
    ) -> Result<AuthorizedPath> {
        self.authorize_snapshot_scope(
            principal,
            &snapshot.root,
            snapshot.identity,
            &snapshot.exclusions,
        )
    }

    fn authorize_snapshot_scope(
        &self,
        principal: &Principal,
        path: &Path,
        identity: (u64, u64),
        exclusions: &[PathBuf],
    ) -> Result<AuthorizedPath> {
        let policy = self.policy(principal, Capability::Metadata)?;
        let root = scope::authorize(
            &policy.roots,
            &policy.exclusions,
            &scope::ordinary_path(path)?,
        )
        .map_err(|_| denied())?;
        if root.identity() != identity {
            return Err(denied());
        }
        for excluded in &policy.exclusions {
            if excluded.path.starts_with(root.path()) {
                excluded.reopen().map_err(|_| denied())?;
                let covered = exclusions.iter().any(|path| {
                    scope::ordinary_path(path).is_ok_and(|path| excluded.path.starts_with(path))
                });
                if !covered {
                    return Err(denied());
                }
            }
        }
        Ok(root)
    }

    pub fn revoke(&mut self, id: i64) -> Result<()> {
        if self
            .connection
            .execute("UPDATE agents SET revoked=1 WHERE id=?1", [id])?
            != 1
        {
            return Err(denied());
        }
        Ok(())
    }

    pub fn rotate(&mut self, id: i64) -> Result<Credential> {
        let credential = credential()?;
        if self.connection.execute(
            "UPDATE agents SET credential_hash=?1 WHERE id=?2 AND revoked=0",
            params![digest(credential.expose()), id],
        )? != 1
        {
            return Err(denied());
        }
        Ok(credential)
    }

    /// Human management listing. Never expose complete policies through an agent tool.
    pub fn list(&self) -> Result<Vec<ConnectionInfo>> {
        let mut statement = self.connection.prepare(
            "SELECT id,name,policy,revoked,created,last_used FROM agents ORDER BY id LIMIT 501",
        )?;
        let mut rows = statement.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            if items.len() == 500 {
                return Err(Error::Invalid("Connection registry limit reached.".into()));
            }
            let json: String = row.get(2)?;
            items.push(ConnectionInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                policy: serde_json::from_str(&json).map_err(|_| denied())?,
                revoked: row.get(3)?,
                created: row.get(4)?,
                last_used: row.get(5)?,
            });
        }
        Ok(items)
    }
}

/// Opaque per-call authority. Every use rechecks the current credential and policy.
pub struct PlanContext<'a> {
    registry: &'a Registry,
    principal: &'a Principal,
    capability: Capability,
}

impl PlanContext<'_> {
    pub(crate) fn check_directory(&self, identity: (u64, u64)) -> Result<()> {
        if self.registry.directory.identity() != identity {
            return Err(denied());
        }
        Ok(())
    }
    pub(crate) fn check(&self, capability: Capability) -> Result<Policy> {
        if capability != self.capability {
            return Err(denied());
        }
        self.registry.policy(self.principal, capability)
    }

    pub(crate) fn owner(&self) -> i64 {
        self.principal.id
    }

    pub(crate) fn binding(&self) -> Result<String> {
        let policy = self.registry.policy(self.principal, self.capability)?;
        let key = blake3::hash(self.principal.credential_hash.as_bytes());
        let bytes = serde_json::to_vec(&policy).map_err(|_| denied())?;
        Ok(blake3::keyed_hash(key.as_bytes(), &bytes)
            .to_hex()
            .to_string())
    }

    pub(crate) fn snapshot(&self, scope: &crate::snapshots::Scope) -> Result<AuthorizedPath> {
        self.registry.authorize_snapshot(self.principal, scope)
    }

    pub(crate) fn path(&self, path: &Path) -> Result<AuthorizedPath> {
        self.registry.authorize(
            self.principal,
            self.capability,
            &scope::ordinary_path(path)?,
        )
    }
}

pub struct ScanPermit {
    pub root: AuthorizedPath,
    pub options: crate::ScanOptions,
    _exclusions: Vec<AuthorizedPath>,
}

fn credential() -> Result<Credential> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Invalid("OS credential generation failed.".into()))?;
    Ok(Credential(
        bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
    ))
}

fn digest(value: &str) -> String {
    blake3::hash(value.as_bytes()).to_hex().to_string()
}

fn now() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_secs()).ok())
        .ok_or_else(denied)
}

pub(crate) fn denied() -> Error {
    Error::Invalid("Not authorized or unavailable in this connection's scope.".into())
}
