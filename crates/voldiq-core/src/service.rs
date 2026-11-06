//! Authorized services shared by headless adapters. Human management uses separate APIs.
use crate::{
    Error, Kind, Result, ScanPhase,
    agents::{Capability, Principal, Registry, denied},
    index::{Details, Index, Page, Query},
    jobs::{Job, JobState, Jobs},
    operations::Coordinator,
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

mod applications;
pub use applications::ApplicationPage;

#[derive(Clone)]
pub struct AgentService {
    directory: Coordinator,
    principal: Principal,
    cache: Arc<Mutex<Option<CachedIndex>>>,
}

struct CachedIndex {
    snapshot: i64,
    index: Arc<Index>,
    applications: Option<crate::applications::Capture>,
    application_access: Vec<bool>,
}

#[derive(Serialize)]
pub struct HistoryPage {
    pub items: Vec<crate::snapshots::Snapshot>,
    pub next_offset: Option<usize>,
    pub limited: bool,
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
    pub fn plan_execute(&self, id: &str, hash: &str) -> Result<serde_json::Value> {
        let registry = self.registry()?;
        let execute = registry.plan_context(&self.principal, Capability::ExecuteApproved)?;
        let metadata = registry.plan_context(&self.principal, Capability::Metadata)?;
        let mut store = crate::plans::Store::open(self.directory.directory())?;
        let view = store.get(&metadata, id, crate::plans::now()?)?;
        if view.hash != hash {
            return Err(denied());
        }
        let existing = view.state.terminal() || view.state == crate::plans::State::Executing;
        if !existing
            && (!cfg!(windows)
                || !crate::plans::HumanAuthority::is_active(self.directory.directory())?)
        {
            return Err(Error::NeedsHumanApproval);
        }
        let queued = matches!(
            store.request_execution(&execute, id, hash, crate::plans::now()?)?,
            crate::plans::Enqueue::Queued
        );
        let view = store.get(&metadata, id, crate::plans::now()?)?;
        Ok(serde_json::json!({"dispatch":if queued {"queued"} else {"existing"},"plan":view}))
    }

    pub fn plan_cancel(&self, id: &str, hash: &str) -> Result<crate::plans::View> {
        let registry = self.registry()?;
        let execute = registry.plan_context(&self.principal, Capability::ExecuteApproved)?;
        let mut store = crate::plans::Store::open(self.directory.directory())?;
        store.cancel_execution(&execute, id, hash, crate::plans::now()?)?;
        let metadata = registry.plan_context(&self.principal, Capability::Metadata)?;
        store.get(&metadata, id, crate::plans::now()?)
    }
    pub fn plan_prepare(
        &self,
        snapshot: i64,
        selection: &[crate::NodeId],
    ) -> Result<crate::plans::View> {
        let registry = self.registry()?;
        let context = registry.plan_context(&self.principal, Capability::PreparePlans)?;
        let index = self.index(snapshot)?;
        crate::plans::Store::open(self.directory.directory())?.create(
            &context,
            snapshot,
            &index,
            selection,
            crate::plans::now()?,
        )
    }

    pub fn plan_get(&self, id: &str) -> Result<crate::plans::View> {
        let registry = self.registry()?;
        let context = registry.plan_context(&self.principal, Capability::Metadata)?;
        crate::plans::Store::open(self.directory.directory())?.get(
            &context,
            id,
            crate::plans::now()?,
        )
    }

    pub fn plan_prepare_offload(
        &self,
        snapshot: i64,
        selection: &[crate::NodeId],
        request: &crate::plans::OffloadRequest,
    ) -> Result<crate::plans::View> {
        let registry = self.registry()?;
        let context = registry.plan_context(&self.principal, Capability::PreparePlans)?;
        let index = self.index(snapshot)?;
        crate::plans::Store::open(self.directory.directory())?.create_offload(
            &context,
            snapshot,
            &index,
            selection,
            request,
            crate::plans::now()?,
        )
    }

    pub fn plan_prepare_restore(
        &self,
        snapshot: i64,
        request: &crate::plans::RestoreRequest,
    ) -> Result<crate::plans::View> {
        let registry = self.registry()?;
        let context = registry.plan_context(&self.principal, Capability::PreparePlans)?;
        let index = self.index(snapshot)?;
        crate::plans::Store::open(self.directory.directory())?.create_restore(
            &context,
            snapshot,
            &index,
            request,
            crate::plans::now()?,
        )
    }

    pub fn plan_request(&self, id: &str) -> Result<crate::plans::View> {
        let registry = self.registry()?;
        let context = registry.plan_context(&self.principal, Capability::RequestApproval)?;
        crate::plans::Store::open(self.directory.directory())?.request(
            &context,
            id,
            crate::plans::now()?,
        )?;
        Err(Error::NeedsHumanApproval)
    }

    pub fn plan_audit(
        &self,
        id: &str,
        after: i64,
        limit: usize,
    ) -> Result<Vec<crate::plans::AuditEvent>> {
        let registry = self.registry()?;
        let context = registry.plan_context(&self.principal, Capability::Metadata)?;
        crate::plans::Store::open(self.directory.directory())?.audit(&context, id, after, limit)
    }

    pub fn new(directory: PathBuf, principal: Principal) -> Result<Self> {
        Ok(Self {
            directory: Coordinator::open(&directory)?,
            principal,
            cache: Arc::new(Mutex::new(None)),
        })
    }

    fn registry(&self) -> Result<Registry> {
        Registry::open(&self.directory.directory().join("agents.db"))
    }

    pub fn capabilities(&self) -> Result<serde_json::Value> {
        let policy = self
            .registry()?
            .policy(&self.principal, Capability::Metadata)?;
        let execution = cfg!(windows)
            && policy.capabilities().contains(&Capability::ExecuteApproved)
            && crate::plans::HumanAuthority::is_active(self.directory.directory())?;
        Ok(
            serde_json::json!({"schema_version":1,"capabilities":policy.capabilities(),"limits":policy.limits(),
            "transport":"local_stdio","approval":"independent_human_required","plan_preparation_available":true,"execution_available":execution,"application_attribution_available":cfg!(windows),
            "scope":"Stored snapshots whose native root identity and exclusion coverage satisfy this connection's current policy."}),
        )
    }

    pub fn start_scan(&self, root: &Path) -> Result<i64> {
        let permit = self.registry()?.scan_permit(&self.principal, root)?;
        let lease = self.directory.acquire()?;
        let mut jobs = Jobs::open(self.directory.directory())?;
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
            Jobs::open(self.directory.directory())?.finish(
                self.principal.id(),
                id,
                JobState::Failed,
                None,
            )?;
            return Err(Error::Invalid("Unable to start scan worker.".into()));
        }
        Ok(id)
    }

    fn run_scan(&self, id: i64, jobs: &Jobs, permit: crate::agents::ScanPermit) -> Result<()> {
        let cancel = AtomicBool::new(false);
        let registry = self.registry()?;
        let mut phase = ScanPhase::Scanning;
        let mut reporting_failed = false;
        let mut index = scanner::scan(permit.root.path(), &permit.options, &cancel, |progress| {
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
        index.captured_applications = Some(
            crate::applications::installed_programs(&cancel)
                .and_then(|catalog| {
                    crate::applications::capture_authorized(&index, catalog, &cancel, |path| {
                        registry.authorize(&self.principal, Capability::Metadata, path)
                    })
                })
                .unwrap_or_else(|_| crate::applications::Capture::unavailable()),
        );
        let mut store = Store::open(&self.directory.directory().join("snapshots.db"))?;
        let snapshot = store.save(&index, &cancel, &[])?;
        jobs.finish(self.principal.id(), id, JobState::Completed, Some(snapshot))
    }

    pub fn job(&self, id: i64) -> Result<Job> {
        let registry = self.registry()?;
        registry.policy(&self.principal, Capability::Metadata)?;
        let job = Jobs::open(self.directory.directory())?.get(self.principal.id(), id)?;
        registry.authorize(&self.principal, Capability::Metadata, &job.root)?;
        if job.state == JobState::Running {
            Jobs::reconcile_abandoned(self.directory.directory(), self.principal.id())?;
            return Jobs::open(self.directory.directory())?.get(self.principal.id(), id);
        }
        Ok(job)
    }

    pub fn cancel(&self, id: i64) -> Result<()> {
        self.registry()?.policy(&self.principal, Capability::Scan)?;
        let jobs = Jobs::open(self.directory.directory())?;
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
        let mut cache = self.cache.lock().map_err(|_| denied())?;
        let reusable = if let Some(cached) =
            cache.as_ref().filter(|cached| cached.snapshot == snapshot)
        {
            registry.authorize_index(&self.principal, &cached.index)?;
            let access =
                self.application_access(&registry, &cached.index, cached.applications.as_ref())?;
            access == cached.application_access
        } else {
            false
        };
        let index = if reusable {
            cache.as_ref().ok_or_else(denied)?.index.clone()
        } else {
            let store = Store::open(&self.directory.directory().join("snapshots.db"))?;
            let scope = store.scope(snapshot).map_err(|_| denied())?;
            let _guard = registry.authorize_snapshot(&self.principal, &scope)?;
            let mut index = store
                .load_evidence(snapshot, &AtomicBool::new(false))
                .map_err(|_| denied())?;
            registry.authorize_index(&self.principal, &index)?;
            let applications = index.captured_applications.clone();
            let application_access =
                self.application_access(&registry, &index, applications.as_ref())?;
            index.captured_applications = applications
                .as_ref()
                .map(|capture| capture.permitted(&application_access))
                .transpose()?;
            let index = Arc::new(index);
            *cache = Some(CachedIndex {
                snapshot,
                index: index.clone(),
                applications,
                application_access,
            });
            index
        };
        self.validate_index(&registry, &index)?;
        Ok(index)
    }

    fn application_access(
        &self,
        registry: &Registry,
        index: &Index,
        capture: Option<&crate::applications::Capture>,
    ) -> Result<Vec<bool>> {
        capture
            .map(|capture| {
                capture.access_mask(index, |path| {
                    registry
                        .authorize(&self.principal, Capability::Metadata, path)
                        .map(|_| ())
                })
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn validate_index(&self, registry: &Registry, index: &Index) -> Result<()> {
        registry.authorize_index(&self.principal, index)?;
        if self
            .application_access(registry, index, index.captured_applications.as_ref())?
            .iter()
            .any(|allowed| !allowed)
        {
            return Err(denied());
        }
        Ok(())
    }

    pub fn translate_query(&self, source: &str) -> Result<crate::search::Translation> {
        self.registry()?
            .policy(&self.principal, Capability::Metadata)?;
        Ok(crate::search::translate(source)?)
    }

    pub fn query(&self, snapshot: i64, query: &Query, cancel: &AtomicBool) -> Result<Option<Page>> {
        let policy = self
            .registry()?
            .policy(&self.principal, Capability::Metadata)?;
        let query = Query {
            limit: query.limit.min(policy.limits().page_size).max(1),
            ..query.clone()
        };
        query.compile()?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let index = self.index(snapshot)?;
        let result = index.query_cancellable(&query, || cancel.load(Ordering::Relaxed))?;
        self.validate_index(&self.registry()?, &index)?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        Ok(result)
    }

    pub fn export_csv(
        &self,
        snapshot: i64,
        mut request: crate::exports::CsvRequest,
        cancel: &AtomicBool,
    ) -> Result<Option<crate::exports::CsvPage>> {
        let registry = self.registry()?;
        registry.policy(&self.principal, Capability::Export)?;
        let policy = registry.policy(&self.principal, Capability::Metadata)?;
        if snapshot <= 0
            || request.query.offset > 10_000
            || !(1..=500).contains(&request.query.limit)
        {
            return Err(Error::Invalid(
                "CSV requires a positive snapshot, offset up to 10000 and limit from 1 to 500."
                    .into(),
            ));
        }
        request.query.limit = request.query.limit.min(policy.limits().page_size);
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let index = self.index(snapshot)?;
        let result = crate::exports::csv(&index, &request, cancel)?;
        let current = self.registry()?;
        current.policy(&self.principal, Capability::Export)?;
        self.validate_index(&current, &index)?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        Ok(result)
    }

    pub fn details(&self, snapshot: i64, node: crate::NodeId) -> Result<Details> {
        self.index(snapshot)?.details(node).map_err(|_| denied())
    }

    pub fn folder_metadata(
        &self,
        snapshot: i64,
        node: crate::NodeId,
        include_note: bool,
    ) -> Result<crate::metadata::FolderRead> {
        let index = self.index(snapshot)?;
        let registry = self.registry()?;
        let capability = if include_note {
            Capability::ReadContent
        } else {
            Capability::Metadata
        };
        let _folder = registry.authorize_index_entry(&self.principal, capability, &index, node)?;
        crate::metadata::Store::open(self.directory.directory())?.read_indexed(
            &index,
            node,
            include_note,
        )
    }

    pub fn folder_budget(
        &self,
        snapshot: i64,
        node: crate::NodeId,
    ) -> Result<crate::budgets::Report> {
        let index = self.index(snapshot)?;
        let registry = self.registry()?;
        let _folder =
            registry.authorize_index_entry(&self.principal, Capability::Metadata, &index, node)?;
        let history = Store::open(&self.directory.directory().join("snapshots.db"))?;
        let report = crate::budgets::Store::open(self.directory.directory())?
            .read(&index, node, &history, true, &AtomicBool::new(false))?
            .ok_or_else(denied)?;
        self.registry()?.authorize_index(&self.principal, &index)?;
        Ok(report)
    }

    pub fn history(&self, offset: usize, limit: usize) -> Result<HistoryPage> {
        let registry = self.registry()?;
        let policy = registry.policy(&self.principal, Capability::Metadata)?;
        if offset > 10_000 || !(1..=100).contains(&limit) {
            return Err(Error::Invalid(
                "History requires an offset up to 10000 and a page size from 1 to 100.".into(),
            ));
        }
        let limit = limit.clamp(1, policy.limits().page_size.min(100));
        let store = Store::open(&self.directory.directory().join("snapshots.db"))?;
        let _read_view = store.history_read()?;
        let mut scopes = std::collections::HashMap::new();
        let mut seen = 0;
        let mut items = Vec::new();
        let mut has_more = false;
        let mut limited = true;
        for batch in 0..100 {
            let snapshots = store.list(batch * 100, 100)?;
            let exhausted = snapshots.len() < 100;
            for snapshot in snapshots {
                let allowed = match scopes.get(&snapshot.root_key) {
                    Some(allowed) => *allowed,
                    None => {
                        let scope = store.scope(snapshot.id).map_err(|_| denied())?;
                        let allowed = registry.authorize_snapshot(&self.principal, &scope).is_ok();
                        scopes.insert(snapshot.root_key, allowed);
                        allowed
                    }
                };
                if !allowed {
                    continue;
                }
                if seen < offset {
                    seen += 1;
                    continue;
                }
                if items.len() == limit {
                    has_more = true;
                    break;
                }
                items.push(snapshot);
            }
            if exhausted || has_more {
                limited = false;
                break;
            }
        }
        registry.policy(&self.principal, Capability::Metadata)?;
        Ok(HistoryPage {
            next_offset: has_more.then_some(offset + items.len()),
            items,
            limited,
        })
    }

    pub fn compare_snapshots(
        &self,
        before: i64,
        after: i64,
        offset: usize,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Option<crate::snapshots::Comparison>> {
        let registry = self.registry()?;
        let policy = registry.policy(&self.principal, Capability::Metadata)?;
        if before <= 0
            || after <= 0
            || before == after
            || offset > 10_000
            || !(1..=200).contains(&limit)
        {
            return Err(Error::Invalid("Comparison requires two different positive snapshot IDs, offset up to 10000 and limit from 1 to 200.".into()));
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let store = Store::open(&self.directory.directory().join("snapshots.db"))?;
        let old = store.scope(before).map_err(|_| denied())?;
        let new = store.scope(after).map_err(|_| denied())?;
        let _old_pin = registry.authorize_snapshot(&self.principal, &old)?;
        let _new_pin = registry.authorize_snapshot(&self.principal, &new)?;
        let result = store.compare_cancellable(
            before,
            after,
            offset,
            limit.min(policy.limits().page_size),
            cancel,
        )?;
        let registry = self.registry()?;
        let _old_check = registry.authorize_snapshot(&self.principal, &old)?;
        let _new_check = registry.authorize_snapshot(&self.principal, &new)?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        Ok(result)
    }

    pub fn calendar(
        &self,
        request: crate::snapshots::CalendarRequest,
        cancel: &AtomicBool,
    ) -> Result<Option<crate::snapshots::CalendarRead>> {
        use crate::snapshots::{CalendarRead, CalendarRequest};
        let snapshot = match &request {
            CalendarRequest::Month { snapshot, .. } | CalendarRequest::Day { snapshot, .. } => {
                *snapshot
            }
        };
        self.read_activity(snapshot, cancel, |store, page_size| match request {
            CalendarRequest::Month { snapshot, month } => store
                .calendar(snapshot, &month, cancel)
                .map(|report| report.map(|report| CalendarRead::Month { report })),
            CalendarRequest::Day {
                snapshot,
                month,
                day,
                offset,
                limit,
            } => {
                if !(1..=100).contains(&limit) {
                    return Err(Error::Invalid(
                        "Calendar page limit must be from 1 to 100.".into(),
                    ));
                }
                store
                    .calendar_entries(snapshot, &month, day, offset, limit.min(page_size), cancel)
                    .map(|page| page.map(|page| CalendarRead::Day { page }))
            }
        })
    }

    pub fn diary(
        &self,
        snapshot: i64,
        offset: usize,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Option<crate::snapshots::DiaryPage>> {
        self.read_activity(snapshot, cancel, |store, page_size| {
            if !(1..=10).contains(&limit) {
                return Err(Error::Invalid(
                    "Diary page limit must be from 1 to 10.".into(),
                ));
            }
            store.diary(
                snapshot,
                offset,
                limit.min(page_size),
                5.min(page_size),
                cancel,
            )
        })
    }

    fn read_activity<T>(
        &self,
        snapshot: i64,
        cancel: &AtomicBool,
        read: impl FnOnce(&Store, usize) -> Result<Option<T>>,
    ) -> Result<Option<T>> {
        let registry = self.registry()?;
        let policy = registry.policy(&self.principal, Capability::Metadata)?;
        if snapshot <= 0 {
            return Err(Error::Invalid("A positive snapshot ID is required.".into()));
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let store = Store::open(&self.directory.directory().join("snapshots.db"))?;
        let scope = store.scope(snapshot).map_err(|_| denied())?;
        let _pin = registry.authorize_snapshot(&self.principal, &scope)?;
        let result = read(&store, policy.limits().page_size)?;
        let registry = self.registry()?;
        let _check = registry.authorize_snapshot(&self.principal, &scope)?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        Ok(result)
    }

    pub fn space_account(
        &self,
        snapshot: i64,
        cancel: &AtomicBool,
    ) -> Result<Option<crate::space::Report>> {
        self.registry()?
            .policy(&self.principal, Capability::Metadata)?;
        if snapshot <= 0 {
            return Err(Error::Invalid("A positive snapshot ID is required.".into()));
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let index = self.index(snapshot)?;
        let Some(indexed) = crate::space::summarize(&index, cancel)? else {
            return Ok(None);
        };
        let _check = self.registry()?.authorize_index(&self.principal, &index)?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        Ok(Some(crate::space::Report::compare(
            indexed,
            crate::space::VolumeContext::ScopedOnly,
        )))
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
