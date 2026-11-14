//! Read-only scheduled captures. Schedule state and successful snapshot publication share a database.
use crate::{
    Error, Result, ScanMode, ScanOptions, ScanPhase, ScanProfile,
    agents::AuthorizedPath,
    index::Index,
    operations::{Coordinator, Lease},
    scanner::{self, ScanFolder},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const SCHEMA: &str = "
CREATE TABLE scan_schedules(
  id INTEGER PRIMARY KEY AUTOINCREMENT, revision INTEGER NOT NULL DEFAULT 1,
  folder TEXT NOT NULL, exclusions TEXT NOT NULL, interval INTEGER NOT NULL CHECK(interval IN (3600,86400,604800)),
  enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), due INTEGER NOT NULL
);
CREATE TABLE scheduled_runs(
  id INTEGER PRIMARY KEY AUTOINCREMENT, schedule_id INTEGER NOT NULL REFERENCES scan_schedules(id) ON DELETE CASCADE,
  started INTEGER NOT NULL, finished INTEGER, outcome TEXT NOT NULL CHECK(outcome IN ('running','saved','failed','cancelled','interrupted')),
  snapshot INTEGER REFERENCES snapshots(id) ON DELETE SET NULL, problem TEXT
);
CREATE INDEX scheduled_runs_by_schedule ON scheduled_runs(schedule_id,id DESC);
PRAGMA user_version=11;";

pub(super) const GROWTH_SCHEMA: &str = "
ALTER TABLE scan_schedules ADD COLUMN growth_mib INTEGER CHECK(growth_mib BETWEEN 1 AND 4294967295);
ALTER TABLE scheduled_runs ADD COLUMN growth_bytes INTEGER CHECK(growth_bytes > 0);
ALTER TABLE scheduled_runs ADD COLUMN baseline_observed INTEGER CHECK(baseline_observed >= 0);
PRAGMA user_version=12;";

const MAX_SCHEDULES: usize = 16;
const MAX_EXCLUSION_BYTES: usize = 32_768;

#[derive(Serialize)]
pub struct Schedule {
    pub id: i64,
    pub revision: i64,
    pub path: String,
    pub exclusions: Vec<String>,
    pub hours: u32,
    pub growth_mib: Option<u32>,
    pub enabled: bool,
    pub next_due: Option<i64>,
    pub runs: Vec<Run>,
}

#[derive(Serialize)]
pub struct Run {
    pub started: i64,
    pub finished: Option<i64>,
    pub outcome: String,
    pub snapshot: Option<i64>,
    pub problem: Option<String>,
    pub partial: Option<bool>,
    pub growth: Option<Growth>,
}

#[derive(Serialize)]
pub struct Growth {
    pub bytes: String,
    pub baseline_observed: i64,
}

pub struct SavedScan {
    pub snapshot: i64,
    pub growth_alert: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanSettings {
    pub hours: u32,
    pub exclusions: Vec<String>,
    pub growth_mib: Option<u32>,
}

impl ScanSettings {
    fn interval(&self) -> Result<i64> {
        if self.growth_mib == Some(0) {
            return Err(Error::Invalid(
                "Use a positive growth threshold in MiB, or leave it blank.".into(),
            ));
        }
        interval(self.hours)
    }
}

pub struct ScheduledScan<'a> {
    lease: &'a Lease,
    profile: &'a Path,
    claim: Claim,
    folder: ScanFolder,
    exclusions: Vec<String>,
}

pub struct Measurement {
    run: i64,
    index: Index,
}

pub(super) struct Claim {
    id: i64,
    schedule: i64,
}

pub fn time_now() -> Result<i64> {
    super::integer(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::Invalid(e.to_string()))?
            .as_secs(),
    )
}

fn invalid_schedule() -> Error {
    Error::Invalid("The schedule changed. Reload Scheduled scans and try again.".into())
}

