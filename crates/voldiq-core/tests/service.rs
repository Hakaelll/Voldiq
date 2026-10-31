use std::sync::atomic::AtomicBool;
use voldiq_core::{
    agents::{Capability, Limits, Policy, Registry},
    index::Query,
    jobs::{JobState, Jobs},
    service::AgentService,
};

#[test]
fn authorized_service_scans_once_persists_and_queries_only_the_current_scope() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let private = source.path().join("private");
    std::fs::create_dir(&private).unwrap();
    std::fs::write(source.path().join("report.txt"), [1; 7]).unwrap();
    std::fs::write(private.join("key.pem"), [2; 19]).unwrap();
    let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
    let policy = Policy::new(
        vec![source.path().to_owned()],
        vec![private],
        vec![Capability::Metadata, Capability::Scan],
        Limits::default(),
    )
    .unwrap();
    let provisioned = registry.provision("Reader", policy).unwrap();
    let principal = registry
        .authenticate(provisioned.credential.expose())
        .unwrap();
    let service = AgentService::new(state.path().to_owned(), principal);
    let job = service.start_scan(source.path()).unwrap();
    let result = service.wait(job).unwrap();
    assert_eq!(result.state, JobState::Completed);
    let snapshot = result.snapshot_id.unwrap();
    assert_eq!(service.stats(snapshot).unwrap().logical, 7);
    let page = service
        .query(
            snapshot,
            &Query {
                files_only: true,
                ..Query::default()
            },
        )
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].name, "report.txt");
    assert!(
        service
            .query(
                snapshot,
                &Query {
                    sort: "invented".into(),
                    ..Query::default()
                }
            )
            .is_err()
    );
    assert!(service.query(i64::MAX, &Query::default()).is_err());
    let other = registry
        .provision(
            "Another reader",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![Capability::Metadata],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let other = AgentService::new(
        state.path().to_owned(),
        registry.authenticate(other.credential.expose()).unwrap(),
    );
    assert!(other.stats(snapshot).is_err());
    registry.revoke(provisioned.id).unwrap();
    assert!(service.query(snapshot, &Query::default()).is_err());
    assert!(service.job(job).is_err());
    assert_eq!(
        Jobs::open(state.path())
            .unwrap()
            .get(provisioned.id, job)
            .unwrap()
            .state,
        JobState::Completed
    );
    // Raw snapshot data remains real and independent of whether a client is connected.
    let saved = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .load(snapshot, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(saved.entries[0].logical, 7);
}
