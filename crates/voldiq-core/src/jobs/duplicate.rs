use super::{Jobs, Lease, now, unavailable};
use crate::{
    Result,
    duplicates::scope::{Report, ScopeRequest},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Deserialize, Serialize)]
pub(crate) struct DuplicateRun {
    pub snapshot: i64,
    pub request: ScopeRequest,
    pub binding: String,
    pub protections: String,
}

const MAX_REPORT_BYTES: usize = 12 * 1024 * 1024;

pub(super) fn decode_run(stored: Option<String>) -> Result<Option<DuplicateRun>> {
    stored
        .map(|json| {
            if json.len() > 4096 {
                return Err(unavailable());
            }
            let run: DuplicateRun = serde_json::from_str(&json).map_err(|_| unavailable())?;
            if run.snapshot <= 0 || !run.request.valid() {
                return Err(unavailable());
            }
            Ok(run)
        })
        .transpose()
}

impl Jobs {
    pub(crate) fn begin_duplicates(
        &mut self,
        lease: &Lease,
        owner: i64,
        root: &Path,
        run: &DuplicateRun,
    ) -> Result<i64> {
        self.begin_run(lease, owner, root, Some(run))
    }

    pub(crate) fn duplicate_run(&self, owner: i64, id: i64) -> Result<DuplicateRun> {
        let request = self
            .connection
            .query_row(
                "SELECT duplicate_request FROM scan_jobs WHERE owner=?1 AND id=?2",
                params![owner, id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(unavailable)?;
        decode_run(request)?.ok_or_else(unavailable)
    }

    pub(crate) fn duplicate_progress(
        &self,
        owner: i64,
        id: i64,
        value: &impl Serialize,
    ) -> Result<bool> {
        let json = serde_json::to_string(value).map_err(|_| unavailable())?;
        if json.len() > 4096 || self.connection.execute(
            "UPDATE scan_jobs SET progress=?1 WHERE owner=?2 AND id=?3 AND state='running' AND duplicate_request IS NOT NULL",
            params![json, owner, id],
        )? != 1 {
            return Err(unavailable());
        }
        self.cancellation_requested(owner, id)
    }

    pub(crate) fn finish_duplicates(&mut self, owner: i64, id: i64, report: &Report) -> Result<()> {
        let report = serde_json::to_string(report).map_err(|_| unavailable())?;
        if report.len() > MAX_REPORT_BYTES {
            return Err(crate::Error::DuplicateScopeLimit);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute(
            "UPDATE scan_jobs SET state=CASE WHEN cancel_requested=1 THEN 'cancelled' ELSE 'completed' END,
                duplicate_report=CASE WHEN cancel_requested=0 THEN ?1 ELSE NULL END,finished=?2
                WHERE owner=?3 AND id=?4 AND state='running' AND duplicate_request IS NOT NULL",
            params![report, now()?, owner, id],
        )? != 1 {
            return Err(unavailable());
        }
        tx.execute(
            "UPDATE scan_jobs SET duplicate_report=NULL WHERE id IN (
                SELECT id FROM scan_jobs WHERE duplicate_report IS NOT NULL ORDER BY id DESC LIMIT -1 OFFSET 12)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn duplicate_report(&self, owner: i64, id: i64) -> Result<Report> {
        let report: String = self.connection.query_row(
            "SELECT duplicate_report FROM scan_jobs WHERE owner=?1 AND id=?2 AND state='completed'
                AND duplicate_report IS NOT NULL AND length(duplicate_report)<=?3",
            params![owner, id, MAX_REPORT_BYTES as i64], |row| row.get(0),
        ).optional()?.ok_or_else(unavailable)?;
        serde_json::from_str(&report).map_err(|_| unavailable())
    }
}
