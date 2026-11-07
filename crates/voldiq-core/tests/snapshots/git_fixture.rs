use std::{fs, sync::atomic::AtomicBool};
use voldiq_core::{
    NodeId,
    developer::git::{Query, query},
    snapshots::Store,
};

pub(super) fn compare_indexed_git_storage() {
    let files = tempfile::tempdir().unwrap();
    let db = tempfile::tempdir().unwrap();
    let write = |name: &str, data: &[u8]| {
        let path = files.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    };
    let pack = "a".repeat(40);
    let lfs = format!("cafe{}", "1".repeat(60));
    write("worktree/.git/HEAD", b"ref: refs/heads/main\n");
    write("worktree/.git/refs/heads/main", b"fixture");
    write(
        &format!("worktree/.git/objects/aa/{}", "0".repeat(38)),
        &[0; 11],
    );
    write(
        &format!("worktree/.git/objects/pack/pack-{pack}.pack"),
        &[0; 19],
    );
    write(
        &format!("worktree/.git/objects/pack/pack-{pack}.idx"),
        &[0; 5],
    );
    write("worktree/.git/objects/pack/readme.pack", &[0; 80]);
    write(
        "worktree/.git/objects/info/alternates",
        b"../../../external/objects\n",
    );
    write(&format!("worktree/.git/lfs/objects/ca/fe/{lfs}"), &[0; 23]);
    write(&format!("worktree/.git/lfs/objects/ca/ff/{lfs}"), &[0; 60]);
    write("external/HEAD", b"ref: refs/heads/main\n");
    write("external/refs/heads/main", b"fixture");
    write(&format!("external/objects/bb/{}", "0".repeat(62)), &[0; 90]);
    write("linked/.git", b"gitdir: ../external\n");
    write("shared/HEAD", b"ref: refs/heads/main\n");
    write("shared/commondir", b"../external\n");
    write("lookalike/.git/objects/pack/noise", &[0; 120]);

    let live = super::capture(files.path());
    let mut store = Store::open(&db.path().join("git.db")).unwrap();
    let running = AtomicBool::new(false);
    let snapshot = store.save(&live, &running, &[]).unwrap();
    drop(files);
    let index = store.load(snapshot, &running).unwrap();
    let root = Query {
        scope: NodeId(0),
        offset: 0,
        limit: 1,
    };
    let first = query(&index, &root, &running).unwrap().unwrap();
    assert_eq!(
        (
            first.total,
            first.git_file_markers,
            first.shared_store_markers
        ),
        (2, 1, 1)
    );
    assert_eq!(first.items[0].total_bytes, "90");
    assert_eq!(first.items[0].lfs_bytes, None);
    let second = query(&index, &Query { offset: 1, ..root }, &running)
        .unwrap()
        .unwrap();
    let git = &second.items[0];
    assert_eq!(
        (
            &*git.loose_bytes,
            &*git.pack_bytes,
            &*git.pack_metadata_bytes
        ),
        ("11", "19", "5")
    );
    assert_eq!(
        (git.lfs_bytes.as_deref(), &*git.total_bytes),
        (Some("23"), "58")
    );
    assert!(git.alternates);
    assert_eq!(index.entry(git.id).unwrap().name, "worktree");

    let scoped = Query {
        scope: git.store_id,
        offset: 0,
        limit: 10,
    };
    let result = query(&index, &scoped, &running).unwrap().unwrap();
    assert_eq!(
        (
            result.total,
            result.git_file_markers,
            result.shared_store_markers
        ),
        (1, 0, 0)
    );
    assert_eq!(result.items[0].id, git.store_id);
    assert_eq!(result.items[0].total_bytes, "58");
    assert!(
        query(&index, &scoped, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
}
