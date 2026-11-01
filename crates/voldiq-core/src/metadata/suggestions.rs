use super::{Protections, path_key};
use crate::{
    Error, Kind, NodeId, Result,
    agents::{AuthorizedPath, pin_directory},
    cleanup,
    index::Index,
};
use std::{
    collections::HashSet,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eligibility {
    Allowed,
    Protected,
    Unverified,
}

/// A captured policy projection, not file-mutation authority.
pub struct SuggestionMask<'a> {
    index: &'a Index,
    entries: Vec<Eligibility>,
    unverified: Vec<NodeId>,
    _root: AuthorizedPath,
}

impl Protections {
    pub fn for_index<'a>(
        &self,
        index: &'a Index,
        cancel: &AtomicBool,
    ) -> Result<SuggestionMask<'a>> {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Invalid("Suggestion review cancelled.".into()));
        }
        let root = pin_directory(&index.root)?;
        if Some(root.identity()) != index.root_identity {
            return Err(Error::Invalid(
                "Scan root changed. Rescan before reviewing suggestions.".into(),
            ));
        }
        let system = cleanup::protected_identities()?;
        let protected_ancestor = root
            .ancestor_identities()?
            .iter()
            .any(|identity| self.identities.contains(identity) || system.contains(identity));
        let paths: HashSet<_> = self.paths.iter().map(|path| path_key(path)).collect();
        #[cfg(windows)]
        let paths = paths
            .into_iter()
            .chain(
                crate::windows::protected_directories()?
                    .iter()
                    .map(|path| path_key(path)),
            )
            .collect::<HashSet<_>>();
        let mut entries = Vec::with_capacity(index.entries.len());
        let mut unverified = Vec::new();
        for (position, entry) in index.entries.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Invalid("Suggestion review cancelled.".into()));
            }
            if entry.id.0 as usize != position
                || (position == 0 && (entry.parent.is_some() || entry.kind != Kind::Directory))
                || (position > 0 && entry.parent.is_none())
            {
                return Err(invalid());
            }
            let inherited = match entry.parent {
                Some(parent)
                    if (parent.0 as usize) < position
                        && index.entries[parent.0 as usize].kind == Kind::Directory =>
                {
                    entries[parent.0 as usize]
                }
                Some(_) => return Err(invalid()),
                None => Eligibility::Allowed,
            };
            let identity = if position == 0 {
                index.root_identity
            } else {
                entry.file_state.map(|state| state.identity)
            };
            let sensitive = cleanup::record::sensitive(if position == 0 {
                &index.root
            } else {
                Path::new(&entry.name)
            });
            let protected_path = if entry.kind == Kind::Directory {
                path_key(&index.path(entry.id)?)
                    .ancestors()
                    .any(|ancestor| paths.contains(ancestor))
            } else {
                false
            };
            let blocked = (position == 0 && protected_ancestor)
                || inherited == Eligibility::Protected
                || sensitive
                || identity.is_some_and(|identity| {
                    self.identities.contains(&identity) || system.contains(&identity)
                })
                || protected_path;
            let eligibility = if blocked {
                Eligibility::Protected
            } else if inherited == Eligibility::Unverified {
                Eligibility::Unverified
            } else if entry.kind == Kind::Directory && identity.is_none() {
                unverified.push(entry.id);
                Eligibility::Unverified
            } else {
                Eligibility::Allowed
            };
            entries.push(eligibility);
        }
        if entries.is_empty() {
            return Err(invalid());
        }
        Ok(SuggestionMask {
            index,
            entries,
            unverified,
            _root: root,
        })
    }
}

impl SuggestionMask<'_> {
    pub fn eligibility(&self, id: NodeId) -> Result<Eligibility> {
        self.entries.get(id.0 as usize).copied().ok_or_else(invalid)
    }

    pub fn allows(&self, id: NodeId) -> bool {
        self.eligibility(id)
            .is_ok_and(|state| state == Eligibility::Allowed)
    }

    pub fn unverified_branches(&self, scope: NodeId) -> Result<usize> {
        self.index.entry(scope)?;
        Ok(self
            .unverified
            .iter()
            .filter(|&&id| self.index.in_scope(id, scope) || self.index.in_scope(scope, id))
            .count())
    }
}

fn invalid() -> Error {
    Error::Invalid("Suggestion index has an invalid hierarchy.".into())
}
