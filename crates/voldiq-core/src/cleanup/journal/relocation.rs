//! Durable admission/rollback evidence. Opening a journal never moves filesystem objects.
use super::{Journal, integer};
use crate::{
    Error, NodeId, Result,
    cleanup::{Plan, VerifiedFile},
    windows::cleanup_stage::{StageEntry, StageIdentity, StageName, identity},
};
use rusqlite::params;
use std::{
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(super) const SCHEMA: &str = "
CREATE TABLE relocations(
 id INTEGER PRIMARY KEY AUTOINCREMENT, node INTEGER NOT NULL, path TEXT NOT NULL,
 source_volume BLOB NOT NULL CHECK(length(source_volume)=8),
 source_file BLOB NOT NULL CHECK(length(source_file)=16),
 parent_file BLOB NOT NULL CHECK(length(parent_file)=16),
 change_time BLOB NOT NULL CHECK(length(change_time)=8), size INTEGER NOT NULL CHECK(size>=0),
 modified_seconds INTEGER NOT NULL, modified_nanos INTEGER NOT NULL,
 stage_name TEXT NOT NULL, stage_file BLOB CHECK(length(stage_file)=16),
 state TEXT NOT NULL CHECK(state IN ('prepared','stage_ready','relocating','staged','returning','returned','unmoved','unknown','cancelled')),
 message TEXT, created INTEGER NOT NULL,
 CHECK(state IN ('prepared','cancelled','unknown') OR stage_file IS NOT NULL),
 UNIQUE(source_volume,parent_file,stage_name));
CREATE UNIQUE INDEX live_relocation_source ON relocations(source_volume,source_file)
 WHERE state IN ('prepared','stage_ready','relocating','staged','returning','unknown');
PRAGMA user_version=2;";

pub(super) const RECYCLING_MIGRATION: &str = "
DROP INDEX live_relocation_source;
ALTER TABLE relocations RENAME TO relocations_v2;
CREATE TABLE relocations(
 id INTEGER PRIMARY KEY AUTOINCREMENT, node INTEGER NOT NULL, path TEXT NOT NULL,
 source_volume BLOB NOT NULL CHECK(length(source_volume)=8),
 source_file BLOB NOT NULL CHECK(length(source_file)=16),
 parent_file BLOB NOT NULL CHECK(length(parent_file)=16),
 change_time BLOB NOT NULL CHECK(length(change_time)=8), size INTEGER NOT NULL CHECK(size>=0),
 modified_seconds INTEGER NOT NULL, modified_nanos INTEGER NOT NULL,
 stage_name TEXT NOT NULL, stage_file BLOB CHECK(length(stage_file)=16),
 state TEXT NOT NULL CHECK(state IN ('prepared','stage_ready','relocating','staged','returning','returned','unmoved','unknown','cancelled','recycling','recycled','restoring','restored')),
 message TEXT, created INTEGER NOT NULL,
 bin_path BLOB CHECK(bin_path IS NULL OR (length(bin_path) BETWEEN 2 AND 65534 AND length(bin_path)%2=0)),
 restore_path BLOB CHECK(restore_path IS NULL OR (length(restore_path) BETWEEN 2 AND 65534 AND length(restore_path)%2=0)),
 CHECK(state IN ('prepared','cancelled','unknown') OR stage_file IS NOT NULL),
 CHECK(state NOT IN ('recycled','restoring','restored') OR bin_path IS NOT NULL),
 CHECK(state NOT IN ('restoring','restored') OR restore_path IS NOT NULL),
 UNIQUE(source_volume,parent_file,stage_name));
INSERT INTO relocations(id,node,path,source_volume,source_file,parent_file,change_time,size,modified_seconds,modified_nanos,stage_name,stage_file,state,message,created)
 SELECT id,node,path,source_volume,source_file,parent_file,change_time,size,modified_seconds,modified_nanos,stage_name,stage_file,state,message,created FROM relocations_v2;
INSERT INTO sqlite_sequence(name,seq) SELECT 'relocations',seq FROM sqlite_sequence WHERE name='relocations_v2'
 AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='relocations');
UPDATE sqlite_sequence SET seq=max(seq,coalesce((SELECT seq FROM sqlite_sequence WHERE name='relocations_v2'),0)) WHERE name='relocations';
DROP TABLE relocations_v2;
CREATE UNIQUE INDEX live_relocation_source ON relocations(source_volume,source_file)
 WHERE state IN ('prepared','stage_ready','relocating','staged','returning','unknown','recycling','restoring');
