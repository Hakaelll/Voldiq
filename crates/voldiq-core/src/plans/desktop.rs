//! Human-only facade, intentionally absent from the headless service and tool catalogue.
use super::{HumanAuthority, State, Store, View, invalid, now};
use crate::{
    Result,
    agents::{Capability, Registry},
};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub struct Desktop {
    directory: PathBuf,
    human: HumanAuthority,
}

#[derive(Serialize)]
pub struct Review {
    pub connection: String,
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
        progress: impl FnMut(usize),
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
        progress: impl FnMut(usize),
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
        match store.begin(&context, id, &self.human, now()?)? {
            super::Begin::Existing(_) => {}
            super::Begin::Started(lease) => {
                let prepared =
                    store.prepare_recycling(&context, &self.human, lease, journal, worker)?;
                prepared.execute_using(
                    &mut store,
                    &context,
                    &self.human,
                    journal,
                    progress,
                    launch,
                )?;
            }
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
        let names = registry
            .list()?
            .into_iter()
            .map(|connection| (connection.id, connection.name))
            .collect::<std::collections::HashMap<_, _>>();
        let mut store = Store::open(&self.directory)?;
        let refs = store.review_refs(&self.human, offset, limit)?;
        let next_offset = (refs.len() > limit).then_some(offset + limit);
        let mut items = Vec::new();
        for (id, owner) in refs.into_iter().take(limit) {
            let view = (|| {
                let principal = registry.human_principal(owner)?;
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
                connection: names
                    .get(&owner)
                    .cloned()
                    .unwrap_or_else(|| format!("Connection #{owner}")),
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
        let connection = registry
            .list()?
            .into_iter()
            .find(|connection| connection.id == owner)
            .ok_or_else(invalid)?
            .name;
        Ok(Review { connection, plan })
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
