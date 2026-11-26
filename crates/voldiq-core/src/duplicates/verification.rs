use crate::NodeId;
use serde::Serialize;
use std::collections::HashSet;

pub fn valid_verification_selection(nodes: &[NodeId]) -> bool {
    (2..=64).contains(&nodes.len())
        && nodes.iter().all(|node| node.0 > 0)
        && nodes.iter().copied().collect::<HashSet<_>>().len() == nodes.len()
}

#[derive(Serialize)]
pub struct Verification {
    pub observed_at: i64,
    pub distinct_files: usize,
    pub compared_files: usize,
    pub aliases_omitted: usize,
    pub bytes_read: String,
    pub groups: Vec<VerifiedGroup>,
}

#[derive(Serialize)]
pub struct VerifiedGroup {
    pub logical_bytes: String,
    pub node_ids: Vec<NodeId>,
}

#[cfg(windows)]
mod native {
    use super::{Verification, VerifiedGroup, valid_verification_selection};
    use crate::{
        Error, Kind, NodeId, Result,
        agents::{AuthorizedPath, denied},
        cloud::CloudState,
        duplicates::{
            SAMPLE_BYTES, Stamp, sample_hash, stamp, stream_hash, validate_content_metadata,
        },
        index::Index,
        metadata::Protections,
    };
    use std::{
        collections::{BTreeMap, HashMap, HashSet},
        fs::File,
        sync::atomic::{AtomicBool, Ordering},
    };

    const MAX_READ_BYTES: u128 = 256 * 1024 * 1024;

    struct SelectedFile {
        node: NodeId,
        handle: File,
        stamp: Stamp,
        _path: AuthorizedPath,
    }

    pub(crate) fn verify_selection(
        index: &Index,
        nodes: &[NodeId],
        protections: &Protections,
        cancel: &AtomicBool,
        authorize: impl Fn(NodeId) -> Result<AuthorizedPath>,
        finish: impl FnOnce() -> Result<()>,
    ) -> Result<Option<Verification>> {
        if !valid_verification_selection(nodes) {
            return Err(Error::Invalid("Choose 2–64 distinct file IDs.".into()));
        }
        let cancelled = || cancel.load(Ordering::Relaxed);
        let mask = match protections.for_index(index, cancel) {
            Ok(mask) => mask,
            Err(_) if cancelled() => return Ok(None),
            Err(error) => return Err(error),
        };
        let protected = crate::cleanup::protected_identities()?;
        let mut selected = Vec::with_capacity(nodes.len());
        let mut sizes = BTreeMap::<u64, usize>::new();
        let mut seen = HashSet::new();
        for &node in nodes {
            if cancelled() {
                return Ok(None);
            }
            let entry = index.entry(node)?;
            if entry.kind != Kind::File
                || entry.incomplete
                || entry.cloud != CloudState::NotPlaceholder
                || !mask.allows(node)
            {
                return Err(denied());
            }
            let recorded = entry.file_state.ok_or_else(denied)?;
            let unique = seen.insert(recorded.identity);
            if unique {
                *sizes.entry(entry.logical).or_default() += 1;
            }
            selected.push((entry, recorded, unique));
        }
        let worst_case: u128 = sizes
            .iter()
            .filter(|&(_, count)| *count > 1)
            .map(|(&size, &count)| {
                let sample_overhead = if size > SAMPLE_BYTES { SAMPLE_BYTES } else { 0 };
                (u128::from(size) + u128::from(sample_overhead)) * count as u128
            })
            .sum();
        if worst_case > MAX_READ_BYTES {
            return Err(Error::DuplicateVerificationLimit);
        }

        let mut files = BTreeMap::<u64, Vec<SelectedFile>>::new();
        for (entry, recorded, unique) in selected {
            if cancelled() {
                return Ok(None);
            }
            let path = authorize(entry.id)?;
            if path.identity() != recorded.identity
                || crate::cleanup::record::sensitive(path.path())
            {
                return Err(denied());
            }
            let ancestors = path.ancestor_identities()?;
            if ancestors.iter().any(|id| protected.contains(id)) {
                return Err(denied());
            }
            protections.check_evidence(path.path(), path.identity(), &ancestors)?;
            let handle = path.open_file_content()?;
            validate_content_metadata(&handle.metadata()?)?;
            let (current, identity) = stamp(&handle)?;
            if current.identity != recorded.identity
                || current.change_time != recorded.change_time
                || current.size != entry.logical
                || identity.cloud != CloudState::NotPlaceholder
            {
                return Err(denied());
            }
            if unique {
                files.entry(current.size).or_default().push(SelectedFile {
                    node: entry.id,
                    handle,
                    stamp: current,
                    _path: path,
                });
            }
        }
        let mut bytes_read = 0;
        let mut compared_files = 0;
        let mut groups = Vec::new();
        for (&size, members) in files
            .iter_mut()
            .rev()
            .filter(|(_, members)| members.len() > 1)
        {
            let mut samples = HashMap::<blake3::Hash, Vec<&mut SelectedFile>>::new();
            for file in members {
                if cancelled() {
                    return Ok(None);
                }
                let sample = sample_hash(&mut file.handle, &file.stamp, &mut bytes_read)?;
                samples.entry(sample).or_default().push(file);
                compared_files += 1;
            }
            let mut equal = HashMap::<blake3::Hash, Vec<NodeId>>::new();
            for (sample, candidates) in samples
                .into_iter()
                .filter(|(_, candidates)| candidates.len() > 1)
            {
                for file in candidates {
                    if cancelled() {
                        return Ok(None);
                    }
                    let hash = if size <= SAMPLE_BYTES {
                        sample
                    } else {
                        let Some((digest, count)) =
                            stream_hash(&mut file.handle, &file.stamp, cancel, |_| {})?
                        else {
                            return Ok(None);
                        };
                        bytes_read += count;
                        digest
                    };
                    equal.entry(hash).or_default().push(file.node);
                }
            }
            let mut bucket = equal
                .into_values()
                .filter(|members| members.len() > 1)
                .collect::<Vec<_>>();
            for members in &mut bucket {
                members.sort_unstable_by_key(|node| node.0);
            }
            bucket.sort_unstable_by_key(|members| members[0].0);
            groups.extend(bucket.into_iter().map(|node_ids| VerifiedGroup {
                logical_bytes: size.to_string(),
                node_ids,
            }));
        }
        for file in files.values().flatten() {
            if cancelled() {
                return Ok(None);
            }
            validate_content_metadata(&file.handle.metadata()?)?;
            if stamp(&file.handle)?.0 != file.stamp {
                return Err(denied());
            }
        }
        let observed_at = crate::plans::now()?;
        finish()?;
        Ok((!cancelled()).then_some(Verification {
            observed_at,
            distinct_files: seen.len(),
            compared_files,
            aliases_omitted: nodes.len() - seen.len(),
            bytes_read: bytes_read.to_string(),
            groups,
        }))
    }
}

#[cfg(windows)]
pub(crate) use native::verify_selection;
