#![cfg(windows)]

use std::sync::{Arc, RwLock, atomic::AtomicBool};
use voldiq_core::{
    NodeId, ScanOptions,
    cleanup::Plan,
    duplicates::{HashCache, find},
    scanner::scan,
};

#[test]
fn duplicate_review_preserves_a_real_copy_and_rejects_full_unknown_or_changed_groups() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "c"] {
        std::fs::write(dir.path().join(name), vec![5; 16_384]).unwrap();
    }
    std::fs::write(dir.path().join("unrelated"), [1; 3]).unwrap();
    for name in ["other-a", "other-b"] {
        std::fs::write(dir.path().join(name), vec![9; 32_768]).unwrap();
    }
    let cancel = AtomicBool::new(false);
    let index = Arc::new(RwLock::new(
        scan(dir.path(), &ScanOptions::default(), &cancel, |_| {}).unwrap(),
    ));
    let mut analysis = find(
        &index,
        NodeId(0),
        0,
        &cancel,
        &mut HashCache::default(),
        |_| {},
    )
    .unwrap();
    let position = analysis
        .groups
        .iter()
        .position(|g| g.members.len() == 3)
        .unwrap();
    let group = &analysis.groups[position];
    let ids: Vec<_> = group.members.iter().map(|m| m.id).collect();
    let guard = index.read().unwrap();
    let plan = Plan::duplicates(&guard, &analysis, &ids[..2], &cancel).unwrap();
    assert_eq!(plan.preview().rule, "duplicate-files");
    assert_eq!(plan.preview().items.len(), 2);
    assert_eq!(plan.preview().logical, 32_768);
    assert_eq!(
        plan.preview().retained,
        vec![guard.path(ids[2]).unwrap().to_string_lossy().into_owned()]
    );
    assert!(Plan::duplicates(&guard, &analysis, &ids, &cancel).is_err());
    let other = analysis
        .groups
        .iter()
        .find(|g| g.members.len() == 2)
        .unwrap();
    let full_other: Vec<_> = std::iter::once(ids[0])
        .chain(other.members.iter().map(|m| m.id))
        .collect();
    assert!(Plan::duplicates(&guard, &analysis, &full_other, &cancel).is_err());
    assert!(Plan::duplicates(&guard, &analysis, &[ids[0], ids[0]], &cancel).is_err());
    let unrelated = guard
        .entries
        .iter()
        .find(|e| e.name == "unrelated")
        .unwrap()
        .id;
    assert!(Plan::duplicates(&guard, &analysis, &[unrelated], &cancel).is_err());
    assert!(Plan::duplicates(&guard, &analysis, &[ids[0]], &AtomicBool::new(true)).is_err());
    let original_digest = analysis.groups[position].digest.clone();
    analysis.groups[position].digest = blake3::hash(b"wrong content").to_hex().to_string();
    assert!(Plan::duplicates(&guard, &analysis, &ids[..2], &cancel).is_err());
    analysis.groups[position].digest = original_digest;
    std::fs::write(guard.path(ids[2]).unwrap(), vec![6; 16_384]).unwrap();
    assert!(Plan::duplicates(&guard, &analysis, &ids[..2], &cancel).is_err());
}
