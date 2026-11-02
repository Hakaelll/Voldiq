use crate::{Entry, Error, Issue, Kind, NodeId, Result, search::CompiledQuery};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
mod rank;

pub struct Index {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub issues: Vec<Issue>,
    pub error_count: u64,
    pub native_names: std::collections::HashMap<NodeId, std::ffi::OsString>,
    pub root_identity: Option<(u64, u64)>,
    pub exclusions: Vec<PathBuf>,
    pub snapshot_id: Option<i64>,
    pub captured_caches: Vec<crate::developer::CacheEvidence>,
    pub ntfs_checkpoint: Option<crate::ntfs::Checkpoint>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Query {
    pub parent: Option<NodeId>,
    pub scope: Option<NodeId>,
    pub text: String,
    pub expression: String,
    pub extension: String,
    pub without_extension: bool,
    pub min_size: u64,
    pub modified_before: Option<u64>,
    pub files_only: bool,
    pub kind: Option<Kind>,
    pub cloud: Option<crate::cloud::CloudFilter>,
    pub sort: String,
    pub offset: usize,
    pub limit: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            parent: None,
            scope: None,
            text: String::new(),
            expression: String::new(),
            extension: String::new(),
            without_extension: false,
            min_size: 0,
            modified_before: None,
            files_only: false,
            kind: None,
            cloud: None,
            sort: "size".into(),
            offset: 0,
            limit: 100,
        }
    }
}

#[derive(Serialize)]
pub struct Page {
    pub items: Vec<Entry>,
    pub total: usize,
    pub largest: u64,
}

#[derive(Serialize)]
pub struct Details {
    pub entry: Entry,
    pub path: String,
    pub path_exact: bool,
    pub ancestors: Vec<Entry>,
}

#[derive(Serialize)]
pub struct FileType {
    pub extension: Option<String>,
    pub bytes: u64,
    pub count: u64,
}

#[derive(Serialize)]
pub struct FileTypes {
    pub items: Vec<FileType>,
    pub total_bytes: u64,
    pub total_files: u64,
}

impl Index {
    pub fn entry(&self, id: NodeId) -> Result<&Entry> {
        self.entries.get(id.0 as usize).ok_or_else(|| {
            Error::Invalid("Item is no longer in the index. Rescan and try again.".into())
        })
    }

    pub fn path(&self, id: NodeId) -> Result<PathBuf> {
        self.path_inner(id, || false)?
            .ok_or_else(|| Error::Invalid("Query cancelled.".into()))
    }

    pub fn path_cancellable(
        &self,
        id: NodeId,
        cancelled: impl FnMut() -> bool,
    ) -> Result<Option<PathBuf>> {
        self.path_inner(id, cancelled)
    }

