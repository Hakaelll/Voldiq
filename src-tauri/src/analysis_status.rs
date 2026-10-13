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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn terminal_dates_distinguish_fresh_partial_and_retained_results() {
        let now = UNIX_EPOCH + Duration::from_secs(1700000000);
        let mut progress = Progress {
            previous: false,
            phase: ScanPhase::Complete,
            entries: 1,
            files: 0,
            logical: 0,
            elapsed_ms: 1,
            errors: 0,
            method: "Directory enumeration".into(),
        };
        assert_eq!(analyzed_at(&progress, Some(100), now), Some(1700000000));
        progress.phase = ScanPhase::Cancelled;
        assert_eq!(analyzed_at(&progress, Some(100), now), Some(1700000000));
        progress.previous = true;
        assert_eq!(analyzed_at(&progress, Some(100), now), Some(100));
        assert_eq!(analyzed_at(&progress, None, now), None);
        progress.previous = false;
        progress.entries = 0;
        assert_eq!(analyzed_at(&progress, Some(100), now), None);
        progress.entries = 1;
        progress.phase = ScanPhase::Failed;
        assert_eq!(analyzed_at(&progress, Some(100), now), None);
        progress.phase = ScanPhase::Scanning;
        assert_eq!(analyzed_at(&progress, Some(100), now), None);
        progress.phase = ScanPhase::Complete;
        assert_eq!(
            analyzed_at(&progress, None, UNIX_EPOCH - Duration::from_secs(1)),
            None
        );
        let payload = serde_json::to_value(ScanProgress {
            progress,
            analyzed_at: Some(100),
        })
        .unwrap();
        assert_eq!(payload["phase"], "complete");
        assert_eq!(payload["analyzed_at"], 100);
        assert!(payload.get("progress").is_none());
    }
}
