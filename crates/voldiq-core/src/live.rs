//! Explicit, metadata-only observation sessions. Notifications invalidate measurements;
//! they never establish file-operation authority or physical bytes written.
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{Control, Session};

use crate::index::Index;
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
