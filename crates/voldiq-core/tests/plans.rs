use std::sync::atomic::AtomicBool;
use voldiq_core::{
    NodeId,
    agents::{Capability, Limits, Policy, Registry},
    plans::{Begin, HumanAuthority, State, Store},
};

#[test]
fn a_database_approval_is_not_authority_and_live_approval_is_consumed_once() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let file = source.path().join("selected.data");
    std::fs::write(&file, b"owned fixture").unwrap();
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
    let mut snapshots =
        voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db")).unwrap();
    let snapshot = snapshots
        .save(&index, &AtomicBool::new(false), &[])
        .unwrap();
    let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
    let created = registry
        .provision(
            "Fixture client",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![
                    Capability::Metadata,
                    Capability::PreparePlans,
                    Capability::RequestApproval,
                    Capability::ExecuteApproved,
                ],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let principal = registry.authenticate(created.credential.expose()).unwrap();
    let prepare = registry
        .plan_context(&principal, Capability::PreparePlans)
        .unwrap();
    let request = registry
        .plan_context(&principal, Capability::RequestApproval)
        .unwrap();
    let execute = registry
        .plan_context(&principal, Capability::ExecuteApproved)
        .unwrap();
    let metadata = registry
        .plan_context(&principal, Capability::Metadata)
        .unwrap();
    let mut plans = Store::open(state.path()).unwrap();
    let plan = plans
        .create(&prepare, snapshot, &index, &[selected], 1000)
        .unwrap();
    assert_eq!(plan.state, State::Draft);
    assert_eq!(plan.manifest.entries[0].logical_bytes, "13");
    assert_eq!(
        plan.manifest.entries[0].path,
        std::fs::canonicalize(&file).unwrap().to_str().unwrap()
    );
    assert_eq!(plan.manifest.immediate_free_bytes, "0");
    assert_eq!(plan.manifest.source_snapshot, snapshot);
    assert_eq!(plan.hash.len(), 64);
    let id = &plan.manifest.id;
    plans.request(&request, id, 1001).unwrap();
    let human = HumanAuthority::acquire(state.path()).unwrap();
    assert!(HumanAuthority::acquire(state.path()).is_err());
    assert!(matches!(
        plans.begin(&execute, id, &human, 1002),
        Err(voldiq_core::Error::NeedsHumanApproval)
    ));
    let database = rusqlite::Connection::open(state.path().join("agent-plans.db")).unwrap();
    database.execute("UPDATE agent_plans SET state='approved',approval_issuer=?1,approval_expires=1100 WHERE id=?2", rusqlite::params![human.issuer(), id]).unwrap();
    assert!(matches!(
        plans.begin(&execute, id, &human, 1003),
        Err(voldiq_core::Error::NeedsHumanApproval)
    ));
    database.execute("UPDATE agent_plans SET state='awaiting_approval',approval_issuer=NULL,approval_expires=NULL WHERE id=?1", [id]).unwrap();
    assert!(
        plans
            .approve(&metadata, id, &"0".repeat(64), &human, 1004)
            .is_err()
    );
    plans
        .approve(&metadata, id, &plan.hash, &human, 1004)
        .unwrap();
    let lease = match plans.begin(&execute, id, &human, 1005).unwrap() {
        Begin::Started(lease) => lease,
        _ => panic!("Expected one execution lease"),
    };
    assert!(matches!(
        plans.begin(&execute, id, &human, 1006).unwrap(),
        Begin::Existing(State::Executing)
    ));
    plans
        .finish(
            lease,
            State::Cancelled,
            serde_json::json!({"cancelled_before_io":true}),
            1007,
        )
        .unwrap();
    assert!(matches!(
        plans.begin(&execute, id, &human, 1008).unwrap(),
        Begin::Existing(State::Cancelled)
    ));
    assert_eq!(std::fs::read(&file).unwrap(), b"owned fixture");
    let events = plans.audit(&metadata, id, 0, 100).unwrap();
    assert!(events.iter().any(|event| event.event == "approved"));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event == "executing")
            .count(),
        1
    );
    assert!(
        plans
            .create(&prepare, snapshot, &index, &[NodeId(0)], 1000)
            .is_err()
    );
}

