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
