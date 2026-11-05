//! Durable observations, never approval or permission to resume file operations.
use super::{Action, HumanAuthority, Manifest, State, Store, invalid};
use crate::{NodeId, Result};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

const MANIFEST_LIMIT: usize = 440 * 1024;
const FILE_COLUMNS: &str =
    "node,output_name,phase,private_name,destination_identity,content_blake3,
    verified_bytes,verified_at,published_at,updated_at,interrupted_at,
    (length(output_name)<=1024 AND length(phase)<=32 AND coalesce(length(private_name),0)<=64
    AND coalesce(length(destination_identity),0)<=48 AND coalesce(length(content_blake3),0)<=64
    AND coalesce(length(verified_bytes),0)<=20)";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferPhase {
    Prepared,
    Private,
    Verified,
    PublishPending,
    Published,
    OriginalPending,
    Kept,
    Recycled,
    Failed,
    Cancelled,
}
impl TransferPhase {
    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "prepared" => Self::Prepared,
            "private" => Self::Private,
            "verified" => Self::Verified,
            "publish_pending" => Self::PublishPending,
            "published" => Self::Published,
            "original_pending" => Self::OriginalPending,
            "kept" => Self::Kept,
            "recycled" => Self::Recycled,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return Err(invalid()),
        })
    }
    #[cfg(windows)]
    fn name(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Private => "private",
            Self::Verified => "verified",
            Self::PublishPending => "publish_pending",
            Self::Published => "published",
            Self::OriginalPending => "original_pending",
            Self::Kept => "kept",
            Self::Recycled => "recycled",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Native-worker bookkeeping only. No deserialization or agent-facing update endpoint.
/// Recording evidence cannot replace retained-handle verification or grant source authority.
pub enum TransferUpdate {
    Private { name: String, identity: String },
    Verified { bytes: u64, blake3: String },
    PublishIntent,
    Published,
    OriginalIntent,
    Kept,
    Recycled,
    Failed,
    Cancelled,
}

#[derive(Debug, Serialize)]
pub struct TransferFile {
    pub node: NodeId,
    pub output_name: String,
    pub phase: TransferPhase,
    pub private_name: Option<String>,
    pub destination_identity: Option<String>,
    pub content_blake3: Option<String>,
    pub verified_bytes: Option<String>,
    pub verified_at: Option<i64>,
    pub published_at: Option<i64>,
    pub updated_at: i64,
    pub interrupted_at: Option<i64>,
}

#[derive(Serialize)]
pub struct TransferCatalogue {
    pub manifest: Manifest,
    pub manifest_hash: String,
    pub state: State,
    pub last_observed_at: u64,
    pub files: Vec<TransferFile>,
    pub next_after: Option<NodeId>,
    pub currently_connected: Option<bool>,
    pub restore_available: bool,
    pub limitations: &'static str,
}

pub(super) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    let (count,size,hash_size): (i64,i64,i64) = tx.query_row("SELECT count(*),coalesce(max(length(manifest)),0),coalesce(max(length(hash)),0) FROM agent_plans", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?;
    if count > 10_000 || size > MANIFEST_LIMIT as i64 || hash_size > 64 {
        return Err(invalid());
    }
    tx.execute_batch("CREATE TABLE transfer_operations(
        plan_id TEXT PRIMARY KEY REFERENCES agent_plans(id),owner INTEGER NOT NULL,
        hash TEXT NOT NULL CHECK(length(hash)=64),last_observed_at INTEGER NOT NULL CHECK(last_observed_at>=0));
        CREATE TABLE transfer_files(
        plan_id TEXT NOT NULL REFERENCES transfer_operations(plan_id),node INTEGER NOT NULL,
        output_name TEXT NOT NULL,phase TEXT NOT NULL CHECK(phase IN
        ('prepared','private','verified','publish_pending','published','original_pending','kept','recycled','failed','cancelled')),
        private_name TEXT,destination_identity TEXT,content_blake3 TEXT,verified_bytes TEXT,
        verified_at INTEGER,published_at INTEGER,updated_at INTEGER NOT NULL,interrupted_at INTEGER,
        PRIMARY KEY(plan_id,node));
        PRAGMA user_version=3;")?;
    let mut statement = tx.prepare("SELECT manifest,hash FROM agent_plans WHERE length(manifest)<=?1 ORDER BY rowid LIMIT 10001")?;
    let mut rows = statement.query([MANIFEST_LIMIT as i64])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        count += 1;
        if count > 10_000 {
            return Err(invalid());
        }
        let json: String = row.get(0)?;
        let hash: String = row.get(1)?;
        if super::digest(json.as_bytes()) != hash {
            return Err(invalid());
        }
        let manifest: Manifest = serde_json::from_str(&json).map_err(|_| invalid())?;
        prepare(tx, &manifest, &hash)?;
    }
    Ok(())
}

