use std::{
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, UNIX_EPOCH},
};
use voldiq_core::{
    Kind, NodeId,
    metadata::{Eligibility, Store},
    scanner,
};

fn old_file(path: &Path, content: &[u8]) {
    std::fs::write(path, content).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1000)))
        .unwrap();
}

fn node(index: &voldiq_core::index::Index, name: &str) -> NodeId {
    index
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .unwrap()
        .id
}

#[test]
fn suggestions_exclude_protected_descendants_renamed_identities_sensitive_paths_and_state() {
    let source = tempfile::tempdir().unwrap();
    let root = source.path();
    let state = root.join("state");
    let protected = root.join("project");
    let sibling = root.join("project-other");
    for path in [&state, &protected, &sibling, &root.join(".git")] {
        std::fs::create_dir(path).unwrap();
    }
    old_file(&protected.join("kept.tmp"), b"protected");
    old_file(&sibling.join("candidate.tmp"), b"candidate");
    old_file(&state.join("internal.tmp"), b"internal");
    old_file(&root.join(".git").join("config"), b"sensitive");
    old_file(&root.join(".env"), b"sensitive");
    let mut metadata = Store::open(&state).unwrap();
    metadata.protect_folder(&protected).unwrap();
    let moved = root.join("renamed-project");
    std::fs::rename(&protected, &moved).unwrap();
    std::fs::create_dir(&protected).unwrap();
    old_file(&protected.join("replacement.tmp"), b"replacement");
    let index = scanner::scan(root, &Default::default(), &AtomicBool::new(false), |_| {}).unwrap();
    assert!(
        index
            .entries
            .iter()
            .filter(|entry| entry.kind == Kind::Directory)
            .all(|entry| entry.file_state.is_some())
    );
    let protections = metadata.protections().unwrap();
    let mask = protections
        .for_index(&index, &AtomicBool::new(false))
        .unwrap();
    assert!(mask.allows(node(&index, "candidate.tmp")));
    for name in [
        "kept.tmp",
        "replacement.tmp",
        "internal.tmp",
        "config",
        ".env",
    ] {
        assert_eq!(
            mask.eligibility(node(&index, name)).unwrap(),
            Eligibility::Protected,
            "{name}"
        );
    }
    let page =
        voldiq_core::cleanup::rules::query_old(Some(&index), 100_000_000, 0, 100, &protections)
            .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.logical, 9);
    assert_eq!(page.items[0].id, node(&index, "candidate.tmp"));
    assert_eq!(page.unverified_branches, 0);
    let empty =
        voldiq_core::cleanup::rules::query_old(Some(&index), 100_000_000, 1, 1, &protections)
            .unwrap();
    assert!(empty.items.is_empty());
    assert_eq!(empty.total, 1);
    assert_eq!(std::fs::read(moved.join("kept.tmp")).unwrap(), b"protected");
}

#[test]
fn missing_directory_evidence_is_disclosed_and_cancellation_does_not_create_a_partial_mask() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let unknown = source.path().join("unknown");
    std::fs::create_dir(&unknown).unwrap();
    old_file(&unknown.join("hidden.tmp"), b"unverified");
    old_file(&source.path().join("visible.tmp"), b"visible");
    let mut index = scanner::scan(
        source.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let id = node(&index, "unknown");
    index.entries[id.0 as usize].file_state = None;
    let protections = Store::open(state.path()).unwrap().protections().unwrap();
    let mask = protections
        .for_index(&index, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        mask.eligibility(node(&index, "hidden.tmp")).unwrap(),
        Eligibility::Unverified
    );
    assert!(mask.eligibility(NodeId(u32::MAX)).is_err());
    assert_eq!(mask.unverified_branches(NodeId(0)).unwrap(), 1);
    let page =
        voldiq_core::cleanup::rules::query_old(Some(&index), 100_000_000, 0, 100, &protections)
            .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.logical, 7);
    assert_eq!(page.unverified_branches, 1);
    assert!(!page.complete);
    assert!(
        protections
            .for_index(&index, &AtomicBool::new(true))
            .is_err()
    );
}

