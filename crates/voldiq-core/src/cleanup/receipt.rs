//! Read-only accounting for an explicitly confirmed local desktop cleanup.
use super::{Plan, execute, recycle::batch};
use crate::{
    NodeId,
    space::{Bytes, Measurement, VolumeIdentity, measure_index_volume},
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Recycled,
    PermanentlyRemoved,
}

#[derive(Serialize)]
pub struct LogicalFiles {
    pub files: usize,
    pub logical: Bytes,
}

impl LogicalFiles {
    fn empty() -> Self {
        Self {
            files: 0,
            logical: Bytes(0),
        }
    }

    fn record(&mut self, logical: u64) {
        self.files += 1;
        self.logical.0 += logical;
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VolumeChange {
    Observed {
        volume: VolumeIdentity,
        capacity: Bytes,
        before_free: Bytes,
        after_free: Bytes,
        before_at: u64,
        after_at: u64,
        delta: String,
    },
    Unavailable,
}

impl VolumeChange {
    pub(super) fn between(before: &Measurement, after: &Measurement) -> Self {
        let (Some(before_at), Some(after_at)) = (before.observed_at, after.observed_at) else {
            return Self::Unavailable;
        };
        if !before
            .volume
            .guid_root
            .eq_ignore_ascii_case(&after.volume.guid_root)
            || before.volume.serial != after.volume.serial
            || before.total.0 == 0
            || before.total != after.total
            || before_at > after_at
        {
            return Self::Unavailable;
        }
        Self::Observed {
            volume: after.volume.clone(),
            capacity: after.total,
            before_free: before.free,
            after_free: after.free,
            before_at,
            after_at,
            delta: (i128::from(after.free.0) - i128::from(before.free.0)).to_string(),
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProtectedCopies {
    VerifiedKeepers { files: usize, logical: String },
    Unavailable,
    NotApplicable,
}

#[derive(Serialize)]
pub struct Receipt {
    pub disposition: Disposition,
    pub selected: LogicalFiles,
    pub confirmed: LogicalFiles,
    pub unrecorded: LogicalFiles,
    pub potential_allocation: Bytes,
    pub protected: ProtectedCopies,
    /// Whole-volume counters include other activity during the observation window.
    pub volume_change: VolumeChange,
}

#[derive(Serialize)]
pub struct WithReceipt<T> {
    #[serde(flatten)]
    pub report: T,
    pub receipt: Receipt,
}

pub struct PendingReceipt {
    selection: HashMap<NodeId, SelectedFile>,
    keepers: Vec<Keeper>,
    selected: LogicalFiles,
    potential_allocation: Bytes,
    volume: Option<BeforeVolume>,
}

struct SelectedFile {
    logical: u64,
    keeper_group: Option<String>,
}

struct Keeper {
    identity: (u64, u64),
    logical: u64,
    group: String,
}

struct BeforeVolume {
    path: PathBuf,
    identity: (u64, u64),
    measurement: Measurement,
}

impl BeforeVolume {
    fn capture(plan: &Plan) -> Option<Self> {
        let (path, identity) = plan.files.first()?.parents.last()?;
        if plan.files.iter().any(|file| {
            file.stamp.identity.0 != identity.0
                || file
                    .parents
                    .last()
                    .is_none_or(|(_, ancestor)| ancestor != identity)
        }) {
            return None;
        }
        // Each read independently pins the frozen ancestor. No handle survives into execution.
        let measurement = measure_index_volume(path, *identity)?;
        Some(Self {
            path: path.clone(),
            identity: *identity,
            measurement,
        })
    }

    fn finish(self) -> VolumeChange {
        let Some(after) = measure_index_volume(&self.path, self.identity) else {
            return VolumeChange::Unavailable;
        };
        VolumeChange::between(&self.measurement, &after)
    }
}

impl PendingReceipt {
    /// Capture selection evidence before passing the exact plan to a desktop executor.
    pub fn start(plan: &Plan) -> Self {
        Self {
            selection: plan
                .files
                .iter()
                .map(|file| {
                    (
                        file.id,
                        SelectedFile {
                            logical: file.stamp.size,
                            keeper_group: file.digest.clone(),
                        },
                    )
                })
                .collect(),
            keepers: plan
                .retained
                .iter()
                .map(|(file, digest)| Keeper {
                    identity: file.stamp.identity,
                    logical: file.stamp.size,
                    group: digest.clone(),
                })
                .collect(),
            selected: LogicalFiles {
                files: plan.files.len(),
                logical: Bytes(plan.preview.logical),
            },
            potential_allocation: Bytes(plan.preview.reclaimable),
            volume: BeforeVolume::capture(plan),
        }
    }

    pub fn after_recycling(self, report: batch::Report) -> WithReceipt<batch::Report> {
        let confirmed = report
            .items
            .iter()
            .filter(|item| item.outcome == batch::Outcome::Recycled)
            .map(|item| (item.id, item.recorded));
        let receipt = self.finish(Disposition::Recycled, confirmed);
        WithReceipt { report, receipt }
    }

    pub fn after_permanent(self, report: execute::Report) -> WithReceipt<execute::Report> {
        let confirmed = report
            .items
            .iter()
            .filter(|item| item.removed)
            .map(|item| (item.id, item.recorded));
        let receipt = self.finish(Disposition::PermanentlyRemoved, confirmed);
        WithReceipt { report, receipt }
    }

    fn finish(
        mut self,
        disposition: Disposition,
        results: impl Iterator<Item = (NodeId, bool)>,
    ) -> Receipt {
        let mut confirmed = LogicalFiles::empty();
        let mut unrecorded = LogicalFiles::empty();
        let mut keeper_groups = HashSet::new();
        for (id, recorded) in results {
            if let Some(file) = self.selection.remove(&id) {
                confirmed.record(file.logical);
                if let Some(group) = file.keeper_group {
                    keeper_groups.insert(group);
                }
                if !recorded {
                    unrecorded.record(file.logical);
                }
            }
        }
        Receipt {
            disposition,
            selected: self.selected,
            confirmed,
            unrecorded,
            potential_allocation: self.potential_allocation,
            protected: protected_copies(&self.keepers, &keeper_groups),
            volume_change: self
                .volume
                .map_or(VolumeChange::Unavailable, BeforeVolume::finish),
        }
    }
}

fn protected_copies(keepers: &[Keeper], proven_groups: &HashSet<String>) -> ProtectedCopies {
    if keepers.is_empty() {
        return ProtectedCopies::NotApplicable;
    }
    // Both executors verify and retain the keeper handles before a duplicate can be confirmed.
    let mut identities = HashSet::new();
    let logical = keepers
        .iter()
        .filter(|file| proven_groups.contains(&file.group) && identities.insert(file.identity))
        .map(|file| u128::from(file.logical))
        .sum::<u128>();
    if identities.is_empty() {
        ProtectedCopies::Unavailable
    } else {
        ProtectedCopies::VerifiedKeepers {
            files: identities.len(),
            logical: logical.to_string(),
        }
    }
}
