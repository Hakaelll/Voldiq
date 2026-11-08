//! Read-only attribution of indexed game and media files using bounded metadata.

use crate::{NodeId, index::Index};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

mod capture;
#[cfg(windows)]
mod games;
pub use capture::{Capture, capture};
pub(crate) use capture::{MAX_CAPTURE_BYTES, MAX_ROOTS};
pub mod media;
#[cfg(windows)]
mod scoped;

#[derive(Deserialize)]
pub struct Query {
    pub scope: NodeId,
    pub offset: usize,
    pub limit: usize,
    #[serde(default)]
    pub manual: Vec<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    Games,
    Media,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeadlessQuery {
    pub scope: NodeId,
    pub offset: usize,
    pub limit: usize,
}

impl Default for HeadlessQuery {
    fn default() -> Self {
        Self {
            scope: NodeId(0),
            offset: 0,
            limit: 25,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    Available {
        items: Vec<Game>,
        total: usize,
        recorded: bool,
        shared_bytes: String,
        unattributed_bytes: String,
        omitted_manifests: usize,
        coverage_partial: bool,
        manual: Vec<Option<ManualLibrary>>,
    },
    RecordedScan,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Steam,
    Epic,
    Gog,
    Itch,
}

#[derive(Serialize)]
pub struct Game {
    pub id: NodeId,
    pub provider: Provider,
    pub product: String,
    pub title: String,
    pub path: String,
    pub installation_bytes: Option<String>,
    pub shaders_bytes: Option<String>,
    pub workshop_bytes: Option<String>,
    pub user_data_bytes: Option<String>,
    pub dlc_bytes: Option<String>,
    pub dlc_records: usize,
    pub partial: bool,
}

#[derive(Serialize)]
pub struct ManualLibrary {
    pub id: NodeId,
    pub logical_bytes: String,
    pub partial: bool,
}

pub fn query(index: &Index, query: &Query, cancel: &AtomicBool) -> crate::Result<Option<Report>> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    validate(index, query)?;
    if index.snapshot_id.is_some() && index.captured_libraries.is_none() {
        return Ok(Some(Report::RecordedScan));
    }
    #[cfg(windows)]
    return games::query(index, query, cancel);
    #[cfg(not(windows))]
    Ok(Some(Report::Unsupported))
}

fn validate(index: &Index, query: &Query) -> crate::Result<()> {
    if index.entry(query.scope)?.kind != crate::Kind::Directory
        || !(1..=100).contains(&query.limit)
        || query.manual.len() > 8
        || query.manual.iter().any(|path| path.len() > 32768)
    {
        return Err(crate::Error::Invalid(
            "Invalid library scope, page or folder selection.".into(),
        ));
    }
    Ok(())
}
