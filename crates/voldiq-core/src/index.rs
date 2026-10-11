use crate::{Entry, Error, Issue, Kind, NodeId, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct Index {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub issues: Vec<Issue>,
    pub error_count: u64,
    pub native_names: std::collections::HashMap<NodeId, std::ffi::OsString>,
    pub root_identity: Option<(u64, u64)>,
    pub exclusions: Vec<PathBuf>,
    pub snapshot_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Query {
    pub parent: Option<NodeId>,
    pub scope: Option<NodeId>,
    pub text: String,
    pub extension: String,
    pub min_size: u64,
    pub modified_before: Option<u64>,
    pub files_only: bool,
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
            extension: String::new(),
            min_size: 0,
            modified_before: None,
            files_only: false,
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
    pub extension: String,
    pub bytes: u64,
    pub count: u64,
}

impl Index {
    pub fn entry(&self, id: NodeId) -> Result<&Entry> {
        self.entries.get(id.0 as usize).ok_or_else(|| {
            Error::Invalid("Item is no longer in the index. Rescan and try again.".into())
        })
    }

    pub fn path(&self, id: NodeId) -> Result<PathBuf> {
        let mut names = Vec::new();
        let mut node = self.entry(id)?;
        while let Some(parent) = node.parent {
            names.push(
                self.native_names
                    .get(&node.id)
                    .map(|name| name.as_os_str())
                    .unwrap_or_else(|| std::ffi::OsStr::new(&node.name)),
            );
            node = self.entry(parent)?;
        }
        let mut path = self.root.clone();
        for name in names.into_iter().rev() {
            path.push(name);
        }
        Ok(path)
    }

    pub fn details(&self, id: NodeId) -> Result<Details> {
        let entry = self.entry(id)?.clone();
        let mut ancestors = Vec::new();
        let mut parent = entry.parent;
        while let Some(id) = parent {
            let node = self.entry(id)?;
            ancestors.push(node.clone());
            parent = node.parent;
        }
        ancestors.reverse();
        let path = self.path(id)?;
        Ok(Details {
            entry,
            path: path.to_string_lossy().into_owned(),
            path_exact: path.to_str().is_some(),
            ancestors,
        })
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
        if let Some(id) = query.parent {
            self.entry(id)?;
        }
        if let Some(id) = query.scope {
            self.entry(id)?;
        }
        let text = query.text.to_lowercase();
        let extension = query.extension.trim_start_matches('.').to_lowercase();
        let mut matches: Vec<&Entry> = self
            .entries
            .iter()
            .skip(1)
            .filter(|entry| {
                query.parent.is_none_or(|id| entry.parent == Some(id))
                    && query.scope.is_none_or(|id| self.in_scope(entry.id, id))
                    && (!query.files_only || entry.kind == Kind::File)
                    && entry.logical >= query.min_size
                    && (text.is_empty() || entry.name.to_lowercase().contains(&text))
                    && (extension.is_empty()
                        || std::path::Path::new(&entry.name)
                            .extension()
                            .is_some_and(|ext| ext.to_string_lossy().to_lowercase() == extension))
                    && query
                        .modified_before
                        .is_none_or(|before| entry.modified.is_some_and(|date| date < before))
            })
            .collect();
        let total = matches.len();
        if query.offset >= total {
            return Ok(Page {
                total,
                items: Vec::new(),
            });
        }
        let compare = |a: &&Entry, b: &&Entry| match query.sort.as_str() {
            "name" => compare_name(&a.name, &b.name).then(a.id.0.cmp(&b.id.0)),
            "modified" => b.modified.cmp(&a.modified).then(a.id.0.cmp(&b.id.0)),
            _ => b.logical.cmp(&a.logical).then(a.id.0.cmp(&b.id.0)),
        };
        if query.offset > 0 {
            matches.select_nth_unstable_by(query.offset, compare);
        }
        let remaining = &mut matches[query.offset..];
        let limit = query.limit.clamp(1, 500).min(remaining.len());
        if limit < remaining.len() {
            remaining.select_nth_unstable_by(limit, compare);
        }
        let page = &mut remaining[..limit];
        page.sort_unstable_by(compare);
        Ok(Page {
            total,
            items: page.iter().map(|entry| (*entry).clone()).collect(),
        })
    }

    pub fn file_types(&self, scope: NodeId) -> Result<Vec<FileType>> {
        self.entry(scope)?;
        let mut types = std::collections::HashMap::<String, (u64, u64)>::new();
        for entry in &self.entries {
            if entry.kind != Kind::File || !self.in_scope(entry.id, scope) {
                continue;
            }
            let ext = std::path::Path::new(&entry.name)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| "No extension".into());
            let total = types.entry(ext).or_default();
            total.0 += entry.logical;
            total.1 += 1;
        }
        let mut result: Vec<_> = types
            .into_iter()
            .map(|(extension, (bytes, count))| FileType {
                extension,
                bytes,
                count,
            })
            .collect();
        result.sort_unstable_by_key(|item| std::cmp::Reverse(item.bytes));
        // shortcut: only the largest 30 extension groups cross IPC; request a filtered file list to inspect the rest.
        result.truncate(30);
        Ok(result)
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
