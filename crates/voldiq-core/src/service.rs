//! Authorized services shared by headless adapters. Human management uses separate APIs.
use crate::{
    Error, Kind, Result, ScanPhase,
    agents::{Capability, Principal, Registry, denied},
    index::{Details, Index, Page, Query},
    jobs::{Job, JobState, Jobs, ScanLease},
    scanner,
    snapshots::Store,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone)]
pub struct AgentService {
    directory: PathBuf,
    principal: Principal,
    cache: Arc<Mutex<Option<CachedIndex>>>,
}

struct CachedIndex {
    snapshot: i64,
    index: Arc<Index>,
}

#[derive(Serialize)]
pub struct Stats {
    pub snapshot_id: i64,
    pub root: PathBuf,
    pub logical: u64,
    pub allocated: Option<u64>,
    pub files: u64,
    pub folders: usize,
    pub incomplete: bool,
    pub errors: u64,
}

impl AgentService {
    pub fn new(directory: PathBuf, principal: Principal) -> Self {
        Self {
            directory,
            principal,
            cache: Arc::new(Mutex::new(None)),
        }
    }

    fn registry(&self) -> Result<Registry> {
        Registry::open(&self.directory.join("agents.db"))
    }

    pub fn capabilities(&self) -> Result<serde_json::Value> {
        let policy = self
            .registry()?
            .policy(&self.principal, Capability::Metadata)?;
        Ok(
            serde_json::json!({"schema_version":1,"capabilities":policy.capabilities(),"limits":policy.limits(),
            "transport":"local_stdio","approval":"independent_human_required","execution_available":false,
            "scope":"Only this connection's completed scan snapshots are queryable."}),
        )
    }

    pub fn start_scan(&self, root: &Path) -> Result<i64> {
        let permit = self.registry()?.scan_permit(&self.principal, root)?;
        let lease = ScanLease::acquire(&self.directory)?;
        let mut jobs = Jobs::open(&self.directory)?;
        let id = jobs.begin(&lease, self.principal.id(), permit.root.path())?;
        let service = self.clone();
        let worker = std::thread::Builder::new()
            .name(format!("voldiq-scan-{id}"))
            .spawn(move || {
                let _lease = lease;
                let result = service.run_scan(id, &jobs, permit);
                if result.is_err() {
                    let _ = jobs.finish(service.principal.id(), id, JobState::Failed, None);
                }
            });
        if worker.is_err() {
            Jobs::open(&self.directory)?.finish(self.principal.id(), id, JobState::Failed, None)?;
            return Err(Error::Invalid("Unable to start scan worker.".into()));
        }
        Ok(id)
    }

    fn run_scan(&self, id: i64, jobs: &Jobs, permit: crate::agents::ScanPermit) -> Result<()> {
        let cancel = AtomicBool::new(false);
        let registry = self.registry()?;
        let mut phase = ScanPhase::Scanning;
        let mut reporting_failed = false;
        let index = scanner::scan(permit.root.path(), &permit.options, &cancel, |progress| {
            phase = progress.phase;
            let revoked = registry.policy(&self.principal, Capability::Scan).is_err();
            match jobs.progress(self.principal.id(), id, &progress) {
                Ok(requested) => {
                    if revoked || requested {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                Err(_) => {
                    reporting_failed = true;
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        })?;
        if reporting_failed {
            return Err(Error::Invalid("Scan progress persistence failed.".into()));
        }
        if cancel.load(Ordering::Relaxed) || phase == ScanPhase::Cancelled {
            return jobs.finish(self.principal.id(), id, JobState::Cancelled, None);
        }
        registry.authorize_index(&self.principal, &index)?;
        let mut store = Store::open(&self.directory.join("snapshots.db"))?;
        let snapshot = store.save(&index, &cancel, &[])?;
        jobs.finish(self.principal.id(), id, JobState::Completed, Some(snapshot))
    }

    pub fn job(&self, id: i64) -> Result<Job> {
        let registry = self.registry()?;
        registry.policy(&self.principal, Capability::Metadata)?;
        let job = Jobs::open(&self.directory)?.get(self.principal.id(), id)?;
        registry.authorize(&self.principal, Capability::Metadata, &job.root)?;
        if job.state == JobState::Running {
            Jobs::reconcile_abandoned(&self.directory, self.principal.id())?;
            return Jobs::open(&self.directory)?.get(self.principal.id(), id);
        }
        Ok(job)
    }

    pub fn cancel(&self, id: i64) -> Result<()> {
        self.registry()?.policy(&self.principal, Capability::Scan)?;
        let jobs = Jobs::open(&self.directory)?;
        let job = jobs.get(self.principal.id(), id)?;
        self.registry()?
            .authorize(&self.principal, Capability::Scan, &job.root)?;
        jobs.cancel(self.principal.id(), id)
    }

    pub fn wait(&self, id: i64) -> Result<Job> {
        loop {
            let job = self.job(id)?;
            if job.state != JobState::Running {
                return Ok(job);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn index(&self, snapshot: i64) -> Result<Arc<Index>> {
        let registry = self.registry()?;
        registry.policy(&self.principal, Capability::Metadata)?;
        if !Jobs::open(&self.directory)?.owns_snapshot(self.principal.id(), snapshot)? {
            return Err(denied());
        }
        let mut cache = self.cache.lock().map_err(|_| denied())?;
        let index =
            if let Some(cached) = cache.as_ref().filter(|cached| cached.snapshot == snapshot) {
                cached.index.clone()
            } else {
                let index = Arc::new(
                    Store::open(&self.directory.join("snapshots.db"))?
                        .load(snapshot, &AtomicBool::new(false))
                        .map_err(|_| denied())?,
                );
                *cache = Some(CachedIndex {
                    snapshot,
                    index: index.clone(),
                });
                index
            };
        registry.authorize_index(&self.principal, &index)?;
        Ok(index)
    }

    pub fn query(&self, snapshot: i64, query: &Query) -> Result<Page> {
        if !matches!(query.sort.as_str(), "size" | "name" | "modified")
            || query.text.len() > 4096
            || query.extension.len() > 128
        {
            return Err(Error::Invalid(
                "Query requires a supported sort and bounded text/extension filters.".into(),
            ));
        }
        let policy = self
            .registry()?
            .policy(&self.principal, Capability::Metadata)?;
        let query = Query {
            limit: query.limit.min(policy.limits().page_size).max(1),
            ..query.clone()
        };
        self.index(snapshot)?.query(&query).map_err(|_| denied())
    }

    pub fn details(&self, snapshot: i64, node: crate::NodeId) -> Result<Details> {
        self.index(snapshot)?.details(node).map_err(|_| denied())
    }

    pub fn stats(&self, snapshot: i64) -> Result<Stats> {
        let index = self.index(snapshot)?;
        let root = index.entry(crate::NodeId(0))?;
        Ok(Stats {
            snapshot_id: snapshot,
            root: index.root.clone(),
            logical: root.logical,
            allocated: root.allocation_known.then_some(root.allocated),
            files: root.files,
            folders: index
                .entries
                .iter()
                .filter(|entry| entry.kind == Kind::Directory)
                .count(),
            incomplete: root.incomplete,
            errors: index.error_count,
        })
    }
}
