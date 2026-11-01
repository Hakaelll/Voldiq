use std::sync::atomic::AtomicBool;
use voldiq_core::{
    NodeId,
    agents::{Capability, Limits, Policy, Principal, Registry},
    index::Index,
    plans::{Begin, Enqueue, HumanAuthority, State, Store, View},
};

struct Fixture {
    index: Index,
    selected: NodeId,
    registry: Registry,
    principal: Principal,
    connection_id: i64,
    source: tempfile::TempDir,
    state: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let state = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("selected.data"), b"unchanged source").unwrap();
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
        let connection = registry
            .provision(
                "Execution fixture",
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
        let principal = registry
            .authenticate(connection.credential.expose())
            .unwrap();
        Self {
            state,
            source,
            index,
            selected,
            registry,
            principal,
            connection_id: connection.id,
        }
    }

    fn draft(&self, store: &mut Store) -> View {
        let prepare = self
            .registry
            .plan_context(&self.principal, Capability::PreparePlans)
            .unwrap();
        store
            .create(&prepare, 1, &self.index, &[self.selected], 1000)
            .unwrap()
    }

    fn approve(&self, store: &mut Store, human: &HumanAuthority, plan: &View, at: i64) {
        let request = self
            .registry
            .plan_context(&self.principal, Capability::RequestApproval)
            .unwrap();
        let metadata = self
            .registry
            .plan_context(&self.principal, Capability::Metadata)
            .unwrap();
        store.request(&request, &plan.manifest.id, at).unwrap();
        store
            .approve(&metadata, &plan.manifest.id, &plan.hash, human, at + 1)
            .unwrap();
    }

    fn unchanged(&self) {
        assert_eq!(
            std::fs::read(self.source.path().join("selected.data")).unwrap(),
            b"unchanged source"
        );
    }
}

#[test]
fn exact_requests_are_idempotent_and_cancellation_is_an_intent_during_execution() {
    let fixture = Fixture::new();
    let mut store = Store::open(fixture.state.path()).unwrap();
    let human = HumanAuthority::acquire(fixture.state.path()).unwrap();
    let plan = fixture.draft(&mut store);
    let id = &plan.manifest.id;
    let execute = fixture
        .registry
        .plan_context(&fixture.principal, Capability::ExecuteApproved)
        .unwrap();
    let metadata = fixture
        .registry
        .plan_context(&fixture.principal, Capability::Metadata)
        .unwrap();
    assert!(matches!(
        store.request_execution(&execute, id, &plan.hash, 1001),
        Err(voldiq_core::Error::NeedsHumanApproval)
    ));
    fixture.approve(&mut store, &human, &plan, 1002);
    assert!(
        store
            .request_execution(&execute, id, &"0".repeat(64), 1004)
            .is_err()
    );
    assert!(
        store
            .request_execution(&metadata, id, &plan.hash, 1004)
            .is_err()
    );
    assert!(matches!(
        store
            .request_execution(&execute, id, &plan.hash, 1004)
            .unwrap(),
        Enqueue::Queued
    ));
    assert!(matches!(
        store
            .request_execution(&execute, id, &plan.hash, 1005)
            .unwrap(),
        Enqueue::Queued
    ));
    let queue = store.pending_execution(&human, 25, 1006).unwrap();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].plan_id, *id);
    assert_eq!(queue[0].manifest_hash, plan.hash);
    assert_eq!(queue[0].requested, 1004);
    assert!(!queue[0].cancel_requested);
    assert_eq!(
        store
            .audit(&metadata, id, 0, 100)
            .unwrap()
            .iter()
            .filter(|event| event.event == "execution_requested")
            .count(),
        1
    );
    let Begin::Started(ticket) = store.begin(&execute, id, &human, 1007).unwrap() else {
        panic!("Expected one ticket")
    };
    assert!(
        store
            .pending_execution(&human, 25, 1008)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        store
            .request_execution(&execute, id, &plan.hash, 1008)
            .unwrap(),
        Enqueue::Existing(State::Executing)
    ));
    assert_eq!(
        store
            .cancel_execution(&execute, id, &plan.hash, 1009)
            .unwrap(),
        State::Executing
    );
    assert_eq!(
        store
            .cancel_execution(&execute, id, &plan.hash, 1010)
            .unwrap(),
        State::Executing
    );
    assert!(
        store
            .execution_request(&execute, id, &plan.hash)
            .unwrap()
            .unwrap()
            .cancel_requested
    );
    assert_eq!(
        store.get(&metadata, id, 1010).unwrap().state,
        State::Executing
    );
    assert_eq!(
        store
            .audit(&metadata, id, 0, 100)
            .unwrap()
            .iter()
            .filter(|event| event.event == "execution_cancel_requested")
            .count(),
        1
    );
    store
        .finish(
            ticket,
            State::Cancelled,
            serde_json::json!({"originals_admitted":0}),
            1011,
        )
        .unwrap();
    assert!(matches!(
        store
            .request_execution(&execute, id, &plan.hash, 1012)
            .unwrap(),
        Enqueue::Existing(State::Cancelled)
    ));
    fixture.unchanged();
}

