//! Registration claims describe indexed locations, not verified ownership or cleanup authority.
use crate::{Error, Kind, NodeId, Result, index::Index};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(windows)]
mod registry;

pub const MAX_REGISTRATIONS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    CurrentUser64,
    CurrentUser32,
    Machine64,
    Machine32,
    SuppliedMetadata,
}

#[derive(Clone, Debug)]
pub struct Registration {
    pub source: Source,
    pub key: String,
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_location: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceCoverage {
    pub source: Source,
    pub complete: bool,
    pub problem: Option<String>,
    pub visited: usize,
    pub missing_location: usize,
    pub rejected: usize,
}

#[derive(Clone)]
pub struct Catalog {
    pub started_at: u64,
    pub observed_at: u64,
    pub registrations: Vec<Registration>,
    pub coverage: Vec<SourceCoverage>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Size,
    Name,
    Files,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Query {
    pub scope: NodeId,
    pub offset: usize,
    pub limit: usize,
    pub sort: Sort,
    pub filters: crate::index::Query,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            scope: NodeId(0),
            offset: 0,
            limit: 50,
            sort: Sort::Size,
            filters: Default::default(),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct Footprint {
    pub logical_bytes: String,
    pub files: u64,
}

#[derive(Clone, Serialize)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub root: NodeId,
    pub sources: Vec<Source>,
    pub evidence: &'static str,
    pub exclusive: Footprint,
    pub overlapping: Footprint,
}

#[derive(Serialize)]
pub struct Page {
    pub scope: NodeId,
    pub indexed_at: Option<u64>,
    pub registrations_observed_at: u64,
    pub registrations_started_at: u64,
    pub excluded_roots: usize,
    pub partial_index: bool,
    pub coverage: Vec<SourceCoverage>,
    pub unbound_records: usize,
    pub duplicate_records: usize,
    pub exclusive: Footprint,
    pub overlapping: Footprint,
    pub unattributed: Footprint,
    pub total: Footprint,
    pub applications: Vec<Application>,
    pub application_count: usize,
    pub chart: Vec<Application>,
    pub chart_omitted: usize,
}

#[derive(Clone, Default)]
struct Count {
    bytes: u128,
    files: u64,
}
impl Count {
    fn add(&mut self, bytes: u64) {
        self.bytes += u128::from(bytes);
        self.files += 1;
    }
    fn view(&self) -> Footprint {
        Footprint {
            logical_bytes: self.bytes.to_string(),
            files: self.files,
        }
    }
}
struct Bound {
    id: String,
    registration: Registration,
    root: NodeId,
    sources: Vec<Source>,
    exclusive: Count,
    overlapping: Count,
}

pub fn installed_programs(cancel: &AtomicBool) -> Result<Catalog> {
    #[cfg(windows)]
    {
        registry::read(cancel)
    }
    #[cfg(not(windows))]
    {
        let _ = cancel;
        Err(Error::Invalid(
            "Windows registration metadata is unavailable on this platform.".into(),
        ))
    }
}

pub fn read_desktop(index: &Index, request: &Query, cancel: &AtomicBool) -> Result<Page> {
    compiled_request(index, request)?;
    query(index, installed_programs(cancel)?, request, cancel)
}

pub(crate) fn compiled_request<'a>(
    index: &Index,
    request: &'a Query,
) -> Result<crate::search::CompiledQuery<'a>> {
    if request.limit == 0
        || request.limit > 100
        || request.offset > MAX_REGISTRATIONS
        || request.filters.parent.is_some()
        || request.filters.scope.is_some()
        || request.filters.offset != 0
        || request.filters.limit != 100
        || request.filters.sort != "size"
    {
        return Err(Error::Invalid(
            "Invalid application scope, filters or page bounds.".into(),
        ));
    }
    if index.entry(request.scope)?.kind != Kind::Directory {
        return Err(Error::Invalid(
            "Application scope must be an indexed directory.".into(),
        ));
    }
    Ok(request.filters.compile()?)
}

