use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};
use voldiq_core::{
    NodeId, ScanOptions,
    duplicates::{HashCache, find},
    scanner::{SharedIndex, scan},
};

fn index(path: &std::path::Path) -> SharedIndex {
    Arc::new(RwLock::new(
        scan(
            path,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap(),
    ))
}

#[test]
fn verifies_full_content_skips_unique_sizes_and_reuses_valid_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let same = vec![7; 16384];
    let mut different = same.clone();
    different[8192] = 4;
    std::fs::write(dir.path().join("a"), &same).unwrap();
    std::fs::write(dir.path().join("b"), &same).unwrap();
    std::fs::write(dir.path().join("c"), &different).unwrap();
    std::fs::write(dir.path().join("unique"), [1; 8193]).unwrap();
    let index = index(dir.path());
    let mut cache = HashCache::default();
    let result = find(
        &index,
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut cache,
        |_| {},
    )
    .unwrap();
    assert_eq!(result.groups.len(), 1);
    assert_eq!(result.groups[0].members.len(), 2);
    assert_eq!(
        result.groups[0].digest,
        blake3::hash(&same).to_hex().to_string()
    );
    assert_eq!(result.groups[0].recoverable, 16384);
    assert_eq!(result.examined, 3);
    assert!(result.bytes_read > same.len() as u64 * 2);
    let cached = find(
        &index,
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut cache,
        |_| {},
    )
    .unwrap();
    assert_eq!(cached.groups.len(), 1);
    assert_eq!(cached.bytes_read, 0);
    assert_eq!(cached.cache_hits, 3);
    std::fs::write(dir.path().join("b"), &different).unwrap();
    let changed = find(
        &index,
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut cache,
        |_| {},
    )
    .unwrap();
    assert_eq!(changed.groups.len(), 1);
    assert_eq!(
        changed.groups[0].digest,
        blake3::hash(&different).to_hex().to_string()
    );
    assert!(changed.bytes_read > 0);
}

#[test]
fn hard_links_are_not_duplicates_or_recoverable_allocation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), [8; 8192]).unwrap();
    std::fs::hard_link(dir.path().join("a"), dir.path().join("alias")).unwrap();
    let mut cache = HashCache::default();
    let linked = find(
        &index(dir.path()),
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut cache,
        |_| {},
    )
    .unwrap();
    assert!(linked.groups.is_empty());
    std::fs::write(dir.path().join("copy"), [8; 8192]).unwrap();
    let copy = find(
        &index(dir.path()),
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut cache,
        |_| {},
    )
    .unwrap();
    assert_eq!(copy.groups.len(), 1);
    assert_eq!(copy.groups[0].members.len(), 2);
    assert_eq!(copy.groups[0].recoverable, 0);
}

#[test]
fn vanished_changed_and_cancelled_candidates_do_not_become_false_matches() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), [1; 32]).unwrap();
    std::fs::write(dir.path().join("b"), [1; 32]).unwrap();
    let index = index(dir.path());
    std::fs::remove_file(dir.path().join("b")).unwrap();
    let mut cache = HashCache::default();
    let result = find(
        &index,
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut cache,
        |_| {},
    )
    .unwrap();
    assert_eq!(result.errors, 1);
    assert!(result.groups.is_empty());
    let cancel = AtomicBool::new(false);
    let result = find(&index, NodeId(0), 0, &cancel, &mut cache, |_| {
        cancel.store(true, Ordering::Relaxed)
    })
    .unwrap();
    assert!(result.cancelled);
    assert!(result.groups.is_empty());
    assert_eq!(result.bytes_read, 0);
}

#[test]
fn different_samples_avoid_full_reads_and_hashing_can_be_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), vec![1; 16384]).unwrap();
    std::fs::write(dir.path().join("b"), vec![2; 16384]).unwrap();
    let mut cache = HashCache::default();
    let result = find(
        &index(dir.path()),
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut cache,
        |_| {},
    )
    .unwrap();
    assert!(result.groups.is_empty());
    assert_eq!(result.bytes_read, 2 * 8192);
    std::fs::write(dir.path().join("b"), vec![1; 16384]).unwrap();
    let cancel = AtomicBool::new(false);
    let result = find(
        &index(dir.path()),
        NodeId(0),
        0,
        &cancel,
        &mut cache,
        |update| {
            if update.phase == "hashing" {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    )
    .unwrap();
    assert!(result.cancelled);
    assert!(result.groups.is_empty());
    assert!(result.bytes_read <= 2 * 8192);
}

#[cfg(windows)]
#[test]
fn locked_content_is_reported_without_a_false_match() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), [1; 32]).unwrap();
    std::fs::write(dir.path().join("b"), [1; 32]).unwrap();
    let index = index(dir.path());
    let _lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(dir.path().join("b"))
        .unwrap();
    let result = find(
        &index,
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut HashCache::default(),
        |_| {},
    )
    .unwrap();
    assert_eq!(result.errors, 1);
    assert!(result.groups.is_empty());
}