#[test]
fn cancelled_and_old_session_requests_never_restart_after_renewed_approval() {
    let fixture = Fixture::new();
    let mut store = Store::open(fixture.state.path()).unwrap();
    let human = HumanAuthority::acquire(fixture.state.path()).unwrap();
    let cancelled = fixture.draft(&mut store);
    let renewed = fixture.draft(&mut store);
    fixture.approve(&mut store, &human, &cancelled, 1001);
    fixture.approve(&mut store, &human, &renewed, 1001);
    let execute = fixture
        .registry
        .plan_context(&fixture.principal, Capability::ExecuteApproved)
        .unwrap();
    store
        .request_execution(&execute, &cancelled.manifest.id, &cancelled.hash, 1004)
        .unwrap();
    store
        .request_execution(&execute, &renewed.manifest.id, &renewed.hash, 1004)
        .unwrap();
    assert_eq!(store.pending_execution(&human, 1, 1004).unwrap().len(), 1);
    assert_eq!(store.pending_execution(&human, 25, 1004).unwrap().len(), 2);
    assert_eq!(
        store
            .cancel_execution(&execute, &cancelled.manifest.id, &cancelled.hash, 1005)
            .unwrap(),
        State::Cancelled
    );
    assert!(matches!(
        store
            .begin(&execute, &cancelled.manifest.id, &human, 1006)
            .unwrap(),
        Begin::Existing(State::Cancelled)
    ));
    assert_eq!(
        store.pending_execution(&human, 1, 1006).unwrap()[0].plan_id,
        renewed.manifest.id
    );
    assert!(store.pending_execution(&human, 0, 1006).is_err());
    drop(human);
    let replacement = HumanAuthority::acquire(fixture.state.path()).unwrap();
    store.recover(&replacement, 1007).unwrap();
    assert!(
        store
            .pending_execution(&replacement, 25, 1008)
            .unwrap()
            .is_empty()
    );
    fixture.approve(&mut store, &replacement, &renewed, 1009);
    assert!(
        store
            .execution_request(&execute, &renewed.manifest.id, &renewed.hash)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .pending_execution(&replacement, 25, 1011)
            .unwrap()
            .is_empty()
    );
    store
        .request_execution(&execute, &renewed.manifest.id, &renewed.hash, 1012)
        .unwrap();
    assert_eq!(
        store.pending_execution(&replacement, 25, 1013).unwrap()[0].requested,
        1012
    );
    assert!(
        store
            .pending_execution(&replacement, 25, 1200)
            .unwrap()
            .is_empty()
    );
    let foreign = tempfile::tempdir().unwrap();
    let foreign_human = HumanAuthority::acquire(foreign.path()).unwrap();
    assert!(store.pending_execution(&foreign_human, 25, 1013).is_err());
    fixture.unchanged();
}

#[test]
fn legacy_plan_migration_preserves_evidence_and_queued_rows_never_prove_approval() {
    let fixture = Fixture::new();
    let mut store = Store::open(fixture.state.path()).unwrap();
    let plan = fixture.draft(&mut store);
    let metadata = fixture
        .registry
        .plan_context(&fixture.principal, Capability::Metadata)
        .unwrap();
    let before = store.audit(&metadata, &plan.manifest.id, 0, 100).unwrap();
    drop(store);
    let database = rusqlite::Connection::open(fixture.state.path().join("agent-plans.db")).unwrap();
    database
        .execute_batch("DROP TABLE execution_requests; PRAGMA user_version=1;")
        .unwrap();
    let mut migrated = Store::open(fixture.state.path()).unwrap();
    let preserved = migrated.get(&metadata, &plan.manifest.id, 1001).unwrap();
    assert_eq!(preserved.manifest, plan.manifest);
    assert_eq!(preserved.hash, plan.hash);
    let audit = migrated
        .audit(&metadata, &plan.manifest.id, 0, 100)
        .unwrap();
    assert_eq!(audit.len(), before.len());
    assert_eq!(audit[0].sequence, before[0].sequence);
    let first = HumanAuthority::acquire(fixture.state.path()).unwrap();
    fixture.approve(&mut migrated, &first, &plan, 1002);
    let execute = fixture
        .registry
        .plan_context(&fixture.principal, Capability::ExecuteApproved)
        .unwrap();
    migrated
        .request_execution(&execute, &plan.manifest.id, &plan.hash, 1004)
        .unwrap();
    drop(first);
    let replacement = HumanAuthority::acquire(fixture.state.path()).unwrap();
    migrated.recover(&replacement, 1005).unwrap();
    database.execute("UPDATE agent_plans SET state='approved',approval_issuer=?1,approval_expires=1120 WHERE id=?2",
        rusqlite::params![replacement.issuer(),plan.manifest.id]).unwrap();
    migrated
        .request_execution(&execute, &plan.manifest.id, &plan.hash, 1006)
        .unwrap();
    assert_eq!(
        migrated
            .pending_execution(&replacement, 25, 1007)
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        migrated.begin(&execute, &plan.manifest.id, &replacement, 1007),
        Err(voldiq_core::Error::NeedsHumanApproval)
    ));
    assert_eq!(
        migrated
            .get(&metadata, &plan.manifest.id, 1008)
            .unwrap()
            .state,
        State::Approved
    );
    drop(migrated);
    database.execute_batch("PRAGMA user_version=3;").unwrap();
    assert!(Store::open(fixture.state.path()).is_err());
    fixture.unchanged();
}

