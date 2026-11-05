//! Recycling approved transfer originals through the existing retained-source executor.
use super::{
    Action, CopiedTransfer, ExecutionLease, HumanAuthority, Originals, RecyclingWorker, State,
    Store, TransferUpdate, TransferWorker, copying::PublishedDestination, invalid, now,
};
use crate::{
    NodeId, Result,
    agents::PlanContext,
    cleanup::{
        helper::Session,
        journal::{AgentExecutionRef, Journal},
        recycle::{
            ShellApartment,
            batch::{Batch, Outcome, Report},
        },
    },
    transfer::Progress,
};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Serialize)]
pub struct OriginalTransferResult {
    pub copied_files: usize,
    pub verified_bytes: String,
    pub report: Option<Report>,
    pub error: Option<String>,
    pub catalogue_error: Option<String>,
    pub recorded_state: Option<State>,
    pub finalization_error: Option<String>,
}

/// Holds destination identities/bytes and the same source handles through native recycling.
/// No adapter can construct this from catalogue rows, paths or serialized copy receipts.
pub struct PreparedOffloadRecycling<'a> {
    lease: ExecutionLease,
    destinations: Vec<PublishedDestination<'a>>,
    batch: Batch,
    worker: RecyclingWorker<'a>,
    copied_files: usize,
    verified_bytes: u128,
}

impl PreparedOffloadRecycling<'_> {
    pub fn cancel(self, plans: &mut Store, journal: &mut Journal) -> OriginalTransferResult {
        let report = self.batch.abort(journal, &AtomicBool::new(true), |_| {});
        plans.finish_original_transfer(
            self.lease,
            (self.copied_files, self.verified_bytes),
            Some(report),
            None,
            None,
        )
    }

    pub fn execute(
        self,
        plans: &mut Store,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        journal: &mut Journal,
        progress: impl FnMut(usize),
    ) -> OriginalTransferResult {
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
    ) -> OriginalTransferResult {
        let Self {
            lease,
            destinations,
            batch,
            worker,
            copied_files,
            verified_bytes,
        } = self;
        let transfer_worker = TransferWorker::new(worker.operation, worker.cancel);
        let report = {
            let check = || -> Result<()> {
                plans.copy_checkpoint(context, human, &lease, &transfer_worker)?;
                for destination in &destinations {
                    destination.check()?;
                }
                Ok(())
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
                            worker.cancel.store(true, Ordering::Release);
                        }
                        let mut report = batch.abort(journal, worker.cancel, &mut progress);
                        report.helper_error = Some(error.to_string());
                        report
                    }
                }
            }
        };
        let final_check: Result<()> = (|| {
            plans.copy_checkpoint(context, human, &lease, &transfer_worker)?;
            for destination in &destinations {
                destination.check()?;
            }
            Ok(())
        })();
        let mut catalogue_error = None;
        for item in &report.items {
            if item.outcome == Outcome::Recycled && item.recorded && item.error.is_none() {
                if !destinations
                    .iter()
                    .any(|destination| destination.node == item.id)
                {
                    catalogue_error = Some(
                        "Unexpected original result; catalogue reconciliation is required.".into(),
                    );
                    break;
                }
                if let Err(error) =
                    plans.record_transfer(context, human, &lease, item.id, TransferUpdate::Recycled)
                {
                    catalogue_error = Some(error.to_string());
                    break;
                }
            }
        }
        plans.finish_original_transfer(
            lease,
            (copied_files, verified_bytes),
            Some(report),
            final_check.err().map(|error| error.to_string()),
            catalogue_error,
        )
    }
}