/// Only supplied claims and observed directory identities are used; no file contents are read.
pub fn query(index: &Index, catalog: Catalog, query: &Query, cancel: &AtomicBool) -> Result<Page> {
    let compiled = compiled_request(index, query)?;
    if catalog.registrations.len() > MAX_REGISTRATIONS || catalog.coverage.len() > 5 {
        return Err(Error::Invalid(
            "Application query exceeds its work bounds.".into(),
        ));
    }
    check(cancel)?;
    let root_pin = index.pin_folder(NodeId(0))?;
    let ancestors = root_pin.ancestor_identities()?;
    #[cfg(windows)]
    let generic_roots = registry::generic_roots()?;
    #[cfg(not(windows))]
    let generic_roots = Vec::<(u64, u64)>::new();
    let mut unbound_records = 0;
    let mut resolved = Vec::new();
    for registration in catalog.registrations {
        check(cancel)?;
        if !valid_registration(&registration) {
            return Err(Error::Invalid(
                "Invalid application registration metadata.".into(),
            ));
        }
        if !local_install_path(&registration.install_location) {
            unbound_records += 1;
            continue;
        }
        let pin = match crate::agents::pin_directory(&registration.install_location) {
            Ok(pin) => pin,
            Err(_) => {
                unbound_records += 1;
                continue;
            }
        };
        let identity = pin.identity();
        if generic_roots.contains(&identity) {
            unbound_records += 1;
            continue;
        }
        resolved.push((registration, identity));
    }
    let wanted: HashSet<_> = resolved.iter().map(|(_, identity)| *identity).collect();
    let mut indexed_roots = HashMap::new();
    for entry in &index.entries {
        if entry.id.0 % 256 == 0 {
            check(cancel)?;
        }
        if entry.kind == Kind::Directory
            && let Some(state) = entry.file_state
            && wanted.contains(&state.identity)
        {
            indexed_roots.entry(state.identity).or_insert(entry.id);
        }
    }
    let mut bound = Vec::<Bound>::new();
    let mut unique = BTreeMap::new();
    let mut duplicate_records = 0;
    for (registration, identity) in resolved {
        check(cancel)?;
        let root = match indexed_roots.get(&identity).copied() {
            Some(root) => root,
            None if ancestors.contains(&identity) => NodeId(0),
            None => {
                unbound_records += 1;
                continue;
            }
        };
        let key = (
            registration.key.to_ascii_lowercase(),
            registration.name.clone(),
            registration.publisher.clone(),
            registration.version.clone(),
            identity,
            match registration.source {
                Source::SuppliedMetadata => 2,
                _ => 0,
            },
        );
        if let Some(&slot) = unique.get(&key) {
            let app: &mut Bound = &mut bound[slot];
            if !app.sources.contains(&registration.source) {
                app.sources.push(registration.source);
            }
            duplicate_records += 1;
            continue;
        }
        let id = blake3::hash(format!("{key:?}").as_bytes())
            .to_hex()
            .to_string();
        unique.insert(key, bound.len());
        let source = registration.source;
        bound.push(Bound {
            id,
            registration,
            root,
            sources: vec![source],
            exclusive: Count::default(),
            overlapping: Count::default(),
        });
    }
    let mut claims: HashMap<NodeId, Vec<usize>> = HashMap::new();
    for (slot, app) in bound.iter().enumerate() {
        claims.entry(app.root).or_default().push(slot);
    }
    let (mut exclusive, mut overlapping, mut unattributed, mut total) = (
        Count::default(),
        Count::default(),
        Count::default(),
        Count::default(),
    );
    let mut owners = Vec::new();
    let mut work = crate::search::GLOB_WORK_LIMIT;
    let mut cancelled = || cancel.load(Ordering::Relaxed);
    for entry in &index.entries {
        if entry.id.0 % 256 == 0 {
            check(cancel)?;
        }
        if !entry.is_metadata_file() {
            continue;
        }
        owners.clear();
        let mut within_scope = false;
        let mut parent = Some(entry.id);
        let mut depth = 0;
        while let Some(id) = parent {
            if depth % 256 == 0 {
                check(cancel)?;
            }
            if depth > index.entries.len() {
                return Err(Error::Invalid("Invalid index ancestry.".into()));
            }
            within_scope |= id == query.scope;
            if let Some(apps) = claims.get(&id) {
                owners.extend_from_slice(apps);
            }
            parent = index.entry(id)?.parent;
            depth += 1;
        }
        if !within_scope {
            continue;
        }
        match index.matches_compiled_cancellable(&compiled, entry, &mut work, &mut cancelled)? {
            Some(true) => {}
            Some(false) => continue,
            None => return Err(Error::Invalid("Application query cancelled.".into())),
        }
        owners.sort_unstable();
        owners.dedup();
        total.add(entry.logical);
        match owners.as_slice() {
            [] => unattributed.add(entry.logical),
            [slot] => {
                exclusive.add(entry.logical);
                bound[*slot].exclusive.add(entry.logical);
            }
            _ => {
                overlapping.add(entry.logical);
                for &slot in &owners {
                    bound[slot].overlapping.add(entry.logical);
                }
            }
        }
    }
    check(cancel)?;
    let mut applications: Vec<_> = bound
        .into_iter()
        .filter(|app| app.exclusive.files + app.overlapping.files > 0)
        .map(|app| Application {
            id: app.id,
            name: app.registration.name,
            publisher: app.registration.publisher,
            version: app.registration.version,
            root: app.root,
            sources: app.sources,
            evidence: if app.registration.source == Source::SuppliedMetadata {
                "supplied_claim"
            } else {
                "registered_location_claim"
            },
            exclusive: app.exclusive.view(),
            overlapping: app.overlapping.view(),
        })
        .collect();
    applications.sort_by(|a, b| {
        match query.sort {
            Sort::Name => a.name.cmp(&b.name),
            Sort::Files => b.exclusive.files.cmp(&a.exclusive.files),
            Sort::Size => b
                .exclusive
                .logical_bytes
                .len()
                .cmp(&a.exclusive.logical_bytes.len())
                .then_with(|| b.exclusive.logical_bytes.cmp(&a.exclusive.logical_bytes)),
        }
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.id.cmp(&b.id))
    });
    let application_count = applications.len();
    let mut chart = applications.clone();
    chart.sort_by(|a, b| {
        b.exclusive
            .logical_bytes
            .len()
            .cmp(&a.exclusive.logical_bytes.len())
            .then_with(|| b.exclusive.logical_bytes.cmp(&a.exclusive.logical_bytes))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
    chart.truncate(200);
    let applications = applications
        .into_iter()
        .skip(query.offset)
        .take(query.limit)
        .collect();
    Ok(Page {
        scope: query.scope,
        indexed_at: index
            .observation
            .as_ref()
            .map(|observation| observation.completed_at),
        registrations_observed_at: catalog.observed_at,
        registrations_started_at: catalog.started_at,
        excluded_roots: index.exclusions.len(),
        partial_index: index.entry(query.scope)?.incomplete
            || index
                .observation
                .as_ref()
                .is_none_or(|observation| !observation.complete),
        coverage: catalog.coverage,
        unbound_records,
        duplicate_records,
        exclusive: exclusive.view(),
        overlapping: overlapping.view(),
        unattributed: unattributed.view(),
        total: total.view(),
        applications,
        application_count,
        chart,
        chart_omitted: application_count.saturating_sub(200),
    })
}

fn valid_registration(record: &Registration) -> bool {
    let text = |value: &str| {
        !value.trim().is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
    };
    text(&record.key)
        && text(&record.name)
        && record.publisher.as_deref().is_none_or(text)
        && record.version.as_deref().is_none_or(text)
        && record.install_location.is_absolute()
}
pub(crate) fn local_install_path(path: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        use windows_sys::Win32::{
            Storage::FileSystem::GetDriveTypeW,
            System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE},
        };
        let mut parts = path.components();
        let Some(Component::Prefix(prefix)) = parts.next() else {
            return false;
        };
        let drive = match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
            _ => return false,
        };
        if path
            .components()
            .filter(|part| matches!(part, Component::Normal(_)))
            .count()
            < 1
        {
            return false;
        }
        let root = [u16::from(drive), 58, 92, 0];
        matches!(
            unsafe { GetDriveTypeW(root.as_ptr()) },
            DRIVE_FIXED | DRIVE_REMOVABLE
        )
    }
    #[cfg(not(windows))]
    {
        path.is_absolute()
    }
}
fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Invalid("Application query cancelled.".into()))
    } else {
        Ok(())
    }
}
