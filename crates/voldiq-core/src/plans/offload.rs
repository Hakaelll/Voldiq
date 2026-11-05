//! Exact transfer proposals. Deserialized evidence never grants file-writing authority.
use super::{FileEvidence, invalid};
use crate::transfer::metadata::BasicMetadata;
use crate::{
    Result,
    agents::{AuthorizedPath, PlanContext},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataProfile {
    NtfsBasicV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataEntry {
    id: crate::NodeId,
    metadata: BasicMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataPlan {
    profile: MetadataProfile,
    files: Vec<MetadataEntry>,
    limitations: String,
}

impl MetadataPlan {
    fn check_shape(&self, entries: &[FileEvidence]) -> Result<()> {
        if self.limitations != crate::transfer::metadata::LIMITATIONS
            || self.files.len() != entries.len()
        {
            return Err(invalid());
        }
        for (file, entry) in self.files.iter().zip(entries) {
            if file.id != entry.id {
                return Err(invalid());
            }
            file.metadata.check_shape(&entry.modified_unix_ns)?;
        }
        Ok(())
    }

    #[cfg(windows)]
    pub(super) fn capture(context: &PlanContext<'_>, entries: &[FileEvidence]) -> Result<Self> {
        let mut files = Vec::with_capacity(entries.len());
        for entry in entries {
            let pin = context.path(Path::new(&entry.path))?;
            let file = crate::windows::open_metadata(pin.path())?;
            let id = crate::windows::file_identity(&file)?;
            if id.key != pin.identity() || entry.identity != identity(id.key) {
                return Err(invalid());
            }
            let metadata = BasicMetadata::observe(&file).map_err(|error| {
                crate::Error::Invalid(format!(
                    "File #{} metadata admission failed: {error}",
                    entry.id.0
                ))
            })?;
            metadata.check_shape(&entry.modified_unix_ns)?;
            files.push(MetadataEntry {
                id: entry.id,
                metadata,
            });
        }
        let plan = Self {
            profile: MetadataProfile::NtfsBasicV1,
            files,
            limitations: crate::transfer::metadata::LIMITATIONS.into(),
        };
        plan.check_shape(entries)?;
        Ok(plan)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeBinding {
    guid_root: String,
    serial: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    path: PathBuf,
    identity: String,
    ancestors: Vec<String>,
    pub(super) native_identity: String,
    volume: VolumeBinding,
    filesystem: String,
    filesystem_flags: u32,
    maximum_name_units: u32,
    allocation_unit_bytes: u64,
    caller_available_bytes: String,
    pub(super) observed_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Originals {
    Keep,
    RecycleAfterVerification,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffloadRequest {
    pub destination: PathBuf,
    pub originals: Originals,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub(super) id: crate::NodeId,
    pub(super) name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Offload {
    source_volume: VolumeBinding,
    pub(super) destination: Destination,
    pub(super) originals: Originals,
    pub(super) outputs: Vec<Output>,
    required_available_bytes: String,
    reserve_bytes: String,
    limitations: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) metadata: Option<MetadataPlan>,
}

const RESERVE: u64 = 16 * 1024 * 1024;
pub(super) const LIMITATIONS: &str = "Planning only; transfer execution and approval are unavailable until the copy/readback/catalogue pipeline is implemented. Exact flat output names; no replacement. Caller-available capacity includes a conservative allocation/entry estimate and a 16 MiB reserve, not a reservation or proof of future space. Source payload, streams, encryption, metadata preservation and destination disconnect behavior require execution preflight. No copy or original modification has occurred. Recycling originals does not imply immediate free space.";

impl Offload {
    pub(super) fn schema_version(&self) -> u32 {
        if self.metadata.is_some() { 4 } else { 3 }
    }

    pub(super) fn file_metadata(&self, node: crate::NodeId) -> Result<&BasicMetadata> {
        self.metadata
            .as_ref()
            .and_then(|plan| plan.files.iter().find(|file| file.id == node))
            .map(|file| &file.metadata)
            .ok_or_else(|| {
                crate::Error::Invalid(
                    "Prepare a new transfer plan with an explicit metadata profile.".into(),
                )
            })
    }
    #[cfg(windows)]
    pub(super) fn copy_destination(
        &self,
        context: &PlanContext<'_>,
        state: &Path,
        source: &Path,
        entry: &FileEvidence,
    ) -> Result<AuthorizedPath> {
        let target = self.authorize(context, source)?;
        let current = Destination::observe(&target, state)?;
        let mut expected = self.destination.clone();
        expected.caller_available_bytes = current.caller_available_bytes.clone();
        expected.observed_at = current.observed_at;
        if current != expected || current.filesystem != "NTFS" {
            return Err(crate::Error::Invalid(
                "Verified content copying requires the unchanged NTFS destination profile.".into(),
            ));
        }
        let (_, required) = layout(std::slice::from_ref(entry), &current)?;
        if current
            .caller_available_bytes
            .parse::<u64>()
            .map_err(|_| invalid())?
            < required
        {
            return Err(crate::Error::Invalid(
                "Destination capacity is insufficient for this file and the declared reserve."
                    .into(),
            ));
        }
        let output = self
            .outputs
            .iter()
            .find(|output| output.id == entry.id)
            .ok_or_else(invalid)?;
        absent(&target.path().join(&output.name))?;
        Ok(target)
    }
    pub(super) fn authorize(
        &self,
        context: &PlanContext<'_>,
        source: &Path,
    ) -> Result<AuthorizedPath> {
        let _capacity_scope = capacity_scope(context, &self.destination.path)?;
        let pin = context.path(&self.destination.path)?;
        let directory = crate::agents::pin_directory(pin.path())?;
        if volume(&directory)? != self.destination.volume
            || volume(&crate::agents::pin_directory(source)?)? != self.source_volume
        {
            return Err(invalid());
        }
        let native: String = directory
            .identity_bytes()?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if identity(directory.identity()) != self.destination.identity
            || directory
                .ancestor_identities()?
                .into_iter()
                .map(identity)
                .collect::<Vec<_>>()
                != self.destination.ancestors
            || native != self.destination.native_identity
        {
            return Err(invalid());
        }
        Ok(directory)
    }
    pub(super) fn capture(
        context: &PlanContext<'_>,
        state: &Path,
        source: &Path,
        entries: &[FileEvidence],
        destination: &Path,
        originals: Originals,
    ) -> Result<Self> {
        let target = context.path(destination)?;
        let _capacity_scope = capacity_scope(context, target.path())?;
        let destination = Destination::observe(&target, state)?;
        let source = crate::agents::pin_directory(source)?;
        let source_volume = volume(&source)?;
        if source_volume.guid_root == destination.volume.guid_root
            || source_volume.serial == destination.volume.serial
        {
            return Err(crate::Error::Invalid(
                "Select a destination on a different volume.".into(),
            ));
        }
        let (outputs, required) = layout(entries, &destination)?;
        if destination.filesystem != "NTFS" {
            return Err(crate::Error::Invalid(
                "The basic metadata transfer profile currently requires an NTFS destination."
                    .into(),
            ));
        }
        #[cfg(windows)]
        let metadata = Some(MetadataPlan::capture(context, entries)?);
        #[cfg(not(windows))]
        let metadata = None;
        let result = Self {
            source_volume,
            destination,
            originals,
            outputs,
            required_available_bytes: required.to_string(),
            reserve_bytes: RESERVE.to_string(),
            limitations: LIMITATIONS.into(),
            metadata,
        };
        result.check(context, state, source.path(), entries)?;
        Ok(result)
    }

    pub(super) fn check_shape(&self, entries: &[FileEvidence]) -> Result<()> {
        if let Some(metadata) = &self.metadata {
            metadata.check_shape(entries)?;
        }
        let (outputs, required) = layout(entries, &self.destination)?;
        if outputs != self.outputs
            || self.required_available_bytes != required.to_string()
            || self.reserve_bytes != RESERVE.to_string()
            || self.limitations != LIMITATIONS
            || self.source_volume.guid_root == self.destination.volume.guid_root
            || self.source_volume.serial == self.destination.volume.serial
            || self
                .destination
                .caller_available_bytes
                .parse::<u64>()
                .map_err(|_| invalid())?
                < required
            || entries.iter().any(|entry| {
                !entry
                    .identity
                    .starts_with(&format!("{}:", self.source_volume.serial))
            })
        {
            return Err(invalid());
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
        #[cfg(windows)]
        if let Some(expected) = &self.metadata
            && MetadataPlan::capture(context, entries)? != *expected
        {
            return Err(invalid());
        }
        let target = self.authorize(context, source)?;
        let current = Destination::observe(&target, state)?;
        let current_source = volume(&crate::agents::pin_directory(source)?)?;
        let mut expected = self.destination.clone();
        // Space and observation time are fresh measurements, not immutable identity fields.
        expected.caller_available_bytes = current.caller_available_bytes.clone();
        expected.observed_at = current.observed_at;
        if current != expected
            || current_source != self.source_volume
            || current.volume.guid_root == current_source.guid_root
            || current.volume.serial == current_source.serial
            || current
                .caller_available_bytes
                .parse::<u64>()
                .map_err(|_| invalid())?
                < self
                    .required_available_bytes
                    .parse::<u64>()
                    .map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        for output in &self.outputs {
            let path = current.path.join(&output.name);
            absent(&path)?;
        }
        Ok(())
    }
}

fn absent(path: &Path) -> Result<()> {
    // An inaccessible, reparse or placeholder entry is never treated as absent.
    #[cfg(windows)]
    let observed = crate::windows::open_metadata(path).map(|_| ());
    #[cfg(not(windows))]
    let observed = std::fs::symlink_metadata(path)
        .map(|_| ())
        .map_err(crate::Error::from);
    match observed {
        Ok(()) => Err(crate::Error::Invalid(
            "A transfer output already exists.".into(),
        )),
        Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn identity(value: (u64, u64)) -> String {
    format!("{:016x}:{:016x}", value.0, value.1)
}

fn capacity_scope(context: &PlanContext<'_>, path: &Path) -> Result<AuthorizedPath> {
    // Folder permission does not authorize disclosure of whole-volume capacity.
    context.path(path.ancestors().last().ok_or_else(invalid)?)
}

pub(super) fn layout(
    entries: &[FileEvidence],
    destination: &Destination,
) -> Result<(Vec<Output>, u64)> {
    if entries.is_empty()
        || entries.len() > 500
        || destination.allocation_unit_bytes == 0
        || destination.allocation_unit_bytes > 16 * 1024 * 1024
        || destination.maximum_name_units == 0
        || destination.maximum_name_units > 255
    {
        return Err(invalid());
    }
    let mut outputs: Vec<Output> = Vec::with_capacity(entries.len());
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    let mut required = RESERVE;
    for file in entries {
        let name = Path::new(&file.path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(invalid)?;
        if name.encode_utf16().count() > destination.maximum_name_units as usize {
            return Err(invalid());
        }
        crate::agents::ordinary_path(&destination.path.join(name))?;
        if !ids.insert(file.id) || !names.insert(name.to_uppercase()) {
            return Err(crate::Error::Invalid(
                "Transfer output names collide. Select distinct names.".into(),
            ));
        }
        let size = file.logical_bytes.parse::<u64>().map_err(|_| invalid())?;
        if destination.filesystem == "FAT32" && size > u64::from(u32::MAX) {
            return Err(invalid());
        }
        let unit = destination.allocation_unit_bytes;
        let allocated = size
            .checked_add(unit - 1)
            .and_then(|n| (n / unit).checked_mul(unit))
            .ok_or_else(invalid)?;
        required = required
            .checked_add(allocated)
            .and_then(|n| n.checked_add(unit))
            .ok_or_else(invalid)?;
        outputs.push(Output {
            id: file.id,
            name: name.into(),
        });
    }
    Ok((outputs, required))
}

#[cfg(windows)]
mod native;
#[cfg(windows)]
use native::volume;
#[cfg(not(windows))]
fn volume(_: &AuthorizedPath) -> Result<VolumeBinding> {
    Err(invalid())
}

#[cfg(not(windows))]
impl Destination {
    fn observe(_: &AuthorizedPath, _: &Path) -> Result<Self> {
        Err(invalid())
    }
}
