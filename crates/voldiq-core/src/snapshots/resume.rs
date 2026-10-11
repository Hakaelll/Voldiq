use super::{Store, encode, encode_paths};
use crate::{Error, Result, index::Index};
use rusqlite::{OptionalExtension, params};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

impl Store {
    fn resume_candidate(&self, id: i64, cancel: &AtomicBool, limit: usize) -> Result<Index> {
        let checkpoint = self
            .checkpoint(id)?
            .ok_or_else(|| Error::Invalid("Snapshot has no journal checkpoint.".into()))?;
        let mut index = self.load_index(
            id,
            true,
            Some(cancel),
            Some(limit.clamp(1, u32::MAX as usize)),
        )?;
        if index.entries.iter().skip(1).any(|entry| {
            entry
                .file_state
                .is_none_or(|state| state.identity.0 != checkpoint.volume)
        }) {
            return Err(Error::Invalid(
                "Snapshot lacks native entry identities; rescan required.".into(),
            ));
        }
        index.snapshot_id = None;
        index.ntfs_checkpoint = Some(checkpoint);
        Ok(index)
    }

    pub fn replay_snapshot(
        &self,
        id: i64,
        changes: crate::ntfs::Records,
        open: impl FnMut(u64) -> Result<std::fs::File>,
        limit: usize,
        cancel: &AtomicBool,
        verify: impl FnOnce(crate::ntfs::Checkpoint) -> Result<()>,
    ) -> Result<Index> {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Invalid("Snapshot resume cancelled.".into()));
        }
        crate::ntfs::replay(
            &self.resume_candidate(id, cancel, limit)?,
            changes,
            open,
            limit,
            cancel,
            verify,
        )
    }

    pub fn resume_latest(
        &self,
        root: &Path,
        options: &crate::ScanOptions,
        cancel: &AtomicBool,
    ) -> Result<Option<Index>> {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Invalid("Snapshot resume cancelled.".into()));
        }
        if crate::windows::is_reparse(&std::fs::symlink_metadata(root)?) {
            return Ok(None);
        }
        let root = std::fs::canonicalize(root)?;
        if root.parent().is_some() {
            return Ok(None);
        }
        let identity = crate::windows::identity(&root)?.key;
        let identity = [identity.0.to_le_bytes(), identity.1.to_le_bytes()].concat();
        let mut exclusions = options
            .exclusions
            .iter()
            .map(std::fs::canonicalize)
            .collect::<std::io::Result<Vec<_>>>()?;
        exclusions.sort();
        exclusions.dedup();
        let id: Option<i64> = self.connection.query_row(
            "SELECT s.id FROM snapshots s JOIN snapshot_roots r ON r.id=s.root_id JOIN snapshot_journals j ON j.snapshot_id=s.id
             WHERE r.path=?1 AND r.identity=?2 AND r.coverage=?3 AND s.complete=1 AND s.errors=0 ORDER BY s.id DESC LIMIT 1",
            params![encode(root.as_os_str()), identity, encode_paths(&exclusions)?], |row| row.get(0),
        ).optional()?;
        let Some(id) = id else {
            return Ok(None);
        };
        let limit = options
            .max_entries
            .unwrap_or(5_000_000)
            .clamp(1, u32::MAX as usize);
        let index = self.resume_candidate(id, cancel, limit)?;
        crate::ntfs::refresh(&index, limit, cancel).map(Some)
    }
}
