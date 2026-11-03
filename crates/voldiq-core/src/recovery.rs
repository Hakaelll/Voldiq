//! Read-only recovery discovery. Catalogue identifiers never authorize file access or restoration.
use crate::{index::Index, space::VolumeIdentity};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(windows)]
mod native;

pub const COPY_LIMIT: usize = 100;
pub const ENUMERATION_WINDOW_MS: u64 = 5000;

#[derive(Clone, Debug, Serialize)]
pub struct ShadowCopy {
    pub id: String,
    pub provider_id: Option<String>,
    pub installed_at: Option<u64>,
    pub previous_versions: Option<bool>,
    pub persistent: Option<bool>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnumerationCoverage {
    Complete,
    ItemLimit,
    TimeLimit,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    Unsupported,
    RootUnavailable,
    AccessDenied,
    ProviderUnavailable,
    NativeFailure,
    InvalidMetadata,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Discovery {
    Available {
        copies: Vec<ShadowCopy>,
        coverage: EnumerationCoverage,
    },
    Unavailable {
        reason: Unavailable,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub root: PathBuf,
    pub volume: Option<VolumeIdentity>,
    pub observed_at: Option<u64>,
    pub copy_limit: usize,
    pub enumeration_window_ms: u64,
    pub shadows: Discovery,
}

/// Parse fully specified CIM timestamps at second precision. Wildcards and invalid dates stay unknown.
pub fn cim_datetime(value: &str) -> Option<u64> {
    if !value.is_ascii()
        || value.len() != 25
        || &value[14..15] != "."
        || !matches!(&value[21..22], "+" | "-")
        || !value[..14]
            .bytes()
            .chain(value[15..21].bytes())
            .chain(value[22..].bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let utc = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &value[..4],
        &value[4..6],
        &value[6..8],
        &value[8..10],
        &value[10..12],
        &value[12..14]
    );
    let wall = crate::search::utc_date(&utc, 0).ok()?.start;
    let offset = value[22..].parse::<u64>().ok()? * 60;
    if &value[21..22] == "+" {
        wall.checked_sub(offset)
    } else {
        wall.checked_add(offset)
    }
}

pub fn read_desktop(index: &Index, cancelled: &AtomicBool) -> Option<Report> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    #[cfg(windows)]
    let result = native::read(index, cancelled);
    #[cfg(not(windows))]
    let result = Some((
        None,
        Discovery::Unavailable {
            reason: Unavailable::Unsupported,
        },
    ));
    let (volume, shadows) = result?;
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    Some(Report {
        root: index.root.clone(),
        volume,
        observed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|value| value.as_secs()),
        copy_limit: COPY_LIMIT,
        enumeration_window_ms: ENUMERATION_WINDOW_MS,
        shadows,
    })
}
