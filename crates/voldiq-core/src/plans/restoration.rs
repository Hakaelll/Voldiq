//! Read-only restore proposals use fresh archive scan evidence and current scope, never catalogue authority.
use super::{
    Action, FileEvidence, Manifest, Offload, Originals, Store, TransferFile, View, invalid,
};
use crate::{
    Error, NodeId, Result,
    agents::{Capability, PlanContext},
    cleanup::record::Record,
    index::Index,
    transfer::metadata::BasicMetadata,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub(super) const LIMITATIONS: &str = "Planning only; restoration approval and execution are unavailable. Keep archive copies. Existing original parent folders only; no replacement, directory recreation or inferred relocation. Verify every archive's recorded BLAKE3 before publication. Basic NTFS metadata only; original unknown creation remains disclosed, with the archive's observed creation date used for the new copy. Permissions, access/change dates and link relationships are not restored. No full-restoration or freed-space guarantee.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    pub transfer_plan_id: String,
    pub node_ids: Vec<NodeId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreFile {
    source_id: NodeId,
    original_id: NodeId,
    original_path: String,
    archive_identity: String,
    content_blake3: String,
    bytes: String,
    original_metadata: BasicMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Restore {
    transfer_plan_id: String,
    transfer_manifest_hash: String,
    files: Vec<RestoreFile>,
    groups: Vec<Offload>,
    required_available_bytes: String,
    limitations: String,
}

struct Archive {
    manifest: Manifest,
    hash: String,
    record: Record,
    files: Vec<TransferFile>,
}

impl Restore {
    pub(super) fn check_shape(&self, entries: &[FileEvidence]) -> Result<()> {
        if !hex(&self.transfer_plan_id, 64)
            || !hex(&self.transfer_manifest_hash, 64)
            || entries.is_empty()
            || entries.len() > 500
            || self.files.len() != entries.len()
            || self.groups.is_empty()
            || self.groups.len() > entries.len()
            || self.limitations != LIMITATIONS
        {
            return Err(invalid());
        }
        let mut source_ids = HashSet::new();
        let mut original_ids = HashSet::new();
        let mut grouped = HashSet::new();
        let mut parents = HashSet::new();
        let mut required = super::offload::RESERVE;
        for group in &self.groups {
            if group.originals != Originals::Keep
                || group.schema_version() != 5
                || group.destination.volume != self.groups[0].destination.volume
                || group.source_volume != self.groups[0].source_volume
                || !parents.insert(&group.destination.native_identity)
            {
                return Err(invalid());
            }
            let selected = entries
                .iter()
                .filter(|entry| group.outputs.iter().any(|output| output.id == entry.id))
                .cloned()
                .collect::<Vec<_>>();
            group.check_shape(&selected)?;
            let needed = group
                .required_available_bytes
                .parse::<u64>()
                .map_err(|_| invalid())?;
            required = required
                .checked_add(
                    needed
                        .checked_sub(super::offload::RESERVE)
                        .ok_or_else(invalid)?,
                )
                .ok_or_else(invalid)?;
            for entry in selected {
                if !grouped.insert(entry.id) {
                    return Err(invalid());
                }
            }
        }
        if grouped.len() != entries.len()
            || self.required_available_bytes != required.to_string()
            || self.groups.iter().any(|group| {
                group
                    .destination
                    .caller_available_bytes
                    .parse::<u64>()
                    .ok()
                    .is_none_or(|available| available < required)
            })
        {
            return Err(invalid());
        }
        for (file, entry) in self.files.iter().zip(entries) {
            let group = self
                .groups
                .iter()
                .find(|group| group.outputs.iter().any(|output| output.id == entry.id))
                .ok_or_else(invalid)?;
            let copy = group.file_metadata(entry.id)?;
            file.original_metadata
                .check_shape(&entry.modified_unix_ns)?;
            if file.source_id != entry.id
                || !source_ids.insert(file.source_id)
                || !original_ids.insert(file.original_id)
                || !hex(&file.archive_identity, 48)
                || !hex(&file.content_blake3, 64)
                || file.archive_identity != copy.native_identity()
                || file.bytes != entry.logical_bytes
                || Path::new(&file.original_path).parent() != Some(group.destination.path.as_path())
                || Path::new(&file.original_path).file_name() != Path::new(&entry.path).file_name()
                || file.original_metadata.modified_windows_100ns() != copy.modified_windows_100ns()
                || file.original_metadata.dos_attributes() != copy.dos_attributes()
                || file
                    .original_metadata
                    .created_windows_100ns()
                    .is_some_and(|created| Some(created) != copy.created_windows_100ns())
            {
                return Err(invalid());
            }
            crate::agents::ordinary_path(Path::new(&file.original_path))?;
        }
        Ok(())
    }

    #[cfg(windows)]
    pub(super) fn execution_limit(&self) -> Result<i64> {
        if self.groups.is_empty() {
            return Err(invalid());
        }
        let limit = self.groups[0].execution_limit()?;
        for group in &self.groups {
            if group.execution_limit()? != limit {
                return Err(invalid());
            }
        }
        Ok(limit)
    }

    pub(super) fn authorize(&self, context: &PlanContext<'_>, source: &Path) -> Result<()> {
        for group in &self.groups {
            let _pin = group.authorize(context, source)?;
        }
        Ok(())
    }

    pub(super) fn check(
        &self,
        context: &PlanContext<'_>,
        state: &Path,
        source: &Path,
        entries: &[FileEvidence],
    ) -> Result<()> {
        self.check_shape(entries)?;
        for group in &self.groups {
            let selected = entries
                .iter()
                .filter(|entry| group.outputs.iter().any(|output| output.id == entry.id))
                .cloned()
                .collect::<Vec<_>>();
            let available = group.check(context, state, source, &selected)?;
            if available
                < self
                    .required_available_bytes
                    .parse::<u64>()
                    .map_err(|_| invalid())?
            {
                return Err(Error::Invalid(
                    "Insufficient currently available capacity for the complete restore batch."
                        .into(),
                ));
            }
        }
        Ok(())
    }

    fn capture(
        context: &PlanContext<'_>,
        state: &Path,
        index: &Index,
        entries: &[FileEvidence],
        archive: &Archive,
    ) -> Result<Self> {
        let Action::Offload(old) = &archive.manifest.action else {
            return Err(invalid());
        };
        let mut files = Vec::with_capacity(entries.len());
        let mut parents: Vec<(PathBuf, Vec<FileEvidence>)> = Vec::new();
        for entry in entries {
            let name = Path::new(&entry.path)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(invalid)?;
            let recorded = archive
                .files
                .iter()
                .find(|file| file.output_name == name)
                .ok_or_else(invalid)?;
            let original = archive
                .manifest
                .entries
                .iter()
                .find(|file| file.id == recorded.node)
                .ok_or_else(invalid)?;
            let (parent, identity) = archive.record.restore_parent(original.id)?;
            let resolved = old.source_volume.restore_parent_path(parent)?;
            let current = context.path(&resolved)?;
            if current.identity() != identity {
                return Err(Error::Invalid(
                    "Original parent identity changed. Prepare a separately reviewed destination."
                        .into(),
                ));
            }
            archive
                .record
                .check_restore_ancestry(original.id, &current)?;
            let parent = current.path().to_owned();
            let restored_path =
                parent.join(Path::new(&original.path).file_name().ok_or_else(invalid)?);
            if let Some((_, group)) = parents.iter_mut().find(|(path, _)| path == &parent) {
                group.push(entry.clone());
            } else {
                parents.push((parent, vec![entry.clone()]));
            }
            files.push(RestoreFile {
                source_id: entry.id,
                original_id: original.id,
                original_path: restored_path.to_str().ok_or_else(invalid)?.to_owned(),
                archive_identity: recorded.destination_identity.clone().ok_or_else(invalid)?,
                content_blake3: recorded.content_blake3.clone().ok_or_else(invalid)?,
                bytes: recorded.verified_bytes.clone().ok_or_else(invalid)?,
                original_metadata: old.file_metadata(original.id)?.clone(),
            });
        }
        let mut groups = Vec::with_capacity(parents.len());
        let mut required = super::offload::RESERVE;
        for (parent, entries) in parents {
            let group = Offload::capture(
                context,
                state,
                &index.root,
                &entries,
                &parent,
                Originals::Keep,
            )?;
            if group.destination.volume != old.source_volume
                || group.source_volume != old.destination.volume
            {
                return Err(invalid());
            }
            required = required
                .checked_add(
                    group
                        .required_available_bytes
                        .parse::<u64>()
                        .map_err(|_| invalid())?
                        .checked_sub(super::offload::RESERVE)
                        .ok_or_else(invalid)?,
                )
                .ok_or_else(invalid)?;
            groups.push(group);
        }
        let restore = Self {
            transfer_plan_id: archive.manifest.id.clone(),
            transfer_manifest_hash: archive.hash.clone(),
            files,
            groups,
            required_available_bytes: required.to_string(),
            limitations: LIMITATIONS.into(),
        };
        restore.check(context, state, &index.root, entries)?;
        Ok(restore)
    }
}

impl Store {
    pub fn create_restore(
        &mut self,
        context: &PlanContext<'_>,
        snapshot: i64,
        index: &Index,
        request: &RestoreRequest,
        now: i64,
    ) -> Result<View> {
        let policy = context.check(Capability::PreparePlans)?;
        if !hex(&request.transfer_plan_id, 64)
            || request.node_ids.is_empty()
            || request.node_ids.len() > policy.limits().plan_files
            || request.node_ids.len() > 500
            || request
                .node_ids
                .iter()
                .copied()
                .collect::<HashSet<_>>()
                .len()
                != request.node_ids.len()
        {
            return Err(invalid());
        }
        let tx = self.connection.unchecked_transaction()?;
        // Old policy bindings describe the original operation; only the new proposal may get fresh approval.
        let stored = self.read_restore_archive(context, &request.transfer_plan_id)?;
        let Action::Offload(offload) = &stored.view.manifest.action else {
            return Err(invalid());
        };
        let files =
            self.restoration_files(&stored.view.manifest, &stored.view.hash, &request.node_ids)?;
        let root = context.path(&index.root)?;
        if format!("{:016x}:{:016x}", root.identity().0, root.identity().1)
            != offload.destination.identity
            || root
                .identity_bytes()?
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                != offload.destination.native_identity
        {
            return Err(invalid());
        }
        let protections = crate::metadata::Store::open(self.directory.path())?.protections()?;
        stored.record.check_protections(&protections)?;
        let selection = files
            .iter()
            .map(|file| {
                let candidate = index
                    .entries
                    .iter()
                    .find(|entry| {
                        entry.name == file.output_name
                            && index
                                .path(entry.id)
                                .is_ok_and(|path| path.parent() == Some(index.root.as_path()))
                    })
                    .ok_or_else(invalid)?;
                Ok(candidate.id)
            })
            .collect::<Result<Vec<_>>>()?;
        let archive = Archive {
            manifest: stored.view.manifest,
            hash: stored.view.hash,
            record: stored.record,
            files,
        };
        tx.commit()?;
        let state = self.directory.path().to_owned();
        self.create_using(context, snapshot, index, &selection, now, |entries| {
            Restore::capture(context, &state, index, entries, &archive)
                .map(|plan| Action::Restore(Box::new(plan)))
        })
    }
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