#[test]
fn expired_changed_and_tampered_plans_never_become_new_authority_after_restart() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let file = source.path().join("selected.data");
    std::fs::write(&file, b"original").unwrap();
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
    let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
    let created = registry
        .provision(
            "Fixture",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![
                    Capability::Metadata,
                    Capability::PreparePlans,
                    Capability::RequestApproval,
                    Capability::ExecuteApproved,
                ],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let principal = registry.authenticate(created.credential.expose()).unwrap();
    let prepare = registry
        .plan_context(&principal, Capability::PreparePlans)
        .unwrap();
    let request = registry
        .plan_context(&principal, Capability::RequestApproval)
        .unwrap();
    let metadata = registry
        .plan_context(&principal, Capability::Metadata)
        .unwrap();
    let execute = registry
        .plan_context(&principal, Capability::ExecuteApproved)
        .unwrap();
    let mut plans = Store::open(state.path()).unwrap();
    let human = HumanAuthority::acquire(state.path()).unwrap();
    let expired = plans
        .create(&prepare, 1, &index, &[selected], 1000)
        .unwrap();
    plans.request(&request, &expired.manifest.id, 1000).unwrap();
    plans
        .approve(&metadata, &expired.manifest.id, &expired.hash, &human, 1001)
        .unwrap();
    assert!(matches!(
        plans
            .begin(&execute, &expired.manifest.id, &human, 1121)
            .unwrap(),
        Begin::Existing(State::Expired)
    ));
    let abandoned = plans
        .create(&prepare, 1, &index, &[selected], 1000)
        .unwrap();
    plans
        .request(&request, &abandoned.manifest.id, 1000)
        .unwrap();
    plans
        .approve(
            &metadata,
            &abandoned.manifest.id,
            &abandoned.hash,
            &human,
            1001,
        )
        .unwrap();
    assert!(matches!(
        plans
            .begin(&execute, &abandoned.manifest.id, &human, 1002)
            .unwrap(),
        Begin::Started(_)
    ));
    drop(human);
    drop(plans);
    let human = HumanAuthority::acquire(state.path()).unwrap();
    let mut plans = Store::open(state.path()).unwrap();
    plans.recover(&human, 1003).unwrap();
    assert!(matches!(
        plans
            .begin(&execute, &abandoned.manifest.id, &human, 1004)
            .unwrap(),
        Begin::Existing(State::Failed)
    ));
    let changed = plans
        .create(&prepare, 1, &index, &[selected], 1000)
        .unwrap();
    std::fs::write(&file, b"replacement content").unwrap();
    assert!(plans.request(&request, &changed.manifest.id, 1001).is_err());
    assert_eq!(
        plans
            .get(&metadata, &changed.manifest.id, 1001)
            .unwrap()
            .state,
        State::Invalidated
    );
    let database = rusqlite::Connection::open(state.path().join("agent-plans.db")).unwrap();
    database.execute("UPDATE agent_plans SET manifest=replace(manifest,'selected.data','tampered.data') WHERE id=?1", [&changed.manifest.id]).unwrap();
    assert!(plans.get(&metadata, &changed.manifest.id, 1002).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), b"replacement content");
}

#[test]
fn competing_clients_receive_one_ticket_and_replays_report_the_same_execution() {
    let fixture = Fixture::new();
    let mut plans = Store::open(fixture.state.path()).unwrap();
    let prepare = fixture
        .registry
        .plan_context(&fixture.principal, Capability::PreparePlans)
        .unwrap();
    let request = fixture
        .registry
        .plan_context(&fixture.principal, Capability::RequestApproval)
        .unwrap();
    let metadata = fixture
        .registry
        .plan_context(&fixture.principal, Capability::Metadata)
        .unwrap();
    let plan = plans
        .create(&prepare, 1, &fixture.index, &[fixture.selected], 1000)
        .unwrap();
    plans.request(&request, &plan.manifest.id, 1000).unwrap();
    let human = HumanAuthority::acquire(fixture.state.path()).unwrap();
    plans
        .approve(&metadata, &plan.manifest.id, &plan.hash, &human, 1000)
        .unwrap();
    let barrier = std::sync::Barrier::new(8);
    let path = fixture.state.path();
    let principal = &fixture.principal;
    let id = &plan.manifest.id;
    let results = std::thread::scope(|scope| {
        let threads = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let registry = Registry::open(&path.join("agents.db")).unwrap();
                    let execute = registry
                        .plan_context(principal, Capability::ExecuteApproved)
                        .unwrap();
                    let mut plans = Store::open(path).unwrap();
                    barrier.wait();
                    plans.begin(&execute, id, &human, 1001).unwrap()
                })
            })
            .collect::<Vec<_>>();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Begin::Started(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Begin::Existing(State::Executing)))
            .count(),
        7
    );
    assert_eq!(
        plans
            .audit(&metadata, id, 0, 100)
            .unwrap()
            .iter()
            .filter(|event| event.event == "executing")
            .count(),
        1
    );
    assert_eq!(
        std::fs::read(fixture.source.path().join("selected.data")).unwrap(),
        b"original"
    );
}

