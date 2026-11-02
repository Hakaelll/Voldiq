use crate::{Error, Kind, NodeId, Result, cloud::CloudFilter, index::Index, search::CompiledQuery};
use serde::Serialize;
use std::collections::HashMap;

pub const MAX_MAP_TARGETS: usize = 350;
const ANCESTRY_CACHE_LIMIT: usize = 4096;

#[derive(Debug, Serialize)]
pub struct MapMetric {
    pub id: NodeId,
    pub logical: u64,
    pub allocated: u64,
    pub allocation_known: bool,
    pub files: u64,
    pub latest_modified: Option<u64>,
    pub modified_known: u64,
    pub modified_total: u64,
    pub matches: Option<MapMatches>,
}

#[derive(Debug, Serialize)]
pub struct MapMatches {
    pub own: bool,
    pub descendants: u64,
}

impl MapMetric {
    fn include(&mut self, entry: &crate::Entry) {
        self.logical += entry.logical;
        self.allocated += entry.allocated;
        self.allocation_known &= entry.allocation_known;
        self.files += entry.files;
        if entry.is_metadata_file() {
            self.latest_modified = self.latest_modified.max(entry.modified);
            self.modified_known += u64::from(entry.modified.is_some());
            self.modified_total += 1;
        }
    }
}

impl Index {
    /// Metadata-only projection onto bounded native targets. Directories group matching leaves;
    /// their unfiltered totals are never added to filtered descendant bytes.
    pub fn map_metrics_cancellable(
        &self,
        scope: NodeId,
        targets: &[NodeId],
        cloud: Option<CloudFilter>,
        search: Option<&CompiledQuery<'_>>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<Vec<MapMetric>>> {
        if cancelled() {
            return Ok(None);
        }
        self.entry(scope)?;
        if let Some(search) = search {
            self.validate_query_scope(search)?;
        }
        if targets.len() > MAX_MAP_TARGETS {
            return Err(Error::Invalid("Too many map targets.".into()));
        }
        let mut positions = HashMap::new();
        let mut metrics = Vec::new();
        for &id in std::iter::once(&scope).chain(targets) {
            if cancelled() {
                return Ok(None);
            }
            let entry = self.entry(id)?;
            let mut next = Some(id);
            let mut depth = 0;
            while let Some(ancestor) = next {
                if ancestor == scope {
                    break;
                }
                if depth % 256 == 0 && cancelled() {
                    return Ok(None);
                }
                next = self.entry(ancestor)?.parent;
                depth += 1;
            }
            if next.is_none() {
                return Err(Error::Invalid("Map target is outside its scope.".into()));
            }
            if positions.contains_key(&id) {
                continue;
            }
            if metrics.len() == MAX_MAP_TARGETS {
                return Err(Error::Invalid("Too many map targets.".into()));
            }
            positions.insert(id, metrics.len());
            metrics.push(MapMetric {
                id,
                logical: 0,
                allocated: 0,
                // Missing scan coverage may conceal matching entries even if known leaves exclude it.
                allocation_known: entry.allocation_known,
                files: 0,
                latest_modified: None,
                modified_known: 0,
                modified_total: 0,
                matches: search.map(|_| MapMatches {
                    own: false,
                    descendants: 0,
                }),
            });
        }
        let mut ancestry = HashMap::<NodeId, Vec<usize>>::new();
        let mut budget = crate::search::GLOB_WORK_LIMIT;
        for (number, entry) in self.entries.iter().enumerate() {
            if number % 256 == 0 && cancelled() {
                return Ok(None);
            }
            let include_bytes =
                entry.kind != Kind::Directory && cloud.is_none_or(|f| f.matches(entry.cloud));
            if entry.id == NodeId(0) || (!include_bytes && search.is_none()) {
                continue;
            }
            let Some(parent) = entry.parent else {
                continue;
            };
            if !ancestry.contains_key(&parent) {
                let mut path = Vec::new();
                let mut ancestors = Vec::new();
                let mut next = Some(parent);
                while let Some(id) = next {
                    if path.len() % 256 == 0 && cancelled() {
                        return Ok(None);
                    }
                    if let Some(cached) = ancestry.get(&id) {
                        ancestors.extend(cached);
                        break;
                    }
                    path.push(id);
                    next = if id == scope {
                        None
                    } else {
                        self.entry(id)?.parent
                    };
                }
                for (depth, id) in path.into_iter().rev().enumerate() {
                    if depth % 256 == 0 && cancelled() {
                        return Ok(None);
                    }
                    if let Some(&position) = positions.get(&id) {
                        ancestors.push(position);
                    }
                    if ancestry.len() == ANCESTRY_CACHE_LIMIT {
                        ancestry.clear();
                    }
                    ancestry.insert(id, ancestors.clone());
                }
            }
            let own = positions.get(&entry.id).copied();
            let ancestors = &ancestry[&parent];
            if own.is_none() && ancestors.first() != Some(&0) {
                continue;
            }
            if include_bytes {
                for &position in ancestors {
                    metrics[position].include(entry);
                }
                if let Some(position) = own {
                    metrics[position].include(entry);
                }
            }
            if let Some(search) = search {
                match self.matches_compiled_cancellable(
                    search,
                    entry,
                    &mut budget,
                    &mut cancelled,
                )? {
                    None => return Ok(None),
                    Some(false) => continue,
                    Some(true) => {}
                }
                for &position in ancestors {
                    if let Some(matches) = &mut metrics[position].matches {
                        matches.descendants += 1;
                    }
                }
                if let Some(position) = own
                    && let Some(matches) = &mut metrics[position].matches
                {
                    matches.own = true;
                }
            }
        }
        if cancelled() {
            Ok(None)
        } else {
            Ok(Some(metrics))
        }
    }
}
