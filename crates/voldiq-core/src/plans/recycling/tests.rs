use super::*;
use crate::agents::{Capability, Limits, Policy, Principal, Registry};

// Close native directory pins before TempDir tries to remove its owned fixtures.
struct Fixture {
    store: Store,
    registry: Registry,
    human: HumanAuthority,
    lease: ExecutionLease,
    principal: Principal,
    owner: i64,
    source: tempfile::TempDir,
    state: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let state = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("selected.data"), b"runtime fixture").unwrap();
        let index = crate::scanner::scan(
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
        let provisioned = registry
            .provision(
                "Runtime fixture",
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
            .authenticate(provisioned.credential.expose())
            .unwrap();
        let mut store = Store::open(state.path()).unwrap();
        let human = HumanAuthority::acquire(state.path()).unwrap();
        let at = now().unwrap();
        let plan = store
            .create(
                &registry
                    .plan_context(&principal, Capability::PreparePlans)
                    .unwrap(),
                1,
                &index,
                &[selected],
                at,
            )
            .unwrap();
        store
            .request(
                &registry
                    .plan_context(&principal, Capability::RequestApproval)
                    .unwrap(),
                &plan.manifest.id,
                at,
            )
            .unwrap();
        store
            .approve(
                &registry
                    .plan_context(&principal, Capability::Metadata)
                    .unwrap(),
                &plan.manifest.id,
                &plan.hash,
                &human,
                at,
            )
            .unwrap();
        let execute = registry
            .plan_context(&principal, Capability::ExecuteApproved)
            .unwrap();
        store
            .request_execution(&execute, &plan.manifest.id, &plan.hash, at)
            .unwrap();
        let super::super::Begin::Started(lease) = store
            .begin(&execute, &plan.manifest.id, &human, at)
            .unwrap()
        else {
            panic!("Expected one live ticket")
        };
        Self {
            store,
            registry,
            human,
            lease,
            principal,
            owner: provisioned.id,
            source,
            state,
        }
    }
    fn unchanged(&self) {
        assert_eq!(
            std::fs::read(self.source.path().join("selected.data")).unwrap(),
            b"runtime fixture"
        );
    }
}

#[test]
fn runtime_authority_rechecks_session_deadline_policy_request_user_and_protections() {
    for change in [
        "cancel",
        "revoke",
        "rotate",
        "close",
        "expire",
        "clock",
        "request",
        "state",
        "user",
        "protection",
        "slow",
        "closed_after_query",
    ] {
        let mut fixture = Fixture::new();
        let execute = fixture
            .registry
            .plan_context(&fixture.principal, Capability::ExecuteApproved)
            .unwrap();
        let mut at = now().unwrap();
        assert!(matches!(
            fixture
                .store
                .check_execution(&execute, &fixture.human, &fixture.lease, || Ok(at))
                .unwrap(),
            ExecutionControl::Proceed
        ));
        match change {
            "cancel" => {
                fixture
                    .store
                    .cancel_execution(&execute, &fixture.lease.id, &fixture.lease.hash, at)
                    .unwrap();
            }
            "revoke" => {
                Registry::open(&fixture.state.path().join("agents.db"))
                    .unwrap()
                    .revoke(fixture.owner)
                    .unwrap();
            }
            "rotate" => {
                Registry::open(&fixture.state.path().join("agents.db"))
                    .unwrap()
                    .rotate(fixture.owner)
                    .unwrap();
            }
            "close" => fixture.human.close(),
            "expire" => at = fixture.lease.runtime.deadline,
            "clock" => at = fixture.lease.runtime.created - 1,
            "request" => {
                rusqlite::Connection::open(fixture.state.path().join("agent-plans.db"))
                    .unwrap()
                    .execute(
                        "DELETE FROM execution_requests WHERE plan_id=?1",
                        [&fixture.lease.id],
                    )
                    .unwrap();
            }
            "state" => {
                rusqlite::Connection::open(fixture.state.path().join("agent-plans.db"))
                    .unwrap()
                    .execute(
                        "UPDATE agent_plans SET state='cancelled' WHERE id=?1",
                        [&fixture.lease.id],
                    )
                    .unwrap();
            }
            "user" => fixture.lease.runtime.user = "wrong user".into(),
            "protection" => {
                crate::metadata::Store::open(fixture.state.path())
                    .unwrap()
                    .protect_folder(fixture.source.path())
                    .unwrap();
            }
            "slow" | "closed_after_query" => {}
            _ => unreachable!(),
        }
        let mut samples = 0;
        let checked =
            fixture
                .store
                .check_execution(&execute, &fixture.human, &fixture.lease, || {
                    samples += 1;
                    if samples == 2 && change == "slow" {
                        return Ok(fixture.lease.runtime.deadline);
                    }
                    if samples == 2 && change == "closed_after_query" {
                        fixture.human.close();
                    }
                    Ok(at)
                });
        if change == "cancel" {
            assert!(matches!(checked.unwrap(), ExecutionControl::Cancel));
        } else {
            assert!(checked.is_err(), "{change} did not stop authority");
        }
        fixture.unchanged();
    }
}

