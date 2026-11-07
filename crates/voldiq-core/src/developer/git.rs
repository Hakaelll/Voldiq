//! Git storage accounting from indexed names and file lengths. No repository files are opened.

use crate::{Entry, Error, Kind, NodeId, Result, index::Index};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Deserialize)]
pub struct Query {
    pub scope: NodeId,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Serialize)]
pub struct Report {
    pub total: usize,
    pub items: Vec<Item>,
    pub coverage_partial: bool,
    pub git_file_markers: usize,
    pub shared_store_markers: usize,
}

#[derive(Serialize)]
pub struct Item {
    pub id: NodeId,
    pub store_id: NodeId,
    pub path: String,
    pub loose_bytes: String,
    pub pack_bytes: String,
    pub pack_metadata_bytes: String,
    pub lfs_bytes: Option<String>,
    pub total_bytes: String,
    pub partial: bool,
    pub alternates: bool,
}

#[derive(Default)]
struct Evidence {
    head: bool,
    refs: bool,
    shared: bool,
    objects: Option<NodeId>,
    lfs: Option<NodeId>,
}

#[derive(Default)]
struct Bytes {
    loose: u128,
    pack: u128,
    metadata: u128,
    lfs: u128,
}

impl Bytes {
    fn total(&self) -> u128 {
        self.loose + self.pack + self.metadata + self.lfs
    }
}

struct Store {
    id: NodeId,
    store_id: NodeId,
    has_lfs: bool,
    bytes: Bytes,
    partial: bool,
    alternates: bool,
}

