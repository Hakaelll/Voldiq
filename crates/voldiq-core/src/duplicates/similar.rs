//! Bounded visual comparisons. A match carries no exact-duplicate or cleanup authority.

mod image;

use crate::{
    Error, Kind, NodeId, Result,
    index::Index,
    inspection,
    metadata::{Eligibility, Protections},
    scanner::SharedIndex,
};
use serde::Serialize;
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

const CANDIDATE_LIMIT: usize = 512;
const FILE_BYTES: u64 = 32 * 1024 * 1024;
const BATCH_BYTES: u64 = 256 * 1024 * 1024;
const PAIR_LIMIT: usize = 100;
const MAX_DISTANCE: u8 = 16;

#[derive(Debug, Serialize)]
pub struct Report {
    pub scope: String,
    pub incomplete_scope: bool,
    pub limit: Option<Limit>,
    pub compared_images: usize,
    pub unavailable_images: usize,
    pub protected_images: usize,
    pub repeated_images: usize,
    pub unverified_branches: usize,
    pub matching_pairs: usize,
    pub pairs: Vec<Pair>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Limit {
    Files,
    Bytes,
}

#[derive(Debug, Serialize)]
pub struct Pair {
    pub ids: [NodeId; 2],
    pub names: [String; 2],
    pub distance: u32,
}

#[derive(Debug, Serialize)]
pub struct Comparison {
    pub distance: u32,
    pub images: [Preview; 2],
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub id: NodeId,
    pub name: String,
    pub path: String,
    pub logical: u64,
    pub modified: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub png: String,
}

fn read_index(index: &SharedIndex) -> Result<std::sync::RwLockReadGuard<'_, Index>> {
    index
        .read()
        .map_err(|error| Error::Invalid(error.to_string()))
}

fn check_scope(index: &Index, scope: NodeId) -> Result<()> {
    if index.snapshot_id.is_some() || index.entry(scope)?.kind != Kind::Directory {
        return Err(Error::Invalid(
            "Image comparison requires a live indexed folder.".into(),
        ));
    }
    Ok(())
}

fn cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

pub fn find(
    shared: &SharedIndex,
    scope: NodeId,
    max_distance: u8,
    cancel: &AtomicBool,
    protections: &Protections,
) -> Result<Option<Report>> {
    if max_distance > MAX_DISTANCE {
        return Err(Error::Invalid("Set a visual distance from 0 to 16.".into()));
    }
    if cancelled(cancel) {
        return Ok(None);
    }
    let (candidates, mut report) = {
        let index = read_index(shared)?;
        check_scope(&index, scope)?;
        let mask = protections.for_index(&index, cancel)?;
        let mut report = Report {
            scope: index.path(scope)?.to_string_lossy().into_owned(),
            incomplete_scope: index.entry(scope)?.incomplete,
            limit: None,
            compared_images: 0,
            unavailable_images: 0,
            protected_images: 0,
            repeated_images: 0,
            unverified_branches: mask.unverified_branches(scope)?,
            matching_pairs: 0,
            pairs: Vec::new(),
        };
        let mut candidates = Vec::new();
        let mut bytes = 0;
        let mut identities = HashSet::new();
        for entry in &index.entries {
            if cancelled(cancel) {
                return Ok(None);
            }
            let Some(format) = image::Format::of(&entry.name) else {
                continue;
            };
            if !entry.is_metadata_file() || !index.in_scope(entry.id, scope) {
                continue;
            }
            match mask.eligibility(entry.id)? {
                Eligibility::Allowed => {}
                Eligibility::Protected => {
                    report.protected_images += 1;
                    continue;
                }
                Eligibility::Unverified => {
                    report.unavailable_images += 1;
                    continue;
                }
            }
            let Some(state) = entry.file_state.filter(|_| {
                entry.kind == Kind::File
                    && !entry.incomplete
                    && !entry.cloud.is_placeholder()
                    && (1..=FILE_BYTES).contains(&entry.logical)
            }) else {
                report.unavailable_images += 1;
                continue;
            };
            if !identities.insert(state.identity) {
                report.repeated_images += 1;
                continue;
            }
            if candidates.len() == CANDIDATE_LIMIT {
                report.limit = Some(Limit::Files);
                break;
            }
            if bytes + entry.logical > BATCH_BYTES {
                report.limit = Some(Limit::Bytes);
                break;
            }
            bytes += entry.logical;
            candidates.push((entry.id, entry.name.clone(), entry.logical as usize, format));
        }
        (candidates, report)
    };
    if candidates.is_empty() {
        return Ok(Some(report));
    }
    let decoder = image::Decoder::new()?;
    let mut images = Vec::with_capacity(candidates.len());
    let mut encoded = HashSet::new();
    for (id, name, limit, format) in candidates {
        if cancelled(cancel) {
            return Ok(None);
        }
        let input = inspection::read_indexed_prefix(&*read_index(shared)?, id, limit);
        let Ok(input) = input else {
            report.unavailable_images += 1;
            continue;
        };
        if cancelled(cancel) {
            return Ok(None);
        }
        let digest = blake3::hash(&input);
        if encoded.contains(&digest) {
            report.repeated_images += 1;
            continue;
        }
        match decoder.decode(&input, format, false) {
            Ok(image) => {
                encoded.insert(digest);
                images.push((id, name, image.hash));
            }
            Err(_) => report.unavailable_images += 1,
        }
    }
    report.compared_images = images.len();
    // Keep only enough pair indices at each distance to fill the final sorted page.
    let mut buckets = vec![Vec::new(); usize::from(max_distance) + 1];
    for (left, a) in images.iter().enumerate() {
        if cancelled(cancel) {
            return Ok(None);
        }
        for (right, b) in images.iter().enumerate().skip(left + 1) {
            let distance = (a.2 ^ b.2).count_ones();
            if let Some(bucket) = buckets.get_mut(distance as usize) {
                report.matching_pairs += 1;
                if bucket.len() < PAIR_LIMIT {
                    bucket.push((left, right, distance));
                }
            }
        }
    }
    report.pairs = buckets
        .into_iter()
        .flatten()
        .take(PAIR_LIMIT)
        .map(|(left, right, distance)| {
            let (a, b) = (&images[left], &images[right]);
            Pair {
                ids: [a.0, b.0],
                names: [a.1.clone(), b.1.clone()],
                distance,
            }
        })
        .collect();
    Ok((!cancelled(cancel)).then_some(report))
}