#[test]
fn requests_enforce_owner_revocation_and_current_material() {
    let mut fixture = Fixture::new();
    let other = fixture
        .registry
        .provision(
            "Other fixture",
            Policy::new(
                vec![fixture.source.path().to_owned()],
                vec![],
                vec![Capability::Metadata, Capability::ExecuteApproved],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let other_principal = fixture
        .registry
        .authenticate(other.credential.expose())
        .unwrap();
    let mut store = Store::open(fixture.state.path()).unwrap();
    let human = HumanAuthority::acquire(fixture.state.path()).unwrap();
    let plan = fixture.draft(&mut store);
    let changed = fixture.draft(&mut store);
    fixture.approve(&mut store, &human, &plan, 1001);
    fixture.approve(&mut store, &human, &changed, 1001);
    let execute = fixture
        .registry
        .plan_context(&fixture.principal, Capability::ExecuteApproved)
        .unwrap();
    let foreign = fixture
        .registry
        .plan_context(&other_principal, Capability::ExecuteApproved)
        .unwrap();
    assert!(
        store
            .request_execution(&foreign, &plan.manifest.id, &plan.hash, 1004)
            .is_err()
    );
    assert!(
        store
            .execution_request(&foreign, &plan.manifest.id, &plan.hash)
            .is_err()
    );
    assert!(
        store
            .cancel_execution(&foreign, &plan.manifest.id, &plan.hash, 1004)
            .is_err()
    );
    store
        .request_execution(&execute, &plan.manifest.id, &plan.hash, 1004)
        .unwrap();
    std::fs::write(
        fixture.source.path().join("selected.data"),
        b"changed owned fixture",
    )
    .unwrap();
    assert!(
        store
            .request_execution(&execute, &changed.manifest.id, &changed.hash, 1005)
            .is_err()
    );
    let metadata = fixture
        .registry
        .plan_context(&fixture.principal, Capability::Metadata)
        .unwrap();
    assert_eq!(
        store
            .get(&metadata, &changed.manifest.id, 1006)
            .unwrap()
            .state,
        State::Invalidated
    );
    let mut administrator = Registry::open(&fixture.state.path().join("agents.db")).unwrap();
    administrator.revoke(fixture.connection_id).unwrap();
    assert!(
        store
            .request_execution(&execute, &plan.manifest.id, &plan.hash, 1007)
            .is_err()
    );
    assert!(
        store
            .execution_request(&execute, &plan.manifest.id, &plan.hash)
            .is_err()
    );
    assert!(
        store
            .cancel_execution(&execute, &plan.manifest.id, &plan.hash, 1007)
            .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.source.path().join("selected.data")).unwrap(),
        b"changed owned fixture"
    );
}

#[test]
fn concurrent_requests_create_one_intent_and_one_audit_event() {
    let fixture = Fixture::new();
    let mut store = Store::open(fixture.state.path()).unwrap();
    let human = HumanAuthority::acquire(fixture.state.path()).unwrap();
    let plan = fixture.draft(&mut store);
    fixture.approve(&mut store, &human, &plan, 1001);
    let barrier = std::sync::Barrier::new(8);
    let path = fixture.state.path();
    let principal = &fixture.principal;
    let id = &plan.manifest.id;
    let hash = &plan.hash;
    std::thread::scope(|scope| {
        let threads = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let registry = Registry::open(&path.join("agents.db")).unwrap();
                    let execute = registry
                        .plan_context(principal, Capability::ExecuteApproved)
                        .unwrap();
                    let mut store = Store::open(path).unwrap();
                    barrier.wait();
                    assert!(matches!(
                        store.request_execution(&execute, id, hash, 1004).unwrap(),
                        Enqueue::Queued
                    ));
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap();
        }
    });
    let metadata = fixture
        .registry
        .plan_context(principal, Capability::Metadata)
        .unwrap();
    assert_eq!(store.pending_execution(&human, 25, 1005).unwrap().len(), 1);
    assert_eq!(
        store
            .audit(&metadata, id, 0, 100)
            .unwrap()
            .iter()
            .filter(|event| event.event == "execution_requested")
            .count(),
        1
    );
    fixture.unchanged();
}
