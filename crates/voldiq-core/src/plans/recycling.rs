use super::{ExecutionLease, HumanAuthority, State, Store, invalid, now};
use crate::{
    Result,
    agents::PlanContext,
    cleanup::{
        journal::{AgentExecutionRef, Journal},
        recycle::{
            ShellApartment,
            batch::{Batch, Outcome, Report},
        },
    },
};
use std::{path::Path, sync::atomic::AtomicBool};

/// Owns the exact pinned selection and its one consumed ticket. No helper or source admission has run.
pub struct PreparedRecycling {
    lease: ExecutionLease,
    batch: Batch,
}

impl PreparedRecycling {
    pub fn cancel(self, plans: &mut Store, journal: &mut Journal) -> Result<Report> {
        let report = self.batch.abort(journal, &AtomicBool::new(true), |_| {});
        plans.finish(self.lease, State::Cancelled, summary(&report), now()?)?;
        Ok(report)
    }
}

impl Store {
    pub fn prepare_recycling(
        &mut self,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        lease: ExecutionLease,
        journal: &mut Journal,
        cancel: &AtomicBool,
        apartment: &ShellApartment,
    ) -> Result<PreparedRecycling> {
        let checked = (|| {
            let record = self.execution_record(context, human, &lease)?;
            let evidence = record.evidence()?;
            let _pins = evidence
                .iter()
                .map(|file| context.path(Path::new(&file.path)))
                .collect::<Result<Vec<_>>>()?;
            record.validate(lease.directory)?;
            record.into_plan()
        })();
        let plan = match checked {
            Ok(plan) => plan,
            Err(error) => {
                self.finish(lease,State::Invalidated,serde_json::json!({"phase":"preflight","code":"revalidation_failed","originals_admitted":0}),now()?)?;
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
        let prepared = Batch::prepare_agent(plan, journal, cancel, apartment, &origin);
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
            let report = batch.abort(journal, cancel, |_| {});
            self.finish(
                lease,
                if report.cancelled {
                    State::Cancelled
                } else {
                    State::Failed
                },
                summary(&report),
                now()?,
            )?;
            return Err(error);
        }
        Ok(PreparedRecycling { lease, batch })
    }
}

fn summary(report: &Report) -> serde_json::Value {
    serde_json::json!({"phase":"preflight","originals_admitted":0,"total":report.items.len(),"cancelled":report.cancelled,
        "preflight_failed":report.preflight_error.is_some(),"journal_failed":report.journal_error.is_some(),
        "cleanup_requires_review":report.items.iter().any(|item| item.cleanup_error.is_some()),
        "unknown":report.items.iter().filter(|item| item.outcome==Outcome::Unknown).count(),
        "failed_nodes":report.items.iter().filter(|item| item.error.is_some()).map(|item| item.id).collect::<Vec<_>>()})
}
