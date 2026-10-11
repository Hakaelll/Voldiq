use crate::{Error, Issue, Kind, NodeId, Result, scanner::SharedIndex, windows};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Instant, SystemTime},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    identity: (u64, u64),
    size: u64,
    modified: SystemTime,
    change_time: u64,
}

struct CachedHash {
    stamp: Stamp,
    sample: blake3::Hash,
    full: Option<blake3::Hash>,
}

#[derive(Default)]
pub struct HashCache(HashMap<(u64, u64), CachedHash>);

#[derive(Clone, Debug, Serialize)]
pub struct Member {
    pub id: NodeId,
    pub allocated: u64,
    pub links: u32,
}

#[derive(Debug, Serialize)]
pub struct Group {
    pub digest: String,
    pub logical: u64,
    pub recoverable: u64,
    pub members: Vec<Member>,
}

#[derive(Default, Debug, Serialize)]
pub struct Analysis {
    pub groups: Vec<Group>,
    pub issues: Vec<Issue>,
    pub errors: u64,
    pub cancelled: bool,
    pub bytes_read: u64,
    pub cache_hits: u64,
    pub examined: u64,
}

#[derive(Clone, Serialize)]
pub struct Progress {
    pub phase: String,
    pub examined: u64,
    pub candidates: usize,
    pub bytes_read: u64,
    pub cache_hits: u64,
    pub errors: u64,
    pub elapsed_ms: u64,
    pub message: String,
}

struct Candidate {
    member: Member,
    path: PathBuf,
    stamp: Stamp,
    sample: blake3::Hash,
}

pub fn find(
    index: &SharedIndex,
    scope: NodeId,
    min_size: u64,
    cancel: &AtomicBool,
    cache: &mut HashCache,
    mut progress: impl FnMut(Progress),
) -> Result<Analysis> {
    let start = Instant::now();
    let mut sizes = BTreeMap::<u64, Vec<NodeId>>::new();
    {
        let index = index.read().map_err(|e| Error::Invalid(e.to_string()))?;
        index.entry(scope)?;
        for entry in &index.entries {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if entry.kind == Kind::File
                && entry.logical >= min_size
                && index.in_scope(entry.id, scope)
            {
                sizes.entry(entry.logical).or_default().push(entry.id);
            }
        }
    }
    sizes.retain(|_, ids| ids.len() > 1);
    let total = sizes.values().map(Vec::len).sum();
    let mut result = Analysis::default();
    let mut seen = HashSet::new();
    let emit = |phase: &str, result: &Analysis, progress: &mut dyn FnMut(Progress)| {
        progress(Progress {
            phase: phase.into(),
            examined: result.examined,
            candidates: total,
            bytes_read: result.bytes_read,
            cache_hits: result.cache_hits,
            errors: result.errors,
            elapsed_ms: start.elapsed().as_millis() as u64,
            message: String::new(),
        });
    };
    emit("sampling", &result, &mut progress);
    let mut last_report = Instant::now();
    'analysis: for (size, ids) in sizes {
        let mut samples = HashMap::<blake3::Hash, Vec<Candidate>>::new();
        for id in ids {
            if cancel.load(Ordering::Relaxed) {
                break 'analysis;
            }
            let path = index
                .read()
                .map_err(|e| Error::Invalid(e.to_string()))?
                .path(id)?;
            match sample(&path, id, size, cache, &mut result.bytes_read) {
                Ok(candidate) => {
                    if seen.insert(candidate.stamp.identity) {
                        samples.entry(candidate.sample).or_default().push(candidate);
                    }
                }
                Err(error) => record_issue(&mut result, &path, error),
            }
            result.examined += 1;
            if last_report.elapsed().as_millis() >= 100 {
                emit("sampling", &result, &mut progress);
                last_report = Instant::now();
            }
        }
        for candidates in samples.into_values().filter(|files| files.len() > 1) {
            emit("hashing", &result, &mut progress);
            let mut hashes = HashMap::<blake3::Hash, Vec<Member>>::new();
            for candidate in candidates {
                if cancel.load(Ordering::Relaxed) {
                    break 'analysis;
                }
                let mut read_total = 0;
                let mut report_bytes = |bytes_read| {
                    read_total = bytes_read;
                    if last_report.elapsed().as_millis() >= 100 {
                        let mut update = Progress {
                            phase: "hashing".into(),
                            examined: result.examined,
                            candidates: total,
                            bytes_read: result.bytes_read,
                            cache_hits: result.cache_hits,
                            errors: result.errors,
                            elapsed_ms: start.elapsed().as_millis() as u64,
                            message: String::new(),
                        };
                        update.bytes_read += bytes_read;
                        progress(update);
                        last_report = Instant::now();
                    }
                };
                let hashed = full_hash(&candidate, cancel, cache, &mut report_bytes);
                result.bytes_read += read_total;
                match hashed {
                    Ok(Some((hash, _, cached))) => {
                        result.cache_hits += u64::from(cached);
                        hashes.entry(hash).or_default().push(candidate.member);
                    }
                    Ok(None) => {
                        break 'analysis;
                    }
                    Err(error) => record_issue(&mut result, &candidate.path, error),
                }
            }
            for (hash, mut members) in hashes.into_iter().filter(|(_, members)| members.len() > 1) {
                members.sort_unstable_by_key(|item| item.id.0);
                let allocations: Vec<_> = members
                    .iter()
                    .filter(|m| m.links == 1)
                    .map(|m| m.allocated)
                    .collect();
                let recoverable = allocations.iter().sum::<u64>()
                    - allocations.iter().max().copied().unwrap_or(0);
                result.groups.push(Group {
                    digest: hash.to_hex().to_string(),
                    logical: size,
                    recoverable,
                    members,
                });
            }
        }
    }
    result.cancelled = cancel.load(Ordering::Relaxed);
    result.groups.sort_unstable_by(|a, b| {
        b.recoverable
            .cmp(&a.recoverable)
            .then(a.digest.cmp(&b.digest))
    });
    emit(
        if result.cancelled {
            "cancelled"
        } else {
            "complete"
        },
        &result,
        &mut progress,
    );
    Ok(result)
}

