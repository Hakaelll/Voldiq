use super::{Comparison, Store, decode, integer, position_integer, read_observation, unsigned};
use crate::{
    Error, Result,
    cloud::CloudState,
    index::{ACCOUNTING_VERSION, Observation},
};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

const DAY: u64 = 86400;
// Codes are supplied from CloudState; logical file metadata includes confirmed file placeholders.
const FILES: &str = "FROM snapshot_paths p JOIN snapshot_versions v ON v.path_id=p.id WHERE p.root_id=?1 AND v.start_id<=?2 AND (v.end_id IS NULL OR v.end_id>?2) AND (v.kind=1 OR (v.kind=2 AND v.cloud IN(?3,?4)))";

pub enum CalendarRequest {
    Month {
        snapshot: i64,
        month: String,
    },
    Day {
        snapshot: i64,
        month: String,
        day: u8,
        offset: usize,
        limit: usize,
    },
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CalendarRead {
    Month { report: CalendarReport },
    Day { page: CalendarPage },
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureAccounting {
    Supported,
    Unknown,
    Unsupported,
}
#[derive(Serialize)]
pub struct ActivityCapture {
    pub id: i64,
    pub captured: u64,
    pub logical: String,
    pub observation: Option<Observation>,
    pub complete: bool,
    pub accounting: CaptureAccounting,
}
#[derive(Serialize)]
pub struct CalendarTotals {
    pub files: u64,
    pub logical: String,
}
#[derive(Serialize)]
pub struct CalendarDay {
    pub day: u8,
    pub date: String,
    pub totals: CalendarTotals,
}
#[derive(Serialize)]
pub struct CalendarReport {
    pub capture: ActivityCapture,
    pub root: String,
    pub month: String,
    pub days: Vec<CalendarDay>,
    pub total: CalendarTotals,
    pub undated: CalendarTotals,
    pub outside: CalendarTotals,
}
#[derive(Serialize)]
pub struct CalendarEntry {
    pub path_id: String,
    pub path: String,
    pub logical: String,
    pub modified: u64,
    pub cloud: CloudState,
}
#[derive(Serialize)]
pub struct CalendarPage {
    pub snapshot: i64,
    pub date: String,
    pub offset: usize,
    pub total: u64,
    pub entries: Vec<CalendarEntry>,
    pub has_more: bool,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiaryDetail {
    Baseline,
    Compared {
        delta: String,
        comparison: Box<Comparison>,
    },
    Incompatible,
}
#[derive(Serialize)]
pub struct DiaryEvent {
    pub before: Option<ActivityCapture>,
    pub after: ActivityCapture,
    pub detail: DiaryDetail,
}
#[derive(Serialize)]
pub struct DiaryPage {
    pub anchor: i64,
    pub root: String,
    pub events: Vec<DiaryEvent>,
    pub has_more: bool,
}

fn invalid() -> Error {
    Error::Invalid("Historical activity is unavailable or unsupported.".into())
}
fn source(connection: &Connection, snapshot: i64) -> Result<(i64, String, ActivityCapture)> {
    if snapshot <= 0 {
        return Err(invalid());
    }
    let (root, path, mut capture) = connection.query_row("SELECT s.root_id,r.path,s.captured,s.logical,s.complete,s.observed_at,s.observed_complete,s.engine_version,s.accounting_version FROM snapshots s JOIN snapshot_roots r ON r.id=s.root_id WHERE s.id=?1", [snapshot], |row| {
        let observation = read_observation(row,5)?;
        let accounting = match &observation {
            Some(value) if value.accounting_version == ACCOUNTING_VERSION => CaptureAccounting::Supported,
            Some(_) => CaptureAccounting::Unsupported,
            None => CaptureAccounting::Unknown,
        };
        Ok((row.get::<_,i64>(0)?, row.get::<_,Vec<u8>>(1)?, ActivityCapture { id:snapshot, captured:unsigned(row,2)?, logical:unsigned(row,3)?.to_string(), complete:row.get(4)?, observation, accounting }))
    })?;
    if root <= 0 {
        return Err(invalid());
    }
    capture.complete &= capture
        .observation
        .as_ref()
        .is_none_or(|value| value.complete);
    Ok((root, decode(&path)?.to_string_lossy().into_owned(), capture))
}
struct Month {
    name: String,
    start: u64,
    end: u64,
    days: u8,
}
impl Month {
    fn parse(value: &str) -> Result<Self> {
        let invalid = || {
            Error::Query(crate::search::QueryError {
                offset: 0,
                message: "Use a valid UTC month YYYY-MM, from 1970 through 9999.",
            })
        };
        if value.len() != 7 || !value.is_ascii() {
            return Err(invalid());
        }
        let start = crate::search::utc_date(&format!("{value}-01"), 0)
            .map_err(|_| invalid())?
            .start;
        let days = (28..=31)
            .rev()
            .find(|day| crate::search::utc_date(&format!("{value}-{day:02}"), 0).is_ok())
            .ok_or_else(invalid)?;
        Ok(Self {
            name: value.to_owned(),
            start,
            end: start + u64::from(days) * DAY,
            days,
        })
    }
    fn date(&self, day: u8) -> Result<(String, u64, u64)> {
        if day == 0 || day > self.days {
            return Err(Error::Query(crate::search::QueryError {
                offset: 0,
                message: "Choose a day in the selected UTC month.",
            }));
        }
        let start = self.start + u64::from(day - 1) * DAY;
        Ok((format!("{}-{day:02}", self.name), start, start + DAY))
    }
}
#[derive(Clone, Copy, Default)]
struct Count {
    files: u64,
    logical: u64,
}
impl Count {
    fn add(&mut self, size: u64) -> Result<()> {
        self.files = self.files.checked_add(1).ok_or_else(invalid)?;
        self.logical = self.logical.checked_add(size).ok_or_else(invalid)?;
        Ok(())
    }
    fn report(self) -> CalendarTotals {
        CalendarTotals {
            files: self.files,
            logical: self.logical.to_string(),
        }
    }
}

impl Store {
    pub fn calendar(
        &self,
        snapshot: i64,
        month: &str,
        cancel: &AtomicBool,
    ) -> Result<Option<CalendarReport>> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let month = Month::parse(month)?;
        let _read = self.history_read()?;
        let (root, path, capture) = source(&self.connection, snapshot)?;
        if capture.accounting == CaptureAccounting::Unsupported {
            return Err(invalid());
        }
        let mut bins = vec![Count::default(); usize::from(month.days)];
        let (mut total, mut undated, mut outside) =
            (Count::default(), Count::default(), Count::default());
        let mut statement = self
            .connection
            .prepare(&format!("SELECT v.modified,v.logical {FILES}"))?;
        let mut rows = statement.query(params![
            root,
            snapshot,
            CloudState::FileNoRecallFlag.code(),
            CloudState::FileRecallRequired.code()
        ])?;
        while let Some(row) = rows.next()? {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let modified = super::optional_unsigned(row, 0)?;
            let size = unsigned(row, 1)?;
            total.add(size)?;
            match modified {
                None => undated.add(size)?,
                Some(date) if date >= month.start && date < month.end => {
                    bins[((date - month.start) / DAY) as usize].add(size)?
                }
                Some(_) => outside.add(size)?,
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let days = bins
            .into_iter()
            .zip(1..=month.days)
            .map(|(count, day)| CalendarDay {
                day,
                date: format!("{}-{day:02}", month.name),
                totals: count.report(),
            })
            .collect();
        Ok(Some(CalendarReport {
            capture,
            root: path,
            month: month.name,
            days,
            total: total.report(),
            undated: undated.report(),
            outside: outside.report(),
        }))
    }

    pub fn calendar_entries(
        &self,
        snapshot: i64,
        month: &str,
        day: u8,
        offset: usize,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Option<CalendarPage>> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if offset > 10000 || !(1..=100).contains(&limit) {
            return Err(invalid());
        }
        let (date, start, end) = Month::parse(month)?.date(day)?;
        let _read = self.history_read()?;
        let (root, _, capture) = source(&self.connection, snapshot)?;
        if capture.accounting == CaptureAccounting::Unsupported {
            return Err(invalid());
        }
        let filter = format!("{FILES} AND v.modified>=?5 AND v.modified<?6");
        let total = self.connection.query_row(
            &format!("SELECT COUNT(*) {filter}"),
            params![
                root,
                snapshot,
                CloudState::FileNoRecallFlag.code(),
                CloudState::FileRecallRequired.code(),
                integer(start)?,
                integer(end)?
            ],
            |row| unsigned(row, 0),
        )?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let mut statement = self.connection.prepare(&format!("SELECT p.id,v.logical,v.modified,v.cloud {filter} ORDER BY v.logical DESC,p.id LIMIT ?7 OFFSET ?8"))?;
        let mut rows = statement.query(params![
            root,
            snapshot,
            CloudState::FileNoRecallFlag.code(),
            CloudState::FileRecallRequired.code(),
            integer(start)?,
            integer(end)?,
            position_integer(limit)?,
            position_integer(offset)?
        ])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let id: i64 = row.get(0)?;
            entries.push(CalendarEntry {
                path_id: id.to_string(),
                path: self.path(root, id)?.to_string_lossy().into_owned(),
                logical: unsigned(row, 1)?.to_string(),
                modified: unsigned(row, 2)?,
                cloud: CloudState::from_code(row.get(3)?).ok_or_else(invalid)?,
            });
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let has_more = (offset as u64) + (entries.len() as u64) < total;
        Ok(Some(CalendarPage {
            snapshot,
            date,
            offset,
            total,
            entries,
            has_more,
        }))
    }

    pub fn diary(
        &self,
        anchor: i64,
        offset: usize,
        limit: usize,
        changes_limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Option<DiaryPage>> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if offset > 10000 || !(1..=10).contains(&limit) || !(1..=5).contains(&changes_limit) {
            return Err(invalid());
        }
        let _read = self.history_read()?;
        let (root, path, _) = source(&self.connection, anchor)?;
        let mut statement = self.connection.prepare("SELECT s.id,(SELECT MAX(p.id) FROM snapshots p WHERE p.root_id=s.root_id AND p.id<s.id) FROM snapshots s WHERE s.root_id=?1 AND s.id<=?2 ORDER BY s.id DESC LIMIT ?3 OFFSET ?4")?;
        let mut records = statement
            .query_map(
                params![
                    root,
                    anchor,
                    position_integer(limit + 1)?,
                    position_integer(offset)?
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = records.len() > limit;
        records.truncate(limit);
        let mut events = Vec::new();
        for (after_id, before_id) in records {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let (_, _, after) = source(&self.connection, after_id)?;
            let before = before_id
                .map(|id| source(&self.connection, id).map(|(_, _, capture)| capture))
                .transpose()?;
            let detail = match &before {
                None => DiaryDetail::Baseline,
                Some(before)
                    if [before.accounting, after.accounting]
                        .contains(&CaptureAccounting::Unsupported) =>
                {
                    DiaryDetail::Incompatible
                }
                Some(before) => {
                    let Some(comparison) =
                        self.compare_read(before.id, after.id, 0, changes_limit, cancel)?
                    else {
                        return Ok(None);
                    };
                    DiaryDetail::Compared {
                        delta: comparison.delta.to_string(),
                        comparison: Box::new(comparison),
                    }
                }
            };
            events.push(DiaryEvent {
                before,
                after,
                detail,
            });
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        Ok(Some(DiaryPage {
            anchor,
            root: path,
            events,
            has_more,
        }))
    }
}
