//! Human-only facade, intentionally absent from the headless service and tool catalogue.
use super::{Action, HumanAuthority, Originals, State, Store, Validation, View, invalid, now};
use crate::{
    Result,
    agents::{Capability, Registry},
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub struct Desktop {
    directory: PathBuf,
    human: HumanAuthority,
}

#[derive(Serialize)]
pub struct Review {
    pub connection: String,
    pub desktop_owned: bool,
    pub plan: View,
}

#[derive(Serialize)]
pub struct ReviewPage {
    pub items: Vec<ReviewSummary>,
    pub next_offset: Option<usize>,
}

#[derive(Serialize)]
pub struct ReviewSummary {
    pub id: String,
    pub connection_id: i64,
    pub connection: String,
    #[serde(flatten)]
    pub status: ReviewStatus,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionProgress {
    Recycling {
        processed: usize,
    },
    Copy {
        node: crate::NodeId,
        stage: crate::transfer::Phase,
        bytes: String,
        total: String,
    },
}

#[cfg(windows)]
impl ExecutionProgress {
    fn copy(node: crate::NodeId, progress: crate::transfer::Progress) -> Self {
        Self::Copy {
            node,
            stage: progress.phase,
            bytes: progress.bytes.to_string(),
            total: progress.total.to_string(),
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub enum ReviewStatus {
    Ready {
        state: State,
        expires: i64,
        files: usize,
        logical_bytes: String,
    },
    Unavailable {
        reason: &'static str,
    },
}

impl Desktop {
    #[cfg(windows)]
    pub fn prepare_restore(
        &self,
        request: &super::RestoreRequest,
        hash: &str,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<View> {
        let mut registry = Registry::open(&self.directory.join("agents.db"))?;
        Store::open(&self.directory)?.create_desktop_restore(
            &self.human,
            &mut registry,
            request,
            hash,
            cancel,
        )
    }

    #[cfg(windows)]
    pub fn prepare_offload(
        &self,
        index: &crate::index::Index,
        nodes: &[crate::NodeId],
        target: &Path,
        expected_volume: &crate::space::VolumeIdentity,
        originals: Originals,
    ) -> Result<View> {
        use super::offload::{OffloadRequest, VolumeBinding};
        let target = crate::agents::pin_directory(target)?;
        let volume = VolumeBinding::observe(target.path())?;
        if !volume.matches(expected_volume) {
            return Err(crate::Error::Invalid(
                "The destination volume changed. Refresh destinations and choose again.".into(),
            ));
        }
        let source_volume = VolumeBinding::observe(&index.root)?;
        if source_volume.guid_root == volume.guid_root || source_volume.serial == volume.serial {
            return Err(crate::Error::Invalid(
                "Select a destination on a different volume.".into(),
            ));
        }
        let root = target.path().ancestors().last().ok_or_else(invalid)?;
        let mut registry = Registry::open(&self.directory.join("agents.db"))?;
        let principal = registry.desktop_transfer_principal(&self.human, &index.root, root)?;
        let mut store = Store::open(&self.directory)?;
        store.create_offload(
            &registry.plan_context(&principal, Capability::PreparePlans)?,
            index.snapshot_id.unwrap_or(0),
            index,
            nodes,
            &OffloadRequest {
                destination: target.path().to_owned(),
                originals,
            },
            now()?,
        )
    }

    pub fn queue_desktop_execution(&self, id: &str, hash: &str) -> Result<()> {
        let registry = Registry::open(&self.directory.join("agents.db"))?;
        let mut store = Store::open(&self.directory)?;
        let owner = store.review_owner(&self.human, id)?;
        let principal = registry.human_principal(owner)?;
        let execute = registry.plan_context(&principal, Capability::ExecuteApproved)?;
        if !execute.is_desktop() {
            return Err(invalid());
        }
        store.request_execution(&execute, id, hash, now()?)?;
        Ok(())
    }

    pub fn vault_quota(&self) -> Result<super::VaultQuota> {
        Store::open(&self.directory)?.vault_quota(&self.human)
    }

    pub fn set_vault_quota(&self, limit: Option<&str>) -> Result<super::VaultQuota> {
        let mut store = Store::open(&self.directory)?;
        store.set_vault_quota(&self.human, limit)?;
        store.vault_quota(&self.human)
    }

    pub fn set_vault_retention(&self, days: Option<u16>) -> Result<()> {
        Store::open(&self.directory)?.set_vault_retention(&self.human, days)
    }

    pub fn transfer_operations(
        &self,
        before: Option<&str>,
        limit: usize,
        filter: &super::TransferFilter,
    ) -> Result<super::TransferPage> {
        Store::open(&self.directory)?.transfer_operations(&self.human, before, limit, filter)
    }
    pub fn transfer_catalogue(
        &self,
        id: &str,
        after: Option<crate::NodeId>,
        limit: usize,
        filter: &super::TransferFilter,
    ) -> Result<super::TransferCatalogue> {
        Store::open(&self.directory)?.transfer_catalogue(&self.human, id, after, limit, filter)
    }

    pub fn pending_execution(&self, limit: usize) -> Result<Vec<super::ExecutionRequest>> {
        Store::open(&self.directory)?.pending_execution(&self.human, limit, now()?)
    }

    pub fn cancel(&self, id: &str, hash: &str) -> Result<View> {
        let registry = Registry::open(&self.directory.join("agents.db"))?;
        let mut store = Store::open(&self.directory)?;
        let owner = store.review_owner(&self.human, id)?;
        let principal = registry.human_principal(owner)?;
        let execute = registry.plan_context(&principal, Capability::ExecuteApproved)?;
        store.cancel_execution(&execute, id, hash, now()?)?;
        store.get(
            &registry.plan_context(&principal, Capability::Metadata)?,
            id,
            now()?,
        )
    }

    #[cfg(windows)]
    pub fn execute_request(
        &self,
        id: &str,
        hash: &str,
        journal: &mut crate::cleanup::journal::Journal,
        worker: super::RecyclingWorker<'_>,
        progress: impl FnMut(ExecutionProgress),
    ) -> Result<View> {
        let result = self.execute_request_using(
            id,
            hash,
            journal,
            worker,
            progress,
            crate::cleanup::helper::Session::launch,
        );
        if result.is_err() {
            Store::open(&self.directory)?.discard_request(&self.human, id, hash, now()?)?;
        }
        result
    }

    #[cfg(windows)]
    fn execute_request_using(
        &self,
        id: &str,
        hash: &str,
        journal: &mut crate::cleanup::journal::Journal,
        worker: super::RecyclingWorker<'_>,
        mut progress: impl FnMut(ExecutionProgress),
        launch: impl FnOnce(
            &crate::cleanup::recycle::ShellApartment,
        ) -> std::io::Result<crate::cleanup::helper::Session>,
    ) -> Result<View> {
        let registry = Registry::open(&self.directory.join("agents.db"))?;
        let mut store = Store::open(&self.directory)?;
        let owner = store.review_owner(&self.human, id)?;
        let principal = registry.human_principal(owner)?;
        let metadata = registry.plan_context(&principal, Capability::Metadata)?;
        let view = store.get(&metadata, id, now()?)?;
        if view.hash != hash {
            return Err(invalid());
        }
        let context = registry.plan_context(&principal, Capability::ExecuteApproved)?;
        match store.begin(&context, id, hash, &self.human, now()?)? {
            super::Begin::Existing(_) => {}
            super::Begin::Started(lease) => match view.manifest.action {
                Action::Recycle => {
                    let prepared =
                        store.prepare_recycling(&context, &self.human, lease, journal, worker)?;
                    prepared.execute_using(
                        &mut store,
                        &context,
                        &self.human,
                        journal,
                        |processed| progress(ExecutionProgress::Recycling { processed }),
                        launch,
                    )?;
                }
                Action::Offload(_) | Action::Restore(_) => {
                    let copied = store.copy_transfer_batch(
                        &context,
                        &self.human,
                        lease,
                        super::TransferWorker::new(worker.operation, worker.cancel),
                        |node, value| progress(ExecutionProgress::copy(node, value)),
                    );
                    match copied {
                        Ok(copied) if copied.originals() == Originals::Keep => {
                            if let Err(failure) = store.finish_kept_transfer(
                                &context,
                                &self.human,
                                copied,
                                super::TransferWorker::new(worker.operation, worker.cancel),
                                |node, value| progress(ExecutionProgress::copy(node, value)),
                            ) && (failure.recorded_state.is_none()
                                || failure.finalization_error.is_some())
                            {
                                return Err(crate::Error::Invalid(failure.to_string()));
                            }
                        }
                        Ok(copied) => {
                            let prepared = store.prepare_offload_recycling(
                                &context,
                                &self.human,
                                copied,
                                journal,
                                worker,
                                |node, value| progress(ExecutionProgress::copy(node, value)),
                            );
                            let result = match prepared {
                                Ok(prepared) => prepared.execute_using(
                                    &mut store,
                                    &context,
                                    &self.human,
                                    journal,
                                    |processed| {
                                        progress(ExecutionProgress::Recycling { processed })
                                    },
                                    launch,
                                ),
                                Err(result) => *result,
                            };
                            if result.recorded_state.is_none()
                                || result.finalization_error.is_some()
                            {
                                return Err(crate::Error::Invalid("Transfer result could not be finalized; reconcile its catalogue and journal before further work.".into()));
                            }
                        }
                        Err(failure) => {
                            if failure.recorded_state.is_none()
                                || failure.finalization_error.is_some()
                            {
                                return Err(crate::Error::Invalid(failure.to_string()));
                            }
                        }
                    }
                }
            },
        }
        store.get(&metadata, id, now()?)
    }
    pub fn close(&self) {
        self.human.close();
    }

    pub fn open(directory: &Path) -> Result<Self> {
        let directory = std::fs::canonicalize(directory)?;
        let human = HumanAuthority::acquire(&directory)?;
        Store::open(&directory)?.recover(&human, now()?)?;
        Ok(Self { directory, human })
    }

    pub fn list(&self, offset: usize, limit: usize) -> Result<ReviewPage> {
        let registry = Registry::open(&self.directory.join("agents.db"))?;
        let mut store = Store::open(&self.directory)?;
        let refs = store.review_refs(&self.human, offset, limit)?;
        let next_offset = (refs.len() > limit).then_some(offset + limit);
        let mut items = Vec::new();
        for (id, owner) in refs.into_iter().take(limit) {
            let mut connection = format!("Connection #{owner}");
            let view = (|| {
                let principal = registry.human_principal(owner)?;
                (connection, _) = registry.review_identity(&principal)?;
                let context = registry.plan_context(&principal, Capability::Metadata)?;
                store.get(&context, &id, now()?)
            })();
            let status = match view {
                Ok(view) => ReviewStatus::Ready {
                    state: view.state,
                    expires: view.manifest.expires,
                    files: view.manifest.entries.len(),
                    logical_bytes: view.manifest.logical_bytes,
                },
                Err(_) => ReviewStatus::Unavailable {
                    reason: "Connection, policy, scope or plan evidence is no longer available. Execution is blocked.",
                },
            };
            items.push(ReviewSummary {
                connection,
                id,
                connection_id: owner,
                status,
            });
        }
        Ok(ReviewPage { items, next_offset })
    }

    pub fn review(&self, id: &str) -> Result<Review> {
        let registry = Registry::open(&self.directory.join("agents.db"))?;
        let mut store = Store::open(&self.directory)?;
        let owner = store.review_owner(&self.human, id)?;
        let principal = registry.human_principal(owner)?;
        let context = registry.plan_context(&principal, Capability::Metadata)?;
        let plan = store.get(&context, id, now()?)?;
        let (connection, desktop_owned) = registry.review_identity(&principal)?;
        Ok(Review {
            connection,
            desktop_owned,
            plan,
        })
    }

    pub fn validate(&self, id: &str, hash: &str) -> Result<Validation> {
        let registry = Registry::open(&self.directory.join("agents.db"))?;
        let store = Store::open(&self.directory)?;
        let owner = store.review_owner(&self.human, id)?;
        let principal = registry.human_principal(owner)?;
        let context = registry.plan_context(&principal, Capability::PreparePlans)?;
        store
            .validate(&context, id, hash, &AtomicBool::new(false))?
            .ok_or_else(invalid)
    }

    /// Caller must obtain a separate human confirmation for the exact displayed hash.
    pub fn approve(&self, id: &str, expected_hash: &str) -> Result<View> {
        let registry = Registry::open(&self.directory.join("agents.db"))?;
        let mut store = Store::open(&self.directory)?;
        let owner = store.review_owner(&self.human, id)?;
        let principal = registry.human_principal(owner)?;
        let context = registry.plan_context(&principal, Capability::Metadata)?;
        store.approve(&context, id, expected_hash, &self.human, now()?)
    }

    pub fn reject(&self, id: &str, expected_hash: &str) -> Result<View> {
        let registry = Registry::open(&self.directory.join("agents.db"))?;
        let mut store = Store::open(&self.directory)?;
        let owner = store.review_owner(&self.human, id)?;
        let principal = registry.human_principal(owner)?;
        let context = registry.plan_context(&principal, Capability::Metadata)?;
        if store.get(&context, id, now()?)?.hash != expected_hash {
            return Err(invalid());
        }
        store.reject(&context, id, &self.human, now()?)
    }
}

#[cfg(all(test, windows))]
mod tests;
