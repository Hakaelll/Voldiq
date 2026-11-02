use super::{Budget, Coverage};
use crate::{
    NodeId, Result,
    index::{ACCOUNTING_VERSION, Index},
    snapshots::{HistoryPoint, Store},
};
use std::{
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastReason {
    NotRequested,
    NoBudget,
    Unmeasured,
    Partial,
    InsufficientHistory,
    Stale,
    Volatile,
    NotGrowing,
    BeyondHorizon,
    AtLimit,
    Incompatible,
}
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Forecast {
    Unavailable {
        reason: ForecastReason,
    },
    Estimate {
        as_of: u64,
        estimated_at: u64,
        earliest_at: u64,
        latest_at: u64,
        bytes_per_day: f64,
        samples: usize,
        span_days: u64,
    },
}
fn unavailable(reason: ForecastReason) -> Forecast {
    Forecast::Unavailable { reason }
}

pub(super) fn estimate(
    index: &Index,
    node: NodeId,
    budget: Option<&Budget>,
    coverage: Coverage,
    history: &Store,
    cancel: &AtomicBool,
) -> Result<Forecast> {
    let Some(budget) = budget else {
        return Ok(unavailable(ForecastReason::NoBudget));
    };
    if coverage != Coverage::Complete {
        return Ok(unavailable(ForecastReason::Partial));
    }
    let Some(observation) = &index.observation else {
        return Ok(unavailable(ForecastReason::Unmeasured));
    };
    if observation.accounting_version != ACCOUNTING_VERSION {
        return Ok(unavailable(ForecastReason::Incompatible));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|time| time.as_secs());
    if now.is_none_or(|now| {
        observation.completed_at > now.saturating_add(60)
            || now.saturating_sub(observation.completed_at) > 86400
    }) {
        return Ok(unavailable(ForecastReason::Stale));
    }
    let logical = index.entry(node)?.logical;
    if logical >= budget.limit_bytes {
        return Ok(unavailable(ForecastReason::AtLimit));
    }
    let Some(mut points) = history.budget_history(index, node, observation.completed_at, cancel)?
    else {
        return Ok(unavailable(ForecastReason::InsufficientHistory));
    };
    if points
        .last()
        .is_some_and(|point| point.at / 86400 == observation.completed_at / 86400)
    {
        points.pop();
    }
    points.push(HistoryPoint {
        at: observation.completed_at,
        logical,
    });
    if points.len() < 6 || observation.completed_at.saturating_sub(points[0].at) < 7 * 86400 {
        return Ok(unavailable(ForecastReason::InsufficientHistory));
    }
    let mut rates = Vec::new();
    for pair in points.windows(2) {
        let elapsed = pair[1].at.saturating_sub(pair[0].at);
        if elapsed == 0 || elapsed > 7 * 86400 {
            return Ok(unavailable(ForecastReason::InsufficientHistory));
        }
        if pair[1].logical < pair[0].logical {
            return Ok(unavailable(ForecastReason::Volatile));
        }
        rates.push((pair[1].logical - pair[0].logical) as f64 / elapsed as f64);
    }
    rates.sort_by(f64::total_cmp);
    let low = rates[0];
    let high = rates[rates.len() - 1];
    if high == 0.0 {
        return Ok(unavailable(ForecastReason::NotGrowing));
    }
    if low <= 0.0 || high > low * 4.0 {
        return Ok(unavailable(ForecastReason::Volatile));
    }
    let remaining = (budget.limit_bytes - logical) as f64;
    let slow = (remaining / low).ceil();
    if slow > 366.0 * 86400.0 {
        return Ok(unavailable(ForecastReason::BeyondHorizon));
    }
    let when = |rate: f64| observation.completed_at + (remaining / rate).ceil() as u64;
    Ok(Forecast::Estimate {
        as_of: observation.completed_at,
        estimated_at: when(rates[rates.len() / 2]),
        earliest_at: when(high),
        latest_at: when(low),
        bytes_per_day: rates[rates.len() / 2] * 86400.0,
        samples: points.len(),
        span_days: (observation.completed_at - points[0].at) / 86400,
    })
}