#[test]
fn worker_state_must_match_and_shared_exclusion_lasts_through_preparation() {
    for foreign in [true, false] {
        let mut fixture = Fixture::new();
        let other = tempfile::tempdir().unwrap();
        let directory = if foreign {
            other.path()
        } else {
            fixture.state.path()
        };
        let operation = crate::operations::Lease::acquire(directory).unwrap();
        let cancel = AtomicBool::new(false);
        let apartment = ShellApartment::new().unwrap();
        let mut journal = Journal::open(&fixture.state.path().join("cleanup.db")).unwrap();
        let execute = fixture
            .registry
            .plan_context(&fixture.principal, Capability::ExecuteApproved)
            .unwrap();
        let prepared = fixture.store.prepare_recycling(
            &execute,
            &fixture.human,
            fixture.lease,
            &mut journal,
            RecyclingWorker::new(&operation, &cancel, &apartment),
        );
        if foreign {
            assert!(prepared.is_err());
            assert!(journal.relocations(0, 25).unwrap().is_empty());
        } else {
            let prepared = prepared.unwrap();
            assert!(crate::operations::Lease::acquire(directory).is_err());
            assert!(
                crate::metadata::Store::open(directory)
                    .unwrap()
                    .save_note(fixture.source.path(), "busy")
                    .is_err()
            );
            let report = prepared.cancel(&mut fixture.store, &mut journal).unwrap();
            assert!(
                report
                    .items
                    .iter()
                    .all(|item| item.outcome == Outcome::NotStarted && item.recorded)
            );
            drop(operation);
            assert!(crate::operations::Lease::acquire(directory).is_ok());
        }
        assert_eq!(
            std::fs::read(fixture.source.path().join("selected.data")).unwrap(),
            b"runtime fixture"
        );
    }
}

#[test]
fn unknown_results_and_partial_recycling_stay_explicit_in_bounded_summaries() {
    use crate::cleanup::recycle::batch::ItemResult;
    let mut report = Report {
        items: vec![],
        cancelled: false,
        journal_error: None,
        helper_error: None,
        preflight_error: None,
    };
    for id in 0..500 {
        report.items.push(ItemResult {
            id: crate::NodeId(id),
            path: "private path".into(),
            outcome: Outcome::Recycled,
            recorded: true,
            operation: Some(123),
            bin_path: Some(vec![65]),
            error: None,
            cleanup_error: None,
        });
    }
    assert_eq!(outcome(&report, false), State::Completed);
    report.items[499].outcome = Outcome::Unknown;
    report.cancelled = true;
    assert_eq!(outcome(&report, false), State::PartiallyCompleted);
    let summary = execution_summary(&report);
    assert_eq!(summary["confirmed_admissions"], 499);
    assert_eq!(summary["unknown"], 1);
    assert_eq!(summary["immediate_free_bytes"], "0");
    let encoded = serde_json::to_string(&summary).unwrap();
    assert!(encoded.len() <= 65536);
    assert!(!encoded.contains("private path") && !encoded.contains("bin_path"));
    report.items.drain(0..499);
    assert_eq!(outcome(&report, true), State::Failed);
    report.items[0].outcome = Outcome::Returned;
    assert_eq!(outcome(&report, false), State::Cancelled);
    report.journal_error = Some("Owned fixture journal unavailable".into());
    assert_eq!(outcome(&report, false), State::Failed);
    report.journal_error = None;
    report.cancelled = false;
    assert_eq!(outcome(&report, true), State::Invalidated);
}

#[test]
fn runtime_fixture_releases_directory_pins_before_removing_owned_files() {
    let fixture = Fixture::new();
    let state = fixture.state.path().to_owned();
    let source = fixture.source.path().to_owned();
    drop(fixture);
    assert!(!state.exists() && !source.exists());
}
