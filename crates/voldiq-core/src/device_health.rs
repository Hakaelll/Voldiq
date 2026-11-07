//! An on-demand observation of the Windows disks associated with the live index root.

use crate::{Kind, NodeId, index::Index};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
mod native;

pub struct Target {
    root: PathBuf,
    binding: Result<(u64, u64), Unavailable>,
}

impl Target {
    pub fn from_index(index: &Index) -> Self {
        let binding = if index.snapshot_id.is_some() {
            Err(Unavailable::RecordedScan)
        } else if index
            .entry(NodeId(0))
            .is_ok_and(|root| root.kind == Kind::Directory)
        {
            index.root_identity.ok_or(Unavailable::RootChanged)
        } else {
            Err(Unavailable::RootChanged)
        };
        Self {
            root: index.root.clone(),
            binding,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    RecordedScan,
    RootChanged,
    VolumeUnmapped,
    ExtentLimit,
    MappingChanged,
    AccessDenied,
    Unsupported,
    InvalidResponse,
    DriverError,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub root: PathBuf,
    pub checked_at: Option<u64>,
    pub observation: Observation,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Observation {
    Disks { items: Vec<Disk> },
    Unavailable { reason: Unavailable },
}

#[derive(Debug, Serialize)]
pub struct Disk {
    pub number: u32,
    pub health: Health,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Health {
    Nvme {
        critical_warning: u8,
        temperature_kelvin: Option<u16>,
        power_on_hours: String,
        unsafe_shutdowns: String,
        media_errors: String,
    },
    WindowsPrediction {
        alert: bool,
    },
    Unavailable {
        reason: Unavailable,
    },
}

pub fn read(target: Target, cancelled: &AtomicBool) -> Option<Report> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let observation = match target.binding {
        Err(reason) => Observation::Unavailable { reason },
        Ok(identity) => {
            #[cfg(windows)]
            let outcome = native::read(&target.root, identity, cancelled);
            #[cfg(not(windows))]
            let outcome: Result<Vec<Disk>, Unavailable> = {
                let _ = identity;
                Err(Unavailable::Unsupported)
            };
            match outcome {
                Ok(items) => Observation::Disks { items },
                Err(reason) => Observation::Unavailable { reason },
            }
        }
    };
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    Some(Report {
        root: target.root,
        checked_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|now| now.as_secs()),
        observation,
    })
}