pub fn compare(
    shared: &SharedIndex,
    scope: NodeId,
    ids: [NodeId; 2],
    cancel: &AtomicBool,
    protections: &Protections,
) -> Result<Option<Comparison>> {
    if ids[0] == ids[1] {
        return Err(Error::Invalid("Choose two distinct indexed images.".into()));
    }
    if cancelled(cancel) {
        return Ok(None);
    }
    let [first, second] = {
        let index = read_index(shared)?;
        check_scope(&index, scope)?;
        let mask = protections.for_index(&index, cancel)?;
        let load = |id| {
            let entry = index.entry(id)?;
            let format = image::Format::of(&entry.name)
                .filter(|_| {
                    index.in_scope(id, scope)
                        && mask.allows(id)
                        && (1..=FILE_BYTES).contains(&entry.logical)
                })
                .ok_or_else(|| {
                    Error::Invalid(
                        "Image is outside the current comparison scope or limits.".into(),
                    )
                })?;
            let path = index.path(id)?;
            let input = inspection::read_indexed_prefix(&index, id, entry.logical as usize)?;
            Ok::<_, Error>((entry.clone(), path, format, input))
        };
        let first = load(ids[0])?;
        if cancelled(cancel) {
            return Ok(None);
        }
        [first, load(ids[1])?]
    };
    if cancelled(cancel) {
        return Ok(None);
    }
    let decoder = image::Decoder::new()?;
    let read = |(entry, path, format, input): (
        crate::Entry,
        std::path::PathBuf,
        image::Format,
        Vec<u8>,
    )| {
        let image = decoder.decode(&input, format, true)?;
        let path = crate::agents::ordinary_path(&path).unwrap_or(path);
        Ok::<_, Error>((
            image.hash,
            Preview {
                id: entry.id,
                name: entry.name,
                path: path.to_string_lossy().into_owned(),
                logical: entry.logical,
                modified: entry.modified,
                width: image.width,
                height: image.height,
                png: image.png,
            },
        ))
    };
    let (left, a) = read(first)?;
    if cancelled(cancel) {
        return Ok(None);
    }
    let (right, b) = read(second)?;
    if cancelled(cancel) {
        return Ok(None);
    }
    Ok(Some(Comparison {
        distance: (left ^ right).count_ones(),
        images: [a, b],
    }))
}