pub(super) fn prepare(tx: &Transaction<'_>, manifest: &Manifest, hash: &str) -> Result<()> {
    let Action::Offload(_) = &manifest.action else {
        return Ok(());
    };
    let offload = validate_manifest(manifest, hash)?;
    let observed = i64::try_from(offload.destination.observed_at).map_err(|_| invalid())?;
    tx.execute(
        "INSERT INTO transfer_operations(plan_id,owner,hash,last_observed_at) VALUES(?1,?2,?3,?4)",
        params![manifest.id, manifest.connection_id, hash, observed],
    )?;
    for output in &offload.outputs {
        tx.execute("INSERT INTO transfer_files(plan_id,node,output_name,phase,updated_at) VALUES(?1,?2,?3,'prepared',?4)",
            params![manifest.id, output.id.0, output.name, manifest.created])?;
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn apply_update(
    tx: &Transaction<'_>,
    manifest: &Manifest,
    hash: &str,
    node: NodeId,
    update: TransferUpdate,
    at: i64,
) -> Result<()> {
    let Action::Offload(offload) = &manifest.action else {
        return Err(invalid());
    };
    let entry = manifest
        .entries
        .iter()
        .find(|entry| entry.id == node)
        .ok_or_else(invalid)?;
    let mut file = tx.query_row(
        &format!(
            "SELECT {FILE_COLUMNS} FROM transfer_files f
        WHERE plan_id=?1 AND node=?2 AND EXISTS(SELECT 1 FROM transfer_operations o
        WHERE o.plan_id=f.plan_id AND o.owner=?3 AND o.hash=?4)"
        ),
        params![manifest.id, node.0, manifest.connection_id, hash],
        read_file,
    )?;
    if file.interrupted_at.is_some() || at < file.updated_at || at < manifest.created {
        return Err(invalid());
    }
    let previous = file.phase;
    validate_file(&file, manifest)?;
    file.phase = match update {
        TransferUpdate::Private { name, identity } if previous == TransferPhase::Prepared => {
            if !private_name(&name)
                || !hex(&identity, 48)
                || !hex(&offload.destination.native_identity, 48)
                || !identity.starts_with(&offload.destination.native_identity[..16])
            {
                return Err(invalid());
            }
            file.private_name = Some(name);
            file.destination_identity = Some(identity);
            TransferPhase::Private
        }
        TransferUpdate::Verified { bytes, blake3 } if previous == TransferPhase::Private => {
            if !hex(&blake3, 64) || bytes.to_string() != entry.logical_bytes {
                return Err(invalid());
            }
            file.content_blake3 = Some(blake3);
            file.verified_bytes = Some(bytes.to_string());
            file.verified_at = Some(at);
            TransferPhase::Verified
        }
        TransferUpdate::PublishIntent if previous == TransferPhase::Verified => {
            TransferPhase::PublishPending
        }
        TransferUpdate::Published if previous == TransferPhase::PublishPending => {
            file.published_at = Some(at);
            TransferPhase::Published
        }
        TransferUpdate::Kept
            if previous == TransferPhase::Published
                && offload.originals == super::Originals::Keep =>
        {
            TransferPhase::Kept
        }
        TransferUpdate::OriginalIntent
            if previous == TransferPhase::Published
                && offload.originals == super::Originals::RecycleAfterVerification =>
        {
            let missing: i64 = tx.query_row(
                "SELECT count(*) FROM transfer_files WHERE plan_id=?1 AND published_at IS NULL",
                [&manifest.id],
                |row| row.get(0),
            )?;
            if missing != 0 {
                return Err(invalid());
            }
            TransferPhase::OriginalPending
        }
        TransferUpdate::Recycled if previous == TransferPhase::OriginalPending => {
            TransferPhase::Recycled
        }
        TransferUpdate::Failed
            if matches!(
                previous,
                TransferPhase::Prepared
                    | TransferPhase::Private
                    | TransferPhase::Verified
                    | TransferPhase::Published
            ) =>
        {
            TransferPhase::Failed
        }
        TransferUpdate::Cancelled
            if matches!(
                previous,
                TransferPhase::Prepared
                    | TransferPhase::Private
                    | TransferPhase::Verified
                    | TransferPhase::Published
            ) =>
        {
            TransferPhase::Cancelled
        }
        _ => return Err(invalid()),
    };
    if tx.execute("UPDATE transfer_files SET phase=?1,private_name=?2,destination_identity=?3,content_blake3=?4,
        verified_bytes=?5,verified_at=?6,published_at=?7,updated_at=?8
        WHERE plan_id=?9 AND node=?10 AND phase=?11 AND interrupted_at IS NULL AND updated_at=?12",
        params![file.phase.name(),file.private_name,file.destination_identity,file.content_blake3,
        file.verified_bytes,file.verified_at,file.published_at,at,manifest.id,node.0,previous.name(),file.updated_at])? != 1 { return Err(invalid()); }
    Ok(())
}

pub(super) fn interrupt(tx: &Transaction<'_>, id: &str, at: i64) -> Result<()> {
    let future: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM transfer_files WHERE plan_id=?1 AND updated_at>?2)",
        params![id, at],
        |row| row.get(0),
    )?;
    if future {
        return Err(invalid());
    }
    tx.execute(
        "UPDATE transfer_files SET interrupted_at=?1 WHERE plan_id=?2 AND interrupted_at IS NULL
        AND phase NOT IN ('kept','recycled','failed','cancelled')",
        params![at, id],
    )?;
    Ok(())
}

