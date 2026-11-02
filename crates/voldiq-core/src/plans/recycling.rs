use super::store::ExecutionControl;
use super::{ExecutionLease, HumanAuthority, State, Store, invalid, now};
use crate::{
    Result,
    agents::PlanContext,
    cleanup::{
        helper::Session,
        journal::{AgentExecutionRef, Journal},
        recycle::{
            ShellApartment,
            batch::{Batch, Outcome, Report},
        },
    },
};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

/// Borrowed resources must stay on this STA worker and outlive its prepared batch.
pub struct RecyclingWorker<'a> {
    operation: &'a crate::operations::Lease,
    cancel: &'a AtomicBool,
    apartment: &'a ShellApartment,
}

impl<'a> RecyclingWorker<'a> {
    pub fn new(
        operation: &'a crate::operations::Lease,
        cancel: &'a AtomicBool,
        apartment: &'a ShellApartment,
    ) -> Self {
        Self {
            operation,
            cancel,
            apartment,
        }
    }
}

/// Owns the exact pinned selection and its one consumed ticket. No helper or source admission has run.
pub struct PreparedRecycling<'a> {
    lease: ExecutionLease,
    batch: Batch,
    worker: RecyclingWorker<'a>,
}

impl PreparedRecycling<'_> {
    pub fn cancel(self, plans: &mut Store, journal: &mut Journal) -> Result<Report> {
        let report = self.batch.abort(journal, &AtomicBool::new(true), |_| {});
        plans.finish(
            self.lease,
            outcome(&report, false),
            preflight_summary(&report),
            now()?,
        )?;
        Ok(report)
    }

    pub fn execute(
        self,
        plans: &mut Store,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        journal: &mut Journal,
        progress: impl FnMut(usize),
    ) -> Result<Report> {
        self.execute_using(plans, context, human, journal, progress, Session::launch)
    }

    pub(super) fn execute_using(
        self,
        plans: &mut Store,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        journal: &mut Journal,
        mut progress: impl FnMut(usize),
        launch: impl FnOnce(&ShellApartment) -> std::io::Result<Session>,
    ) -> Result<Report> {
        let Self {
            lease,
            batch,
            worker,
        } = self;
        let mut invalidated = false;
        let report = {
            let mut check = || {
                if worker.cancel.load(Ordering::Relaxed) {
                    return Err(crate::Error::Invalid("Execution cancelled.".into()));
                }
                match plans.check_execution(context, human, &lease, now) {
                    Ok(ExecutionControl::Proceed) => Ok(()),
                    Ok(ExecutionControl::Cancel) => {
                        worker.cancel.store(true, Ordering::Relaxed);
                        Err(crate::Error::Invalid(
                            "Execution cancellation requested.".into(),
                        ))
                    }
                    Err(error) => {
                        invalidated = true;
                        Err(error)
                    }
                }
            };
            if let Err(error) = check() {
                let mut report = batch.abort(journal, worker.cancel, &mut progress);
                report.preflight_error = Some(error.to_string());
                report
            } else {
                match launch(worker.apartment) {
                    Ok(session) => batch.execute_guarded(
                        journal,
                        worker.cancel,
                        worker.apartment,
                        session,
                        &mut progress,
                        check,
                    ),
                    Err(error) => {
                        if error.raw_os_error() == Some(1223) {
                            worker.cancel.store(true, Ordering::Relaxed);
                        }
                        let mut report = batch.abort(journal, worker.cancel, &mut progress);
                        report.helper_error = Some(error.to_string());
                        report
                    }
                }
            }
        };
        plans.finish(
            lease,
            outcome(&report, invalidated),
            execution_summary(&report),
            now()?,
        )?;
        Ok(report)
    }
}

