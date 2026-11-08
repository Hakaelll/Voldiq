//! A point-in-time Restart Manager observation of one indexed file name.

use crate::{NodeId, Result, index::Index};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
mod windows;

pub struct Target {
    #[cfg(windows)]
    native: windows::Target,
}

impl Target {
    pub fn from_index(index: &Index, id: NodeId) -> Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                native: windows::Target::from_index(index, id)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (index, id);
            Err(crate::Error::Invalid(
                "File use checks require Windows.".into(),
            ))
        }
    }

    pub fn read(self, cancelled: &AtomicBool) -> Result<Option<Report>> {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        #[cfg(windows)]
        {
            self.native.read(cancelled)
        }
        #[cfg(not(windows))]
        Err(crate::Error::Invalid(
            "File use checks require Windows.".into(),
        ))
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    Observed { applications: Vec<Application> },
    SharingConflict,
    PathLimit,
    Busy,
    ProcessLimit,
    WindowsError { code: u32 },
}

#[derive(Debug, Serialize)]
pub struct Application {
    pub pid: u32,
    pub name: String,
}