pub(super) fn completion_state(
    tx: &Transaction<'_>,
    id: &str,
    owner: i64,
    hash: &str,
    requested: State,
    at: i64,
) -> Result<State> {
    let json: String = tx.query_row("SELECT manifest FROM agent_plans WHERE id=?1 AND owner=?2 AND hash=?3 AND length(manifest)<=?4",
        params![id,owner,hash,MANIFEST_LIMIT as i64], |row| row.get(0))?;
    if super::digest(json.as_bytes()) != hash {
        return Err(invalid());
    }
    let manifest: Manifest = serde_json::from_str(&json).map_err(|_| invalid())?;
    let Action::Offload(offload) = &manifest.action else {
        return Ok(requested);
    };
    validate_manifest(&manifest, hash)?;
    let expected = match offload.originals {
        super::Originals::Keep => "kept",
        super::Originals::RecycleAfterVerification => "recycled",
    };
    let (total,complete,published): (i64,i64,i64) = tx.query_row("SELECT count(*),coalesce(sum(phase=?1 AND interrupted_at IS NULL),0),coalesce(sum(published_at IS NOT NULL),0)
        FROM transfer_files f WHERE plan_id=?2 AND EXISTS(SELECT 1 FROM transfer_operations o WHERE o.plan_id=f.plan_id AND o.hash=?3 AND o.owner=?4)",
        params![expected,id,hash,owner],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?;
    if total != manifest.entries.len() as i64
        || total == 0
        || (requested == State::Completed && complete != total)
    {
        return Err(invalid());
    }
    if requested != State::Completed {
        interrupt(tx, id, at)?;
    }
    Ok(if published > 0 && requested != State::Completed {
        State::PartiallyCompleted
    } else {
        requested
    })
}

impl Store {
    /// Human-only historical read. It deliberately does not reopen a potentially disconnected volume.
    pub(super) fn transfer_catalogue(
        &self,
        human: &HumanAuthority,
        id: &str,
        after: Option<NodeId>,
        limit: usize,
    ) -> Result<TransferCatalogue> {
        human.check_directory(self.directory.identity())?;
        if !hex(id, 64) || !(1..=100).contains(&limit) {
            return Err(invalid());
        }
        let tx = self.connection.unchecked_transaction()?;
        let (json,hash,state,observed): (String,String,String,i64) = tx.query_row("SELECT p.manifest,p.hash,p.state,o.last_observed_at
            FROM agent_plans p JOIN transfer_operations o ON o.plan_id=p.id AND o.owner=p.owner AND o.hash=p.hash
            WHERE p.id=?1 AND length(p.manifest)<=?2 AND length(p.hash)=64 AND length(p.state)<=32",params![id,MANIFEST_LIMIT as i64],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()?.ok_or_else(invalid)?;
        if super::digest(json.as_bytes()) != hash {
            return Err(invalid());
        }
        let manifest: Manifest = serde_json::from_str(&json).map_err(|_| invalid())?;
        validate_manifest(&manifest, &hash)?;
        if manifest.id != id {
            return Err(invalid());
        }
        let total: i64 = tx.query_row(
            "SELECT count(*) FROM transfer_files WHERE plan_id=?1",
            [id],
            |row| row.get(0),
        )?;
        if total != manifest.entries.len() as i64 {
            return Err(invalid());
        }
        let mut statement = tx.prepare(&format!(
            "SELECT {FILE_COLUMNS} FROM transfer_files
            WHERE plan_id=?1 AND (?2 IS NULL OR node>?2) ORDER BY node LIMIT ?3"
        ))?;
        let mut files = statement
            .query_map(
                params![id, after.map(|node| node.0), (limit + 1) as i64],
                read_file,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let next_after = if files.len() > limit {
            files.truncate(limit);
            files.last().map(|file| file.node)
        } else {
            None
        };
        for file in &files {
            validate_file(file, &manifest)?;
        }
        drop(statement);
        tx.commit()?;
        human.check_directory(self.directory.identity())?;
        Ok(TransferCatalogue {
            manifest,
            manifest_hash: hash,
            state: serde_json::from_value(serde_json::Value::String(state))
                .map_err(|_| invalid())?,
            last_observed_at: u64::try_from(observed).map_err(|_| invalid())?,
            files,
            next_after,
            currently_connected: None,
            restore_available: false,
            limitations: "Recorded observations, not current content or connection evidence. Interruption retains the last recorded phase; pending publication or original handling may have occurred. No automatic retry, pathname cleanup, restoration or file-operation authority. Metadata preservation depends on the captured profile; restoration is not available yet.",
        })
    }
}

fn read_file(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransferFile> {
    if !row.get::<_, bool>(11)? {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let phase: String = row.get(2)?;
    let phase = TransferPhase::parse(&phase).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(TransferFile {
        node: NodeId(row.get(0)?),
        output_name: row.get(1)?,
        phase,
        private_name: row.get(3)?,
        destination_identity: row.get(4)?,
        content_blake3: row.get(5)?,
        verified_bytes: row.get(6)?,
        verified_at: row.get(7)?,
        published_at: row.get(8)?,
        updated_at: row.get(9)?,
        interrupted_at: row.get(10)?,
    })
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn private_name(name: &str) -> bool {
    name.strip_prefix(".voldiq-output-")
        .and_then(|name| name.strip_suffix(".tmp"))
        .is_some_and(|suffix| hex(suffix, 32))
}

fn validate_file(file: &TransferFile, manifest: &Manifest) -> Result<()> {
    let Action::Offload(offload) = &manifest.action else {
        return Err(invalid());
    };
    let entry = manifest
        .entries
        .iter()
        .find(|entry| entry.id == file.node)
        .ok_or_else(invalid)?;
    if !offload
        .outputs
        .iter()
        .any(|output| output.id == file.node && output.name == file.output_name)
        || file.updated_at < manifest.created
        || file.interrupted_at.is_some_and(|at| at < file.updated_at)
        || file
            .verified_at
            .is_some_and(|at| at < manifest.created || at > file.updated_at)
        || file
            .published_at
            .is_some_and(|at| at < file.verified_at.unwrap_or(i64::MAX) || at > file.updated_at)
        || file
            .private_name
            .as_ref()
            .is_some_and(|name| !private_name(name))
        || file.destination_identity.as_ref().is_some_and(|id| {
            !hex(id, 48)
                || !hex(&offload.destination.native_identity, 48)
                || !id.starts_with(&offload.destination.native_identity[..16])
        })
        || file
            .content_blake3
            .as_ref()
            .is_some_and(|digest| !hex(digest, 64))
        || file
            .verified_bytes
            .as_ref()
            .is_some_and(|bytes| *bytes != entry.logical_bytes)
    {
        return Err(invalid());
    }
    let native = file.private_name.is_some() && file.destination_identity.is_some();
    let verified = file.verified_bytes.is_some()
        && file.content_blake3.is_some()
        && file.verified_at.is_some();
    let published = file.published_at.is_some();
    if file.private_name.is_some() != native
        || file.destination_identity.is_some() != native
        || file.verified_bytes.is_some() != verified
        || file.content_blake3.is_some() != verified
        || file.verified_at.is_some() != verified
        || (verified && !native)
        || (published && !verified)
    {
        return Err(invalid());
    }
    let valid = match file.phase {
        TransferPhase::Prepared => !native && !verified && !published,
        TransferPhase::Private => native && !verified && !published,
        TransferPhase::Verified | TransferPhase::PublishPending => verified && !published,
        TransferPhase::Published
        | TransferPhase::OriginalPending
        | TransferPhase::Kept
        | TransferPhase::Recycled => published,
        TransferPhase::Failed | TransferPhase::Cancelled => true,
    };
    if !valid
        || (file.phase == TransferPhase::Kept && offload.originals != super::Originals::Keep)
        || (matches!(
            file.phase,
            TransferPhase::OriginalPending | TransferPhase::Recycled
        ) && offload.originals != super::Originals::RecycleAfterVerification)
        || (file.interrupted_at.is_some()
            && matches!(
                file.phase,
                TransferPhase::Kept
                    | TransferPhase::Recycled
                    | TransferPhase::Failed
                    | TransferPhase::Cancelled
            ))
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_manifest<'a>(manifest: &'a Manifest, hash: &str) -> Result<&'a super::Offload> {
    let Action::Offload(offload) = &manifest.action else {
        return Err(invalid());
    };
    let json = serde_json::to_vec(manifest).map_err(|_| invalid())?;
    if json.len() > MANIFEST_LIMIT
        || !hex(hash, 64)
        || super::digest(&json) != hash
        || !hex(&manifest.id, 64)
        || manifest.schema_version != manifest.action.schema_version()
        || manifest.policy_version != super::POLICY_VERSION
        || manifest.connection_id <= 0
        || manifest.user != super::current_user()?
        || manifest.created < 0
        || manifest.created.checked_add(super::PLAN_TTL) != Some(manifest.expires)
        || manifest.entries.is_empty()
        || manifest.entries.len() > 500
        || !hex(&offload.destination.native_identity, 48)
    {
        return Err(invalid());
    }
    offload.check_shape(&manifest.entries)?;
    let (logical, reclaim) = super::store::totals(&manifest.entries)?;
    let reclaim = if offload.originals == super::Originals::Keep {
        0
    } else {
        reclaim
    };
    if manifest.logical_bytes != logical.to_string()
        || manifest.potential_reclaim_bytes != reclaim.to_string()
        || manifest.immediate_free_bytes != "0"
    {
        return Err(invalid());
    }
    Ok(offload)
}