#[test]
fn invalid_hierarchies_and_replaced_scan_roots_do_not_produce_suggestions() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let folder = source.path().join("root");
    std::fs::create_dir(&folder).unwrap();
    old_file(&folder.join("candidate.tmp"), b"fixture");
    let mut index = scanner::scan(
        &folder,
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let protections = Store::open(state.path()).unwrap().protections().unwrap();
    index.entries[1].parent = Some(NodeId(u32::MAX));
    assert!(
        protections
            .for_index(&index, &AtomicBool::new(false))
            .is_err()
    );
    index.entries[1].parent = Some(NodeId(0));
    index.entries[0].kind = Kind::File;
    assert!(
        protections
            .for_index(&index, &AtomicBool::new(false))
            .is_err()
    );
    index.entries[0].kind = Kind::Directory;
    std::fs::rename(&folder, source.path().join("moved-root")).unwrap();
    std::fs::create_dir(&folder).unwrap();
    assert!(
        protections
            .for_index(&index, &AtomicBool::new(false))
            .is_err()
    );
}

#[test]
fn a_renamed_protected_ancestor_outside_the_scan_root_still_excludes_its_contents() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let parent = source.path().join("protected-parent");
    std::fs::create_dir_all(parent.join("nested")).unwrap();
    old_file(&parent.join("nested/candidate.tmp"), b"protected");
    let mut metadata = Store::open(state.path()).unwrap();
    metadata.protect_folder(&parent).unwrap();
    let moved = source.path().join("renamed-parent");
    std::fs::rename(&parent, &moved).unwrap();
    let index = scanner::scan(
        &moved.join("nested"),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let protections = metadata.protections().unwrap();
    let mask = protections
        .for_index(&index, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(mask.eligibility(NodeId(0)).unwrap(), Eligibility::Protected);
    assert!(!mask.allows(node(&index, "candidate.tmp")));
    let page =
        voldiq_core::cleanup::rules::query_old(Some(&index), 100_000_000, 0, 100, &protections)
            .unwrap();
    assert_eq!(page.total, 0);
    assert_eq!(page.logical, 0);
}

#[test]
fn duplicate_candidates_exclude_protected_content_and_results_require_the_same_policy() {
    use voldiq_core::duplicates::{HashCache, find};
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let protected = source.path().join("protected");
    std::fs::create_dir(&protected).unwrap();
    let data = [7_u8; 16_384];
    for path in [
        source.path().join("first.data"),
        source.path().join("second.data"),
        protected.join("third.data"),
        protected.join("fourth.data"),
    ] {
        std::fs::write(path, data).unwrap();
    }
    let mut metadata = Store::open(state.path()).unwrap();
    metadata.protect_folder(&protected).unwrap();
    let captured = scanner::scan(
        source.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let first = node(&captured, "first.data");
    let second = node(&captured, "second.data");
    let index = std::sync::Arc::new(std::sync::RwLock::new(captured));
    let protections = metadata.protections().unwrap();
    let mut candidates = Vec::new();
    let analysis = find(
        &index,
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut HashCache::default(),
        |progress| candidates.push(progress.candidates),
        &protections,
    )
    .unwrap();
    assert_eq!(analysis.groups.len(), 1);
    assert_eq!(
        analysis.groups[0].digest,
        blake3::hash(&data).to_hex().as_str()
    );
    let ids = analysis.groups[0]
        .members
        .iter()
        .map(|member| member.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids, std::collections::HashSet::from([first, second]));
    assert_eq!(analysis.examined, 2);
    assert_eq!(analysis.bytes_read, 49_152);
    assert!(candidates.iter().all(|&count| count == 2));
    analysis.check_protections(&protections).unwrap();
    metadata.protect_folder(source.path()).unwrap();
    let changed = metadata.protections().unwrap();
    assert!(analysis.check_protections(&changed).is_err());
    let excluded = find(
        &index,
        NodeId(0),
        0,
        &AtomicBool::new(false),
        &mut HashCache::default(),
        |_| {},
        &changed,
    )
    .unwrap();
    assert!(excluded.groups.is_empty());
    assert_eq!(excluded.bytes_read, 0);
    assert_eq!(excluded.examined, 0);
    assert_eq!(std::fs::read(protected.join("third.data")).unwrap(), data);
}
