use crate::{Kind, NodeId, Result, index::Index};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

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
    index.entry(scope)?;
    let mut markers: HashMap<NodeId, HashSet<&str>> = HashMap::new();
    for entry in &index.entries {
        if entry.kind != Kind::File {
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
        let classification = match entry.name.as_str() {
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
        };
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
