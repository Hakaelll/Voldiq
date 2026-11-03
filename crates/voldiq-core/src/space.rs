//! Whole-volume counters and caller quota counters are different measurements.
//! Identifiers here describe observations; they never authorize a filesystem action.
#[cfg(windows)]
mod native;
mod reconcile;
use crate::{Error, Result};
pub use reconcile::{
    Accounting, Allocation, AllocationReason, Coverage, Indexed, Report, Residual, ResidualReason,
    VolumeContext, read_desktop, summarize,
};
use serde::{Serialize, Serializer};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bytes(pub u64);
impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct VolumeIdentity {
    pub guid_root: String,
    pub serial: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CallerSpace {
    pub volume: VolumeIdentity,
    pub observed_at: Option<u64>,
    pub total: Bytes,
    pub available: Bytes,
}

#[derive(Clone, Debug, Serialize)]
pub struct Measurement {
    pub volume: VolumeIdentity,
    pub observed_at: Option<u64>,
    pub total: Bytes,
    pub free: Bytes,
    pub used: Bytes,
    pub caller_total: Bytes,
    pub caller_available: Bytes,
    pub reserved: Bytes,
    /// A subset of reserved, never an additional deduction.
    pub storage_reserve: Bytes,
    pub pool_unavailable: Bytes,
}

/// Provider counters in allocation units, before any quota/physical distinction is lost.
#[derive(Clone, Copy)]
pub struct AllocationUnits {
    pub total: u64,
    pub free: u64,
    pub used: u64,
    pub caller_total: u64,
    pub caller_available: u64,
    pub reserved: u64,
    pub storage_reserve: u64,
    pub pool_unavailable: u64,
}
impl Measurement {
    pub fn from_units(
        volume: VolumeIdentity,
        observed_at: Option<u64>,
        unit_bytes: u64,
        units: AllocationUnits,
    ) -> Result<Self> {
        if unit_bytes == 0
            || units.free > units.total
            || units.used > units.total
            || units.caller_available > units.caller_total
            || units.reserved > units.total
            || units.storage_reserve > units.reserved
        {
            return Err(Error::Invalid("Inconsistent volume counters.".into()));
        }
        let bytes = |value: u64| {
            value
                .checked_mul(unit_bytes)
                .map(Bytes)
                .ok_or_else(|| Error::Invalid("Volume counter overflow.".into()))
        };
        Ok(Self {
            volume,
            observed_at,
            total: bytes(units.total)?,
            free: bytes(units.free)?,
            used: bytes(units.used)?,
            caller_total: bytes(units.caller_total)?,
            caller_available: bytes(units.caller_available)?,
            reserved: bytes(units.reserved)?,
            storage_reserve: bytes(units.storage_reserve)?,
            pool_unavailable: bytes(units.pool_unavailable)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    Unsupported,
    MetadataUnavailable,
    NativeFailure,
    InvalidCounters,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Observation {
    Measured {
        measurement: Measurement,
    },
    Unavailable {
        reason: Unavailable,
        caller: Option<CallerSpace>,
    },
}
pub fn observe_volume(path: &Path) -> Observation {
    #[cfg(windows)]
    {
        native::observe(path).unwrap_or(Observation::Unavailable {
            reason: Unavailable::MetadataUnavailable,
            caller: None,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Observation::Unavailable {
            reason: Unavailable::Unsupported,
            caller: None,
        }
    }
}
