use voldiq_core::{
    jobs::{JobState, Jobs},
    operations::Lease,
};

#[test]
fn scan_lease_excludes_other_processes_and_releases_after_owner_exit() {
    if let Some(directory) = std::env::var_os("VOLDIQ_TEST_LOCK_DIRECTORY") {
        assert!(Lease::acquire(std::path::Path::new(&directory)).is_err());
        return;
    }
    let fixture = tempfile::tempdir().unwrap();
    let lease = Lease::acquire(fixture.path()).unwrap();
    assert!(Lease::acquire(fixture.path()).is_err());
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "scan_lease_excludes_other_processes_and_releases_after_owner_exit",
        ])
        .env("VOLDIQ_TEST_LOCK_DIRECTORY", fixture.path())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stdout)
    );
    drop(lease);
    assert!(Lease::acquire(fixture.path()).is_ok());
}

#[test]
fn durable_jobs_are_owner_scoped_cancelable_bounded_and_never_retried_after_interruption() {
    let fixture = tempfile::tempdir().unwrap();
    let mut jobs = Jobs::open(fixture.path()).unwrap();
    let lease = Lease::acquire(fixture.path()).unwrap();
    let id = jobs.begin(&lease, 7, fixture.path()).unwrap();
    assert_eq!(jobs.get(7, id).unwrap().state, JobState::Running);
    assert!(jobs.get(8, id).is_err());
    assert!(jobs.cancel(8, id).is_err());
    assert!(!jobs.cancellation_requested(7, id).unwrap());
    jobs.cancel(7, id).unwrap();
    assert!(jobs.cancellation_requested(7, id).unwrap());
    jobs.finish(7, id, JobState::Cancelled, None).unwrap();
    assert!(jobs.finish(7, id, JobState::Completed, Some(123)).is_err());
    assert!(jobs.cancel(7, id).is_err());
    let interrupted = jobs.begin(&lease, 7, fixture.path()).unwrap();
    assert_eq!(Jobs::reconcile_abandoned(fixture.path(), 7).unwrap(), 0);
    assert_eq!(jobs.get(7, interrupted).unwrap().state, JobState::Running);
    drop(jobs);
    drop(lease);
    let mut jobs = Jobs::open(fixture.path()).unwrap();
    assert_eq!(Jobs::reconcile_abandoned(fixture.path(), 8).unwrap(), 0);
    assert_eq!(Jobs::reconcile_abandoned(fixture.path(), 7).unwrap(), 1);
    assert_eq!(
        jobs.get(7, interrupted).unwrap().state,
        JobState::Interrupted
    );
    let lease = Lease::acquire(fixture.path()).unwrap();
    let next = jobs.begin(&lease, 8, fixture.path()).unwrap();
    assert!(next > interrupted);
    assert_eq!(
        jobs.get(7, interrupted).unwrap().state,
        JobState::Interrupted
    );
    assert_eq!(jobs.list(7, 0, 5000).unwrap().len(), 2);
    assert_eq!(jobs.list(8, 0, 10).unwrap().len(), 1);
    assert!(
        jobs.finish(7, interrupted, JobState::Completed, Some(1))
            .is_err()
    );
    assert!(jobs.finish(8, next, JobState::Running, None).is_err());
}
