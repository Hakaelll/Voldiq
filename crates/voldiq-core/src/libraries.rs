//! Read-only attribution of indexed game files using bounded launcher metadata.

use crate::{NodeId, index::Index};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
mod games;

#[derive(Deserialize)]
pub struct Query {
    pub scope: NodeId,
    pub offset: usize,
    pub limit: usize,
    #[serde(default)]
    pub manual: Vec<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    Available {
        items: Vec<Game>,
        total: usize,
        shared_bytes: String,
        unattributed_bytes: String,
        omitted_manifests: usize,
        coverage_partial: bool,
        manual: Vec<Option<ManualLibrary>>,
    },
    RecordedScan,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
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
    if index.entry(query.scope)?.kind != crate::Kind::Directory
        || !(1..=100).contains(&query.limit)
        || query.manual.len() > 8
        || query.manual.iter().any(|path| path.len() > 32768)
    {
        return Err(crate::Error::Invalid(
            "Invalid game library scope, page or folder selection.".into(),
        ));
    }
    if index.snapshot_id.is_some() {
        return Ok(Some(Report::RecordedScan));
    }
    #[cfg(windows)]
    return games::query(index, query, cancel);
    #[cfg(not(windows))]
    Ok(Some(Report::Unsupported))
}
