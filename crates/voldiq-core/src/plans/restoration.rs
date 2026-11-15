//! Read-only restore proposals use fresh archive scan evidence and current scope, never catalogue authority.
use super::{
    Action, FileEvidence, Manifest, Offload, Originals, Store, TransferFile, View, invalid,
    offload::{Output, TransferLayout, occupied, output_path},
};
use crate::{
    Error, NodeId, Result,
    agents::{AuthorizedPath, Capability, PlanContext},
    cleanup::record::Record,
    index::Index,
    transfer::metadata::BasicMetadata,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};

pub(super) const LEGACY_LIMITATIONS: &str = "Planning only; restoration approval and execution are unavailable. Keep archive copies. Existing original parent folders only; no replacement, directory recreation or inferred relocation. Verify every archive's recorded BLAKE3 before publication. Basic NTFS metadata only; original unknown creation remains disclosed, with the archive's observed creation date used for the new copy. Permissions, access/change dates and link relationships are not restored. No full-restoration or freed-space guarantee.";
pub(super) const LIMITATIONS: &str = "Keep archive copies. Restore to exact names in existing original folders and preserve any existing files. Verify every archive's recorded BLAKE3 before publication. Preserve basic NTFS metadata. Use the observed archive creation date where the original creation date was unknown. Permissions, access/change dates and link relationships are not restored.";
const ALTERNATE_NAMES: u8 = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreCollisionPolicy {
    #[default]
    Fail,
    KeepBoth,
}

impl RestoreCollisionPolicy {
    fn is_fail(&self) -> bool {
        *self == Self::Fail
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    pub transfer_plan_id: String,
    pub node_ids: Vec<NodeId>,
    #[serde(default)]
    pub on_collision: RestoreCollisionPolicy,
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
    pub(super) groups: Vec<Offload>,
    required_available_bytes: String,
    limitations: String,
    #[serde(default, skip_serializing_if = "RestoreCollisionPolicy::is_fail")]
    on_collision: RestoreCollisionPolicy,
}

struct Archive {
    manifest: Manifest,
    hash: String,
    record: Record,
    files: Vec<TransferFile>,
}

impl Restore {
    #[cfg(windows)]
    pub(super) fn has_current_execution_profile(&self) -> bool {
        self.limitations == LIMITATIONS
            && self
                .groups
                .iter()
                .all(Offload::has_current_execution_profile)
            && self.execution_limit().is_ok()
    }

    pub(super) fn schema_version(&self) -> u32 {
        match self.on_collision {
            RestoreCollisionPolicy::Fail => 6,
            RestoreCollisionPolicy::KeepBoth => 7,
        }
    }