impl Store {
    pub fn prepare_recycling<'a>(
        &mut self,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        lease: ExecutionLease,
        journal: &mut Journal,
        worker: RecyclingWorker<'a>,
    ) -> Result<PreparedRecycling<'a>> {
        let checked = (|| {
            if worker.operation.directory_identity() != lease.directory {
                return Err(invalid());
            }
            if worker.cancel.load(Ordering::Relaxed)
                || matches!(
                    self.check_execution(context, human, &lease, now)?,
                    ExecutionControl::Cancel
                )
            {
                worker.cancel.store(true, Ordering::Relaxed);
                return Err(invalid());
            }
            let record = self.execution_record(context, human, &lease)?;
            let evidence = record.evidence()?;
            let _pins = evidence
                .iter()
                .map(|file| context.path(Path::new(&file.path)))
                .collect::<Result<Vec<_>>>()?;
            record.validate(lease.directory)?;
            record.into_plan()?.with_protections(self.directory.path())
        })();
        let plan = match checked {
            Ok(plan) => plan,
            Err(error) => {
                let state = if worker.cancel.load(Ordering::Relaxed) {
                    State::Cancelled
                } else {
                    State::Invalidated
                };
                self.finish(lease,state,serde_json::json!({"phase":"preflight","code":"revalidation_failed","originals_admitted":0}),now()?)?;
                return Err(error);
            }
        };
        let origin = AgentExecutionRef {
            plan_id: &lease.id,
            manifest_hash: &lease.hash,
            connection_id: lease.owner,
            execution_nonce: &lease.nonce,
            directory: lease.directory,
        };
        let prepared =
            Batch::prepare_agent(plan, journal, worker.cancel, worker.apartment, &origin);
        let batch = match prepared {
            Ok(batch) => batch,
            Err(error) => {
                self.finish(lease,State::Failed,serde_json::json!({"phase":"preflight","code":"preparation_failed","originals_admitted":0}),now()?)?;
                return Err(error);
            }
        };
        let checked = if batch.ready_for_admission() {
            self.preflight_ready(context, human, &lease)
        } else {
            Err(invalid())
        };
        if let Err(error) = checked {
            if matches!(
                self.check_execution(context, human, &lease, now),
                Ok(ExecutionControl::Cancel)
            ) {
                worker.cancel.store(true, Ordering::Relaxed);
            }
            let report = batch.abort(journal, worker.cancel, |_| {});
            self.finish(
                lease,
                if report.cancelled {
                    State::Cancelled
                } else {
                    State::Failed
                },
                preflight_summary(&report),
                now()?,
            )?;
            return Err(error);
        }
        Ok(PreparedRecycling {
            lease,
            batch,
            worker,
        })
    }
}

fn preflight_summary(report: &Report) -> serde_json::Value {
    serde_json::json!({"phase":"preflight","originals_admitted":0,"total":report.items.len(),"cancelled":report.cancelled,
        "preflight_failed":report.preflight_error.is_some(),"journal_failed":report.journal_error.is_some(),
        "cleanup_requires_review":report.items.iter().any(|item| item.cleanup_error.is_some()),
        "unknown":report.items.iter().filter(|item| item.outcome==Outcome::Unknown).count(),
        "failed_nodes":report.items.iter().filter(|item| item.error.is_some()).map(|item| item.id).collect::<Vec<_>>()})
}

fn outcome(report: &Report, invalidated: bool) -> State {
    let recycled = report
        .items
        .iter()
        .filter(|item| item.outcome == Outcome::Recycled)
        .count();
    let unresolved = report.journal_error.is_some()
        || report
            .items
            .iter()
            .any(|item| item.outcome == Outcome::Unknown || item.cleanup_error.is_some());
    if recycled == report.items.len()
        && recycled > 0
        && !unresolved
        && report.helper_error.is_none()
        && report
            .items
            .iter()
            .all(|item| item.recorded && item.error.is_none())
    {
        State::Completed
    } else if recycled > 0 {
        State::PartiallyCompleted
    } else if unresolved {
        State::Failed
    } else if report.cancelled {
        State::Cancelled
    } else if invalidated {
        State::Invalidated
    } else {
        State::Failed
    }
}

fn execution_summary(report: &Report) -> serde_json::Value {
    serde_json::json!({"phase":"execution","total":report.items.len(),"cancelled":report.cancelled,
        "immediate_free_bytes":"0","journal_failed":report.journal_error.is_some(),"helper_failed":report.helper_error.is_some(),
        "preflight_failed":report.preflight_error.is_some(),"cleanup_requires_review":report.items.iter().any(|item| item.cleanup_error.is_some()),
        "confirmed_admissions":report.items.iter().filter(|item| matches!(item.outcome,Outcome::Returned|Outcome::Recycled)).count(),
        "unknown":report.items.iter().filter(|item| item.outcome==Outcome::Unknown).count(),
        "items":report.items.iter().map(|item| serde_json::json!({"node_id":item.id,"outcome":item.outcome,"recorded":item.recorded,
            "failed":item.error.is_some(),"cleanup_requires_review":item.cleanup_error.is_some()})).collect::<Vec<_>>()})
}
