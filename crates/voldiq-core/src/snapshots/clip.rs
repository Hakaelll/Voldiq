use super::{Accounting, Store, VisualFrame};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
};

const FRAME_LIMIT: usize = 60;
const BYTE_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportClipRequest {
    pub snapshot_ids: Vec<i64>,
    #[serde(default)]
    pub include_paths: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ExportClipProgress {
    pub prepared_frames: usize,
    pub total_frames: usize,
}

#[derive(Serialize)]
pub struct ExportClipPath {
    pub path: String,
    pub exact: bool,
}

/// Recorded summaries only. A fingerprint binds export evidence, never file-operation authority.
#[derive(Serialize)]
pub struct ExportClip {
    pub schema_version: u8,
    pub root_key: i64,
    pub scope_fingerprint: String,
    pub root_path: Option<ExportClipPath>,
    pub exclusions: Option<Vec<ExportClipPath>>,
    pub exclusion_count: usize,
    pub paths_included: bool,
    pub units: &'static str,
    pub selection: &'static str,
    pub accounting: Accounting,
    pub maximum_logical: String,
    pub partial_frames: usize,
    pub legacy_frames: usize,
    pub limitations: &'static str,
    pub frames: Vec<VisualFrame>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fingerprint: String,
}

fn invalid(message: &str) -> Error {
    Error::Invalid(format!("Invalid history export: {message}"))
}

struct Budget<'a> {
    bytes: usize,
    hash: blake3::Hasher,
    cancel: &'a AtomicBool,
}
impl Write for Budget<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::other("History export cancelled."));
        }
        if bytes.len() > (BYTE_LIMIT - 128).saturating_sub(self.bytes) {
            return Err(io::Error::other(
                "History export exceeds its 8 MiB metadata limit.",
            ));
        }
        self.bytes += bytes.len();
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Store {
    /// Freeze at most sixty bounded root frames within one SQLite read transaction.
    /// Does not load full indexes, inspect live paths, hydrate cloud files or modify captures.
    pub fn export_clip(
        &self,
        request: &ExportClipRequest,
        cancel: &AtomicBool,
        mut progress: impl FnMut(ExportClipProgress),
    ) -> Result<Option<ExportClip>> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if !(2..=FRAME_LIMIT).contains(&request.snapshot_ids.len()) {
            return Err(invalid("choose between 2 and 60 captures"));
        }
        let mut seen = HashSet::new();
        if request
            .snapshot_ids
            .iter()
            .any(|id| *id <= 0 || !seen.insert(*id))
        {
            return Err(invalid("capture identities must be positive and distinct"));
        }
        let _read = self.history_read()?;
        let mut sources = Vec::with_capacity(request.snapshot_ids.len());
        for id in &request.snapshot_ids {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let (root, captured): (i64, u64) = self.connection.query_row(
                "SELECT root_id,captured FROM snapshots WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, super::unsigned(row, 1)?)),
            )?;
            if sources.first().is_some_and(|(group, _, _)| *group != root) {
                return Err(invalid(
                    "captures must share the recorded root, volume and exclusions",
                ));
            }
            sources.push((root, captured, *id));
        }
        sources.sort_unstable_by_key(|(_, captured, id)| (*captured, *id));
        let (root_key, _, first) = sources[0];
        let scope = self.scope(first)?;
        let mut scope_hash = blake3::Hasher::new();
        scope_hash.update(b"Voldiq recorded history scope v1\0");
        for bytes in [
            super::encode(scope.root.as_os_str()),
            [
                scope.identity.0.to_le_bytes(),
                scope.identity.1.to_le_bytes(),
            ]
            .concat(),
            super::encode_paths(&scope.exclusions)?,
        ] {
            scope_hash.update(
                &u64::try_from(bytes.len())
                    .map_err(|_| invalid("source metadata is too large"))?
                    .to_le_bytes(),
            );
            scope_hash.update(&bytes);
        }
        let path = |value: &std::path::Path| ExportClipPath {
            path: value.to_string_lossy().into_owned(),
            exact: value.to_str().is_some(),
        };
        let mut clip = ExportClip {
            schema_version: 1,
            root_key,
            scope_fingerprint: scope_hash.finalize().to_hex().to_string(),
            root_path: request.include_paths.then(|| path(&scope.root)),
            exclusions: request
                .include_paths
                .then(|| scope.exclusions.iter().map(|value| path(value)).collect()),
            exclusion_count: scope.exclusions.len(),
            paths_included: request.include_paths,
            units: "Exact logical path bytes encoded as decimal strings; not unique physical allocation or reclaimable space.",
            selection: "Requested recorded captures in chronological save order; root-level summaries, no search or cloud filter. At most 160 ranked entries plus Other per frame. Scale is the largest recorded logical root in this clip.",
            accounting: Accounting::Compatible,
            maximum_logical: "0".into(),
            partial_frames: 0,
            legacy_frames: 0,
            limitations: "Real recorded summaries, not fresh scans or file-by-file animation. Saved dates and available observation dates remain distinct. Partial sizes are lower bounds; missing legacy provenance is unknown. Playback timing is illustrative, not elapsed real time. Grouping and blank area are not free disk space. Labels contain file names. This evidence and its fingerprint grant no operation permission.",
            frames: Vec::with_capacity(sources.len()),
            fingerprint: String::new(),
        };
        let mut maximum = 0_u64;
        let mut budget = Budget {
            bytes: 0,
            hash: blake3::Hasher::new(),
            cancel,
        };
        progress(ExportClipProgress {
            prepared_frames: 0,
            total_frames: sources.len(),
        });
        for (_, _, id) in &sources {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let Some(frame) = self.visual_capture_in_read(*id, cancel)? else {
                return Ok(None);
            };
            let logical = frame
                .logical
                .as_deref()
                .ok_or_else(|| invalid("capture has no recorded root size"))?
                .parse::<u64>()
                .map_err(|_| invalid("capture root size is invalid"))?;
            maximum = maximum.max(logical);
            clip.partial_frames += usize::from(frame.incomplete);
            clip.legacy_frames += usize::from(frame.observation.is_none());
            let encoded = serde_json::to_writer(&mut budget, &frame);
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            encoded.map_err(|_| invalid("frame summaries exceed the metadata limit"))?;
            clip.frames.push(frame);
            progress(ExportClipProgress {
                prepared_frames: clip.frames.len(),
                total_frames: sources.len(),
            });
        }
        clip.maximum_logical = maximum.to_string();
        if clip.legacy_frames > 0 {
            clip.accounting = Accounting::Unknown;
        }
        budget.bytes = 0;
        budget.hash = blake3::Hasher::new();
        budget.hash.update(b"Voldiq history export clip v1\0");
        let encoded = serde_json::to_writer(&mut budget, &clip);
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        encoded.map_err(|_| invalid("clip exceeds the metadata limit"))?;
        clip.fingerprint = budget.hash.finalize().to_hex().to_string();
        Ok(Some(clip))
    }
}
