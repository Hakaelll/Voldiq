use super::{Bytes, Observation, observe_volume};
use crate::{
    Error, Kind, NodeId, Result,
    index::{ACCOUNTING_VERSION, Index},
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Complete,
    Partial,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Accounting {
    Known,
    Unknown,
    Unsupported,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Allocation {
    Measured {
        known: Bytes,
        complete: bool,
        known_file_records: u64,
        unknown_file_records: u64,
    },
    Unavailable {
        reason: AllocationReason,
    },
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationReason {
    AccountingUnknown,
    AccountingUnsupported,
    IdentityUnknown,
    NoEvidence,
    Inconsistent,
}

#[derive(Clone, Debug, Serialize)]
pub struct Indexed {
    pub root: PathBuf,
    pub snapshot: Option<i64>,
    pub observation: Option<crate::index::Observation>,
    pub coverage: Coverage,
    pub accounting: Accounting,
    pub exclusions: usize,
    pub starts_at_volume_root: bool,
    pub volume_serial: Option<String>,
    pub logical_paths: Bytes,
    pub recorded_allocation: Bytes,
    pub allocation: Allocation,
    pub file_paths: u64,
    pub errors: u64,
    pub hardlink_paths: u64,
    pub cloud_file_paths: u64,
    pub cloud_logical_paths: Bytes,
    pub recall_required_file_paths: u64,
    pub opaque_reparse_records: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VolumeContext {
    ScopedOnly,
    RootUnavailable,
    Observed { observation: Observation },
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Residual {
    Compared {
        bytes: String,
        against_complete_scope: bool,
    },
    Unavailable {
        reason: ResidualReason,
    },
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidualReason {
    ScopedOnly,
    RootUnavailable,
    VolumeUnavailable,
    AllocationUnavailable,
    VolumeMismatch,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub indexed: Indexed,
    pub volume: VolumeContext,
    pub residual: Residual,
}
impl Report {
    /// Arithmetic on supplied metadata, never authority to read a volume or modify a file.
    pub fn compare(indexed: Indexed, volume: VolumeContext) -> Self {
        let residual = match (&indexed.allocation, &volume) {
            (_, VolumeContext::ScopedOnly) => Residual::Unavailable {
                reason: ResidualReason::ScopedOnly,
            },
            (_, VolumeContext::RootUnavailable) => Residual::Unavailable {
                reason: ResidualReason::RootUnavailable,
            },
            (
                _,
                VolumeContext::Observed {
                    observation: Observation::Unavailable { .. },
                },
            ) => Residual::Unavailable {
                reason: ResidualReason::VolumeUnavailable,
            },
            (Allocation::Unavailable { .. }, _) => Residual::Unavailable {
                reason: ResidualReason::AllocationUnavailable,
            },
            (
                Allocation::Measured {
                    known, complete, ..
                },
                VolumeContext::Observed {
                    observation: Observation::Measured { measurement },
                },
            ) => {
                if indexed.volume_serial.as_deref() != Some(measurement.volume.serial.as_str()) {
                    Residual::Unavailable {
                        reason: ResidualReason::VolumeMismatch,
                    }
                } else {
                    Residual::Compared {
                        bytes: (i128::from(measurement.used.0) - i128::from(known.0)).to_string(),
                        against_complete_scope: *complete,
                    }
                }
            }
        };
        Self {
            indexed,
            volume,
            residual,
        }
    }
}

pub fn summarize(index: &Index, cancelled: &AtomicBool) -> Result<Option<Indexed>> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let root = index.entry(NodeId(0))?;
    let coverage = match index.observation.as_ref() {
        None => Coverage::Unknown,
        Some(observation) if observation.complete && !root.incomplete && index.error_count == 0 => {
            Coverage::Complete
        }
        Some(_) => Coverage::Partial,
    };
    let accounting = match index
        .observation
        .as_ref()
        .map(|value| value.accounting_version)
    {
        Some(ACCOUNTING_VERSION) => Accounting::Known,
        Some(_) => Accounting::Unsupported,
        None => Accounting::Unknown,
    };
    let mut known = 0_u64;
    let mut known_files = 0_u64;
    let mut unknown_files = 0_u64;
    let mut hardlinks = 0_u64;
    let mut clouds = 0_u64;
    let mut cloud_logical = 0_u64;
    let mut recall = 0_u64;
    let mut opaque = 0_u64;
    for (position, entry) in index.entries.iter().enumerate() {
        if position % 256 == 0 && cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if entry.is_metadata_file() {
            hardlinks += u64::from(entry.hard_link);
            if entry.cloud.is_file() {
                clouds += 1;
                recall += u64::from(entry.cloud.recall_required());
                cloud_logical = cloud_logical
                    .checked_add(entry.logical)
                    .ok_or_else(|| Error::Invalid("Cloud path total overflow.".into()))?;
            }
            if entry.kind == Kind::File
                && entry.allocation_known
                && entry.file_state.is_some_and(|state| {
                    Some(state.identity.0) == index.root_identity.map(|identity| identity.0)
                })
            {
                // Current accounting already assigns hardlink allocation once across the entire index.
                known = known
                    .checked_add(entry.allocated)
                    .ok_or_else(|| Error::Invalid("Indexed allocation overflow.".into()))?;
                known_files += 1;
            } else {
                unknown_files += 1;
            }
        } else if entry.kind == Kind::Link {
            opaque += 1;
        }
    }
    let allocation = match accounting {
        Accounting::Unknown => Allocation::Unavailable {
            reason: AllocationReason::AccountingUnknown,
        },
        Accounting::Unsupported => Allocation::Unavailable {
            reason: AllocationReason::AccountingUnsupported,
        },
        Accounting::Known if index.root_identity.is_none() => Allocation::Unavailable {
            reason: AllocationReason::IdentityUnknown,
        },
        Accounting::Known
            if known > root.allocated
                || (unknown_files == 0
                    && opaque == 0
                    && root.allocation_known
                    && known != root.allocated) =>
        {
            Allocation::Unavailable {
                reason: AllocationReason::Inconsistent,
            }
        }
        Accounting::Known
            if known_files == 0
                && (unknown_files > 0 || opaque > 0 || coverage != Coverage::Complete) =>
        {
            Allocation::Unavailable {
                reason: AllocationReason::NoEvidence,
            }
        }
        Accounting::Known => Allocation::Measured {
            known: Bytes(known),
            known_file_records: known_files,
            unknown_file_records: unknown_files,
            complete: coverage == Coverage::Complete
                && root.allocation_known
                && unknown_files == 0
                && opaque == 0,
        },
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some(Indexed {
        root: index.root.clone(),
        snapshot: index.snapshot_id,
        observation: index.observation.clone(),
        coverage,
        accounting,
        exclusions: index.exclusions.len(),
        starts_at_volume_root: index.root.parent().is_none(),
        volume_serial: index
            .root_identity
            .map(|identity| format!("{:016x}", identity.0)),
        logical_paths: Bytes(root.logical),
        recorded_allocation: Bytes(root.allocated),
        allocation,
        file_paths: root.files,
        errors: index.error_count,
        hardlink_paths: hardlinks,
        cloud_file_paths: clouds,
        cloud_logical_paths: Bytes(cloud_logical),
        recall_required_file_paths: recall,
        opaque_reparse_records: opaque,
    }))
}

pub fn read_desktop(index: &Index, cancelled: &AtomicBool) -> Result<Option<Report>> {
    let Some(indexed) = summarize(index, cancelled)? else {
        return Ok(None);
    };
    let volume = match index.pin_folder(NodeId(0)) {
        Ok(pin) => {
            let observation = observe_volume(pin.path());
            if index.pin_folder(NodeId(0)).is_err() {
                VolumeContext::RootUnavailable
            } else {
                VolumeContext::Observed { observation }
            }
        }
        Err(_) => VolumeContext::RootUnavailable,
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some(Report::compare(indexed, volume)))
}
