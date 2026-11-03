//! Paged and streaming reports with explicit privacy and literal spreadsheet text.
use crate::{
    Error, Kind, Result,
    index::{Index, Query},
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
mod stream;
pub use stream::{ReportFilter, ReportSelection, StreamOutcome, StreamProgress, stream_csv};
mod xlsx;
pub use xlsx::stream_xlsx;
#[cfg(windows)]
mod file;
#[cfg(windows)]
pub use file::save_report;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportFormat {
    Csv,
    Xlsx,
}
impl ReportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Xlsx => "xlsx",
        }
    }
}

const HEADER: &str = "node_id,kind,name,logical_bytes,recorded_allocated_bytes,allocation_known,modified_unix_seconds,path,path_exact";

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportItems {
    Files,
    Folders,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvRequest {
    pub items: ReportItems,
    #[serde(default)]
    pub query: Query,
    #[serde(default)]
    pub include_paths: bool,
}

#[derive(Serialize)]
pub struct CsvPage {
    pub snapshot_id: Option<i64>,
    pub observation: Option<crate::index::Observation>,
    pub scope_root: Option<String>,
    pub exclusions: usize,
    pub scope_incomplete: bool,
    pub query: serde_json::Value,
    pub filter_text_included: bool,
    pub items: ReportItems,
    pub units: &'static str,
    pub text_encoding: &'static str,
    pub paths_included: bool,
    pub mime: &'static str,
    pub rows: usize,
    pub total_matching: usize,
    pub next_offset: Option<usize>,
    pub csv: String,
}

fn text(value: &str) -> String {
    format!("\"text: {}\"", value.replace('"', "\"\""))
}

fn filter_metadata(filter: &impl Serialize, include_text: bool) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(filter)
        .map_err(|_| Error::Invalid("Unable to encode report filter metadata.".into()))?;
    if !include_text {
        for field in ["text", "expression", "extension"] {
            if value
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|text| !text.is_empty())
            {
                value[field] = serde_json::Value::Null;
            }
        }
    }
    Ok(value)
}

fn row(
    index: &Index,
    entry: &crate::Entry,
    include_paths: bool,
    cancelled: &AtomicBool,
) -> Result<Option<String>> {
    let (path, exact) = if include_paths {
        let Some(details) =
            index.details_cancellable(entry.id, || cancelled.load(Ordering::Relaxed))?
        else {
            return Ok(None);
        };
        (text(&details.path), details.path_exact.to_string())
    } else {
        (String::new(), String::new())
    };
    let kind = match entry.kind {
        Kind::File => "file",
        Kind::Directory => "directory",
        Kind::Link => "link",
    };
    Ok(Some(format!(
        "{},{},{},{},{},{},{},{},{}",
        entry.id.0,
        kind,
        text(&entry.name),
        entry.logical,
        entry.allocated,
        entry.allocation_known,
        entry
            .modified
            .map(|value| value.to_string())
            .unwrap_or_default(),
        path,
        exact
    )))
}

pub fn csv(index: &Index, request: &CsvRequest, cancelled: &AtomicBool) -> Result<Option<CsvPage>> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let query = Query {
        kind: Some(match request.items {
            ReportItems::Files => Kind::File,
            ReportItems::Folders => Kind::Directory,
        }),
        files_only: false,
        ..request.query.clone()
    };
    if query.offset > 10_000 || !(1..=500).contains(&query.limit) {
        return Err(Error::Invalid("CSV page bounds exceeded.".into()));
    }
    let Some(page) = index.query_cancellable(&query, || cancelled.load(Ordering::Relaxed))? else {
        return Ok(None);
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let mut csv = format!("{HEADER}\r\n");
    for entry in &page.items {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let Some(line) = row(index, entry, request.include_paths, cancelled)? else {
            return Ok(None);
        };
        csv.push_str(&line);
        csv.push_str("\r\n");
        if csv.len() > 262_144 {
            return Err(Error::Invalid(
                "CSV page exceeds 256 KiB. Reduce the page limit.".into(),
            ));
        }
    }
    let root = index.entry(crate::NodeId(0))?;
    Ok(Some(CsvPage {
        snapshot_id: index.snapshot_id,
        observation: index.observation.clone(),
        scope_root: request
            .include_paths
            .then(|| index.root.to_string_lossy().into_owned()),
        exclusions: index.exclusions.len(),
        scope_incomplete: root.incomplete,
        items: request.items,
        units: "bytes; Unix seconds UTC; recorded allocation requires allocation_known",
        text_encoding: "Every name/path CSV cell begins with literal text: . Preserve this prefix in spreadsheets; remove exactly one prefix only when decoding as data. Numeric cells contain exact decimal digits; import as text to preserve large integers.",
        paths_included: request.include_paths,
        mime: "text/csv; charset=utf-8",
        rows: page.items.len(),
        total_matching: page.total,
        next_offset: (query.offset.saturating_add(page.items.len()) < page.total)
            .then_some(query.offset.saturating_add(page.items.len())),
        query: filter_metadata(&query, request.include_paths)?,
        filter_text_included: request.include_paths,
        csv,
    }))
}
