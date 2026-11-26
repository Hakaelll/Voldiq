use super::AgentService;
use crate::{
    Result,
    agents::{AuthorizedPath, Capability, Registry, denied},
    duplicates::scope::{ResultRequest, ScopeRequest},
    jobs::{DuplicateRun, Jobs},
    snapshots::Store,
};
use serde_json::{Value, json};

impl AgentService {
    pub fn start_duplicate_analysis(&self, snapshot: i64, request: ScopeRequest) -> Result<i64> {
        #[cfg(windows)]
        {
            use crate::jobs::JobState;
            if snapshot <= 0 || !request.valid() {
                return Err(denied());
            }
            let registry = self.registry()?;
            let context = registry.plan_context(&self.principal, Capability::ReadContent)?;
            let mut notes = crate::metadata::Store::open(self.directory.directory())?;
            let run = DuplicateRun {
                snapshot,
                request,
                binding: context.binding()?,
                protections: notes.protections()?.fingerprint().to_owned(),
            };
            let root = self.authorize_duplicate_run(&registry, &run)?;
            let lease = self.directory.acquire()?;
            let mut jobs = Jobs::open(self.directory.directory())?;
            let owner = self.principal.id();
            let id = jobs.begin_duplicates(&lease, owner, root.path(), &run)?;
            let service = self.clone();
            let worker = std::thread::Builder::new()
                .name(format!("voldiq-duplicates-{id}"))
                .spawn(move || {
                    let (_root, _lease) = (root, lease);
                    if let Err(error) = service.run_duplicate_analysis(&mut jobs, id, &run) {
                        let reason = if matches!(error, crate::Error::DuplicateScopeLimit) {
                            "limit_reached"
                        } else {
                            "analysis_unavailable"
                        };
                        let _ = jobs.duplicate_progress(
                            owner,
                            id,
                            &json!({"phase":"failed","reason":reason}),
                        );
                        let state = if jobs.cancellation_requested(owner, id).unwrap_or(false) {
                            JobState::Cancelled
                        } else {
                            JobState::Failed
                        };
                        let _ = jobs.finish(owner, id, state, None);
                    }
                });
            if worker.is_err() {
                Jobs::open(self.directory.directory())?.finish(
                    owner,
                    id,
                    JobState::Failed,
                    None,
                )?;
                return Err(denied());
            }
            Ok(id)
        }
        #[cfg(not(windows))]
        {
            let _ = (snapshot, request);
            Err(denied())
        }
    }

    pub fn duplicate_analysis_results(&self, id: i64, request: &ResultRequest) -> Result<Value> {
        if !request.valid() {
            return Err(denied());
        }
        let registry = self.registry()?;
        let jobs = Jobs::open(self.directory.directory())?;
        let owner = self.principal.id();
        let run = jobs.duplicate_run(owner, id)?;
        let _root = self.authorize_duplicate_run(&registry, &run)?;
        let mut notes = crate::metadata::Store::open(self.directory.directory())?;
        if notes.protections()?.fingerprint() != run.protections {
            return Err(denied());
        }
        let policy = registry.policy(&self.principal, Capability::ReadContent)?;
        let mut page = jobs
            .duplicate_report(owner, id)?
            .page(request, policy.limits().page_size)?;
        self.authorize_duplicate_run(&registry, &run)?;
        if notes.protections()?.fingerprint() != run.protections {
            return Err(denied());
        }
        page["job_id"] = json!(id);
        page["snapshot_id"] = json!(run.snapshot);
        page["scope"] = json!(run.request.folder());
        Ok(page)
    }

    pub(super) fn authorize_duplicate_run(
        &self,
        registry: &Registry,
        run: &DuplicateRun,
    ) -> Result<AuthorizedPath> {
        let read = registry.plan_context(&self.principal, Capability::ReadContent)?;
        if read.binding()? != run.binding {
            return Err(denied());
        }
        let store = Store::open(&self.directory.directory().join("snapshots.db"))?;
        registry.authorize_snapshot(&self.principal, &store.scope(run.snapshot)?)
    }

    #[cfg(windows)]
    fn run_duplicate_analysis(&self, jobs: &mut Jobs, id: i64, run: &DuplicateRun) -> Result<()> {
        use crate::{duplicates::scope, jobs::JobState};
        use std::{
            sync::atomic::{AtomicBool, Ordering},
            time::{Duration, Instant},
        };

        let owner = self.principal.id();
        let registry = self.registry()?;
        let read = registry.plan_context(&self.principal, Capability::ReadContent)?;
        let _root = self.authorize_duplicate_run(&registry, run)?;
        if jobs.duplicate_progress(owner, id, &json!({"phase":"index"}))? {
            return jobs.finish(owner, id, JobState::Cancelled, None);
        }
        let index = self.index(run.snapshot)?;
        let mut notes = crate::metadata::Store::open(self.directory.directory())?;
        let protections = notes.protections()?;
        let check = || {
            if read.binding()? != run.binding || notes.protection_fingerprint()? != run.protections
            {
                Err(denied())
            } else {
                Ok(())
            }
        };
        check()?;
        let cancel = AtomicBool::new(false);
        let mut last = None;
        let report = scope::analyze(
            &index,
            &run.request,
            &protections,
            &cancel,
            |node| read.path(&index.path(node)?),
            |progress: &scope::AnalysisProgress| {
                if progress.phase != "finished"
                    && last.is_some_and(|time: Instant| time.elapsed() < Duration::from_millis(250))
                {
                    return Ok(());
                }
                check()?;
                if jobs.duplicate_progress(owner, id, progress)? {
                    cancel.store(true, Ordering::Relaxed);
                }
                last = Some(Instant::now());
                Ok(())
            },
        )?;
        let Some(report) = report else {
            return jobs.finish(owner, id, JobState::Cancelled, None);
        };
        self.validate_index(&registry, &index)?;
        check()?;
        jobs.finish_duplicates(owner, id, &report)
    }
}
