//! Exact copy batches stop before any original handling when one member fails.
use super::{
    Action, CopyFailure, CopyPublication, ExecutionLease, HumanAuthority, Originals,
    PublishedContent, State, Store, TransferUpdate, TransferWorker, invalid, now,
};
use crate::{Error, NodeId, agents::PlanContext, transfer::Progress};

/// All selected copies from one consumed job, retained until original handling finishes.
/// This is not serializable, restart authority or evidence of complete restoration support.
pub struct CopiedTransfer<'a> {
    lease: ExecutionLease,
    copies: Vec<PublishedContent<'a>>,
    originals: Originals,
}
impl CopiedTransfer<'_> {
    pub fn originals(&self) -> Originals {
        self.originals
    }
    pub fn copied_files(&self) -> usize {
        self.copies.len()
    }
    pub fn verified_bytes(&self) -> u128 {
        self.copies
            .iter()
            .map(|copy| u128::from(copy.bytes()))
            .sum()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Transfer stopped: {error}")]
pub struct TransferFailure {
    #[source]
    error: CopyFailure,
    pub copied_files: usize,
    pub verified_bytes: u128,
    pub failed_node: Option<NodeId>,
    pub recorded_state: Option<State>,
    pub finalization_error: Option<Box<Error>>,
}
impl TransferFailure {
    pub fn copy_failure(&self) -> &CopyFailure {
        &self.error
    }
}

pub(super) struct StoppedCopies<T> {
    pub copies: Vec<T>,
    pub node: NodeId,
    pub error: CopyFailure,
}

pub(super) fn copy_all<T>(
    nodes: &[NodeId],
    mut copy: impl FnMut(NodeId) -> std::result::Result<T, CopyFailure>,
) -> std::result::Result<Vec<T>, Box<StoppedCopies<T>>> {
    let mut copies = Vec::with_capacity(nodes.len());
    for &node in nodes {
        match copy(node) {
            Ok(content) => copies.push(content),
            Err(error) => {
                return Err(Box::new(StoppedCopies {
                    copies,
                    node,
                    error,
                }));
            }
        }
    }
    Ok(copies)
}

impl Store {
    /// Copies the exact manifest, never a caller-provided subset. Admission remains hard-disabled.
    pub fn copy_transfer_batch<'a>(
        &mut self,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        lease: ExecutionLease,
        worker: TransferWorker<'a>,
        mut progress: impl FnMut(NodeId, Progress),
    ) -> std::result::Result<CopiedTransfer<'a>, Box<TransferFailure>> {
        let prepared = (|| {
            self.copy_checkpoint(context, human, &lease, &worker)?;
            let stored = self.read(context, &lease.id)?;
            if stored.view.hash != lease.hash || stored.view.state != State::Executing {
                return Err(invalid());
            }
            let Action::Offload(action) = &stored.view.manifest.action else {
                return Err(invalid());
            };
            action.execution_limit()?;
            for entry in &stored.view.manifest.entries {
                let _scope = context.path(std::path::Path::new(&entry.path))?;
            }
            stored.record.validate(lease.directory)?;
            action.check(
                context,
                self.directory.path(),
                std::path::Path::new(&stored.view.manifest.root),
                &stored.view.manifest.entries,
            )?;
            self.copy_checkpoint(context, human, &lease, &worker)?;
            Ok((
                stored
                    .view
                    .manifest
                    .entries
                    .iter()
                    .map(|entry| entry.id)
                    .collect::<Vec<_>>(),
                action.originals,
            ))
        })();
        let (nodes, originals) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                return Err(self.stop_transfer(
                    lease,
                    &[],
                    None,
                    CopyFailure::before(error),
                    &worker,
                ));
            }
        };
        let copies = match copy_all(&nodes, |node| {
            self.copy_transfer_file(context, human, &lease, node, worker.reborrow(), |value| {
                progress(node, value)
            })
        }) {
            Ok(copies) => copies,
            Err(stopped) => {
                return Err(self.stop_transfer(
                    lease,
                    &stopped.copies,
                    Some(stopped.node),
                    stopped.error,
                    &worker,
                ));
            }
        };
        Ok(CopiedTransfer {
            lease,
            copies,
            originals,
        })
    }

    /// Finishes only the explicit Keep policy, after every copy passes fresh full readback.
    pub fn finish_kept_transfer(
        &mut self,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        transfer: CopiedTransfer<'_>,
        worker: TransferWorker<'_>,
        mut progress: impl FnMut(NodeId, Progress),
    ) -> std::result::Result<(), Box<TransferFailure>> {
        let result = (|| {
            if transfer.originals != Originals::Keep || transfer.copies.is_empty() {
                return Err(invalid());
            }
            for copy in &transfer.copies {
                self.revalidate_published_content(
                    context,
                    human,
                    &transfer.lease,
                    copy,
                    worker.reborrow(),
                    |value| progress(copy.node(), value),
                )?;
            }
            for copy in &transfer.copies {
                self.copy_checkpoint(context, human, &transfer.lease, &worker)?;
                self.record_transfer(
                    context,
                    human,
                    &transfer.lease,
                    copy.node(),
                    TransferUpdate::Kept,
                )?;
            }
            self.copy_checkpoint(context, human, &transfer.lease, &worker)?;
            Ok(())
        })();
        if let Err(error) = result {
            return Err(self.stop_transfer(
                transfer.lease,
                &transfer.copies,
                None,
                CopyFailure::before(error),
                &worker,
            ));
        }
        let copied_files = transfer.copied_files();
        let verified_bytes = transfer.verified_bytes();
        let result = serde_json::json!({"copied_files":copied_files,"verified_bytes":verified_bytes.to_string(),
            "originals":"kept","immediate_free_bytes":"0","full_restore_verified":false});
        let completed =
            now().and_then(|at| self.finish(transfer.lease, State::Completed, result, at));
        completed.map_err(|error| {
            Box::new(TransferFailure {
                error: CopyFailure::before(invalid()),
                copied_files,
                verified_bytes,
                failed_node: None,
                recorded_state: None,
                finalization_error: Some(Box::new(error)),
            })
        })
    }

    fn stop_transfer(
        &mut self,
        lease: ExecutionLease,
        copies: &[PublishedContent<'_>],
        failed_node: Option<NodeId>,
        error: CopyFailure,
        worker: &TransferWorker<'_>,
    ) -> Box<TransferFailure> {
        let copied_files = copies.len();
        let verified_bytes: u128 = copies.iter().map(|copy| u128::from(copy.bytes())).sum();
        let state = if copied_files > 0 || error.publication() != CopyPublication::NotAttempted {
            State::PartiallyCompleted
        } else if worker.cancelled() {
            State::Cancelled
        } else {
            State::Failed
        };
        let result = serde_json::json!({"copied_files":copied_files,"verified_bytes":verified_bytes.to_string(),
            "failed_node":failed_node,"publication":format!("{:?}",error.publication()),
            "private_cleanup_failed":error.cleanup_error().is_some(),"originals":"unchanged",
            "immediate_free_bytes":"0","full_restore_verified":false});
        let id = lease.id.clone();
        let finalized = now()
            .and_then(|at| self.finish(lease, state, result, at))
            .and_then(|()| {
                let state: String = self.connection.query_row(
                    "SELECT state FROM agent_plans WHERE id=?1",
                    [&id],
                    |row| row.get(0),
                )?;
                serde_json::from_value(serde_json::Value::String(state)).map_err(|_| invalid())
            });
        let recorded_state = finalized.as_ref().ok().copied();
        Box::new(TransferFailure {
            error,
            copied_files,
            verified_bytes,
            failed_node,
            recorded_state,
            finalization_error: finalized.err().map(Box::new),
        })
    }
}
