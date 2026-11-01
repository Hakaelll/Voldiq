#![cfg(windows)]
use std::sync::atomic::AtomicBool;
use voldiq_core::cleanup::{
    Plan,
    journal::Journal,
    recycle::{
        ShellApartment,
        batch::{Batch, Outcome},
    },
};

#[test]
fn one_changed_file_blocks_the_whole_selection_and_cancellation_preserves_every_source() {
    let source = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("a.data"), b"original a").unwrap();
    std::fs::write(source.path().join("b.data"), b"original b").unwrap();
    let index = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let selection = index
        .entries
        .iter()
        .filter(|entry| entry.kind == voldiq_core::Kind::File)
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let plan = Plan::manual(&index, &selection).unwrap();
    std::fs::write(source.path().join("b.data"), b"changed b").unwrap();
    let apartment = ShellApartment::new().unwrap();
    let cancel = AtomicBool::new(false);
    let mut journal = Journal::open(&state.path().join("cleanup.db")).unwrap();
    let batch = Batch::prepare(plan, &mut journal, &cancel, &apartment).unwrap();
    assert!(!batch.ready_for_admission());
    let report = batch.abort(&mut journal, &cancel, |_| {});
    assert_eq!(report.items.len(), 2);
    assert!(
        report
            .items
            .iter()
            .all(|item| item.outcome == Outcome::NotStarted)
    );
    assert!(report.items.iter().any(|item| item.error.is_some()));
    assert!(report.preflight_error.is_some());
    assert!(report.items.iter().all(|item| item.cleanup_error.is_none()));
    assert_eq!(
        std::fs::read(source.path().join("a.data")).unwrap(),
        b"original a"
    );
    assert_eq!(
        std::fs::read(source.path().join("b.data")).unwrap(),
        b"changed b"
    );
    assert!(std::fs::read_dir(source.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("voldiq-stage-")
    }));
    assert!(
        journal
            .relocations(0, 100)
            .unwrap()
            .iter()
            .all(|entry| entry.state == "cancelled")
    );
}

#[test]
fn complete_preflight_retains_all_source_handles_until_abort() {
    let source = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let original = source.path().join("a.data");
    std::fs::write(&original, b"fixture").unwrap();
    let index = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let selection = index
        .entries
        .iter()
        .filter(|entry| entry.kind == voldiq_core::Kind::File)
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let apartment = ShellApartment::new().unwrap();
    let cancel = AtomicBool::new(false);
    let mut journal = Journal::open(&state.path().join("cleanup.db")).unwrap();
    let batch = Batch::prepare(
        Plan::manual(&index, &selection).unwrap(),
        &mut journal,
        &cancel,
        &apartment,
    )
    .unwrap();
    assert!(batch.ready_for_admission());
    assert!(std::fs::rename(&original, source.path().join("replacement.data")).is_err());
    let report = batch.abort(&mut journal, &cancel, |_| {});
    assert_eq!(report.items[0].outcome, Outcome::NotStarted);
    assert!(report.items[0].recorded);
    std::fs::rename(&original, source.path().join("replacement.data")).unwrap();
    assert_eq!(
        std::fs::read(source.path().join("replacement.data")).unwrap(),
        b"fixture"
    );
}

#[test]
fn an_approved_ticket_prepares_only_its_exact_selection_and_persists_agent_receipts() {
    use voldiq_core::{
        agents::{Capability, Limits, Policy, Registry},
        plans::{Begin, HumanAuthority, State, Store},
    };
    for (blocked_writer, delayed) in [(false, false), (true, false), (false, true)] {
        let state = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.data"), b"original a").unwrap();
        std::fs::write(source.path().join("b.data"), b"original b").unwrap();
        let index = voldiq_core::scanner::scan(
            source.path(),
            &Default::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        let selection = index
            .entries
            .iter()
            .filter(|entry| entry.kind == voldiq_core::Kind::File)
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
        let created = registry
            .provision(
                "Preflight fixture",
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let approved_at = if delayed { now - 300 } else { now };
        let mut plans = Store::open(state.path()).unwrap();
        let plan = plans
            .create(&prepare, 1, &index, &selection, approved_at)
            .unwrap();
        let id = &plan.manifest.id;
        plans.request(&request, id, approved_at).unwrap();
        let human = HumanAuthority::acquire(state.path()).unwrap();
        plans
            .approve(&metadata, id, &plan.hash, &human, approved_at)
            .unwrap();
        plans
            .request_execution(&execute, id, &plan.hash, approved_at)
            .unwrap();
        let Begin::Started(lease) = plans.begin(&execute, id, &human, approved_at).unwrap() else {
            panic!("Missing execution ticket")
        };
        let apartment = ShellApartment::new().unwrap();
        let cancel = AtomicBool::new(false);
        let mut journal = Journal::open(&state.path().join("cleanup.db")).unwrap();
        let _writer = blocked_writer.then(|| {
            std::fs::OpenOptions::new()
                .write(true)
                .open(source.path().join("b.data"))
                .unwrap()
        });
        let operation = voldiq_core::operations::Lease::acquire(state.path()).unwrap();
        let worker = voldiq_core::plans::RecyclingWorker::new(&operation, &cancel, &apartment);
        let prepared = plans.prepare_recycling(&execute, &human, lease, &mut journal, worker);
        if delayed {
            assert!(prepared.is_err());
            assert_eq!(
                plans.get(&metadata, id, now).unwrap().state,
                State::Invalidated
            );
        } else if blocked_writer {
            assert!(prepared.is_err());
            assert_eq!(plans.get(&metadata, id, now).unwrap().state, State::Failed);
        } else {
            let prepared = prepared.unwrap();
            let receipts = journal.agent_relocations(id, &plan.hash).unwrap();
            assert_eq!(receipts.len(), 2);
            assert!(receipts.iter().all(
                |receipt| receipt.connection_id == created.id && receipt.state == "stage_ready"
            ));
            assert!(
                std::fs::rename(
                    source.path().join("a.data"),
                    source.path().join("renamed.data")
                )
                .is_err()
            );
            let report = prepared.cancel(&mut plans, &mut journal).unwrap();
            assert!(report.cancelled);
            assert!(
                report
                    .items
                    .iter()
                    .all(|item| item.outcome == Outcome::NotStarted)
            );
            assert_eq!(
                plans.get(&metadata, id, now).unwrap().state,
                State::Cancelled
            );
        }
        assert!(
            journal
                .agent_relocations(id, &plan.hash)
                .unwrap()
                .iter()
                .all(|receipt| receipt.state == "cancelled")
        );
        drop(journal);
        let journal = Journal::open(&state.path().join("cleanup.db")).unwrap();
        assert_eq!(
            journal.agent_relocations(id, &plan.hash).unwrap().len(),
            if delayed {
                0
            } else if blocked_writer {
                1
            } else {
                2
            }
        );
        assert_eq!(
            std::fs::read(source.path().join("a.data")).unwrap(),
            b"original a"
        );
        assert_eq!(
            std::fs::read(source.path().join("b.data")).unwrap(),
            b"original b"
        );
        assert!(std::fs::read_dir(source.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("voldiq-stage-")
        }));
    }
}
