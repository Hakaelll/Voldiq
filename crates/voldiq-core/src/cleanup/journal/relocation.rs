//! Durable admission/rollback evidence. Opening a journal never moves filesystem objects.
use super::{Journal, integer};
use crate::{
    Error, NodeId, Result,
    cleanup::{Plan, VerifiedFile},
    windows::cleanup_stage::{StageEntry, StageIdentity, StageName, identity},
};
use rusqlite::params;

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

#[derive(Clone, Copy, Debug)]
pub struct RelocationId(i64, u64);

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
}

impl Journal {
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

    /// Read-only evidence, never authorization to reopen or retry an interrupted operation.
    pub fn relocations(&self, offset: usize, limit: usize) -> Result<Vec<Record>> {
        let mut statement = self.connection.prepare("SELECT id,node,path,source_volume,source_file,parent_file,stage_name,stage_file,state,message
            FROM relocations ORDER BY id DESC LIMIT ?1 OFFSET ?2")?;
        let rows = statement.query_map(
            params![
                limit.clamp(1, 100) as i64,
                i64::try_from(offset)
                    .map_err(|_| Error::Invalid("Invalid recovery offset.".into()))?
            ],
            |row| {
                let volume = u64::from_le_bytes(row.get::<_, [u8; 8]>(3)?);
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
