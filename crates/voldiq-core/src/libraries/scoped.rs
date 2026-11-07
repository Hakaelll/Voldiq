use super::ManualLibrary;
use crate::{NodeId, agents::ordinary_path, index::Index};
use std::{collections::HashMap, path::Path};

pub(super) const OUTSIDE: u16 = u16::MAX;
pub(super) const UNRESOLVED: u16 = u16::MAX - 1;

pub(super) fn path_key(path: &Path) -> Option<String> {
    Some(
        ordinary_path(path)
            .ok()?
            .to_str()?
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase(),
    )
}

pub(super) type Locations<K> = HashMap<K, Option<NodeId>>;

pub(super) fn observe<K: Eq + std::hash::Hash>(locations: &mut Locations<K>, key: K, id: NodeId) {
    locations
        .entry(key)
        .and_modify(|prior| *prior = None)
        .or_insert(Some(id));
}

pub(super) fn local_folder(index: &Index, id: NodeId, scope: NodeId) -> bool {
    index.in_scope(id, scope)
        && index.pin_folder(id).is_ok_and(|pin| {
            index.root_identity.is_some_and(|root| {
                pin.ancestor_identities()
                    .is_ok_and(|ancestors| ancestors.contains(&root))
            })
        })
}

pub(super) fn ownership(
    index: &Index,
    scope: NodeId,
    boundaries: &HashMap<NodeId, u16>,
    owners: &mut [u16],
    pending: &mut Vec<NodeId>,
    id: NodeId,
) -> u16 {
    let mut cursor = id;
    let mut owner = loop {
        let cached = owners[cursor.0 as usize];
        if cached != UNRESOLVED {
            break cached;
        }
        pending.push(cursor);
        if cursor == scope {
            break 0;
        }
        match index.entries[cursor.0 as usize].parent {
            Some(parent) => cursor = parent,
            None => break OUTSIDE,
        }
    };
    while let Some(node) = pending.pop() {
        if owner != OUTSIDE {
            owner = boundaries.get(&node).copied().unwrap_or(owner);
        }
        owners[node.0 as usize] = owner;
    }
    owner
}

pub(super) fn measure_manual(
    index: &Index,
    scope: NodeId,
    manuals: Vec<Option<String>>,
    locations: &Locations<String>,
) -> Vec<Option<ManualLibrary>> {
    manuals
        .into_iter()
        .map(|path| {
            let id = locations.get(&path?)?.as_ref().copied()?;
            if !local_folder(index, id, scope) {
                return None;
            }
            let entry = index.entry(id).ok()?;
            Some(ManualLibrary {
                id,
                logical_bytes: entry.logical.to_string(),
                partial: entry.incomplete,
            })
        })
        .collect()
}
