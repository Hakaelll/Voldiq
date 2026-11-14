#[cfg(windows)]
use super::integer;
use super::unsigned;
use crate::Result;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const SCHEMA: &str = "
CREATE TABLE snapshot_volume_samples(
 snapshot_id INTEGER PRIMARY KEY REFERENCES snapshots(id) ON DELETE CASCADE,
 guid TEXT NOT NULL, serial TEXT NOT NULL, measured_at INTEGER NOT NULL,
 capacity INTEGER NOT NULL CHECK(capacity>0),
 free INTEGER NOT NULL CHECK(free>=0 AND free<=capacity));
CREATE INDEX snapshot_volume_observed ON snapshot_volume_samples(guid, serial, measured_at DESC, snapshot_id DESC);
PRAGMA user_version=14;";

const DAY: u64 = 86_400;

#[derive(Serialize)]
pub struct VolumeTrend {
    pub points: Vec<VolumePoint>,
    pub forecast: Option<VolumeForecast>,
}

#[derive(Serialize)]
pub struct VolumePoint {
    pub at: u64,
    pub free: String,
    pub capacity: String,
}

#[derive(Serialize)]
pub struct VolumeForecast {
    pub at: u64,
    pub earliest_at: u64,
    pub latest_at: u64,
    pub samples: usize,
    pub confidence: &'static str,
}

#[cfg(windows)]
pub(super) fn record(
    connection: &Connection,
    snapshot: i64,
    value: &crate::space::Measurement,
) -> Result<()> {
    let Some(at) = value.observed_at else {
        return Ok(());
    };
    let (Ok(capacity), Ok(free)) = (integer(value.total.0), integer(value.free.0)) else {
        return Ok(());
    };
    if capacity > 0 && free <= capacity {
        connection.execute(
            "INSERT INTO snapshot_volume_samples(snapshot_id,guid,serial,measured_at,capacity,free) VALUES(?1,?2,?3,?4,?5,?6)",
            params![snapshot, value.volume.guid_root.to_ascii_lowercase(), value.volume.serial, integer(at)?, capacity, free],
        )?;
    }
    Ok(())
}

struct Sample {
    at: u64,
    free: u64,
    capacity: u64,
}

pub(super) fn trend(
    connection: &Connection,
    root: i64,
    days: u16,
    cancel: &AtomicBool,
) -> Result<VolumeTrend> {
    // The latest saved capture selects the device. Older captures cannot substitute for a missing observation.
    let mut statement = connection.prepare(
        "WITH latest AS (
           SELECT v.guid,v.serial,v.measured_at
           FROM snapshot_volume_samples v
           WHERE v.snapshot_id=(SELECT s.id FROM snapshots s WHERE s.root_id=?1 ORDER BY s.id DESC LIMIT 1)
         )
         SELECT v.measured_at,v.free,v.capacity
         FROM latest l JOIN snapshot_volume_samples v ON v.guid=l.guid AND v.serial=l.serial
         WHERE v.measured_at BETWEEN MAX(0,l.measured_at-?2) AND l.measured_at
         AND v.snapshot_id=(SELECT x.snapshot_id FROM snapshot_volume_samples x
           WHERE x.guid=v.guid AND x.serial=v.serial AND x.measured_at=v.measured_at ORDER BY x.snapshot_id DESC LIMIT 1)
         ORDER BY v.measured_at DESC,v.snapshot_id DESC LIMIT 240",
    )?;
    let rows = statement.query_map(params![root, i64::from(days) * DAY as i64], |row| {
        Ok(Sample {
            at: unsigned(row, 0)?,
            free: unsigned(row, 1)?,
            capacity: unsigned(row, 2)?,
        })
    })?;
    let mut samples = Vec::new();
    for row in rows {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        samples.push(row?);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|value| value.as_secs());
    let forecast = now.and_then(|now| forecast(&samples, now));
    Ok(VolumeTrend {
        points: samples
            .into_iter()
            .rev()
            .map(|sample| VolumePoint {
                at: sample.at,
                free: sample.free.to_string(),
                capacity: sample.capacity.to_string(),
            })
            .collect(),
        forecast,
    })
}

fn forecast(samples: &[Sample], now: u64) -> Option<VolumeForecast> {
    let latest = samples.first()?;
    if latest.free == 0 || latest.at > now.saturating_add(60) || now.saturating_sub(latest.at) > DAY
    {
        return None;
    }
    let mut daily = vec![latest];
    let mut previous = latest;
    for sample in samples.iter().skip(1) {
        if latest.at - sample.at > 30 * DAY {
            break;
        }
        if sample.capacity != latest.capacity {
            break;
        }
        if sample.free < previous.free {
            return None;
        }
        previous = sample;
        if daily.last()?.at / DAY != sample.at / DAY {
            daily.push(sample);
        }
    }
    if daily.len() < 6 || latest.at - daily.last()?.at < 7 * DAY {
        return None;
    }
    let mut rates = Vec::new();
    for pair in daily.windows(2) {
        let elapsed = pair[0].at - pair[1].at;
        if elapsed > 7 * DAY {
            return None;
        }
        rates.push((pair[1].free - pair[0].free) as f64 / elapsed as f64);
    }
    rates.sort_by(f64::total_cmp);
    let low = rates[0];
    let high = *rates.last()?;
    if low <= 0.0 || high > 4.0 * low || latest.free as f64 / low > 366.0 * DAY as f64 {
        return None;
    }
    let when = |rate: f64| latest.at + (latest.free as f64 / rate).ceil() as u64;
    let earliest_at = when(high);
    if earliest_at <= now {
        return None;
    }
    Some(VolumeForecast {
        at: when(rates[rates.len() / 2]),
        earliest_at,
        latest_at: when(low),
        samples: daily.len(),
        confidence: if daily.len() >= 8 && high <= 1.5 * low {
            "higher"
        } else {
            "moderate"
        },
    })
}
