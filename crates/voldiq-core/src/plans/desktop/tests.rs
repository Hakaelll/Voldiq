use super::*;
use crate::{
    agents::{Limits, Policy},
    cleanup::{journal::Journal, recycle::ShellApartment},
    operations::Lease,
    service::AgentService,
};
use std::sync::atomic::AtomicBool;

#[test]
fn desktop_dispatch_consumes_only_a_live_grant_and_persists_the_real_helper_result() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let original = source.path().join("selected.data");
    std::fs::write(&original, b"owned dispatch fixture").unwrap();
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
    let snapshot = crate::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save(&index, &AtomicBool::new(false), &[])
        .unwrap();
    let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
    let issued = registry
        .provision(
            "Dispatch fixture",
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
    let service = AgentService::new(
        state.path().to_owned(),
        registry.authenticate(issued.credential.expose()).unwrap(),
    )
    .unwrap();
    let plan = service.plan_prepare(snapshot, &[selected]).unwrap();
    {
        use crate::plans::{
            Action, OffloadRequest, Originals,
            offload::{Destination, layout},
        };
        let destination = tempfile::tempdir().unwrap();
        let request = OffloadRequest {
            destination: destination.path().to_owned(),
            originals: Originals::Keep,
        };
        assert!(
            service
                .plan_prepare_offload(snapshot, &[selected], &request)
                .is_err()
        );
        let root = source.path().ancestors().last().unwrap();
        let volume_reader = registry
            .provision(
                "Volume transfer fixture",
                Policy::new(
                    vec![root.to_owned()],
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
        let volume_service = AgentService::new(
            state.path().to_owned(),
            registry
                .authenticate(volume_reader.credential.expose())
                .unwrap(),
        )
        .unwrap();
        let same_volume = volume_service
            .plan_prepare_offload(snapshot, &[selected], &request)
            .err()
            .unwrap()
            .to_string();
        assert!(same_volume.contains("different volume"));
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 0);
        let pin = crate::agents::pin_directory(destination.path()).unwrap();
        let observed = Destination::observe(&pin, state.path()).unwrap();
        let mut shape = serde_json::to_value(&observed).unwrap();
        assert!(
            shape["volume"]["guid_root"]
                .as_str()
                .unwrap()
                .starts_with(r"\\?\volume{")
        );
        assert_eq!(
            shape["volume"]["serial"],
            format!("{:016x}", pin.identity().0)
        );
        shape["allocation_unit_bytes"] = serde_json::json!(4096);
        let model: Destination = serde_json::from_value(shape.clone()).unwrap();
        let (outputs, required) = layout(&plan.manifest.entries, &model).unwrap();
        assert_eq!(required, 16_785_408);
        assert_eq!(
            serde_json::to_value(outputs).unwrap(),
            serde_json::json!([{"id":selected,"name":"selected.data"}])
        );
        let mut collision = plan.manifest.entries.clone();
        let mut second = collision[0].clone();
        second.id = crate::NodeId(u32::MAX);
        second.path = source
            .path()
            .join("SELECTED.DATA")
            .to_str()
            .unwrap()
            .to_owned();
        collision.push(second);
        assert!(layout(&collision, &model).is_err());
        shape["filesystem"] = serde_json::json!("FAT32");
        let fat: Destination = serde_json::from_value(shape.clone()).unwrap();
        let mut large = plan.manifest.entries.clone();
        large[0].logical_bytes = "4294967296".into();
        assert!(layout(&large, &fat).is_err());
        shape["maximum_name_units"] = serde_json::json!(2);
        assert!(
            layout(
                &plan.manifest.entries,
                &serde_json::from_value(shape).unwrap()
            )
            .is_err()
        );
        let mut alias = plan.manifest.entries.clone();
        alias[0].path = source.path().join("CON.txt").to_str().unwrap().to_owned();
        assert!(layout(&alias, &model).is_err());
        let state_pin = crate::agents::pin_directory(state.path()).unwrap();
        assert!(Destination::observe(&state_pin, state.path()).is_err());
        assert_eq!(
            serde_json::to_value(&plan.manifest.action).unwrap(),
            "recycle"
        );
        assert!(Action::Recycle.execution_available());
        let mut offload = serde_json::json!({"source_volume":serde_json::to_value(&observed).unwrap()["volume"],"destination":observed,
            "originals":"keep","outputs":[],"required_available_bytes":"0","reserve_bytes":"0","limitations":"model only"});
        let action: Action =
            serde_json::from_value(serde_json::json!({"offload":offload})).unwrap();
        assert!(!action.execution_available());
        offload["originals"] = serde_json::json!("delete_permanently");
        assert!(serde_json::from_value::<Action>(serde_json::json!({"offload":offload})).is_err());
        println!(
            "OFFLOAD_PLANNING_PROBE {{\"native_destination\":true,\"same_volume_rejected\":true,\"real_cross_volume_transfer\":false}}"
        );
    }
    assert!(matches!(
        service.plan_execute(&plan.manifest.id, &plan.hash),
        Err(crate::Error::NeedsHumanApproval)
    ));
    let desktop = Desktop::open(state.path()).unwrap();
    assert!(service.plan_request(&plan.manifest.id).is_err());
    desktop.approve(&plan.manifest.id, &plan.hash).unwrap();
    let queued = service.plan_execute(&plan.manifest.id, &plan.hash).unwrap();
    assert_eq!(queued["dispatch"], "queued");
    assert_eq!(desktop.pending_execution(25).unwrap().len(), 1);
    let operation = Lease::acquire(state.path()).unwrap();
    let cancel = AtomicBool::new(false);
    let apartment = ShellApartment::new().unwrap();
    let mut journal = Journal::open(&state.path().join("cleanup.db")).unwrap();
    let mut launches = 0;
    assert!(
        desktop
            .execute_request_using(
                &plan.manifest.id,
                &"0".repeat(64),
                &mut journal,
                super::super::RecyclingWorker::new(&operation, &cancel, &apartment),
                |_| {},
                |_| panic!("Wrong hash reached the helper")
            )
            .is_err()
    );
    let result = desktop
        .execute_request_using(
            &plan.manifest.id,
            &plan.hash,
            &mut journal,
            super::super::RecyclingWorker::new(&operation, &cancel, &apartment),
            |_| {},
            |_| {
                launches += 1;
                Err(std::io::Error::from_raw_os_error(1223))
            },
        )
        .unwrap();
    assert_eq!(launches, 1);
    assert_eq!(result.state, State::Cancelled);
    assert_eq!(result.result.as_ref().unwrap()["confirmed_admissions"], 0);
    assert!(desktop.pending_execution(25).unwrap().is_empty());
    let replay = service.plan_execute(&plan.manifest.id, &plan.hash).unwrap();
    assert_eq!(replay["dispatch"], "existing");
    assert_eq!(replay["plan"]["state"], "cancelled");
    let next = service.plan_prepare(snapshot, &[selected]).unwrap();
    assert!(service.plan_request(&next.manifest.id).is_err());
    desktop.approve(&next.manifest.id, &next.hash).unwrap();
    service.plan_execute(&next.manifest.id, &next.hash).unwrap();
    desktop.close();
    // Other availability readers must not look like an independent human approver.
    let availability_reader = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.path().join("approval.lock"))
        .unwrap();
    availability_reader.lock_shared().unwrap();
    assert!(!super::super::HumanAuthority::is_active(state.path()).unwrap());
    assert!(matches!(
        service.plan_execute(&next.manifest.id, &next.hash),
        Err(crate::Error::NeedsHumanApproval)
    ));
    assert!(
        desktop
            .execute_request_using(
                &next.manifest.id,
                &next.hash,
                &mut journal,
                super::super::RecyclingWorker::new(&operation, &cancel, &apartment),
                |_| {},
                |_| panic!("Closed session reached the helper")
            )
            .is_err()
    );
    assert_eq!(std::fs::read(original).unwrap(), b"owned dispatch fixture");
}
