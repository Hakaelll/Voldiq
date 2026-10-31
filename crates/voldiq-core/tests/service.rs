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
    assert_eq!(other.stats(snapshot).unwrap().logical, 7);
    assert!(other.job(job).is_err());
    let broad = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let mut store =
        voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db")).unwrap();
    let desktop_snapshot = store.save(&broad, &AtomicBool::new(false), &[]).unwrap();
    assert!(service.stats(desktop_snapshot).is_err());
    assert_eq!(other.stats(desktop_snapshot).unwrap().logical, 26);
    let restricted_history = service.history(0, 100).unwrap();
    assert_eq!(
        restricted_history
            .items
            .iter()
            .map(|snapshot| snapshot.id)
            .collect::<Vec<_>>(),
        vec![snapshot]
    );
    assert!(!restricted_history.limited);
    let page = other.history(0, 1).unwrap();
    assert_eq!(page.items[0].id, desktop_snapshot);
    assert_eq!(page.next_offset, Some(1));
    assert_eq!(other.history(1, 1).unwrap().items[0].id, snapshot);
    let forbidden = registry
        .provision(
            "Outside scope",
            Policy::new(
                vec![state.path().to_owned()],
                vec![],
                vec![Capability::Metadata],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let forbidden = AgentService::new(
        state.path().to_owned(),
        registry
            .authenticate(forbidden.credential.expose())
            .unwrap(),
    );
    assert!(forbidden.stats(snapshot).is_err());
    assert!(forbidden.details(snapshot, voldiq_core::NodeId(0)).is_err());
    assert!(forbidden.history(0, 100).unwrap().items.is_empty());
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
