use crate::ScanConcurrency;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanProfile {
    #[default]
    Auto,
    Eco,
    Balanced,
    Fast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanResources {
    pub profile: ScanProfile,
    pub workers: usize,
    pub batch_entries: usize,
    pub ntfs_buffer_bytes: usize,
}

impl ScanProfile {
    pub fn resources(self, concurrency: ScanConcurrency, path: &Path) -> ScanResources {
        let requested = concurrency.0.map_or_else(
            || {
                std::thread::available_parallelism()
                    .map_or(1, usize::from)
                    .clamp(1, 8)
            },
            usize::from,
        );
        let workers = match self {
            Self::Auto if requested == 1 => 1,
            Self::Auto => ScanConcurrency::automatic().for_path(path).min(requested),
            Self::Eco => 1,
            Self::Balanced => requested.min(2),
            Self::Fast => requested,
        };
        ScanResources {
            profile: self,
            workers,
            batch_entries: workers * 16,
            ntfs_buffer_bytes: self.ntfs_buffer_bytes(),
        }
    }

    pub(crate) const fn ntfs_buffer_bytes(self) -> usize {
        match self {
            Self::Eco => 64 * 1024,
            Self::Auto | Self::Balanced => 256 * 1024,
            Self::Fast => 1024 * 1024,
        }
    }

    pub(crate) fn checkpoint(self, cancel: &AtomicBool) -> bool {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        if self == Self::Eco {
            std::thread::sleep(Duration::from_millis(2));
        }
        !cancel.load(Ordering::Relaxed)
    }
}