fn encode(value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::Invalid(e.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(value).map_err(|e| Error::Invalid(e.to_string()))
}

fn interval(hours: u32) -> Result<i64> {
    match hours {
        1 | 24 | 168 => Ok(i64::from(hours) * 3600),
        _ => Err(Error::Invalid(
            "Choose an hourly, daily or weekly scan.".into(),
        )),
    }
}

fn next_due(now: i64, interval: i64) -> Result<i64> {
    now.checked_add(interval)
        .ok_or_else(|| Error::Invalid("Schedule time is out of range.".into()))
}

fn elapsed_interval(tx: &Connection, schedule: i64, completed: i64) -> Result<()> {
    let interval = tx.query_row(
        "SELECT interval FROM scan_schedules WHERE id=?1",
        [schedule],
        |row| row.get(0),
    )?;
    tx.execute(
        "UPDATE scan_schedules SET due=max(due,?2) WHERE id=?1",
        params![schedule, next_due(completed, interval)?],
    )?;
    Ok(())
}

fn validate_exclusions(
    root: &AuthorizedPath,
    exclusions: &[String],
) -> Result<Vec<AuthorizedPath>> {
    if exclusions.len() > 64
        || exclusions.iter().map(String::len).sum::<usize>() > MAX_EXCLUSION_BYTES
    {
        return Err(Error::Invalid(
            "Enter up to 64 excluded folders, within 32 KiB of text.".into(),
        ));
    }
    exclusions
        .iter()
        .map(|path| {
            let pinned = crate::agents::pin_directory(Path::new(path))?;
            if pinned.identity() == root.identity()
                || !pinned.ancestor_identities()?.contains(&root.identity())
            {
                return Err(Error::Invalid(
                    "Exclusions must be folders inside the scheduled folder.".into(),
                ));
            }
            Ok(pinned)
        })
        .collect()
}

fn encode_exclusions(root: &AuthorizedPath, exclusions: &[String]) -> Result<String> {
    let excluded = validate_exclusions(root, exclusions)?;
    let mut paths = excluded
        .iter()
        .map(|folder| folder.path())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();
    encode(&paths)
}

impl super::Store {
    fn check_schedule_lease(&self, lease: &Lease) -> Result<()> {
        let parent = self.path.parent().ok_or_else(invalid_schedule)?;
        if Coordinator::open(parent)?.identity() != lease.directory_identity() {
            return Err(invalid_schedule());
        }
        Ok(())
    }

    pub fn scan_schedules(&self) -> Result<Vec<Schedule>> {
        let tx = self.connection.unchecked_transaction()?;
        let mut schedules = Vec::new();
        {
            let mut statement = tx.prepare("SELECT id,revision,folder,exclusions,interval,enabled,due,growth_mib FROM scan_schedules ORDER BY id LIMIT 17")?;
            let mut rows = statement.query([])?;
            let mut history = tx.prepare("SELECT r.started,r.finished,r.outcome,r.snapshot,r.problem,NOT s.complete,r.growth_bytes,r.baseline_observed FROM scheduled_runs r LEFT JOIN snapshots s ON s.id=r.snapshot WHERE r.schedule_id=?1 ORDER BY r.id DESC LIMIT 20")?;
            while let Some(row) = rows.next()? {
                let id = row.get(0)?;
                let folder: ScanFolder =
                    decode(row.get_ref(2)?.as_str().map_err(|_| invalid_schedule())?)?;
                let enabled: bool = row.get(5)?;
                let hours =
                    u32::try_from(row.get::<_, i64>(4)? / 3600).map_err(|_| invalid_schedule())?;
                interval(hours)?;
                let runs = history
                    .query_map([id], |row| {
                        let growth = row
                            .get::<_, Option<i64>>(6)?
                            .map(|bytes| -> rusqlite::Result<_> {
                                Ok(Growth {
                                    bytes: bytes.to_string(),
                                    baseline_observed: row.get(7)?,
                                })
                            })
                            .transpose()?;
                        Ok(Run {
                            started: row.get(0)?,
                            finished: row.get(1)?,
                            outcome: row.get(2)?,
                            snapshot: row.get(3)?,
                            problem: row.get(4)?,
                            partial: row.get(5)?,
                            growth,
                        })
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                schedules.push(Schedule {
                    id,
                    revision: row.get(1)?,
                    path: folder.path().to_string_lossy().into_owned(),
                    exclusions: decode(row.get_ref(3)?.as_str().map_err(|_| invalid_schedule())?)?,
                    hours,
                    growth_mib: row.get(7)?,
                    enabled,
                    next_due: enabled.then(|| row.get(6)).transpose()?,
                    runs,
                });
            }
        }
        if schedules.len() > MAX_SCHEDULES {
            return Err(invalid_schedule());
        }
        tx.commit()?;
        Ok(schedules)
    }

    pub fn add_scan_schedule(
        &mut self,
        lease: &Lease,
        path: &Path,
        settings: &ScanSettings,
        now: i64,
    ) -> Result<i64> {
        self.check_schedule_lease(lease)?;
        let interval = settings.interval()?;
        let folder = ScanFolder::prepare(path)?;
        let root = folder.reopen()?;
        if root
            .ancestor_identities()?
            .contains(&lease.directory_identity())
        {
            return Err(Error::Invalid(
                "The Voldiq profile cannot be a scheduled scan folder.".into(),
            ));
        }
        let folder = encode(&folder)?;
        let exclusions = encode_exclusions(&root, &settings.exclusions)?;
        if self.connection.execute(
            "INSERT INTO scan_schedules(folder,exclusions,interval,enabled,due,growth_mib) SELECT ?1,?2,?3,1,?4,?5 WHERE (SELECT count(*) FROM scan_schedules)<16",
            params![folder, exclusions, interval, next_due(now, interval)?, settings.growth_mib],
        )? != 1 {
            return Err(Error::Invalid("Remove a schedule before adding another. The maximum is 16.".into()));
        }
        Ok(self.connection.last_insert_rowid())
    }

    pub fn set_scan_schedule_enabled(
        &mut self,
        lease: &Lease,
        id: i64,
        revision: i64,
        enabled: bool,
        now: i64,
    ) -> Result<()> {
        self.check_schedule_lease(lease)?;
        let interval = self
            .connection
            .query_row(
                "SELECT interval FROM scan_schedules WHERE id=?1 AND revision=?2",
                params![id, revision],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(invalid_schedule)?;
        if self.connection.execute(
            "UPDATE scan_schedules SET revision=revision+1,enabled=?3,due=?4 WHERE id=?1 AND revision=?2 AND enabled != ?3",
            params![id, revision, enabled, next_due(now, interval)?],
        )? != 1 { return Err(invalid_schedule()); }
        Ok(())
    }

    pub fn edit_scan_schedule(
        &mut self,
        lease: &Lease,
        id: i64,
        revision: i64,
        settings: &ScanSettings,
        now: i64,
    ) -> Result<()> {
        self.check_schedule_lease(lease)?;
        let interval = settings.interval()?;
        let stored = self
            .connection
            .query_row(
                "SELECT folder FROM scan_schedules WHERE id=?1 AND revision=?2",
                params![id, revision],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(invalid_schedule)?;
        let folder: ScanFolder = decode(&stored)?;
        let root = folder.reopen()?;
        let exclusions = encode_exclusions(&root, &settings.exclusions)?;
        if self.connection.execute("UPDATE scan_schedules SET revision=revision+1,exclusions=?3,interval=?4,due=?5,growth_mib=?6 WHERE id=?1 AND revision=?2",
            params![id, revision, exclusions, interval, next_due(now, interval)?, settings.growth_mib])? != 1 { return Err(invalid_schedule()); }
        Ok(())
    }

    pub fn remove_scan_schedule(&mut self, lease: &Lease, id: i64, revision: i64) -> Result<()> {
        self.check_schedule_lease(lease)?;
        if self.connection.execute(
            "DELETE FROM scan_schedules WHERE id=?1 AND revision=?2",
            params![id, revision],
        )? != 1
        {
            return Err(invalid_schedule());
        }
        Ok(())
    }

    pub fn claim_scheduled_scan<'a>(
        &mut self,
        lease: &'a mut Lease,
        now: i64,
    ) -> Result<Option<ScheduledScan<'a>>> {
        self.check_schedule_lease(lease)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let interrupted = {
            let mut rows = tx.prepare(
                "SELECT DISTINCT schedule_id FROM scheduled_runs WHERE outcome='running'",
            )?;
            rows.query_map([], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for schedule in interrupted {
            elapsed_interval(&tx, schedule, now)?;
        }
        tx.execute(
            "UPDATE scheduled_runs SET outcome='interrupted' WHERE outcome='running'",
            [],
        )?;
        let row = tx.query_row(
            "SELECT id,folder,exclusions,interval FROM scan_schedules WHERE enabled=1 AND due<=?1 ORDER BY due,id LIMIT 1", [now],
            |row| Ok((row.get::<_, i64>(0)?,row.get::<_, String>(1)?,row.get::<_, String>(2)?,row.get::<_, i64>(3)?)),
        ).optional()?;
        let Some((schedule, folder, exclusions, interval)) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let folder = decode(&folder)?;
        let exclusions = decode(&exclusions)?;
        tx.execute(
            "UPDATE scan_schedules SET due=?2 WHERE id=?1",
            params![schedule, next_due(now, interval)?],
        )?;
        tx.execute(
            "INSERT INTO scheduled_runs(schedule_id,started,outcome) VALUES(?1,?2,'running')",
            params![schedule, now],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute("DELETE FROM scheduled_runs WHERE schedule_id=?1 AND id NOT IN (SELECT id FROM scheduled_runs WHERE schedule_id=?1 ORDER BY id DESC LIMIT 20)", [schedule])?;
        tx.commit()?;
        let profile = lease.directory();
        Ok(Some(ScheduledScan {
            lease,
            profile,
            claim: Claim { id, schedule },
            folder,
            exclusions,
        }))
    }

    pub fn scan_schedule_due(&self, now: i64) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM scan_schedules WHERE enabled=1 AND due<=?1) OR EXISTS(SELECT 1 FROM scheduled_runs WHERE outcome='running')",
            [now], |row| row.get(0),
        )?)
    }

    pub fn finish_scheduled_scan(
        &mut self,
        run: ScheduledScan<'_>,
        measured: Result<Measurement>,
        cancel: &AtomicBool,
    ) -> Result<Option<SavedScan>> {
        self.check_schedule_lease(run.lease)?;
        let problem = match measured {
            Ok(measurement) if measurement.run != run.claim.id => return Err(invalid_schedule()),
            Ok(measurement) if !cancel.load(Ordering::Relaxed) => {
                match self.save_capture(&measurement.index, cancel, &[], None, Some(&run.claim)) {
                    Ok((snapshot, growth_alert)) => {
                        return Ok(Some(SavedScan {
                            snapshot,
                            growth_alert,
                        }));
                    }
                    Err(problem) => problem.to_string(),
                }
            }
            Ok(_) => "Scan cancelled.".into(),
            Err(problem) => problem.to_string(),
        };
        let finished = time_now()?;
        let problem: String = problem.chars().take(512).collect();
        let outcome = if cancel.load(Ordering::Relaxed) {
            "cancelled"
        } else {
            "failed"
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        run.claim
            .finish(&tx, finished, outcome, None, Some(&problem))?;
        tx.commit()?;
        Ok(None)
    }
}

impl ScheduledScan<'_> {
    pub fn measure(&self, cancel: &AtomicBool) -> Result<Measurement> {
        let root = self.folder.reopen()?;
        let mut exclusions = validate_exclusions(&root, &self.exclusions)?;
        let profile = crate::agents::pin_directory(self.profile)?;
        if root.ancestor_identities()?.contains(&profile.identity()) {
            return Err(Error::Invalid(
                "The Voldiq profile cannot be a scheduled scan folder.".into(),
            ));
        }
        if profile.ancestor_identities()?.contains(&root.identity()) {
            exclusions.push(profile);
        }
        let mut canonical = exclusions
            .iter()
            .map(|path| std::fs::canonicalize(path.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        canonical.sort_unstable();
        canonical.dedup();
        let requested = canonical
            .iter()
            .map(|path| {
                path.to_str()
                    .ok_or_else(invalid_schedule)
                    .map(str::to_owned)
            })
            .collect::<Result<_>>()?;
        let mut complete = false;
        let options = ScanOptions {
            mode: ScanMode::Directory,
            profile: ScanProfile::Balanced,
            exclusions: requested,
            ..Default::default()
        };
        let index = scanner::scan(root.path(), &options, cancel, |progress| {
            complete = progress.phase == ScanPhase::Complete
        })?;
        if !complete || cancel.load(Ordering::Relaxed) {
            return Err(Error::Invalid("Scheduled scan did not finish.".into()));
        }
        if index.root_identity != Some(root.identity()) || index.exclusions != canonical {
            return Err(invalid_schedule());
        }
        Ok(Measurement {
            run: self.claim.id,
            index,
        })
    }
}

impl Claim {
    pub(super) fn publish(&self, tx: &Connection, snapshot: i64, finished: i64) -> Result<bool> {
        self.finish(tx, finished, "saved", Some(snapshot), None)?;
        let growth = tx.query_row(
            "SELECT cur.logical-prev.logical,prev.observed_at,(cur.logical-prev.logical)>=cfg.growth_mib*1048576
             FROM snapshots cur JOIN scan_schedules cfg ON cfg.id=?2
             JOIN scheduled_runs r ON r.schedule_id=cfg.id AND r.id<?3 AND r.outcome='saved'
             JOIN snapshots prev ON prev.id=r.snapshot AND prev.root_id=cur.root_id
             WHERE cur.id=?1 AND cfg.growth_mib IS NOT NULL AND cur.complete AND cur.observed_complete
               AND prev.complete AND prev.observed_complete AND prev.observed_at<cur.observed_at
               AND prev.accounting_version=cur.accounting_version AND prev.engine_version=cur.engine_version
             ORDER BY r.id DESC LIMIT 1",
            params![snapshot, self.schedule, self.id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, bool>(2)?)),
        ).optional()?;
        if let Some((bytes, baseline, true)) = growth {
            tx.execute(
                "UPDATE scheduled_runs SET growth_bytes=?2,baseline_observed=?3 WHERE id=?1",
                params![self.id, bytes, baseline],
            )?;
            return Ok(true);
        }
        Ok(false)
    }

    fn finish(
        &self,
        tx: &Connection,
        finished: i64,
        outcome: &str,
        snapshot: Option<i64>,
        problem: Option<&str>,
    ) -> Result<()> {
        if tx.execute("UPDATE scheduled_runs SET outcome=?2,finished=max(started,?3),snapshot=?4,problem=?5 WHERE id=?1 AND outcome='running'",
            params![self.id, outcome, finished, snapshot, problem])? != 1 { return Err(invalid_schedule()); }
        elapsed_interval(tx, self.schedule, finished)
    }
}
