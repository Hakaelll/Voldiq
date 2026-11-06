//! Registration claims describe indexed locations, not verified ownership or cleanup authority.
use crate::{Error, Kind, NodeId, Result, index::Index};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) mod capture;
mod classify;
mod locations;
pub use capture::{Capture, capture, capture_authorized, observe, query_captured};
#[cfg(windows)]
mod registry;
pub mod review;
pub use locations::{BrowserLayout, Category, CategoryMeasurement, ProfileLocation};

pub const MAX_REGISTRATIONS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub install_location: Option<PathBuf>,
    pub profile: Option<ProfileLocation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Contextual,
    Supplied,
    Ambiguous,
}

#[derive(Clone, Serialize)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub root: NodeId,
    pub profile_scope: Option<NodeId>,
    pub sources: Vec<Source>,
    pub evidence: &'static str,
    pub confidence: Confidence,
    pub exclusive: Footprint,
    pub overlapping: Footprint,
    pub breakdown: Vec<CategoryMeasurement>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    Current,
    Indexed,
    Captured,
}

#[derive(Serialize)]
pub struct Page {
    pub attribution: Attribution,
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
    metadata: Metadata,
    root: NodeId,
    sources: Vec<Source>,
    exclusive: Count,
    overlapping: Count,
    breakdown: BTreeMap<Category, (Count, Count)>,
    locations: Vec<locations::Location>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    name: String,
    publisher: Option<String>,
    version: Option<String>,
    source: Source,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    started_at: u64,
    observed_at: u64,
    coverage: Vec<SourceCoverage>,
    unbound_records: usize,
    duplicate_records: usize,
}

struct Bindings {
    evidence: Evidence,
    bound: Vec<Bound>,
    _root: Option<crate::agents::AuthorizedPath>,
}

struct Resolved {
    registration: Registration,
    installation: Option<(u64, u64)>,
    profile: Option<(u64, u64)>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RegistrationLocation {
    Native((u64, u64)),
    Declared(Option<PathBuf>),
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
    query_captured(index, request, cancel, |_| Ok(()))
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
    query_authorized(index, catalog, query, cancel, crate::agents::pin_directory)
}

/// The embedding service supplies current metadata authorization for each location independently.
pub fn query_authorized(
    index: &Index,
    catalog: Catalog,
    query: &Query,
    cancel: &AtomicBool,
    authorize: impl FnMut(&std::path::Path) -> Result<crate::agents::AuthorizedPath>,
) -> Result<Page> {
    if index.snapshot_id.is_some() {
        return Err(Error::Invalid(
            "Historical application queries require the evidence recorded in that snapshot.".into(),
        ));
    }
    query_selected(index, catalog, query, cancel, authorize, None)
}

fn query_selected(
    index: &Index,
    catalog: Catalog,
    query: &Query,
    cancel: &AtomicBool,
    authorize: impl FnMut(&std::path::Path) -> Result<crate::agents::AuthorizedPath>,
    selected: Option<&str>,
) -> Result<Page> {
    compiled_request(index, query)?;
    aggregate(
        index,
        bind(index, catalog, cancel, authorize)?,
        query,
        cancel,
        selected,
        Attribution::Current,
    )
}

fn bind(
    index: &Index,
    catalog: Catalog,
    cancel: &AtomicBool,
    mut authorize: impl FnMut(&std::path::Path) -> Result<crate::agents::AuthorizedPath>,
) -> Result<Bindings> {
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
        let mut observe = |path: &std::path::Path| {
            if !local_install_path(path) {
                return None;
            }
            let pin = authorize(path).ok()?;
            let identity = pin.identity();
            (!generic_roots.contains(&identity)).then_some(identity)
        };
        let installation = registration
            .install_location
            .as_deref()
            .and_then(&mut observe);
        let profile = registration
            .profile
            .as_ref()
            .and_then(|profile| observe(&profile.path));
        resolved.push(Resolved {
            registration,
            installation,
            profile,
        });
    }
    let wanted: HashSet<_> = resolved
        .iter()
        .flat_map(|record| [record.installation, record.profile].into_iter().flatten())
        .collect();
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
    for Resolved {
        registration,
        installation,
        profile,
    } in resolved
    {
        check(cancel)?;
        let mut locations = Vec::new();
        if let Some(identity) = installation
            && let Some(location) = locations::Location::bind(
                root_pin.path(),
                &indexed_roots,
                &ancestors,
                identity,
                None,
            )
        {
            locations.push(location);
        }
        if let Some(identity) = profile
            && let Some(profile) = &registration.profile
            && let Some(location) = locations::Location::bind(
                root_pin.path(),
                &indexed_roots,
                &ancestors,
                identity,
                Some(profile.layout),
            )
        {
            locations.push(location);
        }
        let Some(root) = locations.first().map(|location| location.root) else {
            unbound_records += 1;
            continue;
        };
        let key = (
            registration.key.to_ascii_lowercase(),
            registration.name.clone(),
            registration.publisher.clone(),
            registration.version.clone(),
            installation
                .map(RegistrationLocation::Native)
                .unwrap_or_else(|| {
                    RegistrationLocation::Declared(registration.install_location.clone())
                }),
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
            app.locations.extend(locations);
            continue;
        }
        let id = blake3::hash(format!("{key:?}").as_bytes())
            .to_hex()
            .to_string();
        unique.insert(key, bound.len());
        let source = registration.source;
        bound.push(Bound {
            id,
            metadata: Metadata {
                name: registration.name,
                publisher: registration.publisher,
                version: registration.version,
                source,
            },
            root,
            sources: vec![source],
            exclusive: Count::default(),
            overlapping: Count::default(),
            breakdown: BTreeMap::new(),
            locations,
        });
    }
    Ok(Bindings {
        bound,
        _root: Some(root_pin),
        evidence: Evidence {
            started_at: catalog.started_at,
            observed_at: catalog.observed_at,
            coverage: catalog.coverage,
            unbound_records,
            duplicate_records,
        },
    })
}