#[test]
fn owner_rotation_limits_and_rejected_plans_are_enforced_on_every_call() {
    let fixture = Fixture::new();
    let mut plans = Store::open(fixture.state.path()).unwrap();
    let prepare = fixture
        .registry
        .plan_context(&fixture.principal, Capability::PreparePlans)
        .unwrap();
    let request = fixture
        .registry
        .plan_context(&fixture.principal, Capability::RequestApproval)
        .unwrap();
    let metadata = fixture
        .registry
        .plan_context(&fixture.principal, Capability::Metadata)
        .unwrap();
    let execute = fixture
        .registry
        .plan_context(&fixture.principal, Capability::ExecuteApproved)
        .unwrap();
    let plan = plans
        .create(&prepare, 1, &fixture.index, &[fixture.selected], 1000)
        .unwrap();
    let mut other_registry = Registry::open(&fixture.state.path().join("agents.db")).unwrap();
    let other = other_registry
        .provision(
            "Other",
            Policy::new(
                vec![fixture.source.path().to_owned()],
                vec![],
                vec![Capability::Metadata, Capability::PreparePlans],
                Limits {
                    plan_bytes: 1,
                    ..Default::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    let other = other_registry
        .authenticate(other.credential.expose())
        .unwrap();
    let other_metadata = other_registry
        .plan_context(&other, Capability::Metadata)
        .unwrap();
    assert!(plans.get(&other_metadata, &plan.manifest.id, 1000).is_err());
    let other_prepare = other_registry
        .plan_context(&other, Capability::PreparePlans)
        .unwrap();
    assert!(
        plans
            .create(&other_prepare, 1, &fixture.index, &[fixture.selected], 1000)
            .is_err()
    );
    assert!(plans.request(&metadata, &plan.manifest.id, 1000).is_err());
    plans.request(&request, &plan.manifest.id, 1000).unwrap();
    let human = HumanAuthority::acquire(fixture.state.path()).unwrap();
    plans
        .approve(&metadata, &plan.manifest.id, &plan.hash, &human, 1000)
        .unwrap();
    plans
        .reject(&metadata, &plan.manifest.id, &human, 1001)
        .unwrap();
    assert!(matches!(
        plans
            .begin(&execute, &plan.manifest.id, &human, 1002)
            .unwrap(),
        Begin::Existing(State::Rejected)
    ));
    other_registry.rotate(fixture.principal.id()).unwrap();
    assert!(plans.get(&metadata, &plan.manifest.id, 1002).is_err());
    assert!(
        plans
            .create(&prepare, 1, &fixture.index, &[fixture.selected], 1002)
            .is_err()
    );
    assert!(
        plans
            .begin(&execute, &plan.manifest.id, &human, 1002)
            .is_err()
    );
}

#[test]
fn new_hard_links_invalidate_approval_and_old_sessions_require_new_review() {
    let fixture = Fixture::new();
    let prepare = fixture
        .registry
        .plan_context(&fixture.principal, Capability::PreparePlans)
        .unwrap();
    let request = fixture
        .registry
        .plan_context(&fixture.principal, Capability::RequestApproval)
        .unwrap();
    let metadata = fixture
        .registry
        .plan_context(&fixture.principal, Capability::Metadata)
        .unwrap();
    let execute = fixture
        .registry
        .plan_context(&fixture.principal, Capability::ExecuteApproved)
        .unwrap();
    let mut plans = Store::open(fixture.state.path()).unwrap();
    let plan = plans
        .create(&prepare, 1, &fixture.index, &[fixture.selected], 1000)
        .unwrap();
    plans.request(&request, &plan.manifest.id, 1000).unwrap();
    let human = HumanAuthority::acquire(fixture.state.path()).unwrap();
    plans
        .approve(&metadata, &plan.manifest.id, &plan.hash, &human, 1000)
        .unwrap();
    drop(human);
    let human = HumanAuthority::acquire(fixture.state.path()).unwrap();
    plans.recover(&human, 1001).unwrap();
    assert_eq!(
        plans.get(&metadata, &plan.manifest.id, 1001).unwrap().state,
        State::AwaitingApproval
    );
    assert!(matches!(
        plans.begin(&execute, &plan.manifest.id, &human, 1001),
        Err(voldiq_core::Error::NeedsHumanApproval)
    ));
    plans
        .approve(&metadata, &plan.manifest.id, &plan.hash, &human, 1001)
        .unwrap();
    std::fs::hard_link(
        fixture.source.path().join("selected.data"),
        fixture.source.path().join("alias.data"),
    )
    .unwrap();
    assert!(
        plans
            .begin(&execute, &plan.manifest.id, &human, 1002)
            .is_err()
    );
    assert_eq!(
        plans.get(&metadata, &plan.manifest.id, 1002).unwrap().state,
        State::Invalidated
    );
    assert_eq!(
        std::fs::read(fixture.source.path().join("alias.data")).unwrap(),
        b"original"
    );
}

#[test]
fn internal_state_credentials_git_and_virtual_disks_cannot_be_selected() {
    let fixture = Fixture::new();
    for name in [".env", "auth.json", "ext4.vhdx"] {
        std::fs::write(fixture.source.path().join(name), b"protected").unwrap();
    }
    std::fs::create_dir(fixture.source.path().join(".git")).unwrap();
    std::fs::write(fixture.source.path().join(".git/config"), b"protected").unwrap();
    let index = voldiq_core::scanner::scan(
        fixture.source.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let prepare = fixture
        .registry
        .plan_context(&fixture.principal, Capability::PreparePlans)
        .unwrap();
    let mut plans = Store::open(fixture.state.path()).unwrap();
    for name in [".env", "auth.json", "ext4.vhdx", "config"] {
        let selected = index
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
            .id;
        assert!(
            plans
                .create(&prepare, 1, &index, &[selected], 1000)
                .is_err(),
            "{name}"
        );
    }
    let nested_state = fixture.source.path().join("private-state");
    std::fs::create_dir(&nested_state).unwrap();
    let mut nested_registry = Registry::open(&nested_state.join("agents.db")).unwrap();
    let created = nested_registry
        .provision(
            "Nested",
            Policy::new(
                vec![fixture.source.path().to_owned()],
                vec![],
                vec![Capability::Metadata, Capability::PreparePlans],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let principal = nested_registry
        .authenticate(created.credential.expose())
        .unwrap();
    let nested = nested_registry
        .plan_context(&principal, Capability::PreparePlans)
        .unwrap();
    std::fs::write(nested_state.join("private.data"), b"protected").unwrap();
    let index = voldiq_core::scanner::scan(
        fixture.source.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let selected = index
        .entries
        .iter()
        .find(|entry| entry.name == "private.data")
        .unwrap()
        .id;
    assert!(
        Store::open(&nested_state)
            .unwrap()
            .create(&nested, 1, &index, &[selected], 1000)
            .is_err()
    );
    assert!(
        plans
            .create(&nested, 1, &fixture.index, &[fixture.selected], 1000)
            .is_err()
    );
}

struct Fixture {
    state: tempfile::TempDir,
    source: tempfile::TempDir,
    registry: Registry,
    principal: voldiq_core::agents::Principal,
    index: voldiq_core::index::Index,
    selected: NodeId,
}

impl Fixture {
    fn new() -> Self {
        let state = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("selected.data"), b"original").unwrap();
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
        let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
        let created = registry
            .provision(
                "Fixture",
                Policy::new(
                    vec![source.path().to_owned()],
                    vec![],
                    vec![
                        Capability::Metadata,
                        Capability::PreparePlans,
                        Capability::RequestApproval,
                        Capability::ExecuteApproved,
                    ],
                    Limits::default(),
                )
                .unwrap(),
            )
            .unwrap();
        let principal = registry.authenticate(created.credential.expose()).unwrap();
        Self {
            state,
            source,
            registry,
            principal,
            index,
            selected,
        }
    }
}