pub fn query(index: &Index, query: &Query, cancelled: &AtomicBool) -> Result<Option<Report>> {
    let scope = index.entry(query.scope)?;
    if scope.kind != Kind::Directory || !(1..=100).contains(&query.limit) {
        return Err(Error::Invalid(
            "Invalid Git storage scope or page size.".into(),
        ));
    }

    let mut markers = HashMap::<NodeId, Evidence>::new();
    let mut git_file_markers = 0;
    for entry in &index.entries {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if !matches!(
            entry.name.as_str(),
            ".git" | "HEAD" | "refs" | "reftable" | "packed-refs" | "commondir" | "objects" | "lfs"
        ) || !index.in_scope(entry.id, query.scope)
        {
            continue;
        }
        if entry.name == ".git" && entry.kind != Kind::Directory {
            git_file_markers += 1;
        }
        let Some(parent) = entry.parent.filter(|_| entry.id != query.scope) else {
            continue;
        };
        let evidence = markers.entry(parent).or_default();
        match (entry.name.as_str(), entry.kind) {
            ("HEAD", Kind::File) if !entry.incomplete => evidence.head = true,
            ("refs" | "reftable", Kind::Directory) | ("packed-refs", Kind::File) => {
                evidence.refs = true;
            }
            ("commondir", Kind::File | Kind::Link) => evidence.shared = true,
            ("objects", Kind::Directory) => evidence.objects = Some(entry.id),
            ("lfs", Kind::Directory) => evidence.lfs = Some(entry.id),
            _ => {}
        }
    }

    let mut stores = Vec::new();
    let mut git_objects = HashMap::new();
    let mut lfs_objects = HashMap::new();
    let mut shared_store_markers = 0;
    for (id, evidence) in &markers {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if !evidence.head {
            continue;
        }
        if evidence.shared {
            shared_store_markers += 1;
            continue;
        }
        let Some(objects) = evidence.objects.filter(|_| evidence.refs) else {
            continue;
        };
        let directory = index.entry(*id)?;
        let lfs = evidence.lfs.and_then(|root| markers.get(&root)?.objects);
        git_objects.insert(objects, stores.len());
        if let Some(lfs) = lfs {
            lfs_objects.insert(lfs, stores.len());
        }
        let project = directory
            .parent
            .filter(|_| directory.name == ".git" && *id != query.scope)
            .unwrap_or(*id);
        stores.push(Store {
            id: project,
            store_id: *id,
            has_lfs: lfs.is_some(),
            bytes: Bytes::default(),
            partial: directory.incomplete
                || index.entry(objects)?.incomplete
                || lfs.is_some_and(|id| index.entry(id).is_ok_and(|root| root.incomplete)),
            alternates: false,
        });
    }

    if !stores.is_empty() {
        for entry in &index.entries {
            if cancelled.load(Ordering::Relaxed) {
                return Ok(None);
            }
            if entry.kind == Kind::Directory {
                continue;
            }
            let Some(parent) = entry.parent.and_then(|id| index.entry(id).ok()) else {
                continue;
            };
            let Some(grandparent) = parent.parent.and_then(|id| index.entry(id).ok()) else {
                continue;
            };
            if let Some(&owner) = git_objects.get(&grandparent.id) {
                let store = &mut stores[owner];
                if parent.name == "info"
                    && matches!(entry.name.as_str(), "alternates" | "http-alternates")
                {
                    store.alternates = true;
                }
                if let Some(bytes) = git_bytes(&mut store.bytes, parent, &entry.name) {
                    count(entry, bytes, &mut store.partial);
                }
            }
            if !lfs_objects.is_empty()
                && let Some(owner) = grandparent.parent.and_then(|id| lfs_objects.get(&id))
                && oid_part(&parent.name, &[2])
                && oid_part(&grandparent.name, &[2])
                && oid_part(&entry.name, &[64])
                && entry.name.starts_with(&grandparent.name)
                && entry.name[2..].starts_with(&parent.name)
            {
                let store = &mut stores[*owner];
                count(entry, &mut store.bytes.lfs, &mut store.partial);
            }
        }
    }

    stores.sort_unstable_by_key(|store| (Reverse(store.bytes.total()), store.store_id.0));
    let total = stores.len();
    let mut items = Vec::new();
    for store in stores.into_iter().skip(query.offset).take(query.limit) {
        let Some(path) =
            index.path_cancellable(store.store_id, || cancelled.load(Ordering::Relaxed))?
        else {
            return Ok(None);
        };
        items.push(Item {
            id: store.id,
            store_id: store.store_id,
            path: path.to_string_lossy().into_owned(),
            loose_bytes: store.bytes.loose.to_string(),
            pack_bytes: store.bytes.pack.to_string(),
            pack_metadata_bytes: store.bytes.metadata.to_string(),
            lfs_bytes: store.has_lfs.then(|| store.bytes.lfs.to_string()),
            total_bytes: store.bytes.total().to_string(),
            partial: store.partial,
            alternates: store.alternates,
        });
    }
    Ok(Some(Report {
        total,
        items,
        coverage_partial: scope.incomplete || !index.exclusions.is_empty(),
        git_file_markers,
        shared_store_markers,
    }))
}

fn oid_part(name: &str, lengths: &[usize]) -> bool {
    lengths.contains(&name.len()) && name.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn git_bytes<'a>(bytes: &'a mut Bytes, directory: &Entry, name: &str) -> Option<&'a mut u128> {
    if oid_part(&directory.name, &[2]) && oid_part(name, &[38, 62]) {
        return Some(&mut bytes.loose);
    }
    if directory.name != "pack" {
        return None;
    }
    if name == "multi-pack-index" {
        return Some(&mut bytes.metadata);
    }
    let (oid, extension) = name.strip_prefix("pack-")?.split_once('.')?;
    if !oid_part(oid, &[40, 64]) {
        return None;
    }
    match extension {
        "pack" => Some(&mut bytes.pack),
        "idx" | "rev" | "bitmap" | "mtimes" | "promisor" | "keep" => Some(&mut bytes.metadata),
        _ => None,
    }
}

fn count(entry: &Entry, total: &mut u128, partial: &mut bool) {
    if entry.kind == Kind::File && !entry.incomplete {
        *total += u128::from(entry.logical);
    } else {
        *partial = true;
    }
}