fn aggregate(
    index: &Index,
    bindings: Bindings,
    query: &Query,
    cancel: &AtomicBool,
    selected: Option<&str>,
    attribution: Attribution,
) -> Result<Page> {
    let compiled = compiled_request(index, query)?;
    let Bindings {
        mut bound,
        evidence,
        _root,
    } = bindings;
    let locations: Vec<_> = bound.iter().map(|app| app.locations.clone()).collect();
    let (mut exclusive, mut overlapping, mut unattributed, mut total) = (
        Count::default(),
        Count::default(),
        Count::default(),
        Count::default(),
    );
    let mut work = crate::search::GLOB_WORK_LIMIT;
    let mut cancelled = || cancel.load(Ordering::Relaxed);
    let capture = compiled
        .search
        .application_filter()
        .map(|_| Capture::from_bound(&evidence, &bound));
    let prepared = index
        .prepare_query(&compiled, capture.as_ref(), &mut cancelled)?
        .ok_or_else(|| Error::Invalid("Application query cancelled.".into()))?;
    classify::visit(
        index,
        locations.iter().map(Vec::as_slice),
        Some(query.scope),
        &mut || cancel.load(Ordering::Relaxed),
        |entry, owners| {
            for (&slot, &(_, _, category)) in owners {
                bound[slot].breakdown.entry(category).or_default();
            }
            if !entry.is_metadata_file() {
                return Ok(());
            }
            match prepared.matches(entry.id, &mut work, &mut cancelled)? {
                Some(true) => {}
                Some(false) => return Ok(()),
                None => return Err(Error::Invalid("Application query cancelled.".into())),
            }
            total.add(entry.logical);
            match owners.len() {
                0 => unattributed.add(entry.logical),
                1 => exclusive.add(entry.logical),
                _ => overlapping.add(entry.logical),
            }
            for (&slot, &(_, _, category)) in owners {
                let app = &mut bound[slot];
                let counts = app.breakdown.entry(category).or_default();
                if owners.len() == 1 {
                    counts.0.add(entry.logical);
                    app.exclusive.add(entry.logical);
                } else {
                    counts.1.add(entry.logical);
                    app.overlapping.add(entry.logical);
                }
            }
            Ok(())
        },
    )?;
    check(cancel)?;
    let mut applications: Vec<_> = bound
        .into_iter()
        .filter(|app| {
            app.exclusive.files + app.overlapping.files > 0
                && selected.is_none_or(|id| id == app.id)
        })
        .map(|app| {
            let mut roots: Vec<_> = app
                .locations
                .iter()
                .filter(|location| location.profile.is_some())
                .map(|location| location.root.0)
                .collect();
            roots.sort_unstable();
            roots.dedup();
            let profile_scope = if roots.len() == 1 {
                Some(NodeId(roots[0]))
            } else {
                None
            };
            Application {
                id: app.id,
                name: app.metadata.name,
                publisher: app.metadata.publisher,
                version: app.metadata.version,
                root: app.root,
                profile_scope,
                sources: app.sources,
                evidence: if app.metadata.source == Source::SuppliedMetadata {
                    "supplied_claim"
                } else if app
                    .breakdown
                    .keys()
                    .any(|category| *category != Category::Installation)
                {
                    "registered_default_profile_claim"
                } else {
                    "registered_location_claim"
                },
                confidence: if app.breakdown.contains_key(&Category::Unknown) {
                    Confidence::Ambiguous
                } else if app.metadata.source == Source::SuppliedMetadata {
                    Confidence::Supplied
                } else {
                    Confidence::Contextual
                },
                exclusive: app.exclusive.view(),
                overlapping: app.overlapping.view(),
                breakdown: app
                    .breakdown
                    .into_iter()
                    .map(|(category, (exclusive, overlapping))| CategoryMeasurement {
                        category,
                        exclusive: exclusive.view(),
                        overlapping: overlapping.view(),
                    })
                    .collect(),
            }
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
        attribution,
        scope: query.scope,
        indexed_at: index
            .observation
            .as_ref()
            .map(|observation| observation.completed_at),
        registrations_observed_at: evidence.observed_at,
        registrations_started_at: evidence.started_at,
        excluded_roots: index.exclusions.len(),
        partial_index: index.entry(query.scope)?.incomplete
            || index
                .observation
                .as_ref()
                .is_none_or(|observation| !observation.complete),
        coverage: evidence.coverage,
        unbound_records: evidence.unbound_records,
        duplicate_records: evidence.duplicate_records,
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
        && record
            .install_location
            .as_ref()
            .is_none_or(|path| path.is_absolute())
        && (record.install_location.is_some() || record.profile.is_some())
        && record.profile.as_ref().is_none_or(|profile| {
            profile.path.is_absolute() && BrowserLayout::registered(record) == Some(profile.layout)
        })
}
fn local_install_path(path: &std::path::Path) -> bool {
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
