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
    {
        use crate::{
            transfer::{Phase, copy_bytes, verify_bytes},
            windows::{self, cleanup_io, owned_file::OwnedFile},
        };
        use std::{
            ffi::OsStr,
            fs::File,
            io::{Cursor, Seek, SeekFrom, Write},
            sync::atomic::Ordering,
        };
        use windows_sys::Win32::Storage::FileSystem::*;

        // Owned byte transactions only: no transfer grant, catalogue or original-handling authority.
        let destination = tempfile::tempdir().unwrap();
        let input = source.path().join("copy-fixture.data");
        let payload: Vec<u8> = (0..262_177).map(|i| (i % 251) as u8).collect();
        std::fs::write(&input, &payload).unwrap();
        let pin = crate::agents::pin_directory(destination.path()).unwrap();
        let parent = cleanup_io::reopen_directory(
            &windows::open_metadata(pin.path()).unwrap(),
            FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | FILE_ADD_FILE | FILE_TRAVERSE,
        )
        .unwrap();
        let cancelled = AtomicBool::new(false);
        let mut owner = OwnedFile::create(&parent).unwrap();
        let mut phases = Vec::new();
        let copied = copy_bytes(
            &mut File::open(&input).unwrap(),
            &mut owner.file(),
            262_177,
            &cancelled,
            || owner.validate(),
            |value| phases.push(value),
        )
        .unwrap();
        assert_eq!(copied.bytes(), 262_177);
        assert_eq!(copied.digest(), blake3::hash(&payload).to_hex().as_str());
        owner.file().sync_all().unwrap();
        owner.file().seek(SeekFrom::Start(0)).unwrap();
        verify_bytes(
            &mut owner.file(),
            &copied,
            &cancelled,
            || owner.validate(),
            |value| phases.push(value),
        )
        .unwrap();
        assert_eq!(phases.first().unwrap().phase, Phase::Copying);
        assert_eq!(phases.last().unwrap().phase, Phase::Readback);
        assert_eq!(phases.last().unwrap().bytes, 262_177);
        assert!(phases.iter().all(|value| value.total == 262_177));
        assert!(phases.iter().any(|value| value.bytes == 65_536));
        for name in [
            "../escape.data",
            "nested/copy.data",
            "copy.data/",
            "CON.txt",
            "copy.data:stream",
        ] {
            assert!(owner.publish(OsStr::new(name)).is_err());
        }
        let output = destination.path().join("copy.data");
        owner.publish(OsStr::new("copy.data")).unwrap();
        assert!(owner.publish(OsStr::new("renamed.data")).is_err());
        owner.abort().unwrap(); // Published output must not be removed by cleanup or Drop.
        drop(owner);
        assert_eq!(std::fs::read(&output).unwrap(), payload);

        let mut collision = OwnedFile::create(&parent).unwrap();
        collision.file().write_all(b"must not replace").unwrap();
        assert!(collision.publish(OsStr::new("copy.data")).is_err());
        collision.abort().unwrap();
        assert!(collision.publish(OsStr::new("resurrected.data")).is_err());
        drop(collision);
        assert_eq!(std::fs::read(&output).unwrap(), payload);

        let mut interrupted = OwnedFile::create(&parent).unwrap();
        assert!(
            copy_bytes(
                &mut File::open(&input).unwrap(),
                &mut interrupted.file(),
                262_177,
                &cancelled,
                || interrupted.validate(),
                |value| {
                    if value.bytes >= 65_536 {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                },
            )
            .is_err()
        );
        assert_eq!(interrupted.file().metadata().unwrap().len(), 65_536);
        interrupted.abort().unwrap();
        drop(interrupted);
        cancelled.store(false, Ordering::Relaxed);

        let mut damaged = OwnedFile::create(&parent).unwrap();
        let copied = copy_bytes(
            &mut File::open(&input).unwrap(),
            &mut damaged.file(),
            262_177,
            &cancelled,
            || damaged.validate(),
            |_| {},
        )
        .unwrap();
        damaged.file().seek(SeekFrom::Start(65_536)).unwrap();
        damaged.file().write_all(b"changed").unwrap();
        damaged.file().sync_all().unwrap();
        damaged.file().seek(SeekFrom::Start(0)).unwrap();
        assert!(
            verify_bytes(
                &mut damaged.file(),
                &copied,
                &cancelled,
                || damaged.validate(),
                |_| {},
            )
            .is_err()
        );
        // Failed readback owns only a private destination; original and published output survive.
        damaged.abort().unwrap();
        drop(damaged);
        let mut short = Cursor::new(b"short");
        let mut sink = Vec::new();
        assert!(copy_bytes(&mut short, &mut sink, 6, &cancelled, || Ok(()), |_| {}).is_err());
        assert_eq!(sink, b"short");
        assert!(
            copy_bytes(
                &mut Cursor::new(b"extra"),
                &mut Vec::new(),
                4,
                &cancelled,
                || Ok(()),
                |_| {}
            )
            .is_err()
        );
        assert!(
            copy_bytes(
                &mut Cursor::new(b"cannot write"),
                &mut File::open(&input).unwrap(),
                12,
                &cancelled,
                || Ok(()),
                |_| {}
            )
            .is_err()
        );
        let mut denied = Vec::new();
        assert!(
            copy_bytes(
                &mut Cursor::new(b"denied"),
                &mut denied,
                6,
                &cancelled,
                || Err(crate::Error::Invalid("Authority withdrawn".into())),
                |_| {}
            )
            .is_err()
        );
        assert!(denied.is_empty());
        assert!(
            copy_bytes(
                &mut Cursor::new(b"cancel"),
                &mut denied,
                6,
                &cancelled,
                || {
                    cancelled.store(true, Ordering::Relaxed);
                    Ok(())
                },
                |_| {}
            )
            .is_err()
        );
        assert!(denied.is_empty());
        cancelled.store(false, Ordering::Relaxed);
        assert!(
            verify_bytes(
                &mut Cursor::new(&payload[..262_176]),
                &copied,
                &cancelled,
                || Ok(()),
                |_| {}
            )
            .is_err()
        );
        let mut extra = payload.clone();
        extra.push(1);
        assert!(
            verify_bytes(
                &mut Cursor::new(extra),
                &copied,
                &cancelled,
                || Ok(()),
                |_| {}
            )
            .is_err()
        );
        assert!(
            verify_bytes(
                &mut Cursor::new(&payload),
                &copied,
                &cancelled,
                || Ok(()),
                |value| {
                    if value.bytes >= 65_536 {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&input).unwrap(), payload);
        assert_eq!(std::fs::read(&output).unwrap(), payload);
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 1);
        println!(
            "TRANSFER_BYTES_PROBE {{\"copied_bytes\":262177,\"readback\":true,\"original_unchanged\":true,\"transfer_execution\":false}}"
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