    fn path_inner(
        &self,
        id: NodeId,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<PathBuf>> {
        if cancelled() {
            return Ok(None);
        }
        let mut names = Vec::new();
        let mut node = self.entry(id)?;
        while let Some(parent) = node.parent {
            if names.len() % 256 == 0 && cancelled() {
                return Ok(None);
            }
            names.push(
                self.native_names
                    .get(&node.id)
                    .map(|name| name.as_os_str())
                    .unwrap_or_else(|| std::ffi::OsStr::new(&node.name)),
            );
            node = self.entry(parent)?;
        }
        let mut path = self.root.clone();
        for (position, name) in names.into_iter().rev().enumerate() {
            if position % 256 == 0 && cancelled() {
                return Ok(None);
            }
            path.push(name);
        }
        if cancelled() {
            return Ok(None);
        }
        Ok(Some(path))
    }

    pub fn details(&self, id: NodeId) -> Result<Details> {
        self.details_cancellable(id, || false)?
            .ok_or_else(|| Error::Invalid("Query cancelled.".into()))
    }

    pub fn details_cancellable(
        &self,
        id: NodeId,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<Details>> {
        if cancelled() {
            return Ok(None);
        }
        let entry = self.entry(id)?.clone();
        let mut ancestors = Vec::new();
        let mut parent = entry.parent;
        while let Some(id) = parent {
            if ancestors.len() % 256 == 0 && cancelled() {
                return Ok(None);
            }
            let node = self.entry(id)?;
            ancestors.push(node.clone());
            parent = node.parent;
        }
        for position in 0..ancestors.len() / 2 {
            if position % 256 == 0 && cancelled() {
                return Ok(None);
            }
            let last = ancestors.len() - 1 - position;
            ancestors.swap(position, last);
        }
        let Some(path) = self.path_inner(id, &mut cancelled)? else {
            return Ok(None);
        };
        let details = Details {
            entry,
            path: path.to_string_lossy().into_owned(),
            path_exact: path.to_str().is_some(),
            ancestors,
        };
        if cancelled() {
            return Ok(None);
        }
        Ok(Some(details))
    }

    pub fn in_scope(&self, id: NodeId, scope: NodeId) -> bool {
        if scope == NodeId(0) {
            return true;
        }
        let mut next = Some(id);
        while let Some(id) = next {
            if id == scope {
                return true;
            }
            next = self.entries[id.0 as usize].parent;
        }
        false
    }

    pub fn query(&self, query: &Query) -> Result<Page> {
        self.query_compiled(&query.compile()?)
    }

    pub fn query_compiled(&self, query: &CompiledQuery<'_>) -> Result<Page> {
        self.query_inner(query, || false, false)?
            .ok_or_else(|| Error::Invalid("Query cancelled.".into()))
    }

    pub fn query_cancellable(
        &self,
        query: &Query,
        cancelled: impl FnMut() -> bool,
    ) -> Result<Option<Page>> {
        self.query_inner(&query.compile()?, cancelled, true)
    }

    pub(crate) fn validate_query_scope(&self, compiled: &CompiledQuery<'_>) -> Result<()> {
        for id in [compiled.query.parent, compiled.query.scope]
            .into_iter()
            .flatten()
        {
            self.entry(id)?;
        }
        Ok(())
    }

    pub(crate) fn matches_compiled_cancellable(
        &self,
        compiled: &CompiledQuery<'_>,
        entry: &Entry,
        budget: &mut u64,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<bool>> {
        let query = compiled.query;
        if entry.id == NodeId(0) || query.parent.is_some_and(|id| entry.parent != Some(id)) {
            return Ok(Some(false));
        }
        if let Some(scope) = query.scope {
            match self.scope_matches(entry.id, scope, cancelled) {
                None => return Ok(None),
                Some(false) => return Ok(Some(false)),
                Some(true) => {}
            }
        }
        match compiled.search.matches_entry(entry, budget, cancelled)? {
            None => return Ok(None),
            Some(false) => return Ok(Some(false)),
            Some(true) => {}
        }
        if compiled.search.needs_path() {
            let Some(path) = self.path_cancellable(entry.id, &mut *cancelled)? else {
                return Ok(None);
            };
            return Ok(compiled
                .search
                .matches_path(path.to_str(), budget, cancelled)?);
        }
        Ok(Some(true))
    }

    fn query_inner(
        &self,
        compiled: &CompiledQuery<'_>,
        mut cancelled: impl FnMut() -> bool,
        cancellable_ranking: bool,
    ) -> Result<Option<Page>> {
        if cancelled() {
            return Ok(None);
        }
        let query = compiled.query;
        self.validate_query_scope(compiled)?;
        let mut budget = crate::search::GLOB_WORK_LIMIT;
        let mut matches = Vec::new();
        let mut largest = 0;
        for (position, entry) in self.entries.iter().skip(1).enumerate() {
            if position % 256 == 0 && cancelled() {
                return Ok(None);
            }
            match self.matches_compiled_cancellable(compiled, entry, &mut budget, &mut cancelled)? {
                None => return Ok(None),
                Some(false) => continue,
                Some(true) => {}
            }
            matches.push(entry);
            largest = largest.max(entry.logical);
        }
        if cancelled() {
            return Ok(None);
        }
        let total = matches.len();
        if query.offset >= total {
            return Ok(Some(Page {
                total,
                largest,
                items: Vec::new(),
            }));
        }
        let compare = |a: &&Entry, b: &&Entry| match query.sort.as_str() {
            "allocated" => {
                let known = |entry: &Entry| {
                    (entry.allocation_known || entry.allocated > 0).then_some(entry.allocated)
                };
                known(b).cmp(&known(a)).then(a.id.0.cmp(&b.id.0))
            }
            "name" => compare_name(&a.name, &b.name).then(a.id.0.cmp(&b.id.0)),
            "modified" => b.modified.cmp(&a.modified).then(a.id.0.cmp(&b.id.0)),
            _ => b.logical.cmp(&a.logical).then(a.id.0.cmp(&b.id.0)),
        };
        if query.offset > 0 {
            if cancellable_ranking {
                if !rank::select(&mut matches, query.offset, &compare, &mut cancelled) {
                    return Ok(None);
                }
            } else {
                matches.select_nth_unstable_by(query.offset, compare);
            }
        }
        let remaining = &mut matches[query.offset..];
        let limit = query.limit.clamp(1, 500).min(remaining.len());
        if limit < remaining.len() {
            if cancellable_ranking {
                if !rank::select(remaining, limit, &compare, &mut cancelled) {
                    return Ok(None);
                }
            } else {
                remaining.select_nth_unstable_by(limit, compare);
            }
        }
        let page = &mut remaining[..limit];
        page.sort_unstable_by(compare);
        if cancelled() {
            return Ok(None);
        }
        Ok(Some(Page {
            total,
            largest,
            items: page.iter().map(|entry| (*entry).clone()).collect(),
        }))
    }

    pub fn file_types(&self, scope: NodeId) -> Result<FileTypes> {
        self.file_types_inner(scope, || false, false)?
            .ok_or_else(|| Error::Invalid("Query cancelled.".into()))
    }

    pub fn file_types_cancellable(
        &self,
        scope: NodeId,
        cancelled: impl FnMut() -> bool,
    ) -> Result<Option<FileTypes>> {
        self.file_types_inner(scope, cancelled, true)
    }

    fn scope_matches(
        &self,
        id: NodeId,
        scope: NodeId,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<bool> {
        if scope == NodeId(0) {
            return Some(true);
        }
        let mut parent = Some(id);
        let mut depth = 0;
        while let Some(id) = parent {
            if id == scope {
                return Some(true);
            }
            if depth % 256 == 0 && cancelled() {
                return None;
            }
            parent = self.entries[id.0 as usize].parent;
            depth += 1;
        }
        Some(false)
    }

    fn file_types_inner(
        &self,
        scope: NodeId,
        mut cancelled: impl FnMut() -> bool,
        cancellable_ranking: bool,
    ) -> Result<Option<FileTypes>> {
        if cancelled() {
            return Ok(None);
        }
        self.entry(scope)?;
        let mut types =
            std::collections::HashMap::<Option<std::borrow::Cow<'_, str>>, (u64, u64)>::new();
        let mut total_bytes = 0;
        let mut total_files = 0;
        for (position, entry) in self.entries.iter().enumerate() {
            if position % 256 == 0 && cancelled() {
                return Ok(None);
            }
            if !entry.is_metadata_file() {
                continue;
            }
            match self.scope_matches(entry.id, scope, &mut cancelled) {
                None => return Ok(None),
                Some(false) => continue,
                Some(true) => {}
            }
            let ext = std::path::Path::new(&entry.name)
                .extension()
                .map(|extension| {
                    let extension = extension.to_string_lossy();
                    if extension.is_ascii()
                        && !extension.bytes().any(|byte| byte.is_ascii_uppercase())
                    {
                        extension
                    } else {
                        std::borrow::Cow::Owned(extension.to_lowercase())
                    }
                });
            let total = types.entry(ext).or_default();
            total.0 += entry.logical;
            total.1 += 1;
            total_bytes += entry.logical;
            total_files += 1;
        }
        if cancelled() {
            return Ok(None);
        }
        let mut result = Vec::with_capacity(types.len());
        for (position, (extension, (bytes, count))) in types.into_iter().enumerate() {
            if position % 256 == 0 && cancelled() {
                return Ok(None);
            }
            result.push(FileType {
                extension: extension.map(std::borrow::Cow::into_owned),
                bytes,
                count,
            });
        }
        let compare =
            |a: &FileType, b: &FileType| b.bytes.cmp(&a.bytes).then(a.extension.cmp(&b.extension));
        if result.len() > 30 {
            if cancellable_ranking {
                if !rank::select(&mut result, 30, &compare, &mut cancelled) {
                    return Ok(None);
                }
            } else {
                result.select_nth_unstable_by(30, compare);
            }
        }
        // shortcut: only the largest 30 extension groups cross IPC; request a filtered file list to inspect the rest.
        result.truncate(30);
        result.sort_unstable_by(compare);
        if cancelled() {
            return Ok(None);
        }
        Ok(Some(FileTypes {
            items: result,
            total_bytes,
            total_files,
        }))
    }

    pub fn accumulate(&mut self, child: &Entry) {
        let mut parent = child.parent;
        while let Some(id) = parent {
            let entry = &mut self.entries[id.0 as usize];
            entry.logical += child.logical;
            entry.allocated += child.allocated;
            entry.allocation_known &= child.allocation_known;
            entry.files += child.files;
            entry.incomplete |= child.incomplete;
            parent = entry.parent;
        }
    }
}

fn compare_name(a: &str, b: &str) -> std::cmp::Ordering {
    if a.is_ascii() && b.is_ascii() {
        a.bytes()
            .map(|byte| byte.to_ascii_lowercase())
            .cmp(b.bytes().map(|byte| byte.to_ascii_lowercase()))
    } else {
        a.to_lowercase().cmp(&b.to_lowercase())
    }
}