    pub(super) fn expected_content(&self, node: NodeId) -> Result<blake3::Hash> {
        let file = self
            .files
            .iter()
            .find(|file| file.source_id == node)
            .ok_or_else(invalid)?;
        blake3::Hash::from_hex(&file.content_blake3).map_err(|_| invalid())
    }
    pub(super) fn check_shape(&self, entries: &[FileEvidence]) -> Result<()> {
        if !hex(&self.transfer_plan_id, 64)
            || !hex(&self.transfer_manifest_hash, 64)
            || entries.is_empty()
            || entries.len() > 500
            || self.files.len() != entries.len()
            || self.groups.is_empty()
            || self.groups.len() > entries.len()
            || !matches!(self.limitations.as_str(), LIMITATIONS | LEGACY_LIMITATIONS)
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
            group.check_output_shape(&selected)?;
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
            let output = group
                .outputs
                .iter()
                .find(|output| output.id == entry.id)
                .ok_or_else(invalid)?;
            let original_name = Path::new(&file.original_path)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(invalid)?;
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
                || (output.name != original_name
                    && (self.on_collision.is_fail()
                        || !(1..=ALTERNATE_NAMES)
                            .any(|n| alternate_name(original_name, n) == output.name)))
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
        on_collision: RestoreCollisionPolicy,
    ) -> Result<Self> {
        let Action::Offload(old) = &archive.manifest.action else {
            return Err(invalid());
        };
        let mut files = Vec::with_capacity(entries.len());
        let mut parents: Vec<(AuthorizedPath, Vec<FileEvidence>)> = Vec::new();
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
            let restored_path = current
                .path()
                .join(Path::new(&original.path).file_name().ok_or_else(invalid)?);
            if let Some((_, group)) = parents
                .iter_mut()
                .find(|(parent, _)| parent.path() == current.path())
            {
                group.push(entry.clone());
            } else {
                parents.push((current, vec![entry.clone()]));
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
        let protections = crate::metadata::Store::open(state)?.protections()?;
        for (parent, entries) in parents {
            let outputs = restore_outputs(context, &parent, &protections, &entries, on_collision)?;
            let group = Offload::capture(
                context,
                state,
                &index.root,
                &entries,
                parent.path(),
                TransferLayout::Restore(&outputs),
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
            on_collision,
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
        let archive = self.restore_evidence(context, request)?;
        self.create_restore_plan(
            context,
            snapshot,
            index,
            &archive,
            request.on_collision,
            now,
        )
    }

    fn restore_evidence(
        &self,
        context: &PlanContext<'_>,
        request: &RestoreRequest,
    ) -> Result<Archive> {
        let policy = context.check(Capability::PreparePlans)?;
        request.check_selection(policy.limits().plan_files)?;
        let tx = self.connection.unchecked_transaction()?;
        let stored = self.read_restore_archive(context, &request.transfer_plan_id)?;
        let archive = self.archive_evidence(stored, request)?;
        tx.commit()?;
        Ok(archive)
    }

    fn archive_evidence(
        &self,
        stored: super::store::Stored,
        request: &RestoreRequest,
    ) -> Result<Archive> {
        let files =
            self.restoration_files(&stored.view.manifest, &stored.view.hash, &request.node_ids)?;
        let protections = crate::metadata::Store::open(self.directory.path())?.protections()?;
        stored.record.check_protections(&protections)?;
        Ok(Archive {
            manifest: stored.view.manifest,
            hash: stored.view.hash,
            record: stored.record,
            files,
        })
    }

    fn create_restore_plan(
        &mut self,
        context: &PlanContext<'_>,
        snapshot: i64,
        index: &Index,
        archive: &Archive,
        collision: RestoreCollisionPolicy,
        now: i64,
    ) -> Result<View> {
        let Action::Offload(offload) = &archive.manifest.action else {
            return Err(invalid());
        };
        check_archive_root(offload, &context.path(&index.root)?)?;
        let selection = archive
            .files
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
        let state = self.directory.path().to_owned();
        self.create_using(context, snapshot, index, &selection, now, |entries| {
            Restore::capture(context, &state, index, entries, archive, collision)
                .map(|plan| Action::Restore(Box::new(plan)))
        })
    }

    #[cfg(windows)]
    pub(super) fn create_desktop_restore(
        &mut self,
        human: &super::HumanAuthority,
        registry: &mut crate::agents::Registry,
        request: &RestoreRequest,
        expected_hash: &str,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<View> {
        request.check_selection(500)?;
        let tx = self.connection.unchecked_transaction()?;
        let stored = self.read_desktop_restore_archive(human, &request.transfer_plan_id)?;
        if stored.view.hash != expected_hash {
            return Err(invalid());
        }
        let archive = self.archive_evidence(stored, request)?;
        tx.commit()?;
        let Action::Offload(offload) = &archive.manifest.action else {
            return Err(invalid());
        };
        let root = offload
            .destination
            .volume
            .restore_parent_path(&offload.destination.path)?;
        let root = crate::agents::pin_directory(&root)?;
        check_archive_root(offload, &root)?;
        if super::offload::VolumeBinding::observe(root.path())? != offload.destination.volume {
            return Err(invalid());
        }
        let ancestors = root.ancestor_identities()?;
        let protections = crate::metadata::Store::open(self.directory.path())?.protections()?;
        protections.check_evidence(root.path(), root.identity(), &ancestors)?;
        let protected = crate::cleanup::protected_identities()?;
        if crate::cleanup::record::sensitive(root.path())
            || ancestors
                .iter()
                .any(|id| *id == self.directory.identity() || protected.contains(id))
        {
            return Err(invalid());
        }
        let (original_parent, _) = archive.record.restore_parent(request.node_ids[0])?;
        let resolved = offload.source_volume.restore_parent_path(original_parent)?;
        let original_root = resolved.ancestors().last().ok_or_else(invalid)?;
        let index = observe_archive_selection(&root, &archive.files, cancel)?;
        let principal = registry.desktop_transfer_principal(human, root.path(), original_root)?;
        let context = registry.plan_context(&principal, Capability::PreparePlans)?;
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Error::Invalid("Restore preparation was cancelled.".into()));
        }
        human.check_directory(self.directory.identity())?;
        self.create_restore_plan(
            &context,
            0,
            &index,
            &archive,
            request.on_collision,
            super::now()?,
        )
    }
}

#[cfg(windows)]
pub(super) fn observe_archive_selection(
    root: &AuthorizedPath,
    files: &[TransferFile],
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Index> {
    use crate::{Entry, FileState, Kind, scanner::modified, windows};
    use std::sync::atomic::Ordering;
    let root_entry = Entry {
        id: NodeId(0),
        parent: None,
        name: root.path().to_string_lossy().into_owned(),
        kind: Kind::Directory,
        file_state: None,
        cloud: crate::cloud::CloudState::Unknown,
        logical: 0,
        allocated: 0,
        allocation_known: false,
        modified: None,
        files: 0,
        incomplete: false,
        hard_link: false,
    };
    let mut entries = Vec::with_capacity(files.len() + 1);
    entries.push(root_entry);
    for recorded in files {
        if cancel.load(Ordering::Acquire) {
            return Err(Error::Invalid("Restore preparation was cancelled.".into()));
        }
        let name = &recorded.output_name;
        if Path::new(name).file_name() != Some(std::ffi::OsStr::new(name)) {
            return Err(invalid());
        }
        let path = root.path().join(name);
        crate::agents::ordinary_path(&path)?;
        if crate::cleanup::record::sensitive(&path) {
            return Err(invalid());
        }
        let file = windows::open_metadata(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || windows::is_reparse(&metadata)
            || windows::requires_recall(&metadata)
        {
            return Err(Error::Invalid(
                "An archive copy is unavailable as a regular local file.".into(),
            ));
        }
        let identity = windows::file_identity(&file)?;
        let metadata_profile = BasicMetadata::observe(&file)?;
        if metadata_profile.native_identity()
            != recorded
                .destination_identity
                .as_deref()
                .ok_or_else(invalid)?
            || metadata.len().to_string()
                != *recorded.verified_bytes.as_ref().ok_or_else(invalid)?
        {
            return Err(Error::Invalid(
                "An archive copy changed since it was verified.".into(),
            ));
        }
        entries.push(Entry {
            id: NodeId(entries.len() as u32),
            parent: Some(NodeId(0)),
            name: name.clone(),
            kind: Kind::File,
            file_state: Some(FileState {
                identity: identity.key,
                change_time: identity.change_time,
            }),
            cloud: identity.cloud,
            logical: metadata.len(),
            allocated: identity.allocated,
            allocation_known: true,
            modified: modified(&metadata),
            files: 1,
            incomplete: false,
            hard_link: identity.links > 1,
        });
    }
    Ok(Index {
        observation: None,
        root: root.path().to_owned(),
        entries,
        issues: vec![],
        error_count: 0,
        native_names: Default::default(),
        root_identity: Some(root.identity()),
        exclusions: vec![],
        snapshot_id: None,
        captured_caches: vec![],
        captured_applications: None,
        captured_libraries: None,
        ntfs_checkpoint: None,
    })
}

fn check_archive_root(offload: &Offload, root: &AuthorizedPath) -> Result<()> {
    if format!("{:016x}:{:016x}", root.identity().0, root.identity().1)
        != offload.destination.identity
        || root
            .identity_bytes()?
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            != offload.destination.native_identity
    {
        return Err(Error::Invalid(
            "The recorded archive folder identity changed.".into(),
        ));
    }
    Ok(())
}

impl RestoreRequest {
    fn check_selection(&self, limit: usize) -> Result<()> {
        if !hex(&self.transfer_plan_id, 64)
            || self.node_ids.is_empty()
            || self.node_ids.len() > limit.min(500)
            || self.node_ids.iter().copied().collect::<HashSet<_>>().len() != self.node_ids.len()
        {
            return Err(invalid());
        }
        Ok(())
    }
}

pub(super) fn restore_outputs(
    context: &PlanContext<'_>,
    parent: &AuthorizedPath,
    protections: &crate::metadata::Protections,
    entries: &[FileEvidence],
    on_collision: RestoreCollisionPolicy,
) -> Result<Vec<Output>> {
    let names = entries
        .iter()
        .map(|entry| {
            Path::new(&entry.path)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(invalid)
        })
        .collect::<Result<Vec<_>>>()?;
    // A renamed file cannot claim another file's original destination in this batch.
    let mut reserved = names
        .iter()
        .map(|name| name.to_uppercase())
        .collect::<HashSet<_>>();
    if reserved.len() != names.len() {
        return Err(invalid());
    }
    let mut outputs = Vec::with_capacity(entries.len());
    for (entry, name) in entries.iter().zip(names) {
        let original = output_path(context, parent, protections, name)?;
        let name = if occupied(&original)? {
            if on_collision.is_fail() {
                return Err(Error::RestoreCollision);
            }
            let mut available = None;
            for number in 1..=ALTERNATE_NAMES {
                let candidate = alternate_name(name, number);
                let key = candidate.to_uppercase();
                if reserved.contains(&key) {
                    continue;
                }
                let path = output_path(context, parent, protections, &candidate)?;
                if !occupied(&path)? {
                    reserved.insert(key);
                    available = Some(candidate);
                    break;
                }
            }
            available.ok_or_else(|| {
                Error::Invalid(
                    "The original name and 32 alternate restore names are occupied. Select other files or free a name before preparing a new plan.".into(),
                )
            })?
        } else {
            name.to_owned()
        };
        outputs.push(Output { id: entry.id, name });
    }
    Ok(outputs)
}

fn alternate_name(original: &str, number: u8) -> String {
    let path = Path::new(original);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(original);
    let suffix = format!(" (restored {number})");
    let suffix_units = suffix.len();
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| extension.encode_utf16().count() + suffix_units + 2 <= 255);
    let mut available =
        255 - suffix_units - extension.map_or(0, |extension| 1 + extension.encode_utf16().count());
    let stem = stem
        .chars()
        .take_while(|character| {
            let units = character.len_utf16();
            if units > available {
                return false;
            }
            available -= units;
            true
        })
        .collect::<String>();
    match extension {
        Some(extension) => format!("{stem}{suffix}.{extension}"),
        None => format!("{stem}{suffix}"),
    }
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
