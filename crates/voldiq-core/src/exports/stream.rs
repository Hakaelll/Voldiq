use super::{CsvItems, HEADER, row, text};
use crate::{
    NodeId, Result,
    cloud::CloudFilter,
    index::{Index, Query},
};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Filter semantics are shared with Query; streaming deliberately has no sort or pagination fields.
#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CsvFilter {
    pub parent: Option<NodeId>,
    pub scope: Option<NodeId>,
    pub text: String,
    pub expression: String,
    pub extension: String,
    pub without_extension: bool,
    pub min_size: u64,
    pub modified_before: Option<u64>,
    pub cloud: Option<CloudFilter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvStreamRequest {
    pub items: CsvItems,
    #[serde(default)]
    pub filter: CsvFilter,
    #[serde(default)]
    pub include_paths: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct StreamProgress {
    pub inspected: usize,
    pub rows: usize,
    pub total_entries: usize,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "progress", rename_all = "snake_case")]
pub enum StreamOutcome {
    Complete(StreamProgress),
    Cancelled(StreamProgress),
}

fn context(writer: &mut impl Write, kind: &str, data: serde_json::Value) -> Result<()> {
    let mut columns = vec![String::new(); 11];
    columns[0] = kind.into();
    columns[10] = text(&data.to_string());
    writer.write_all(columns.join(",").as_bytes())?;
    writer.write_all(b"\r\n")?;
    Ok(())
}

/// Writes a report prefix on cancellation or I/O failure. Publish persistent output only on Complete.
pub fn stream_csv(
    index: &Index,
    request: &CsvStreamRequest,
    writer: &mut impl Write,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(StreamProgress),
) -> Result<StreamOutcome> {
    let mut state = StreamProgress {
        inspected: 0,
        rows: 0,
        total_entries: index.entries.len().saturating_sub(1),
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ok(StreamOutcome::Cancelled(state));
    }
    let filter = &request.filter;
    let query = Query {
        parent: filter.parent,
        scope: filter.scope,
        text: filter.text.clone(),
        expression: filter.expression.clone(),
        extension: filter.extension.clone(),
        without_extension: filter.without_extension,
        min_size: filter.min_size,
        modified_before: filter.modified_before,
        cloud: filter.cloud,
        kind: Some(match request.items {
            CsvItems::Files => crate::Kind::File,
            CsvItems::Folders => crate::Kind::Directory,
        }),
        ..Default::default()
    };
    let compiled = query.compile()?;
    index.validate_query_scope(&compiled)?;
    let root = index.entry(NodeId(0))?;
    progress(state);
    if cancelled.load(Ordering::Relaxed) {
        return Ok(StreamOutcome::Cancelled(state));
    }
    writer.write_all(format!("record_type,{HEADER},context_json\r\n").as_bytes())?;
    context(
        writer,
        "scope",
        serde_json::json!({
            "schema_version":1, "snapshot_id":index.snapshot_id,
            "generated_at":SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|time|time.as_secs()),
            "observation":index.observation, "root_path":request.include_paths.then(|| index.root.to_string_lossy()),
            "exclusions":index.exclusions.len(), "access_issues":index.error_count, "root_incomplete":root.incomplete,
            "coverage":match index.observation.as_ref() { None=>"unknown",Some(value) if value.complete && !root.incomplete && index.error_count==0=>"complete",_=>"partial" },
            "filter":filter,"items":request.items,"include_paths":request.include_paths,"order":"recorded_index",
            "units":"bytes; Unix seconds UTC", "text_encoding":"Name/path/context cells retain literal text: prefixes; keep them in spreadsheet workflows. Import large numeric values as text.",
            "limitations":"Recorded metadata, not a fresh scan or file authority. Names are display strings. Allocation requires allocation_known; blank dates are unknown. Cloud metadata does not establish resident content. A completion record establishes encoder completion only, not durable publication."
        }),
    )?;
    let mut budget = crate::search::GLOB_WORK_LIMIT;
    let mut notified = Instant::now();
    for (position, entry) in index.entries.iter().skip(1).enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(StreamOutcome::Cancelled(state));
        }
        state.inspected = position + 1;
        let Some(matches) =
            index.matches_compiled_cancellable(&compiled, entry, &mut budget, &mut || {
                cancelled.load(Ordering::Relaxed)
            })?
        else {
            return Ok(StreamOutcome::Cancelled(state));
        };
        if matches {
            let Some(line) = row(index, entry, request.include_paths, cancelled)? else {
                return Ok(StreamOutcome::Cancelled(state));
            };
            writer.write_all(b"item,")?;
            writer.write_all(line.as_bytes())?;
            writer.write_all(b",\r\n")?;
            state.rows += 1;
        }
        if position % 256 == 0 && notified.elapsed() >= Duration::from_millis(100) {
            progress(state);
            notified = Instant::now();
        }
    }
    progress(state);
    if cancelled.load(Ordering::Relaxed) {
        return Ok(StreamOutcome::Cancelled(state));
    }
    context(
        writer,
        "complete",
        serde_json::json!({"rows":state.rows,"inspected":state.inspected}),
    )?;
    Ok(StreamOutcome::Complete(state))
}
