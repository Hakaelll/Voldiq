use std::sync::atomic::AtomicBool;
use voldiq_core::{
    Error,
    agents::{Capability, Limits, Policy, Registry},
    plans::{Desktop, State},
    service::AgentService,
};

#[test]
fn saved_snapshot_selection_is_queued_for_separate_human_review_without_file_changes() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let file = source.path().join("selected.data");
    std::fs::write(&file, b"selected fixture").unwrap();
    let index = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let selected = index
        .entries
        .iter()
        .find(|entry| entry.name == "selected.data")
        .unwrap()
        .id;
    let snapshot = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save(&index, &AtomicBool::new(false), &[])
        .unwrap();
    let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
    let provisioned = registry
        .provision(
            "Review fixture",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![
                    Capability::Metadata,
                    Capability::PreparePlans,
                    Capability::RequestApproval,
                ],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let service = AgentService::new(
        state.path().to_owned(),
        registry
            .authenticate(provisioned.credential.expose())
            .unwrap(),
    );
    let plan = service.plan_prepare(snapshot, &[selected]).unwrap();
    assert_eq!(plan.state, State::Draft);
    assert_eq!(plan.manifest.entries[0].logical_bytes, "16");
    let id = &plan.manifest.id;
    assert!(matches!(
        service.plan_request(id),
        Err(Error::NeedsHumanApproval)
    ));
    assert_eq!(service.plan_get(id).unwrap().state, State::AwaitingApproval);
    let desktop = Desktop::open(state.path()).unwrap();
    assert!(Desktop::open(state.path()).is_err());
    let page = desktop.list(0, 25).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, *id);
    assert_eq!(page.items[0].connection, "Review fixture");
    let database = rusqlite::Connection::open(state.path().join("agents.db")).unwrap();
    database
        .execute(
            "UPDATE agents SET last_used=0 WHERE id=?1",
            [provisioned.id],
        )
        .unwrap();
    let review = desktop.review(id).unwrap();
    assert_eq!(
        registry
            .list()
            .unwrap()
            .into_iter()
            .find(|connection| connection.id == provisioned.id)
            .unwrap()
            .last_used,
        Some(0)
    );
    assert_eq!(review.plan.hash, plan.hash);
    assert_eq!(
        review.plan.manifest.entries[0].path,
        std::fs::canonicalize(&file).unwrap().to_str().unwrap()
    );
    assert!(desktop.approve(id, &"0".repeat(64)).is_err());
    desktop.approve(id, &plan.hash).unwrap();
    assert_eq!(service.plan_get(id).unwrap().state, State::Approved);
    desktop.reject(id, &plan.hash).unwrap();
    assert_eq!(service.plan_get(id).unwrap().state, State::Rejected);
    let audit = service.plan_audit(id, 0, 100).unwrap();
    assert!(audit.iter().any(|event| event.event == "approved"));
    assert!(audit.iter().any(|event| event.event == "rejected"));
    assert_eq!(std::fs::read(&file).unwrap(), b"selected fixture");
    let pending = service.plan_prepare(snapshot, &[selected]).unwrap();
    assert!(matches!(
        service.plan_request(&pending.manifest.id),
        Err(Error::NeedsHumanApproval)
    ));
    desktop.close();
    assert!(
        desktop
            .approve(&pending.manifest.id, &pending.hash)
            .is_err()
    );
    assert!(desktop.list(0, 25).is_err());
    drop(desktop);
    let desktop = Desktop::open(state.path()).unwrap();
    desktop
        .approve(&pending.manifest.id, &pending.hash)
        .unwrap();
    drop(desktop);
    let desktop = Desktop::open(state.path()).unwrap();
    assert_eq!(
        service.plan_get(&pending.manifest.id).unwrap().state,
        State::AwaitingApproval
    );
    desktop.reject(&pending.manifest.id, &pending.hash).unwrap();
    let other = registry
        .provision(
            "Reader",
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
    assert!(other.plan_get(id).is_err());
    assert!(other.plan_audit(id, 0, 100).is_err());
    assert!(other.plan_prepare(snapshot, &[selected]).is_err());
    assert!(other.plan_request(id).is_err());
    assert!(desktop.list(0, 101).is_err());
    assert!(desktop.list(10_001, 25).is_err());
    let mut legacy = index;
    for entry in &mut legacy.entries {
        entry.file_state = None;
    }
    let legacy_snapshot = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save(&legacy, &AtomicBool::new(false), &[])
        .unwrap();
    assert!(service.plan_prepare(legacy_snapshot, &[selected]).is_err());
    std::fs::write(&file, b"changed evidence").unwrap();
    assert!(service.plan_prepare(snapshot, &[selected]).is_err());
    registry.revoke(provisioned.id).unwrap();
    let page = desktop.list(0, 25).unwrap();
    let json = serde_json::to_value(page).unwrap();
    assert_eq!(json["items"][0]["availability"], "unavailable");
    assert!(desktop.review(id).is_err());
}
