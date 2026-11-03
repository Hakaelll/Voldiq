use super::{
    ReportItems, ReportSelection, StreamOutcome, StreamProgress,
    stream::{Record, walk},
};
use crate::{Entry, Error, NodeId, Result, index::Index};
use std::sync::atomic::{AtomicBool, Ordering};

const SHOWN_ROWS: usize = 50;

pub struct SummaryRow {
    pub entry: Entry,
    pub path: Option<SummaryPath>,
}
pub struct SummaryPath {
    pub text: String,
    pub exact: bool,
}
pub enum LogicalTotal {
    /// Observed path lengths include aliases; this is not unique allocation or resident content.
    FilePaths { bytes: u128 },
    /// Ancestor and descendant folder totals overlap and cannot be summed.
    OverlappingFolders,
}
pub struct ReportSummary {
    pub scope: serde_json::Value,
    pub progress: StreamProgress,
    pub rows: Vec<SummaryRow>,
    pub omitted: usize,
    pub logical: LogicalTotal,
    pub incomplete_matches: usize,
    pub unknown_allocation_matches: usize,
    pub unknown_date_matches: usize,
    pub recorded_hard_link_matches: usize,
}

/// Ranks every matching record but retains only 50 IDs, then resolves only the retained rows.
/// This is report data, not a PDF encoder, a fresh measurement or file-operation authority.
pub fn ranked_summary(
    index: &Index,
    selection: &ReportSelection,
    cancelled: &AtomicBool,
    progress: impl FnMut(StreamProgress),
) -> Result<Option<ReportSummary>> {
    let mut scope = serde_json::Value::Null;
    let mut top: Vec<(u64, u32)> = Vec::with_capacity(SHOWN_ROWS + 1);
    let mut logical = 0u128;
    let mut incomplete_matches = 0;
    let mut unknown_allocation_matches = 0;
    let mut unknown_date_matches = 0;
    let mut recorded_hard_link_matches = 0;
    let outcome = walk(index, selection, cancelled, progress, |record| {
        match record {
            Record::Scope(value) => scope = value,
            Record::Item(entry) => {
                if matches!(selection.items, ReportItems::Files) {
                    logical = logical
                        .checked_add(u128::from(entry.logical))
                        .ok_or_else(|| {
                            Error::Invalid("Report path total exceeds its supported range.".into())
                        })?;
                }
                incomplete_matches += usize::from(entry.incomplete);
                unknown_allocation_matches += usize::from(!entry.allocation_known);
                unknown_date_matches += usize::from(entry.modified.is_none());
                recorded_hard_link_matches += usize::from(entry.hard_link);
                // Largest recorded size first; smaller native index IDs win equal-size ties.
                let position = top.partition_point(|&(bytes, id)| {
                    bytes > entry.logical || (bytes == entry.logical && id < entry.id.0)
                });
                if position < SHOWN_ROWS {
                    top.insert(position, (entry.logical, entry.id.0));
                    if top.len() > SHOWN_ROWS {
                        top.pop();
                    }
                }
            }
            Record::Complete(_) => {}
        }
        Ok(true)
    })?;
    let StreamOutcome::Complete(progress) = outcome else {
        return Ok(None);
    };
    let mut rows = Vec::with_capacity(top.len());
    for (_, id) in top {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let entry = index.entry(NodeId(id))?.clone();
        let path = if selection.include_paths {
            let Some(details) =
                index.details_cancellable(entry.id, || cancelled.load(Ordering::Relaxed))?
            else {
                return Ok(None);
            };
            Some(SummaryPath {
                text: details.path,
                exact: details.path_exact,
            })
        } else {
            None
        };
        rows.push(SummaryRow { entry, path });
    }
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let omitted = progress.rows - rows.len();
    scope["report_kind"] = "ranked_summary".into();
    scope["traversal_order"] = scope["order"].clone();
    scope["order"] = "recorded_logical_bytes_descending_then_id".into();
    scope["shown_rows"] = rows.len().into();
    scope["omitted_rows"] = omitted.into();
    scope["matching_rows"] = progress.rows.into();
    scope["row_limit"] = SHOWN_ROWS.into();
    Ok(Some(ReportSummary {
        scope,
        progress,
        rows,
        omitted,
        logical: match selection.items {
            ReportItems::Files => LogicalTotal::FilePaths { bytes: logical },
            ReportItems::Folders => LogicalTotal::OverlappingFolders,
        },
        incomplete_matches,
        unknown_allocation_matches,
        unknown_date_matches,
        recorded_hard_link_matches,
    }))
}
