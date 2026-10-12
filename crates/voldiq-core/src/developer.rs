use crate::{Kind, NodeId, Result, index::Index};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub struct CacheLocation {
    pub path: PathBuf,
    pub tool: &'static str,
    pub category: Category,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Dependencies,
    Build,
    Cache,
}

#[derive(Serialize)]
pub struct Item {
    pub id: NodeId,
    pub path: String,
    pub tool: &'static str,
    pub category: Category,
    pub logical: u64,
    pub allocated: u64,
    pub files: u64,
    pub incomplete: bool,
    pub allocation_known: bool,
}

#[derive(Serialize)]
pub struct Page {
    pub items: Vec<Item>,
    pub total: usize,
    pub logical: u64,
    pub allocated: u64,
}

/// Classifies indexed project directories. It does not discover or remove data on disk.
pub fn query(index: &Index, scope: NodeId, offset: usize, limit: usize) -> Result<Page> {
    query_with_caches(index, scope, offset, limit, &[])
}

fn classified(index: &Index, scope: NodeId, caches: &[CacheLocation]) -> Result<Vec<Item>> {
    index.entry(scope)?;
    let caches = if index.snapshot_id.is_some() {
        &[]
    } else {
        caches
    };
    let cache_names: HashSet<_> = caches
        .iter()
        .filter_map(|cache| cache.path.file_name())
        .collect();
    let mut markers: HashMap<NodeId, HashSet<&str>> = HashMap::new();
    for entry in &index.entries {
        if entry.kind != Kind::File || entry.incomplete {
            continue;
        }
        if let Some(parent) = entry.parent
            && (matches!(
                entry.name.as_str(),
                "Cargo.toml"
                    | "package.json"
                    | "pyproject.toml"
                    | "CMakeLists.txt"
                    | "build.gradle"
                    | "build.gradle.kts"
                    | "settings.gradle"
                    | "settings.gradle.kts"
            ) || entry.name.ends_with(".csproj")
                || entry.name.ends_with(".fsproj"))
        {
            markers.entry(parent).or_default().insert(&entry.name);
        }
    }
    let mut candidates = Vec::new();
    for entry in &index.entries {
        if entry.kind != Kind::Directory || !index.in_scope(entry.id, scope) {
            continue;
        }
        let siblings = entry.parent.and_then(|parent| markers.get(&parent));
        let has = |marker| siblings.is_some_and(|set| set.contains(marker));
        let name = index
            .native_names
            .get(&entry.id)
            .map(|name| name.as_os_str())
            .unwrap_or_else(|| std::ffi::OsStr::new(&entry.name));
        let cache_classification =
            if !caches.is_empty() && (entry.id == scope || cache_names.contains(name)) {
                let path = index.path(entry.id)?;
                caches
                    .iter()
                    .filter(|cache| {
                        path == cache.path || (entry.id == scope && path.starts_with(&cache.path))
                    })
                    .max_by_key(|cache| {
                        (
                            cache.path.components().count(),
                            cache.category == Category::Dependencies,
                        )
                    })
                    .map(|cache| (cache.tool, cache.category))
            } else {
                None
            };
        let classification = cache_classification.or_else(|| match entry.name.as_str() {
            "target" if has("Cargo.toml") => Some(("Cargo", Category::Build)),
            "node_modules" if has("package.json") => Some(("Node.js", Category::Dependencies)),
            "dist" | "build" if has("package.json") => Some(("Node.js", Category::Build)),
            "build" if has("CMakeLists.txt") => Some(("CMake", Category::Build)),
            "__pycache__" => Some(("Python", Category::Cache)),
            ".gradle"
                if has("build.gradle")
                    || has("build.gradle.kts")
                    || has("settings.gradle")
                    || has("settings.gradle.kts") =>
            {
                Some(("Gradle", Category::Cache))
            }
            "bin" | "obj"
                if siblings.is_some_and(|set| {
                    set.iter()
                        .any(|name| name.ends_with(".csproj") || name.ends_with(".fsproj"))
                }) =>
            {
                Some((".NET", Category::Build))
            }
            _ => None,
        });
        if let Some((tool, category)) = classification {
            candidates.push(Item {
                id: entry.id,
                path: String::new(),
                tool,
                category,
                logical: entry.logical,
                allocated: entry.allocated,
                files: entry.files,
                incomplete: entry.incomplete,
                allocation_known: entry.allocation_known,
            });
        }
    }
    Ok(candidates)
}

#[derive(Default)]
pub(crate) struct BuildScopes {
    builds: HashSet<NodeId>,
    excluded: HashSet<NodeId>,
}

impl BuildScopes {
    pub(crate) fn contains(&self, index: &Index, id: NodeId) -> bool {
        let mut next = Some(id);
        let mut inside = false;
        while let Some(id) = next {
            if self.excluded.contains(&id) {
                return false;
            }
            inside |= self.builds.contains(&id);
            next = index.entries[id.0 as usize].parent;
        }
        inside
    }
}

pub(crate) fn project_build_scopes(index: &Index, caches: &[CacheLocation]) -> Result<BuildScopes> {
    let mut scopes = BuildScopes::default();
    for item in classified(index, NodeId(0), caches)? {
        if item.category == Category::Build {
            scopes.builds.insert(item.id);
        } else {
            scopes.excluded.insert(item.id);
        }
    }
    Ok(scopes)
}

pub fn query_with_caches(
    index: &Index,
    scope: NodeId,
    offset: usize,
    limit: usize,
    caches: &[CacheLocation],
) -> Result<Page> {
    let mut candidates = classified(index, scope, caches)?;
    // Nested artifacts belong to the outer candidate so totals never double-count a subtree.
    let ids: HashSet<_> = candidates.iter().map(|item| item.id).collect();
    candidates.retain(|item| {
        let mut parent = index.entries[item.id.0 as usize].parent;
        while let Some(id) = parent {
            if ids.contains(&id) {
                return false;
            }
            parent = index.entries[id.0 as usize].parent;
        }
        true
    });
    let total = candidates.len();
    let logical = candidates.iter().map(|item| item.logical).sum();
    let allocated = candidates.iter().map(|item| item.allocated).sum();
    candidates.sort_unstable_by(|a, b| b.logical.cmp(&a.logical).then(a.id.0.cmp(&b.id.0)));
    let mut items: Vec<_> = candidates
        .into_iter()
        .skip(offset)
        .take(limit.clamp(1, 100))
        .collect();
    for item in &mut items {
        item.path = index.path(item.id)?.to_string_lossy().into_owned();
    }
    Ok(Page {
        items,
        total,
        logical,
        allocated,
    })
}
