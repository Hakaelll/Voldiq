use super::*;

pub(crate) const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;

pub(crate) struct ApplicationMatches(Vec<u64>);
impl ApplicationMatches {
    pub(crate) fn contains(&self, id: NodeId) -> bool {
        self.0
            .get(id.0 as usize / 64)
            .is_some_and(|word| word & (1 << (id.0 % 64)) != 0)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    schema: u8,
    state: State,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum State {
    Recorded {
        evidence: Evidence,
        records: Vec<Record>,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: String,
    metadata: Metadata,
    root: NodeId,
    sources: Vec<Source>,
    locations: Vec<locations::Location>,
}

pub fn capture(index: &Index, catalog: Catalog, cancel: &AtomicBool) -> Result<Capture> {
    capture_authorized(index, catalog, cancel, crate::agents::pin_directory)
}

pub fn observe(index: &Index, cancel: &AtomicBool) -> Result<Capture> {
    if index.snapshot_id.is_some() {
        return Err(Error::Invalid(
            "Historical application evidence cannot be recaptured.".into(),
        ));
    }
    let result =
        super::installed_programs(cancel).and_then(|catalog| capture(index, catalog, cancel));
    check(cancel)?;
    Ok(result.unwrap_or_else(|_| Capture::unavailable()))
}

pub fn capture_authorized(
    index: &Index,
    catalog: Catalog,
    cancel: &AtomicBool,
    authorize: impl FnMut(&std::path::Path) -> Result<crate::agents::AuthorizedPath>,
) -> Result<Capture> {
    if index.snapshot_id.is_some() {
        return Err(Error::Invalid(
            "Historical application evidence cannot be recaptured.".into(),
        ));
    }
    let Bindings {
        bound,
        evidence,
        _root,
    } = bind(index, catalog, cancel, authorize)?;
    let capture = Capture {
        schema: 1,
        state: State::Recorded {
            evidence,
            records: bound
                .into_iter()
                .map(|app| Record {
                    id: app.id,
                    metadata: app.metadata,
                    root: app.root,
                    sources: app.sources,
                    locations: app.locations,
                })
                .collect(),
        },
    };
    capture.validate(index)?;
    check(cancel)?;
    Ok(capture)
}

impl Capture {
    pub(super) fn from_bound(evidence: &Evidence, bound: &[Bound]) -> Self {
        Self {
            schema: 1,
            state: State::Recorded {
                evidence: evidence.clone(),
                records: bound
                    .iter()
                    .map(|app| Record {
                        id: app.id.clone(),
                        metadata: app.metadata.clone(),
                        root: app.root,
                        sources: app.sources.clone(),
                        locations: app.locations.clone(),
                    })
                    .collect(),
            },
        }
    }

    pub(crate) fn access_mask(
        &self,
        index: &Index,
        mut authorize: impl FnMut(&std::path::Path) -> Result<()>,
    ) -> Result<Vec<bool>> {
        self.validate(index)?;
        let State::Recorded { records, .. } = &self.state else {
            return Ok(Vec::new());
        };
        let mut ancestors = HashMap::new();
        let mut mask = Vec::new();
        for record in records {
            for location in &record.locations {
                let allowed = if location.parent_hops == 0 {
                    true
                } else if let Some(&allowed) = ancestors.get(&location.parent_hops) {
                    allowed
                } else {
                    let mut path = index.path(location.root)?;
                    for _ in 0..location.parent_hops {
                        if !path.pop() {
                            return Err(Error::Invalid(
                                "Invalid captured application ancestry.".into(),
                            ));
                        }
                    }
                    let allowed = authorize(&crate::agents::ordinary_path(&path)?).is_ok();
                    ancestors.insert(location.parent_hops, allowed);
                    allowed
                };
                mask.push(allowed);
            }
        }
        Ok(mask)
    }

    pub(crate) fn permitted(&self, mask: &[bool]) -> Result<Self> {
        let mut filtered = self.clone();
        let State::Recorded { records, evidence } = &mut filtered.state else {
            return if mask.is_empty() {
                Ok(filtered)
            } else {
                Err(Error::Invalid(
                    "Invalid application permission projection.".into(),
                ))
            };
        };
        if records
            .iter()
            .map(|record| record.locations.len())
            .sum::<usize>()
            != mask.len()
        {
            return Err(Error::Invalid(
                "Invalid application permission projection.".into(),
            ));
        }
        let mut at = 0;
        for record in records.iter_mut() {
            record.locations.retain(|_| {
                let allowed = mask[at];
                at += 1;
                allowed
            });
            if let Some(location) = record.locations.first() {
                record.root = location.root;
            }
        }
        records.retain(|record| {
            if record.locations.is_empty() {
                evidence.unbound_records += 1;
                false
            } else {
                true
            }
        });
        Ok(filtered)
    }

    pub(crate) fn matching_files(
        &self,
        index: &Index,
        filter: &crate::search::ApplicationFilter,
        scope: Option<NodeId>,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<ApplicationMatches> {
        self.validate(index)?;
        let (_, records) = self.recorded()?;
        let mut selected = Vec::with_capacity(records.len());
        for record in records {
            if cancelled() {
                return Err(Error::Invalid("Application query cancelled.".into()));
            }
            let name = record.metadata.name.to_lowercase();
            selected.push(
                filter.names.iter().all(|part| name.contains(part))
                    && filter.ids.iter().all(|id| *id == record.id),
            );
        }
        let count = index.entries.len() / 64 + 1;
        let mut words = Vec::new();
        words
            .try_reserve_exact(count)
            .map_err(|_| Error::Invalid("Insufficient memory for application filtering.".into()))?;
        words.resize(count, 0);
        if selected.iter().any(|selected| *selected) {
            classify::visit(
                index,
                records.iter().map(|record| record.locations.as_slice()),
                scope,
                cancelled,
                |entry, owners| {
                    if entry.is_metadata_file()
                        && owners.iter().any(|(&slot, &(_, _, category))| {
                            selected[slot]
                                && filter.categories.iter().all(|wanted| *wanted == category)
                        })
                    {
                        words[entry.id.0 as usize / 64] |= 1 << (entry.id.0 % 64);
                    }
                    Ok(())
                },
            )?;
        }
        Ok(ApplicationMatches(words))
    }

    fn recorded(&self) -> Result<(&Evidence, &[Record])> {
        match &self.state {
            State::Recorded { evidence, records } => Ok((evidence, records)),
            State::Unavailable { reason } => Err(Error::Invalid(reason.clone())),
        }
    }

    pub fn unavailable() -> Self {
        Self {
            schema: 1,
            state: State::Unavailable {
                reason: "Application evidence was unavailable when this analysis was recorded."
                    .into(),
            },
        }
    }

    pub(crate) fn validate(&self, index: &Index) -> Result<()> {
        let invalid = || Error::Invalid("Invalid captured application evidence.".into());
        let label = |value: &str| {
            !value.trim().is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
        };
        if self.schema != 1 {
            return Err(invalid());
        }
        let State::Recorded { evidence, records } = &self.state else {
            if let State::Unavailable { reason } = &self.state
                && !label(reason)
            {
                return Err(invalid());
            }
            return Ok(());
        };
        if evidence.observed_at < evidence.started_at
            || records.len() > MAX_REGISTRATIONS
            || evidence.coverage.len() > 5
            || evidence.unbound_records > MAX_REGISTRATIONS
            || evidence.duplicate_records > MAX_REGISTRATIONS
            || records.len() + evidence.unbound_records + evidence.duplicate_records
                > MAX_REGISTRATIONS
        {
            return Err(invalid());
        }
        let mut ids = HashSet::new();
        let mut locations = 0;
        for record in records {
            locations += record.locations.len();
            if record.id.len() != 64
                || !record
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || !ids.insert(&record.id)
                || !label(&record.metadata.name)
                || !record.metadata.publisher.as_deref().is_none_or(label)
                || !record.metadata.version.as_deref().is_none_or(label)
                || record.sources.is_empty()
                || record.sources.len() > 5
                || record.sources.first() != Some(&record.metadata.source)
                || record
                    .sources
                    .iter()
                    .enumerate()
                    .any(|(at, source)| record.sources[..at].contains(source))
                || record.sources.iter().any(|source| {
                    (*source == Source::SuppliedMetadata)
                        != (record.metadata.source == Source::SuppliedMetadata)
                })
                || record
                    .locations
                    .first()
                    .is_none_or(|location| location.root != record.root)
                || locations > 2 * MAX_REGISTRATIONS
            {
                return Err(invalid());
            }
            for location in &record.locations {
                if index.entry(location.root)?.kind != Kind::Directory
                    || location.priority > 32768
                    || location.prefix_depth > 32768
                    || location.parent_hops > 32768
                    || (location.root != NodeId(0) && location.parent_hops != 0)
                    || location.prefix.len() != location.prefix_depth.min(3)
                    || location
                        .prefix
                        .iter()
                        .any(|name| !label(name) || name.contains(['/', '\\']))
                    || (location.profile.is_none() && location.prefix_depth != 0)
                    || (location.profile.is_some() && location.prefix_depth != location.parent_hops)
                {
                    return Err(invalid());
                }
            }
        }
        for source in &evidence.coverage {
            if source.visited > 4096
                || source.missing_location > source.visited
                || source.rejected > source.visited
                || source
                    .problem
                    .as_deref()
                    .is_some_and(|problem| !label(problem))
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    pub(crate) fn roots(&self) -> HashSet<NodeId> {
        match &self.state {
            State::Recorded { records, .. } => records
                .iter()
                .flat_map(|record| record.locations.iter().map(|location| location.root))
                .collect(),
            State::Unavailable { .. } => HashSet::new(),
        }
    }

    pub(crate) fn remap_roots(
        &mut self,
        mut map: impl FnMut(NodeId) -> Result<NodeId>,
    ) -> Result<()> {
        let State::Recorded { records, .. } = &mut self.state else {
            return Ok(());
        };
        for record in records {
            record.root = map(record.root)?;
            for location in &mut record.locations {
                location.root = map(location.root)?;
            }
        }
        Ok(())
    }
}

/// The embedder authorizes the whole index first; ancestor claims require separate permission.
pub fn query_captured(
    index: &Index,
    request: &Query,
    cancel: &AtomicBool,
    authorize: impl FnMut(&std::path::Path) -> Result<()>,
) -> Result<Page> {
    compiled_request(index, request)?;
    check(cancel)?;
    let capture = index.captured_applications.as_ref().ok_or_else(|| Error::Invalid(
        "Application evidence was not captured for this snapshot. It cannot be reconstructed from current registrations.".into()))?;
    let capture = capture.permitted(&capture.access_mask(index, authorize)?)?;
    let (evidence, records) = capture.recorded()?;
    let mut bound = Vec::new();
    for record in records {
        check(cancel)?;
        bound.push(Bound {
            id: record.id.clone(),
            metadata: record.metadata.clone(),
            root: record.root,
            sources: record.sources.clone(),
            locations: record.locations.clone(),
            exclusive: Count::default(),
            overlapping: Count::default(),
            breakdown: BTreeMap::new(),
        });
    }
    aggregate(
        index,
        Bindings {
            evidence: evidence.clone(),
            bound,
            _root: None,
        },
        request,
        cancel,
        None,
        if index.snapshot_id.is_some() {
            Attribution::Captured
        } else {
            Attribution::Indexed
        },
    )
}
