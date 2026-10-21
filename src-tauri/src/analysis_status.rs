use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use voldiq_core::{Progress, ScanPhase, scanner::SharedIndex};

#[derive(Clone)]
pub(crate) struct LoadedAnalysis {
    pub index: SharedIndex,
    pub analyzed_at: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct ScanProgress {
    #[serde(flatten)]
    pub progress: Progress,
    pub analyzed_at: Option<u64>,
}

pub(crate) fn analyzed_at(
    progress: &Progress,
    previous: Option<u64>,
    now: SystemTime,
) -> Option<u64> {
    if progress.previous {
        return previous;
    }
    match progress.phase {
        ScanPhase::Complete | ScanPhase::Cancelled if progress.entries > 0 => now
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|time| time.as_secs()),
        _ => None,
    }
}
