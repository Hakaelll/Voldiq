use super::{Store, decode, read_observation, unsigned};
use crate::{
    Error, Result,
    index::{ACCOUNTING_VERSION, Observation},
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

const DAY: u64 = 86_400;
const MAX_POINTS: usize = 240;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrendRequest {
    pub snapshot: i64,
    pub days: u16,
}

#[derive(Serialize)]
pub struct TrendReport {
    pub anchor: i64,
    pub root: String,
    pub points: Vec<TrendPoint>,
    pub limited: bool,
    pub missing_dates: bool,
    pub forecast: Option<TrendForecast>,
}

#[derive(Serialize)]
pub struct TrendPoint {
    pub snapshot: i64,
    pub measured_at: u64,
    pub logical: String,
    pub complete: bool,
    pub supported: bool,
    pub previous: Option<i64>,
    pub delta: Option<String>,
}

struct Measurement {
    id: i64,
    logical: u64,
    complete: bool,
    observation: Observation,
}

impl Measurement {
    fn comparable(&self, other: &Self) -> bool {
        self.complete
            && other.complete
            && self.observation.accounting_version == ACCOUNTING_VERSION
            && other.observation.accounting_version == ACCOUNTING_VERSION
            && self.observation.engine_version == other.observation.engine_version
            && self.observation.completed_at > other.observation.completed_at
    }
}

#[derive(Serialize)]
pub struct TrendForecast {
    pub at: u64,
    pub logical: String,
    pub low: String,
    pub high: String,
    pub samples: usize,
    pub confidence: &'static str,
}

#[derive(Serialize)]
pub struct TrendChanges {
    pub before: i64,
    pub after: i64,
    pub grew: Vec<FolderChange>,
    pub shrank: Vec<FolderChange>,
}

#[derive(Serialize)]
pub struct FolderChange {
    pub name: String,
    pub delta: String,
}

fn measurement(row: &rusqlite::Row<'_>, start: usize) -> rusqlite::Result<Option<Measurement>> {
    let Some(observation) = read_observation(row, start + 3)? else {
        return Ok(None);
    };
    Ok(Some(Measurement {
        id: row.get(start)?,
        logical: unsigned(row, start + 1)?,
        complete: row.get::<_, bool>(start + 2)? && observation.complete,
        observation,
    }))
}

fn inconsistent() -> Error {
    Error::Invalid("Trend captures have changed or are no longer compatible.".into())
}

impl Store {
    pub fn trend(&self, request: TrendRequest, cancel: &AtomicBool) -> Result<Option<TrendReport>> {
        if !matches!(request.days, 7 | 30 | 90 | 365) {
            return Err(Error::Invalid("Choose a supported trend interval.".into()));
        }
        let tx = self.connection.unchecked_transaction()?;
        let (root, path, latest, missing_dates) = tx.query_row(
            "SELECT r.id,r.path,
             (SELECT MAX(o.observed_at) FROM snapshots o WHERE o.root_id=r.id),
             EXISTS(SELECT 1 FROM snapshots o WHERE o.root_id=r.id AND o.observed_at IS NULL)
             FROM snapshots s JOIN snapshot_roots r ON r.id=s.root_id WHERE s.id=?1",
            [request.snapshot],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            },
        )?;
        let mut report = TrendReport {
            anchor: request.snapshot,
            root: decode(&path)?.to_string_lossy().into_owned(),
            points: Vec::new(),
            limited: false,
            missing_dates,
            forecast: None,
        };
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let Some(latest) = latest else {
            return Ok(Some(report));
        };
        let mut query = tx.prepare(
            "SELECT s.id,s.logical,s.complete,s.observed_at,s.observed_complete,s.engine_version,s.accounting_version
             FROM snapshots s WHERE s.root_id=?1 AND s.observed_at BETWEEN ?2 AND ?3
             AND s.id=(SELECT x.id FROM snapshots x WHERE x.root_id=s.root_id AND x.observed_at=s.observed_at ORDER BY x.id DESC LIMIT 1)
             ORDER BY s.observed_at DESC,s.id DESC LIMIT ?4",
        )?;
        let start = latest
            .saturating_sub(i64::from(request.days) * DAY as i64)
            .max(0);
        let rows = query.query_map(params![root, start, latest, MAX_POINTS as i64 + 1], |row| {
            measurement(row, 0)
        })?;
        let mut measured = Vec::new();
        for row in rows {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            measured.push(row?.ok_or_else(inconsistent)?);
        }
        report.limited = measured.len() > MAX_POINTS;
        measured.truncate(MAX_POINTS);
        measured.reverse();
        for (index, point) in measured.iter().enumerate() {
            let previous = index
                .checked_sub(1)
                .map(|index| &measured[index])
                .filter(|other| point.comparable(other));
            report.points.push(TrendPoint {
                snapshot: point.id,
                measured_at: point.observation.completed_at,
                logical: point.logical.to_string(),
                complete: point.complete,
                supported: point.observation.accounting_version == ACCOUNTING_VERSION,
                previous: previous.map(|other| other.id),
                delta: previous.map(|other| {
                    (i128::from(point.logical) - i128::from(other.logical)).to_string()
                }),
            });
        }
        report.forecast = forecast(&measured);
        Ok(Some(report))
    }

    pub fn trend_changes(
        &self,
        before: i64,
        after: i64,
        cancel: &AtomicBool,
    ) -> Result<Option<TrendChanges>> {
        let tx = self.connection.unchecked_transaction()?;
        let lookup = |id| -> Result<(i64, Measurement)> {
            let (root, point) = tx.query_row(
                "SELECT root_id,id,logical,complete,observed_at,observed_complete,engine_version,accounting_version FROM snapshots WHERE id=?1",
                [id],
                |row| Ok((row.get::<_, i64>(0)?, measurement(row, 1)?)),
            )?;
            Ok((root, point.ok_or_else(inconsistent)?))
        };
        let (root, old) = lookup(before)?;
        let (next_root, new) = lookup(after)?;
        if root != next_root || !new.comparable(&old) {
            return Err(inconsistent());
        }
        let previous: i64 = tx.query_row(
            "SELECT id FROM snapshots WHERE root_id=?1 AND observed_at<?2 ORDER BY observed_at DESC,id DESC LIMIT 1",
            params![root, super::integer(new.observation.completed_at)?],
            |row| row.get(0),
        )?;
        if previous != before {
            return Err(inconsistent());
        }
        let parent: i64 = tx.query_row(
            "SELECT id FROM snapshot_paths WHERE root_id=?1 AND parent_id=0 LIMIT 1",
            [root],
            |row| row.get(0),
        )?;
        let folders = |increasing: bool| -> Result<Option<Vec<FolderChange>>> {
            // Only immediate paths contribute to this list. Descendant totals would overlap.
            let order = if increasing { "DESC" } else { "ASC" };
            let operator = if increasing { ">" } else { "<" };
            let mut query = tx.prepare(&format!(
                "SELECT p.name,COALESCE(n.logical,0)-COALESCE(o.logical,0) AS delta
                 FROM snapshot_paths p
                 LEFT JOIN snapshot_versions o ON o.path_id=p.id AND o.start_id<=?1 AND (o.end_id IS NULL OR o.end_id>?1)
                 LEFT JOIN snapshot_versions n ON n.path_id=p.id AND n.start_id<=?2 AND (n.end_id IS NULL OR n.end_id>?2)
                 WHERE p.root_id=?3 AND p.parent_id=?4
                 AND (o.kind=0 OR o.kind IS NULL) AND (n.kind=0 OR n.kind IS NULL)
                 AND delta {operator} 0 ORDER BY delta {order},p.id LIMIT 6"
            ))?;
            let rows = query.query_map(params![before, after, root, parent], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
            })?;
            let mut changes = Vec::new();
            for row in rows {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                let (name, delta) = row?;
                changes.push(FolderChange {
                    name: decode(&name)?.to_string_lossy().into_owned(),
                    delta: delta.to_string(),
                });
            }
            Ok(Some(changes))
        };
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let Some(grew) = folders(true)? else {
            return Ok(None);
        };
        let Some(shrank) = folders(false)? else {
            return Ok(None);
        };
        Ok(Some(TrendChanges {
            before,
            after,
            grew,
            shrank,
        }))
    }
}