PRAGMA user_version=3;";

pub(super) const VERSION_MIGRATION: &str = "
ALTER TABLE relocations ADD COLUMN recycled_change_time BLOB
 CHECK(recycled_change_time IS NULL OR length(recycled_change_time)=8);
PRAGMA user_version=4;";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecycledVersion {
    pub change_time: u64,
    pub size: u64,
    pub modified: SystemTime,
}

#[derive(Clone, Copy, Debug)]
pub struct RelocationId(i64, u64);

impl RelocationId {
    /// Read-only receipt identifier. This does not convey authority to advance an operation.
    pub fn value(self) -> i64 {
        self.0
    }
}

#[derive(Debug)]
pub struct Record {
    pub id: i64,
    pub node: NodeId,
    pub path: String,
    pub source: StageIdentity,
    pub parent: StageIdentity,
    pub stage_name: String,
    pub stage_file: Option<[u8; 16]>,
    pub state: String,
    pub message: Option<String>,
    pub bin_path: Option<PathBuf>,
    pub restore_path: Option<PathBuf>,
    pub recycled_version: Option<RecycledVersion>,
}

impl Journal {
    pub fn cancel_relocation(&mut self, id: RelocationId) -> Result<()> {
        self.relocation_owner(id)?;
        let transaction = self.connection.transaction()?;
        let changed=transaction.execute("UPDATE relocations SET state='cancelled' WHERE id=?1 AND state IN ('prepared','stage_ready')",[id.0])?;
        require_one(changed)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn unresolved_relocation(&mut self, id: RelocationId, message: &str) -> Result<()> {
        self.relocation_owner(id)?;
        let transaction = self.connection.transaction()?;
        let changed=transaction.execute("UPDATE relocations SET state='unknown',message=?1 WHERE id=?2 AND state IN ('relocating','staged','returning','recycling','restoring')",params![message,id.0])?;
        require_one(changed)?;
        transaction.commit()?;
        Ok(())
    }

    /// Commit before stage creation; caller must retain the verified source and ancestor guards.
    pub fn prepare_relocation(
        &mut self,
        plan: &Plan,
        node: NodeId,
        source: &VerifiedFile,
        stage: &StageName,
    ) -> Result<RelocationId> {
        let planned = plan
            .files
            .iter()
            .find(|file| file.id == node)
            .ok_or_else(|| Error::Invalid("Source is outside the reviewed plan.".into()))?;
        if planned.path != source.path || planned.stamp != source.stamp {
            return Err(Error::Invalid(
                "Source does not match the reviewed plan.".into(),
            ));
        }
        source.check_stamp()?;
        let path = planned
            .path
            .to_str()
            .ok_or_else(|| Error::Invalid("Recovery path is not exact.".into()))?;
        StageEntry::parse(
            planned
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Error::Invalid("Recovery filename is not exact.".into()))?,
        )?;
        let source_id = source_identity(&source.file)?;
        let parent = identity(
            source
                .parents
                .last()
                .ok_or_else(|| Error::Invalid("Missing source parent pin.".into()))?,
        )?;
        if source_id.volume != parent.volume {
            return Err(Error::Invalid(
                "Relocation requires a same-volume parent.".into(),
            ));
        }
        let modified = planned
            .stamp
            .modified
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Invalid("Recovery timestamp precedes the Unix epoch.".into()))?;
        let transaction = self.connection.transaction()?;
        transaction.execute("INSERT INTO relocations(node,path,source_volume,source_file,parent_file,change_time,size,modified_seconds,modified_nanos,stage_name,state,created)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'prepared',?11)",
            params![node.0,path,source_id.volume.to_le_bytes().as_slice(),source_id.file.as_slice(),parent.file.as_slice(),
                planned.stamp.change_time.to_le_bytes().as_slice(),integer(planned.stamp.size)?,integer(modified.as_secs())?,modified.subsec_nanos(),stage.as_str(),integer(crate::cleanup::rules::now()?)?])?;
        let id = transaction.last_insert_rowid();
        transaction.commit()?;
        Ok(RelocationId(id, self.owner))
    }

    /// Record the actual protected-stage identity before permitting admission.
    pub fn relocation_stage_ready(&mut self, id: RelocationId, stage: StageIdentity) -> Result<()> {
        self.relocation_owner(id)?;
        let transaction = self.connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE relocations SET state='stage_ready',stage_file=?1
            WHERE id=?2 AND state='prepared' AND source_volume=?3",
            params![
                stage.file.as_slice(),
                id.0,
                stage.volume.to_le_bytes().as_slice()
            ],
        )?;
        require_one(changed)?;
        transaction.commit()?;
        Ok(())
    }

    /// Must commit before sending the helper its admission request.
    pub fn start_relocation(&mut self, id: RelocationId) -> Result<()> {
        self.relocation_transition(id, "stage_ready", "relocating")
    }

    /// Call only after independently verifying the retained source at the staged location.
    pub fn record_staged(&mut self, id: RelocationId) -> Result<()> {
        self.relocation_transition(id, "relocating", "staged")
    }

    /// A failed helper response alone is insufficient; verify the original object stayed in place.
    pub fn record_unmoved(&mut self, id: RelocationId) -> Result<()> {
        self.relocation_transition(id, "relocating", "unmoved")
    }

    pub fn start_return(&mut self, id: RelocationId) -> Result<()> {
        self.relocation_transition(id, "staged", "returning")
    }

    /// Call only after verifying the original identity at its recorded destination.
    pub fn record_returned(&mut self, id: RelocationId) -> Result<()> {
        self.relocation_transition(id, "returning", "returned")
    }

    /// Commit before invoking Shell; bind the journal intent to the exact retained source and stage.
    pub fn start_recycling(
        &mut self,
        id: RelocationId,
        source: StageIdentity,
        stage: StageIdentity,
    ) -> Result<()> {
        self.relocation_owner(id)?;
        if source.volume != stage.volume {
            return Err(Error::Invalid("Recycling stage volume mismatch.".into()));
        }
        let transaction = self.connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE relocations SET state='recycling' WHERE id=?1 AND state='staged'
            AND source_volume=?2 AND source_file=?3 AND stage_file=?4",
            params![
                id.0,
                source.volume.to_le_bytes().as_slice(),
                source.file.as_slice(),
                stage.file.as_slice()
            ],
        )?;
        require_one(changed)?;
        transaction.commit()?;
        Ok(())
    }

    /// Caller verifies the returned item's identity and observes its version through the retained guard.
    pub fn record_recycled(
        &mut self,
        id: RelocationId,
        source: StageIdentity,
        locator: &Path,
        version: RecycledVersion,
    ) -> Result<()> {
        self.relocation_owner(id)?;
        let locator = native_path(locator)?;
        let modified = version
            .modified
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Invalid("Recycling timestamp precedes the Unix epoch.".into()))?;
        let transaction = self.connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE relocations SET state='recycled',bin_path=?1,recycled_change_time=?5 WHERE id=?2 AND state='recycling'
            AND source_volume=?3 AND source_file=?4 AND size=?6 AND modified_seconds=?7 AND modified_nanos=?8",
            params![
                locator,
                id.0,
                source.volume.to_le_bytes().as_slice(),
                source.file.as_slice(),
                version.change_time.to_le_bytes().as_slice(), integer(version.size)?,
                integer(modified.as_secs())?, modified.subsec_nanos()
            ],
        )?;
        require_one(changed)?;
        transaction.commit()?;
        Ok(())
    }

    /// Records an explicitly chosen destination, not permission to reopen or move a filesystem object.
    pub fn start_restore(
        &mut self,
        id: RelocationId,
        source: StageIdentity,
        destination: &Path,
    ) -> Result<()> {
        self.relocation_owner(id)?;
        let destination = native_path(destination)?;
        let transaction = self.connection.transaction()?;
        let changed=transaction.execute("UPDATE relocations SET state='restoring',restore_path=?1 WHERE id=?2 AND state='recycled' AND source_volume=?3 AND source_file=?4",params![destination,id.0,source.volume.to_le_bytes().as_slice(),source.file.as_slice()])?;
        require_one(changed)?;
        transaction.commit()?;
        Ok(())
    }

    /// Caller must verify the original identity at exactly the recorded destination first.
    pub fn record_restored(
        &mut self,
        id: RelocationId,
        source: StageIdentity,
        destination: &Path,
    ) -> Result<()> {
        self.relocation_owner(id)?;
        let destination = native_path(destination)?;
        let transaction = self.connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE relocations SET state='restored' WHERE id=?1 AND state='restoring'
            AND source_volume=?2 AND source_file=?3 AND restore_path=?4",
            params![
                id.0,
                source.volume.to_le_bytes().as_slice(),
                source.file.as_slice(),
                destination
            ],
        )?;
        require_one(changed)?;
        transaction.commit()?;
        Ok(())
    }

    /// Read-only evidence, never authorization to reopen or retry an interrupted operation.
    pub fn relocations(&self, offset: usize, limit: usize) -> Result<Vec<Record>> {
        let mut statement = self.connection.prepare("SELECT id,node,path,source_volume,source_file,parent_file,stage_name,stage_file,state,message,bin_path,restore_path,recycled_change_time,size,modified_seconds,modified_nanos
            FROM relocations ORDER BY id DESC LIMIT ?1 OFFSET ?2")?;
        let rows = statement.query_map(
            params![
                limit.clamp(1, 100) as i64,
                i64::try_from(offset)
                    .map_err(|_| Error::Invalid("Invalid recovery offset.".into()))?
            ],
            |row| {
                let volume = u64::from_le_bytes(row.get::<_, [u8; 8]>(3)?);
                let recycled_version =
                    if let Some(change_time) = row.get::<_, Option<[u8; 8]>>(12)? {
                        let seconds = nonnegative(row, 14)?;
                        let nanos: u32 = row.get(15)?;
                        if nanos >= 1_000_000_000 {
                            return Err(rusqlite::Error::IntegralValueOutOfRange(
                                15,
                                i64::from(nanos),
                            ));
                        }
                        let modified = UNIX_EPOCH
                            .checked_add(Duration::new(seconds, nanos))
                            .ok_or_else(|| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    14,
                                    rusqlite::types::Type::Integer,
                                    Box::new(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        "Invalid recorded modification time.",
                                    )),
                                )
                            })?;
                        Some(RecycledVersion {
                            change_time: u64::from_le_bytes(change_time),
                            size: nonnegative(row, 13)?,
                            modified,
                        })
                    } else {
                        None
                    };
                Ok(Record {
                    id: row.get(0)?,
                    node: NodeId(row.get(1)?),
                    path: row.get(2)?,
                    source: StageIdentity {
                        volume,
                        file: row.get(4)?,
                    },
                    parent: StageIdentity {
                        volume,
                        file: row.get(5)?,
                    },
                    stage_name: row.get(6)?,
                    stage_file: row.get(7)?,
                    state: row.get(8)?,
                    message: row.get(9)?,
                    bin_path: row
                        .get::<_, Option<Vec<u8>>>(10)?
                        .map(parse_native_path)
                        .transpose()?,
                    restore_path: row
                        .get::<_, Option<Vec<u8>>>(11)?
                        .map(parse_native_path)
                        .transpose()?,
                    recycled_version,
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    fn relocation_owner(&self, id: RelocationId) -> Result<()> {
        if id.1 != self.owner {
            return Err(Error::Invalid(
                "Relocation belongs to another journal session.".into(),
            ));
        }
        Ok(())
    }

    fn relocation_transition(&mut self, id: RelocationId, before: &str, after: &str) -> Result<()> {
        self.relocation_owner(id)?;
        let transaction = self.connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE relocations SET state=?1 WHERE id=?2 AND state=?3",
            params![after, id.0, before],
        )?;
        require_one(changed)?;
        transaction.commit()?;
        Ok(())
    }
}

fn nonnegative(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}

fn native_path(path: &Path) -> Result<Vec<u8>> {
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if !path.is_absolute() || units.is_empty() || units.len() > 32767 || units.contains(&0) {
        return Err(Error::Invalid(
            "Recovery requires an exact absolute native path.".into(),
        ));
    }
    Ok(units.into_iter().flat_map(u16::to_le_bytes).collect())
}

fn parse_native_path(bytes: Vec<u8>) -> rusqlite::Result<PathBuf> {
    let result = (|| {
        if !bytes.len().is_multiple_of(2) {
            return Err(Error::Invalid("Malformed native recovery path.".into()));
        }
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let path: PathBuf = std::ffi::OsString::from_wide(&units).into();
        native_path(&path)?;
        Ok(path)
    })();
    result.map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(error))
    })
}

fn require_one(changed: usize) -> Result<()> {
    if changed != 1 {
        return Err(Error::Invalid(
            "Relocation cannot repeat or change this transition.".into(),
        ));
    }
    Ok(())
}

fn source_identity(file: &std::fs::File) -> std::io::Result<StageIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    let mut id = FILE_ID_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(StageIdentity {
        volume: id.VolumeSerialNumber,
        file: id.FileId.Identifier,
    })
}
