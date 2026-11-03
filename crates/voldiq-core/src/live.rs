//! Explicit, metadata-only observation sessions. Notifications invalidate measurements;
//! they never establish file-operation authority or physical bytes written.
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{Control, Session};

use crate::{Kind, NodeId, index::Index};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Initial,
    Changes,
    LostEvents,
    Requested,
}

#[derive(Clone, Debug, Serialize)]
pub struct Measurement {
    pub logical: String,
    pub files: u64,
    pub measured_at: Option<u64>,
    pub complete: bool,
    pub accounting_version: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub reason: Reason,
    pub before: Measurement,
    pub after: Measurement,
    pub net_change: Option<String>,
}

pub struct Update {
    pub index: Index,
    pub report: Report,
}

/// Read-only navigation continuity. An opaque node number alone never identifies another index.
pub fn corresponding_directory(before: &Index, after: &Index, scope: NodeId) -> NodeId {
    if before.root_identity.is_none()
        || before.root_identity != after.root_identity
        || before.root != after.root
        || before.exclusions != after.exclusions
    {
        return NodeId(0);
    }
    let Some(identity) = before
        .entries
        .get(scope.0 as usize)
        .filter(|entry| entry.kind == Kind::Directory)
        .and_then(|entry| entry.file_state.as_ref())
        .map(|state| state.identity)
    else {
        return NodeId(0);
    };
    let mut matches = after.entries.iter().filter(|entry| {
        entry.kind == Kind::Directory
            && entry
                .file_state
                .as_ref()
                .is_some_and(|state| state.identity == identity)
    });
    match (matches.next(), matches.next()) {
        (Some(entry), None) => entry.id,
        _ => NodeId(0),
    }
}