impl Store {
    /// Requires the complete sealed copy set and the explicit recycle-originals policy.
    /// Offload approval remains unavailable until restore and desktop integration are ready.
    pub fn prepare_offload_recycling<'a>(
        &mut self,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        transfer: CopiedTransfer<'a>,
        journal: &mut Journal,
        worker: RecyclingWorker<'a>,
        mut progress: impl FnMut(NodeId, Progress),
    ) -> std::result::Result<PreparedOffloadRecycling<'a>, Box<OriginalTransferResult>> {
        let stats = (transfer.copied_files(), transfer.verified_bytes());
        let transfer_worker = TransferWorker::new(worker.operation, worker.cancel);
        let checked = (|| {
            self.copy_checkpoint(context, human, &transfer.lease, &transfer_worker)?;
            if transfer.originals != Originals::RecycleAfterVerification
                || transfer.copies.is_empty()
            {
                return Err(invalid());
            }
            let stored = self.read(context, &transfer.lease.id)?;
            if stored.view.hash != transfer.lease.hash || stored.view.state != State::Executing {
                return Err(invalid());
            }
            let Action::Offload(action) = &stored.view.manifest.action else {
                return Err(invalid());
            };
            if action.originals != Originals::RecycleAfterVerification
                || stored
                    .view
                    .manifest
                    .entries
                    .iter()
                    .map(|entry| entry.id)
                    .ne(transfer.copies.iter().map(|copy| copy.node()))
            {
                return Err(invalid());
            }
            action.execution_limit()?;
            stored.record.validate(transfer.lease.directory)?;
            for copy in &transfer.copies {
                self.revalidate_published_content(
                    context,
                    human,
                    &transfer.lease,
                    copy,
                    transfer_worker.reborrow(),
                    |value| progress(copy.node(), value),
                )?;
            }
            let plan = stored
                .record
                .into_plan()?
                .with_protections(self.directory.path())?;
            for copy in &transfer.copies {
                self.copy_checkpoint(context, human, &transfer.lease, &transfer_worker)?;
                self.record_transfer(
                    context,
                    human,
                    &transfer.lease,
                    copy.node(),
                    TransferUpdate::OriginalIntent,
                )?;
            }
            Ok(plan)
        })();
        let plan = match checked {
            Ok(plan) => plan,
            Err(error) => {
                return Err(Box::new(self.finish_original_transfer(
                    transfer.lease,
                    stats,
                    None,
                    Some(error.to_string()),
                    None,
                )));
            }
        };
        let mut sources = Vec::with_capacity(transfer.copies.len());
        let mut destinations = Vec::with_capacity(transfer.copies.len());
        for copy in transfer.copies {
            let node = copy.node();
            let (source, destination) = copy.into_original();
            sources.push((node, source));
            destinations.push(destination);
        }
        let lease = transfer.lease;
        let origin = AgentExecutionRef {
            plan_id: &lease.id,
            manifest_hash: &lease.hash,
            connection_id: lease.owner,
            execution_nonce: &lease.nonce,
            directory: lease.directory,
        };
        let batch = match Batch::prepare_transferred(
            plan,
            sources,
            journal,
            worker.cancel,
            worker.apartment,
            Some(&origin),
        ) {
            Ok(batch) => batch,
            Err(error) => {
                return Err(Box::new(self.finish_original_transfer(
                    lease,
                    stats,
                    None,
                    Some(error.to_string()),
                    None,
                )));
            }
        };
        let checked: Result<()> = (|| {
            self.copy_checkpoint(context, human, &lease, &transfer_worker)?;
            for destination in &destinations {
                destination.check()?;
            }
            if !batch.ready_for_admission() {
                return Err(invalid());
            }
            Ok(())
        })();
        if let Err(error) = checked {
            let report = batch.abort(journal, worker.cancel, |_| {});
            return Err(Box::new(self.finish_original_transfer(
                lease,
                stats,
                Some(report),
                Some(error.to_string()),
                None,
            )));
        }
        Ok(PreparedOffloadRecycling {
            lease,
            destinations,
            batch,
            worker,
            copied_files: stats.0,
            verified_bytes: stats.1,
        })
    }

    fn finish_original_transfer(
        &mut self,
        lease: ExecutionLease,
        stats: (usize, u128),
        report: Option<Report>,
        error: Option<String>,
        catalogue_error: Option<String>,
    ) -> OriginalTransferResult {
        let completed = report
            .as_ref()
            .is_some_and(|report| super::recycling::outcome(report, false) == State::Completed)
            && error.is_none()
            && catalogue_error.is_none();
        let state = if completed {
            State::Completed
        } else {
            State::PartiallyCompleted
        };
        let mut summary = report
            .as_ref()
            .map(super::recycling::execution_summary)
            .unwrap_or_else(
                || serde_json::json!({"phase":"original_preflight","originals_admitted":0}),
            );
        summary["copied_files"] = serde_json::json!(stats.0);
        summary["verified_bytes"] = serde_json::json!(stats.1.to_string());
        summary["immediate_free_bytes"] = serde_json::json!("0");
        summary["full_restore_verified"] = serde_json::json!(false);
        summary["catalogue_requires_review"] = serde_json::json!(catalogue_error.is_some());
        summary["original_phase_failed"] = serde_json::json!(error.is_some());
        let result = now().and_then(|at| self.finish(lease, state, summary, at));
        OriginalTransferResult {
            copied_files: stats.0,
            verified_bytes: stats.1.to_string(),
            report,
            error,
            catalogue_error,
            recorded_state: result.as_ref().ok().map(|()| state),
            finalization_error: result.err().map(|error| error.to_string()),
        }
    }
}
