use voldiq_core::{
    jobs::Jobs,
    operations::{Coordinator, Lease},
};

#[test]
fn coordinators_share_one_os_lease_and_keep_the_existing_lock_contract() {
    let state = tempfile::tempdir().unwrap();
    let first = Coordinator::open(state.path()).unwrap();
    let second = Coordinator::open(state.path()).unwrap();
    let operation = first.acquire().unwrap();
    assert!(second.acquire().is_err());
    assert!(Lease::acquire(state.path()).is_err());
    let old_client = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.path().join("scan.lock"))
        .unwrap();
    assert!(old_client.try_lock().is_err());
    #[cfg(windows)]
    assert!(
        std::fs::rename(
            state.path().join("scan.lock"),
            state.path().join("renamed.lock")
        )
        .is_err()
    );
    drop(old_client);
    drop(operation);
    assert!(second.acquire().is_ok());
}

#[test]
fn a_foreign_state_lease_cannot_start_a_job_or_reconcile_an_active_owner() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let lease = Lease::acquire(first.path()).unwrap();
    let mut wrong = Jobs::open(second.path()).unwrap();
    assert!(wrong.begin(&lease, 7, first.path()).is_err());
    assert!(wrong.list(7, 0, 10).unwrap().is_empty());
    let mut jobs = Jobs::open(first.path()).unwrap();
    let id = jobs.begin(&lease, 7, first.path()).unwrap();
    assert_eq!(Jobs::reconcile_abandoned(first.path(), 7).unwrap(), 0);
    assert_eq!(
        jobs.get(7, id).unwrap().state,
        voldiq_core::jobs::JobState::Running
    );
}
