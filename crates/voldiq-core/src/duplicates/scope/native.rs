use super::{Report, ScopeRequest};
use crate::{
    Entry, Error, Kind, NodeId, Result,
    agents::AuthorizedPath,
    cloud::CloudState,
    duplicates::{
        SAMPLE_BYTES, sample_hash, stream_hash,
        verification::{VerifiedGroup, open_recorded_file},
    },
    index::Index,
    metadata::{Eligibility, Protections},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_IDENTITIES: usize = 250_000;
const MAX_READ_BYTES: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Serialize)]
pub(crate) struct AnalysisProgress {
    pub phase: &'static str,
    pub candidates: usize,
    pub sampled: usize,
    pub hashed: usize,
    pub bytes_read: u64,
}

pub(crate) fn analyze(
    index: &Index,
    request: &ScopeRequest,
    protections: &Protections,
    cancel: &AtomicBool,
    authorize: impl Fn(NodeId) -> Result<AuthorizedPath>,
    mut observe: impl FnMut(&AnalysisProgress) -> Result<()>,
) -> Result<Option<Report>> {
    let scope = request.folder();
    let folder = index.entry(scope)?;
    if !request.valid() || folder.kind != Kind::Directory {
        return Err(Error::Invalid(
            "Choose an indexed folder and valid minimum size.".into(),
        ));
    }
    let cancelled = || cancel.load(Ordering::Relaxed);
    let mask = match protections.for_index(index, cancel) {
        Ok(mask) => mask,
        Err(_) if cancelled() => return Ok(None),
        Err(error) => return Err(error),
    };
    let system = crate::cleanup::protected_identities()?;
    let minimum = request
        .min_bytes
        .as_deref()
        .and_then(super::bytes)
        .unwrap_or(1);
    let mut by_size = BTreeMap::<u64, Vec<&Entry>>::new();
    let mut seen = HashSet::new();
    let mut aliases_omitted = 0;
    let mut unavailable_files = 0;
    let mut progress = AnalysisProgress {
        phase: "selecting",
        candidates: 0,
        sampled: 0,
        hashed: 0,
        bytes_read: 0,
    };
    for entry in &index.entries {
        observe(&progress)?;
        if cancelled() {
            return Ok(None);
        }
        if !entry.is_metadata_file()
            || entry.logical < minimum
            || !index.in_scope(entry.id, scope)
            || mask.eligibility(entry.id)? == Eligibility::Protected
        {
            continue;
        }
        let Some(recorded) = entry.file_state.filter(|_| {
            entry.kind == Kind::File
                && !entry.incomplete
                && entry.cloud == CloudState::NotPlaceholder
                && mask.allows(entry.id)
        }) else {
            unavailable_files += 1;
            continue;
        };
        if !seen.insert(recorded.identity) {
            aliases_omitted += 1;
            continue;
        }
        if seen.len() > MAX_IDENTITIES {
            return Err(Error::DuplicateScopeLimit);
        }
        by_size.entry(entry.logical).or_default().push(entry);
    }
    by_size.retain(|_, entries| entries.len() > 1);
    progress.candidates = by_size.values().map(Vec::len).sum();
    let mut groups = Vec::new();
    for (size, entries) in by_size.into_iter().rev() {
        progress.phase = "sampling";
        let mut samples = HashMap::<blake3::Hash, Vec<&Entry>>::new();
        for entry in entries {
            observe(&progress)?;
            if cancelled() {
                return Ok(None);
            }
            let Ok(mut file) = authorize(entry.id)
                .and_then(|path| open_recorded_file(entry, path, protections, &system))
            else {
                unavailable_files += 1;
                continue;
            };
            check_budget(progress.bytes_read, size.min(SAMPLE_BYTES))?;
            let digest = sample_hash(&mut file.handle, &file.stamp, &mut progress.bytes_read)?;
            progress.sampled += 1;
            samples.entry(digest).or_default().push(entry);
        }
        progress.phase = "hashing";
        let mut matches = HashMap::<blake3::Hash, Vec<NodeId>>::new();
        for (sample, entries) in samples.into_iter().filter(|(_, entries)| entries.len() > 1) {
            for entry in entries {
                observe(&progress)?;
                if cancelled() {
                    return Ok(None);
                }
                let digest = if size <= SAMPLE_BYTES {
                    sample
                } else {
                    let Ok(mut file) = authorize(entry.id)
                        .and_then(|path| open_recorded_file(entry, path, protections, &system))
                    else {
                        unavailable_files += 1;
                        continue;
                    };
                    let baseline = progress.bytes_read;
                    check_budget(baseline, size)?;
                    let mut observation_error = None;
                    let hash = stream_hash(&mut file.handle, &file.stamp, cancel, |read| {
                        progress.bytes_read = baseline + read;
                        if observation_error.is_none()
                            && let Err(error) = observe(&progress)
                        {
                            observation_error = Some(error);
                            cancel.store(true, Ordering::Relaxed);
                        }
                    });
                    if let Some(error) = observation_error {
                        return Err(error);
                    }
                    let Some((digest, _)) = hash? else {
                        return Ok(None);
                    };
                    progress.hashed += 1;
                    digest
                };
                matches.entry(digest).or_default().push(entry.id);
            }
        }
        let mut bucket = matches
            .into_values()
            .filter(|ids| ids.len() > 1)
            .collect::<Vec<_>>();
        for ids in &mut bucket {
            ids.sort_unstable_by_key(|node| node.0);
        }
        bucket.sort_unstable_by_key(|ids| ids[0].0);
        groups.extend(bucket.into_iter().map(|node_ids| VerifiedGroup {
            logical_bytes: size.to_string(),
            node_ids,
        }));
    }
    progress.phase = "finished";
    observe(&progress)?;
    if cancelled() {
        return Ok(None);
    }
    // The report describes exact bytes observed while the recorded files were pinned.
    Ok(Some(Report {
        observed_at: crate::plans::now()?,
        compared_files: progress.sampled,
        aliases_omitted,
        unavailable_files,
        unverified_branches: mask.unverified_branches(scope)?,
        recorded_complete: index
            .observation
            .as_ref()
            .is_some_and(|observation| observation.complete)
            && !folder.incomplete,
        bytes_read: progress.bytes_read.to_string(),
        groups,
    }))
}

fn check_budget(read: u64, upcoming: u64) -> Result<()> {
    if upcoming > MAX_READ_BYTES.saturating_sub(read) {
        return Err(Error::DuplicateScopeLimit);
    }
    Ok(())
}