fn forecast(measured: &[Measurement]) -> Option<TrendForecast> {
    let latest = measured.last()?;
    let last_at = latest.observation.completed_at;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    if last_at > now.saturating_add(60) || now.saturating_sub(last_at) > DAY * 7 {
        return None;
    }
    let mut daily = vec![latest];
    let mut previous = latest;
    for point in measured.iter().rev().skip(1) {
        if !previous.comparable(point)
            || previous.logical < point.logical
            || last_at - point.observation.completed_at > 30 * DAY
        {
            break;
        }
        previous = point;
        if daily.last()?.observation.completed_at / DAY != point.observation.completed_at / DAY {
            daily.push(point);
        }
    }
    let span = last_at - daily.last()?.observation.completed_at;
    if daily.len() < 6 || span < 7 * DAY {
        return None;
    }
    let mut rates = Vec::new();
    for pair in daily.windows(2) {
        let elapsed = pair[0].observation.completed_at - pair[1].observation.completed_at;
        if elapsed > 7 * DAY {
            return None;
        }
        rates.push((pair[0].logical - pair[1].logical) as f64 / elapsed as f64);
    }
    rates.sort_by(f64::total_cmp);
    let low = rates[0];
    let high = *rates.last()?;
    if low <= 0.0 || high > 4.0 * low {
        return None;
    }
    let horizon = span.min(14 * DAY);
    let project = |rate: f64| {
        let growth = (rate * horizon as f64).round();
        (growth < u64::MAX as f64)
            .then(|| latest.logical.checked_add(growth as u64))
            .flatten()
    };
    Some(TrendForecast {
        at: last_at.checked_add(horizon)?,
        logical: project(rates[rates.len() / 2])?.to_string(),
        low: project(low)?.to_string(),
        high: project(high)?.to_string(),
        samples: daily.len(),
        confidence: if daily.len() >= 8 && high <= 1.5 * low {
            "higher"
        } else {
            "moderate"
        },
    })
}
