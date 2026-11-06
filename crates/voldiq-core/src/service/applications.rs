use super::*;
use crate::applications::{self, Application, Footprint, Source};

#[derive(Serialize)]
pub struct ApplicationPage {
    pub snapshot_id: i64,
    pub scope: crate::NodeId,
    pub indexed_at: Option<u64>,
    pub registrations_started_at: u64,
    pub registrations_observed_at: u64,
    pub registration_sources: Vec<RegistrationSource>,
    pub attribution_scope: &'static str,
    pub partial_index: bool,
    pub excluded_roots: usize,
    pub exclusive: Footprint,
    pub overlapping: Footprint,
    pub unattributed: Footprint,
    pub total: Footprint,
    pub applications: Vec<Application>,
    pub application_count: usize,
    pub next_offset: Option<usize>,
}

#[derive(Serialize)]
pub struct RegistrationSource {
    pub source: Source,
    pub complete: bool,
}

impl AgentService {
    pub fn applications(
        &self,
        snapshot: i64,
        mut request: applications::Query,
        cancel: &AtomicBool,
    ) -> Result<Option<ApplicationPage>> {
        let registry = self.registry()?;
        let policy = registry.policy(&self.principal, Capability::Metadata)?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let index = self.index(snapshot)?;
        applications::compiled_request(&index, &request)?;
        request.limit = request.limit.min(policy.limits().page_size);
        let mut catalog = match applications::installed_programs(cancel) {
            Ok(catalog) => catalog,
            Err(_) if cancel.load(Ordering::Relaxed) => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut authorized = Vec::new();
        for record in catalog.registrations {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            // Registry paths are context, never authority to inspect another folder.
            if applications::local_install_path(&record.install_location)
                && registry
                    .authorize(
                        &self.principal,
                        Capability::Metadata,
                        &record.install_location,
                    )
                    .is_ok()
            {
                authorized.push(record);
            }
        }
        catalog.registrations = authorized;
        let mut result = match applications::query(&index, catalog, &request, cancel) {
            Ok(page) => page,
            Err(_) if cancel.load(Ordering::Relaxed) => return Ok(None),
            Err(error) => return Err(error),
        };
        let current = self.registry()?;
        let policy = current.policy(&self.principal, Capability::Metadata)?;
        let _root = current.authorize_index(&self.principal, &index)?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        result.applications.truncate(policy.limits().page_size);
        let next = request.offset + result.applications.len();
        Ok(Some(ApplicationPage {
            snapshot_id: snapshot,
            scope: result.scope,
            indexed_at: result.indexed_at,
            registrations_started_at: result.registrations_started_at,
            registrations_observed_at: result.registrations_observed_at,
            registration_sources: result
                .coverage
                .into_iter()
                .map(|source| RegistrationSource {
                    source: source.source,
                    complete: source.complete,
                })
                .collect(),
            attribution_scope: "current registrations with authorized local installation locations; indexed file paths only; not a historical or complete inventory; data/cache/log categories unavailable",
            partial_index: result.partial_index,
            excluded_roots: result.excluded_roots,
            exclusive: result.exclusive,
            overlapping: result.overlapping,
            unattributed: result.unattributed,
            total: result.total,
            applications: result.applications,
            application_count: result.application_count,
            next_offset: (next < result.application_count).then_some(next),
        }))
    }
}
