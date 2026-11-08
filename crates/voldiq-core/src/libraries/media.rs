use super::{ManualLibrary, Query};
use crate::{NodeId, Result, index::Index};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
pub(super) mod indexed;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    Available {
        items: Vec<Library>,
        total: usize,
        recorded: bool,
        shared_bytes: String,
        unattributed_bytes: String,
        omitted_signatures: usize,
        coverage_partial: bool,
        manual: Vec<Option<ManualLibrary>>,
    },
    RecordedScan,
    Unsupported,
}

#[derive(Serialize)]
pub struct Library {
    pub anchor: NodeId,
    pub folder: NodeId,
    pub provider: Provider,
    pub name: String,
    pub path: String,
    pub sizes: Vec<Size>,
    pub partial: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    LightroomClassic,
    Darktable,
    RawTherapee,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    SourceImages,
    Derivatives,
    Previews,
    Proxies,
    Catalog,
    EditProfiles,
}

#[derive(Serialize)]
pub struct Size {
    pub category: Category,
    pub logical_bytes: String,
}

pub fn query(index: &Index, query: &Query, cancel: &AtomicBool) -> Result<Option<Report>> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    super::validate(index, query)?;
    if index.snapshot_id.is_some() && index.captured_libraries.is_none() {
        return Ok(Some(Report::RecordedScan));
    }
    #[cfg(windows)]
    return indexed::query(index, query, cancel);
    #[cfg(not(windows))]
    Ok(Some(Report::Unsupported))
}
