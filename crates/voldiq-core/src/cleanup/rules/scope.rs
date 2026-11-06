use super::*;

pub(super) struct DirectoryScope {
    pub id: NodeId,
    pub whole: bool,
    pub root: crate::agents::AuthorizedPath,
    index_root: crate::agents::AuthorizedPath,
}

impl DirectoryScope {
    pub(super) fn bind(index: &Index, path: &Path) -> Result<Option<Self>> {
        let root = crate::agents::pin_directory(path)?;
        let index_root = index.pin_folder(NodeId(0))?;
        let identity = root.identity();
        let found = index
            .entries
            .iter()
            .find(|entry| {
                entry.kind == Kind::Directory
                    && entry
                        .file_state
                        .is_some_and(|state| state.identity == identity)
            })
            .map(|entry| entry.id);
        let exact_root = index_root.identity() == identity;
        let (id, whole) = if let Some(id) = found {
            (id, true)
        } else if exact_root {
            (NodeId(0), true)
        } else if index_root.ancestor_identities()?.contains(&identity) {
            (NodeId(0), false)
        } else {
            return Ok(None);
        };
        Ok(Some(Self {
            id,
            whole,
            root,
            index_root,
        }))
    }

    pub(super) fn path(&self, index: &Index, id: NodeId) -> Result<std::path::PathBuf> {
        if !index.in_scope(id, self.id) {
            return Err(Error::Invalid(
                "Entry is outside the native rule scope.".into(),
            ));
        }
        let recorded = index.path(id)?;
        let relative = recorded
            .strip_prefix(&index.root)
            .map_err(|_| Error::Invalid("Invalid indexed rule path.".into()))?;
        Ok(self.index_root.path().join(relative))
    }
}
