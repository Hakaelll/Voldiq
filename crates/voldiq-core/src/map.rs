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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Entry, cloud::CloudState};

    #[test]
    fn compiled_map_matches_are_scoped_and_do_not_change_observed_byte_totals() {
        let index = index();
        let query = crate::index::Query {
            expression: "size>=100B type:file".into(),
            ..Default::default()
        };
        let compiled = query.compile().unwrap();
        let metrics = index
            .map_metrics_cancellable(
                NodeId(1),
                &[NodeId(2), NodeId(3), NodeId(4)],
                None,
                Some(&compiled),
                || false,
            )
            .unwrap()
            .unwrap();
        assert_eq!((metrics[0].logical, metrics[0].files), (360, 4));
        let actual: Vec<_> = metrics
            .iter()
            .map(|m| {
                (
                    m.id.0,
                    m.matches.as_ref().unwrap().own,
                    m.matches.as_ref().unwrap().descendants,
                )
            })
            .collect();
        assert_eq!(
            actual,
            [(1, false, 2), (2, false, 1), (3, true, 0), (4, true, 0)]
        );
        let folder = crate::index::Query {
            expression: "type:folder name:item-2".into(),
            ..Default::default()
        };
        let folder = folder.compile().unwrap();
        let metrics = index
            .map_metrics_cancellable(NodeId(1), &[NodeId(2)], None, Some(&folder), || false)
            .unwrap()
            .unwrap();
        assert_eq!(metrics[0].matches.as_ref().unwrap().descendants, 1);
        assert!(metrics[1].matches.as_ref().unwrap().own);
        let invalid = crate::index::Query {
            scope: Some(NodeId(999)),
            ..Default::default()
        };
        assert!(
            index
                .map_metrics_cancellable(
                    NodeId(1),
                    &[],
                    None,
                    Some(&invalid.compile().unwrap()),
                    || false
                )
                .is_err()
        );
    }

    fn index() -> Index {
        let mut entries = Vec::new();
        for (id, parent, kind, cloud, logical, allocated, known, modified) in [
            (
                0,
                None,
                Kind::Directory,
                CloudState::NotPlaceholder,
                0,
                0,
                true,
                None,
            ),
            (
                1,
                Some(0),
                Kind::Directory,
                CloudState::NotPlaceholder,
                0,
                0,
                true,
                None,
            ),
            (
                2,
                Some(1),
                Kind::Directory,
                CloudState::NotPlaceholder,
                0,
                0,
                true,
                None,
            ),
            (
                3,
                Some(2),
                Kind::Link,
                CloudState::FileRecallRequired,
                120,
                0,
                false,
                Some(20),
            ),
            (
                4,
                Some(1),
                Kind::Link,
                CloudState::FileNoRecallFlag,
                200,
                8,
                false,
                None,
            ),
            (
                5,
                Some(2),
                Kind::File,
                CloudState::NotPlaceholder,
                10,
                16,
                true,
                Some(90),
            ),
            (
                6,
                Some(1),
                Kind::File,
                CloudState::Unknown,
                30,
                32,
                true,
                Some(80),
            ),
            (
                7,
                Some(0),
                Kind::Link,
                CloudState::FileRecallRequired,
                999,
                0,
                false,
                Some(999),
            ),
            (
                8,
                Some(1),
                Kind::Link,
                CloudState::DirectoryRecallRequired,
                0,
                0,
                false,
                None,
            ),
        ] {
            entries.push(Entry {
                id: NodeId(id),
                parent: parent.map(NodeId),
                name: format!("item-{id}"),
                kind,
                cloud,
                logical,
                allocated,
                allocation_known: known,
                modified,
                files: u64::from(kind == Kind::File || cloud.is_file()),
                incomplete: !known,
                hard_link: false,
                file_state: None,
            });
        }
        let mut index = Index {
            root: std::path::PathBuf::from("C:\\synthetic-map"),
            entries,
            issues: Vec::new(),
            error_count: 0,
            native_names: HashMap::new(),
            root_identity: None,
            exclusions: Vec::new(),
            snapshot_id: None,
            captured_caches: Vec::new(),
            ntfs_checkpoint: None,
        };
        for id in [3, 4, 5, 6, 7, 8] {
            let entry = index.entries[id].clone();
            index.accumulate(&entry);
        }
        index
    }

    #[test]
    fn cloud_projection_uses_all_matching_leaves_once_without_mutating_full_totals() {
        let index = index();
        let metrics = index
            .map_metrics_cancellable(
                NodeId(1),
                &[NodeId(2), NodeId(3), NodeId(2)],
                Some(CloudFilter::Placeholder),
                None,
                || false,
            )
            .unwrap()
            .unwrap();
        let actual: Vec<_> = metrics
            .iter()
            .map(|m| {
                (
                    m.id.0,
                    m.logical,
                    m.allocated,
                    m.files,
                    m.allocation_known,
                    m.latest_modified,
                    m.modified_known,
                    m.modified_total,
                )
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                (1, 320, 8, 2, false, Some(20), 1, 2),
                (2, 120, 0, 1, false, Some(20), 1, 1),
                (3, 120, 0, 1, false, Some(20), 1, 1),
            ]
        );
        assert_eq!(
            (
                index.entries[1].logical,
                index.entries[1].allocated,
                index.entries[1].files
            ),
            (360, 56, 4)
        );
        for (filter, logical, files, date) in [
            (None, 360, 4, Some(90)),
            (Some(CloudFilter::RecallRequired), 120, 1, Some(20)),
            (Some(CloudFilter::NoRecallFlag), 200, 1, None),
            (Some(CloudFilter::NotPlaceholder), 10, 1, Some(90)),
            (Some(CloudFilter::Unknown), 30, 1, Some(80)),
        ] {
            let result = index
                .map_metrics_cancellable(NodeId(1), &[], filter, None, || false)
                .unwrap()
                .unwrap();
            assert_eq!(
                (
                    result[0].logical,
                    result[0].files,
                    result[0].latest_modified
                ),
                (logical, files, date)
            );
        }
        let empty = index
            .map_metrics_cancellable(NodeId(2), &[], Some(CloudFilter::Unknown), None, || false)
            .unwrap()
            .unwrap();
        assert_eq!(
            (empty[0].logical, empty[0].files, empty[0].allocation_known),
            (0, 0, false)
        );
    }

    #[test]
    fn projection_rejects_outside_invalid_and_excessive_targets_and_cancels() {
        let index = index();
        assert!(
            index
                .map_metrics_cancellable(NodeId(1), &[NodeId(7)], None, None, || false)
                .is_err()
        );
        assert!(
            index
                .map_metrics_cancellable(NodeId(1), &[NodeId(99)], None, None, || false)
                .is_err()
        );
        assert!(
            index
                .map_metrics_cancellable(
                    NodeId(1),
                    &vec![NodeId(2); MAX_MAP_TARGETS + 1],
                    None,
                    None,
                    || false
                )
                .is_err()
        );
        assert!(
            index
                .map_metrics_cancellable(NodeId(1), &[], None, None, || true)
                .unwrap()
                .is_none()
        );
        let mut checks = 0;
        assert!(
            index
                .map_metrics_cancellable(
                    NodeId(1),
                    &[NodeId(2)],
                    Some(CloudFilter::Placeholder),
                    None,
                    || {
                        checks += 1;
                        checks == 4
                    }
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(checks, 4);
    }
}