fn open_content(path: &Path) -> Result<File> {
    validate_content_metadata(&std::fs::symlink_metadata(path)?)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options.share_mode(FILE_SHARE_READ).custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL | FILE_FLAG_SEQUENTIAL_SCAN,
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    validate_content_metadata(&file.metadata()?)?;
    Ok(file)
}

fn validate_content_metadata(metadata: &std::fs::Metadata) -> Result<()> {
    if windows::requires_recall(metadata) {
        return Err(Error::Invalid(
            "Offline file excluded from content reads.".into(),
        ));
    }
    if !metadata.is_file() || windows::is_reparse(metadata) {
        return Err(Error::Invalid(
            "Reparse points and non-files are excluded from content reads.".into(),
        ));
    }
    Ok(())
}

fn stamp(file: &File) -> Result<(Stamp, windows::FileIdentity)> {
    let metadata = file.metadata()?;
    let identity = windows::file_identity(file)?;
    Ok((
        Stamp {
            identity: identity.key,
            size: metadata.len(),
            modified: metadata.modified()?,
            change_time: identity.change_time,
        },
        identity,
    ))
}

fn sample(
    path: &Path,
    id: NodeId,
    size: u64,
    cache: &mut HashCache,
    bytes_read: &mut u64,
) -> Result<Candidate> {
    let mut file = open_content(path)?;
    let (before, identity) = stamp(&file)?;
    if before.size != size {
        return Err(Error::Invalid(
            "File size changed since scanning. Rescan this folder.".into(),
        ));
    }
    let cached = cache
        .0
        .get(&before.identity)
        .filter(|entry| entry.stamp == before);
    let sample = if let Some(cached) = cached {
        cached.sample
    } else {
        let mut bytes = vec![0; size.min(8192) as usize];
        if size <= 8192 {
            file.read_exact(&mut bytes)?;
        } else {
            file.read_exact(&mut bytes[..4096])?;
            file.seek(SeekFrom::End(-4096))?;
            file.read_exact(&mut bytes[4096..])?;
        }
        *bytes_read += bytes.len() as u64;
        if stamp(&file)?.0 != before {
            return Err(Error::Invalid("File changed during sampling.".into()));
        }
        let hash = blake3::hash(&bytes);
        // shortcut: cache at most 250,000 identities in memory; persist validated stamps with the snapshot store later.
        if cache.0.len() >= 250_000 {
            cache.0.clear();
        }
        cache.0.insert(
            before.identity,
            CachedHash {
                stamp: before.clone(),
                sample: hash,
                full: (size <= 8192).then_some(hash),
            },
        );
        hash
    };
    Ok(Candidate {
        member: Member {
            id,
            allocated: identity.allocated,
            links: identity.links,
        },
        path: path.to_owned(),
        stamp: before,
        sample,
    })
}

fn full_hash(
    candidate: &Candidate,
    cancel: &AtomicBool,
    cache: &mut HashCache,
    progress: &mut impl FnMut(u64),
) -> Result<Option<(blake3::Hash, u64, bool)>> {
    let mut file = open_content(&candidate.path)?;
    if stamp(&file)?.0 != candidate.stamp {
        return Err(Error::Invalid(
            "File changed between sampling and verification.".into(),
        ));
    }
    if let Some(hash) = cache
        .0
        .get(&candidate.stamp.identity)
        .filter(|entry| entry.stamp == candidate.stamp)
        .and_then(|entry| entry.full)
    {
        return Ok(Some((hash, 0, true)));
    }
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut count = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hasher.update(&buffer[..length]);
        count += length as u64;
        progress(count);
    }
    if count != candidate.stamp.size || stamp(&file)?.0 != candidate.stamp {
        return Err(Error::Invalid("File changed during hashing.".into()));
    }
    let hash = hasher.finalize();
    if cache.0.len() >= 250_000 {
        cache.0.clear();
    }
    cache.0.insert(
        candidate.stamp.identity,
        CachedHash {
            stamp: candidate.stamp.clone(),
            sample: candidate.sample,
            full: Some(hash),
        },
    );
    Ok(Some((hash, count, false)))
}

fn record_issue(result: &mut Analysis, path: &Path, error: Error) {
    result.errors += 1;
    if result.issues.len() < 1000 {
        result.issues.push(Issue {
            path: path.to_string_lossy().into_owned(),
            message: error.to_string(),
        });
    }
}
