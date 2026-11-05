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
        {
            use crate::plans::{TransferPhase, TransferUpdate, catalogue};
            // Historical database model only. The fictional volume below never authorizes I/O.
            let mut target = serde_json::to_value(&observed).unwrap();
            target["volume"]["serial"] = serde_json::json!("0123456789abcdef");
            target["volume"]["guid_root"] =
                serde_json::json!(r"\\?\volume{00000000-0000-0000-0000-000000000001}\");
            target["native_identity"] =
                serde_json::json!("efcdab896745230111111111111111111111111111111111");
            let target_model: Destination = serde_json::from_value(target.clone()).unwrap();
            let mut historical = plan.manifest.clone();
            historical.id = "c".repeat(64);
            historical.schema_version = 3;
            historical.logical_bytes = "44".into();
            historical.potential_reclaim_bytes = "0".into();
            let mut second = historical.entries[0].clone();
            second.id = crate::NodeId(u32::MAX - 1);
            second.path = source.path().join("second.data").to_str().unwrap().into();
            let second_id = second.id;
            historical.entries.push(second);
            let (outputs, required) = layout(&historical.entries, &target_model).unwrap();
            assert_eq!(required, 16_793_600);
            historical.action = serde_json::from_value(serde_json::json!({"offload":{
                "source_volume":serde_json::to_value(&observed).unwrap()["volume"],
                "destination":target,"originals":"keep","outputs":outputs,
                "required_available_bytes":required.to_string(),"reserve_bytes":"16777216",
                "limitations":super::super::offload::LIMITATIONS
            }}))
            .unwrap();
            let json = serde_json::to_string(&historical).unwrap();
            let roundtrip: crate::plans::Manifest = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&roundtrip).unwrap(), json);
            assert_eq!(roundtrip.action.schema_version(), 3);
            assert!(
                !serde_json::to_value(&roundtrip).unwrap()["action"]["offload"]
                    .as_object()
                    .unwrap()
                    .contains_key("metadata")
            );
            let hash = super::super::digest(json.as_bytes());
            let ledger = Store::open(state.path()).unwrap();
            ledger.connection.execute("INSERT INTO agent_plans(id,owner,manifest,hash,native_record,binding,state,approval_issuer,execution_nonce)
                SELECT ?1,owner,?2,?3,native_record,binding,'executing','prior catalogue fixture','fixture nonce' FROM agent_plans WHERE id=?4",
                rusqlite::params![historical.id,json,hash,plan.manifest.id]).unwrap();
            // Model a version-2 database with a pre-existing proposal, then run the real migration.
            ledger.connection.execute_batch("DROP TABLE transfer_files; DROP TABLE transfer_operations; PRAGMA user_version=2;").unwrap();
            drop(ledger);
            let mut ledger = Store::open(state.path()).unwrap();
            assert_eq!(
                ledger
                    .connection
                    .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                3
            );
            let human = HumanAuthority::acquire(state.path()).unwrap();
            let first = ledger
                .transfer_catalogue(&human, &historical.id, None, 1)
                .unwrap();
            assert_eq!(first.files.len(), 1);
            assert_eq!(first.files[0].node, selected);
            assert_eq!(first.files[0].phase, TransferPhase::Prepared);
            assert_eq!(first.next_after, Some(selected));
            assert_eq!(first.currently_connected, None);
            assert!(!first.restore_available);
            assert_eq!(first.last_observed_at, observed.observed_at);
            let second_page = ledger
                .transfer_catalogue(&human, &historical.id, first.next_after, 1)
                .unwrap();
            assert_eq!(second_page.files[0].node, second_id);
            assert!(second_page.next_after.is_none());
            assert!(
                ledger
                    .transfer_catalogue(&human, &historical.id, None, 0)
                    .is_err()
            );
            assert!(
                ledger
                    .transfer_catalogue(&human, &historical.id, None, 101)
                    .is_err()
            );
            assert!(
                ledger
                    .transfer_catalogue(&human, &plan.manifest.id, None, 10)
                    .is_err()
            );
            {
                let principal = registry.authenticate(issued.credential.expose()).unwrap();
                let metadata_context = registry
                    .plan_context(&principal, Capability::Metadata)
                    .unwrap();
                let metadata = super::super::offload::MetadataPlan::capture(
                    &metadata_context,
                    &plan.manifest.entries,
                )
                .unwrap();
                let value = serde_json::to_value(&metadata).unwrap();
                assert_eq!(value["profile"], "ntfs_basic_v1");
                assert_eq!(value["files"][0]["id"], selected.0);
                let mut profiled = historical.clone();
                profiled.id = "e".repeat(64);
                profiled.entries.truncate(1);
                profiled.logical_bytes = "22".into();
                let Action::Offload(action) = &mut profiled.action else {
                    unreachable!()
                };
                action.outputs.truncate(1);
                action.metadata = Some(metadata);
                let mut action_value = serde_json::to_value(&profiled.action).unwrap();
                action_value["offload"]["required_available_bytes"] = serde_json::json!("16785408");
                profiled.action = serde_json::from_value(action_value).unwrap();
                profiled.schema_version = profiled.action.schema_version();
                assert_eq!(profiled.schema_version, 4);
                let encoded = serde_json::to_string(&profiled).unwrap();
                let profiled_hash = super::super::digest(encoded.as_bytes());
                assert_ne!(profiled_hash, hash);
                let tx = ledger.connection.transaction().unwrap();
                tx.execute("INSERT INTO agent_plans(id,owner,manifest,hash,native_record,binding,state)
                    SELECT ?1,owner,?2,?3,native_record,binding,'draft' FROM agent_plans WHERE id=?4",
                    rusqlite::params![profiled.id,encoded,profiled_hash,plan.manifest.id]).unwrap();
                catalogue::prepare(&tx, &profiled, &profiled_hash).unwrap();
                tx.commit().unwrap();
                let recorded = ledger
                    .transfer_catalogue(&human, &profiled.id, None, 10)
                    .unwrap();
                assert_eq!(recorded.manifest.schema_version, 4);
                assert_eq!(recorded.manifest_hash, profiled_hash);
                assert!(!recorded.manifest.action.execution_available());
                assert_eq!(
                    serde_json::to_value(&recorded.manifest).unwrap()["action"]["offload"]["metadata"],
                    value
                );
                assert_eq!(serde_json::to_string(&recorded.manifest).unwrap(), encoded);
                let mut bounded = serde_json::to_value(&profiled).unwrap();
                bounded["id"] = serde_json::json!("1".repeat(64));
                bounded["schema_version"] = serde_json::json!(5);
                bounded["action"]["offload"]["execution"] =
                    serde_json::json!({"maximum_seconds":14400});
                let bounded: crate::plans::Manifest = serde_json::from_value(bounded).unwrap();
                assert_eq!(bounded.action.schema_version(), 5);
                let encoded = serde_json::to_string(&bounded).unwrap();
                let bounded_hash = super::super::digest(encoded.as_bytes());
                assert_ne!(bounded_hash, profiled_hash);
                let tx = ledger.connection.transaction().unwrap();
                tx.execute("INSERT INTO agent_plans(id,owner,manifest,hash,native_record,binding,state)
                    SELECT ?1,owner,?2,?3,native_record,binding,'draft' FROM agent_plans WHERE id=?4",
                    rusqlite::params![bounded.id,encoded,bounded_hash,plan.manifest.id]).unwrap();
                catalogue::prepare(&tx, &bounded, &bounded_hash).unwrap();
                tx.commit().unwrap();
                let recorded = ledger
                    .transfer_catalogue(&human, &bounded.id, None, 10)
                    .unwrap();
                assert_eq!(recorded.manifest_hash, bounded_hash);
                assert!(!recorded.manifest.action.execution_available());
                // Clock-policy model only; no synthetic execution ticket or offload grant.
                use super::super::ExecutionWindow;
                let at = bounded.created;
                let mut window = ExecutionWindow::new(&bounded.action, at + 120, at).unwrap();
                assert_eq!(window.admission_expires, at + 120);
                assert_eq!(window.deadline, at + 14400);
                assert!(window.check(at + 121).is_ok());
                assert!(window.check(at + 14399).is_ok());
                assert!(window.check(at + 14400).is_err());
                assert!(window.check(at - 1).is_err());
                assert!(ExecutionWindow::new(&bounded.action, at, at).is_err());
                assert!(ExecutionWindow::new(&profiled.action, at + 120, at).is_err());
                assert!(ExecutionWindow::new(&bounded.action, i64::MAX, i64::MAX - 1).is_err());
                window.started = window
                    .started
                    .checked_sub(std::time::Duration::from_secs(14401))
                    .unwrap();
                assert!(window.check(at + 1).is_err());
                let recycle = ExecutionWindow::new(&Action::Recycle, at + 120, at).unwrap();
                assert!(recycle.check(at + 119).is_ok());
                assert!(recycle.check(at + 120).is_err());
                let mut altered = serde_json::to_value(&bounded.action).unwrap();
                altered["offload"]["execution"]["maximum_seconds"] = serde_json::json!(14401);
                let Action::Offload(unsupported) =
                    serde_json::from_value::<Action>(altered).unwrap()
                else {
                    unreachable!()
                };
                assert!(unsupported.check_shape(&bounded.entries).is_err());
                {
                    let volume: super::super::VolumeBinding = serde_json::from_value(
                        serde_json::to_value(&observed).unwrap()["volume"].clone(),
                    )
                    .unwrap();
                    let recorded_root = source.path().ancestors().last().unwrap();
                    let stale_path = std::path::Path::new("Z:\\")
                        .join(source.path().strip_prefix(recorded_root).unwrap());
                    let resolved = volume.restore_parent_path(&stale_path).unwrap();
                    assert_eq!(
                        crate::windows::identity(&resolved).unwrap().key,
                        crate::windows::identity(source.path()).unwrap().key
                    );
                    let record =
                        crate::cleanup::record::Record::capture(&index, &[selected]).unwrap();
                    record
                        .check_restore_ancestry(
                            selected,
                            &crate::agents::pin_directory(&resolved).unwrap(),
                        )
                        .unwrap();
                    let mut reordered = serde_json::to_value(&record).unwrap();
                    reordered["files"][0]["parents"]
                        .as_array_mut()
                        .unwrap()
                        .swap(0, 1);
                    let reordered: crate::cleanup::record::Record =
                        serde_json::from_value(reordered).unwrap();
                    assert!(
                        reordered
                            .check_restore_ancestry(
                                selected,
                                &crate::agents::pin_directory(&resolved).unwrap()
                            )
                            .is_err()
                    );
                    assert!(
                        record
                            .check_restore_ancestry(
                                selected,
                                &crate::agents::pin_directory(destination.path()).unwrap()
                            )
                            .is_err()
                    );
                    println!(
                        "RESTORE_VOLUME_RESOLUTION_PROBE {{\"recorded_letter_ignored\":true,\"current_guid_mount_resolved\":true,\"ordered_parent_ancestry_checked\":true,\"reordered_ancestry_refused\":true,\"replacement_parent_refused\":true,\"volume_mounted\":false,\"physical_reconnect_tested\":false}}"
                    );
                }
                {
                    // Serialization/capacity model only. Fictional destination volumes never grant I/O.
                    let group = serde_json::to_value(&bounded.action).unwrap()["offload"].clone();
                    let metadata = group["metadata"]["files"][0]["metadata"].clone();
                    let name = std::path::Path::new(&bounded.entries[0].path)
                        .file_name()
                        .unwrap();
                    let target =
                        std::path::Path::new(group["destination"]["path"].as_str().unwrap())
                            .join(name);
                    let reference = serde_json::json!({"source_id":selected.0,"original_id":12345,
                        "original_path":target,"archive_identity":metadata["native_identity"],"content_blake3":"a".repeat(64),
                        "bytes":"22","original_metadata":metadata});
                    let mut proposed = serde_json::json!({"transfer_plan_id":bounded.id,"transfer_manifest_hash":bounded_hash,
                        "files":[reference],"groups":[group],"required_available_bytes":"16785408",
                        "limitations":super::super::restoration::LIMITATIONS});
                    let restore: crate::plans::Restore =
                        serde_json::from_value(proposed.clone()).unwrap();
                    restore.check_shape(&bounded.entries).unwrap();
                    let action = Action::Restore(Box::new(restore));
                    assert_eq!(action.schema_version(), 6);
                    assert!(!action.execution_available());
                    let encoded = serde_json::to_string(&action).unwrap();
                    assert_eq!(
                        serde_json::to_string(&serde_json::from_str::<Action>(&encoded).unwrap())
                            .unwrap(),
                        encoded
                    );
                    let mut second = bounded.entries[0].clone();
                    second.id = crate::NodeId(u32::MAX - 2);
                    second.path = source
                        .path()
                        .join("restore-second.data")
                        .to_str()
                        .unwrap()
                        .into();
                    let mut group = proposed["groups"][0].clone();
                    let second_parent = destination.path().join("second-parent");
                    group["destination"]["path"] = serde_json::json!(second_parent);
                    group["destination"]["native_identity"] =
                        serde_json::json!("efcdab896745230122222222222222222222222222222222");
                    group["outputs"][0] =
                        serde_json::json!({"id":second.id.0,"name":"restore-second.data"});
                    group["metadata"]["files"][0]["id"] = serde_json::json!(second.id.0);
                    let mut reference = proposed["files"][0].clone();
                    reference["source_id"] = serde_json::json!(second.id.0);
                    reference["original_id"] = serde_json::json!(12346);
                    reference["original_path"] =
                        serde_json::json!(second_parent.join("restore-second.data"));
                    proposed["files"].as_array_mut().unwrap().push(reference);
                    proposed["groups"].as_array_mut().unwrap().push(group);
                    proposed["required_available_bytes"] = serde_json::json!("16793600");
                    let entries = [bounded.entries[0].clone(), second];
                    serde_json::from_value::<crate::plans::Restore>(proposed.clone())
                        .unwrap()
                        .check_shape(&entries)
                        .unwrap();
                    proposed["groups"][1]["destination"]["caller_available_bytes"] =
                        serde_json::json!("16785408");
                    assert!(
                        serde_json::from_value::<crate::plans::Restore>(proposed.clone())
                            .unwrap()
                            .check_shape(&entries)
                            .is_err()
                    );
                    proposed["groups"][1]["destination"]["caller_available_bytes"] =
                        serde_json::json!("16793600");
                    proposed["files"][1]["archive_identity"] = serde_json::json!("0".repeat(48));
                    assert!(
                        serde_json::from_value::<crate::plans::Restore>(proposed)
                            .unwrap()
                            .check_shape(&entries)
                            .is_err()
                    );
                    assert!(serde_json::from_value::<crate::plans::RestoreRequest>(serde_json::json!({"transfer_plan_id":bounded.id,"node_ids":[selected],"approved":true})).is_err());
                    let context = registry
                        .plan_context(&principal, Capability::PreparePlans)
                        .unwrap();
                    let refusal = ledger
                        .create_restore(
                            &context,
                            snapshot,
                            &index,
                            &crate::plans::RestoreRequest {
                                transfer_plan_id: bounded.id.clone(),
                                node_ids: vec![selected],
                            },
                            super::super::now().unwrap(),
                        )
                        .err()
                        .unwrap();
                    assert!(
                        refusal.to_string().contains("confirmed publication"),
                        "{refusal}"
                    );
                    assert_eq!(std::fs::read(&original).unwrap(), b"owned dispatch fixture");
                    println!(
                        "RESTORE_PROPOSAL_MODEL_PROBE {{\"schema\":6,\"groups\":2,\"shared_reserve_bytes\":16777216,\"required_bytes\":16793600,\"per_group_capacity_overcommit_refused\":true,\"archive_identity_mismatch_refused\":true,\"approval_injection_refused\":true,\"unpublished_catalogue_refused\":true,\"real_restore\":false}}"
                    );
                }
                println!(
                    "TRANSFER_EXECUTION_WINDOW_PROBE {{\"admission_seconds\":120,\"maximum_seconds\":14400,\"post_admission_checkpoint\":true,\"wall_and_monotonic_limits\":true,\"legacy_3_4_unchanged\":true,\"real_offload\":false}}"
                );
                let mut mismatched = profiled.clone();
                mismatched.id = "f".repeat(64);
                let Action::Offload(action) = &mut mismatched.action else {
                    unreachable!()
                };
                let mut altered = serde_json::to_value(&action.metadata).unwrap();
                altered["files"][0]["metadata"]["dos_attributes"] = serde_json::json!(0x4000);
                action.metadata = Some(serde_json::from_value(altered).unwrap());
                let invalid_json = serde_json::to_string(&mismatched).unwrap();
                let invalid_hash = super::super::digest(invalid_json.as_bytes());
                let tx = ledger.connection.transaction().unwrap();
                tx.execute("INSERT INTO agent_plans(id,owner,manifest,hash,native_record,binding,state)
                    SELECT ?1,owner,?2,?3,native_record,binding,'draft' FROM agent_plans WHERE id=?4",
                    rusqlite::params![mismatched.id,invalid_json,invalid_hash,plan.manifest.id]).unwrap();
                assert!(catalogue::prepare(&tx, &mismatched, &invalid_hash).is_err());
            }
            let at = historical.created;
            {
                let tx = ledger.connection.transaction().unwrap();
                assert!(
                    catalogue::apply_update(
                        &tx,
                        &historical,
                        &"0".repeat(64),
                        selected,
                        TransferUpdate::Published,
                        at
                    )
                    .is_err()
                );
                assert!(
                    catalogue::apply_update(
                        &tx,
                        &historical,
                        &hash,
                        selected,
                        TransferUpdate::Published,
                        at
                    )
                    .is_err()
                );
                catalogue::apply_update(
                    &tx,
                    &historical,
                    &hash,
                    selected,
                    TransferUpdate::Private {
                        name: ".voldiq-output-11111111111111111111111111111111.tmp".into(),
                        identity: "efcdab896745230122222222222222222222222222222222".into(),
                    },
                    at,
                )
                .unwrap();
                // Deliberately roll back an interrupted metadata transaction.
            }
            assert_eq!(
                ledger
                    .transfer_catalogue(&human, &historical.id, None, 1)
                    .unwrap()
                    .files[0]
                    .phase,
                TransferPhase::Prepared
            );
            {
                let tx = ledger.connection.transaction().unwrap();
                for node in [selected, second_id] {
                    assert!(
                        catalogue::apply_update(
                            &tx,
                            &historical,
                            &hash,
                            node,
                            TransferUpdate::Private {
                                name: "../unowned".into(),
                                identity: "0".repeat(48)
                            },
                            at
                        )
                        .is_err()
                    );
                    catalogue::apply_update(
                        &tx,
                        &historical,
                        &hash,
                        node,
                        TransferUpdate::Private {
                            name: format!(".voldiq-output-{:032x}.tmp", node.0),
                            identity: "efcdab896745230122222222222222222222222222222222".into(),
                        },
                        at,
                    )
                    .unwrap();
                    assert!(
                        catalogue::apply_update(
                            &tx,
                            &historical,
                            &hash,
                            node,
                            TransferUpdate::Verified {
                                bytes: 21,
                                blake3: "a".repeat(64)
                            },
                            at
                        )
                        .is_err()
                    );
                    catalogue::apply_update(
                        &tx,
                        &historical,
                        &hash,
                        node,
                        TransferUpdate::Verified {
                            bytes: 22,
                            blake3: "a".repeat(64),
                        },
                        at,
                    )
                    .unwrap();
                    assert!(
                        catalogue::apply_update(
                            &tx,
                            &historical,
                            &hash,
                            node,
                            TransferUpdate::Published,
                            at
                        )
                        .is_err()
                    );
                    catalogue::apply_update(
                        &tx,
                        &historical,
                        &hash,
                        node,
                        TransferUpdate::PublishIntent,
                        at,
                    )
                    .unwrap();
                }
                catalogue::apply_update(
                    &tx,
                    &historical,
                    &hash,
                    selected,
                    TransferUpdate::Published,
                    at,
                )
                .unwrap();
                assert!(
                    catalogue::apply_update(
                        &tx,
                        &historical,
                        &hash,
                        selected,
                        TransferUpdate::OriginalIntent,
                        at
                    )
                    .is_err()
                );
                catalogue::apply_update(
                    &tx,
                    &historical,
                    &hash,
                    selected,
                    TransferUpdate::Kept,
                    at,
                )
                .unwrap();
                assert!(
                    catalogue::apply_update(
                        &tx,
                        &historical,
                        &hash,
                        selected,
                        TransferUpdate::Published,
                        at
                    )
                    .is_err()
                );
                tx.commit().unwrap();
            }
            {
                let tx = ledger.connection.transaction().unwrap();
                assert!(
                    catalogue::completion_state(
                        &tx,
                        &historical.id,
                        historical.connection_id,
                        &hash,
                        State::Completed,
                        at
                    )
                    .is_err()
                );
                assert_eq!(
                    catalogue::completion_state(
                        &tx,
                        &historical.id,
                        historical.connection_id,
                        &hash,
                        State::Failed,
                        at
                    )
                    .unwrap(),
                    State::PartiallyCompleted
                );
                // Roll back completion bookkeeping before testing confirmed full completion.
            }
            {
                let tx = ledger.connection.transaction().unwrap();
                catalogue::apply_update(
                    &tx,
                    &historical,
                    &hash,
                    second_id,
                    TransferUpdate::Published,
                    at,
                )
                .unwrap();
                catalogue::apply_update(
                    &tx,
                    &historical,
                    &hash,
                    second_id,
                    TransferUpdate::Kept,
                    at,
                )
                .unwrap();
                assert_eq!(
                    catalogue::completion_state(
                        &tx,
                        &historical.id,
                        historical.connection_id,
                        &hash,
                        State::Completed,
                        at
                    )
                    .unwrap(),
                    State::Completed
                );
                // Keep the persisted fixture interrupted between publish intent and confirmation.
            }
            let mut recycling = historical.clone();
            recycling.id = "d".repeat(64);
            recycling.potential_reclaim_bytes = super::super::store::totals(&recycling.entries)
                .unwrap()
                .1
                .to_string();
            let Action::Offload(action) = &mut recycling.action else {
                unreachable!()
            };
            action.originals = Originals::RecycleAfterVerification;
            let recycling_json = serde_json::to_string(&recycling).unwrap();
            let recycling_hash = super::super::digest(recycling_json.as_bytes());
            {
                let tx = ledger.connection.transaction().unwrap();
                tx.execute("INSERT INTO agent_plans(id,owner,manifest,hash,native_record,binding,state,approval_issuer,execution_nonce)
                    SELECT ?1,owner,?2,?3,native_record,binding,'executing','prior catalogue fixture','fixture nonce' FROM agent_plans WHERE id=?4",
                    rusqlite::params![recycling.id,recycling_json,recycling_hash,plan.manifest.id]).unwrap();
                catalogue::prepare(&tx, &recycling, &recycling_hash).unwrap();
                for node in [selected, second_id] {
                    for update in [
                        TransferUpdate::Private {
                            name: format!(".voldiq-output-{:032x}.tmp", node.0),
                            identity: "efcdab896745230122222222222222222222222222222222".into(),
                        },
                        TransferUpdate::Verified {
                            bytes: 22,
                            blake3: "b".repeat(64),
                        },
                        TransferUpdate::PublishIntent,
                    ] {
                        catalogue::apply_update(&tx, &recycling, &recycling_hash, node, update, at)
                            .unwrap();
                    }
                }
                catalogue::apply_update(
                    &tx,
                    &recycling,
                    &recycling_hash,
                    selected,
                    TransferUpdate::Published,
                    at,
                )
                .unwrap();
                assert!(
                    catalogue::apply_update(
                        &tx,
                        &recycling,
                        &recycling_hash,
                        selected,
                        TransferUpdate::OriginalIntent,
                        at
                    )
                    .is_err()
                );
                catalogue::apply_update(
                    &tx,
                    &recycling,
                    &recycling_hash,
                    second_id,
                    TransferUpdate::Published,
                    at,
                )
                .unwrap();
                catalogue::apply_update(
                    &tx,
                    &recycling,
                    &recycling_hash,
                    selected,
                    TransferUpdate::OriginalIntent,
                    at,
                )
                .unwrap();
                catalogue::apply_update(
                    &tx,
                    &recycling,
                    &recycling_hash,
                    selected,
                    TransferUpdate::Recycled,
                    at,
                )
                .unwrap();
                catalogue::apply_update(
                    &tx,
                    &recycling,
                    &recycling_hash,
                    second_id,
                    TransferUpdate::OriginalIntent,
                    at,
                )
                .unwrap();
                assert!(
                    catalogue::completion_state(
                        &tx,
                        &recycling.id,
                        recycling.connection_id,
                        &recycling_hash,
                        State::Completed,
                        at
                    )
                    .is_err()
                );
                tx.commit().unwrap();
            }
            ledger.recover(&human, at + 1).unwrap();
            drop(ledger);
            let mut ledger = Store::open(state.path()).unwrap();
            let saved = ledger
                .transfer_catalogue(&human, &historical.id, None, 100)
                .unwrap();
            assert_eq!(saved.state, State::PartiallyCompleted);
            assert_eq!(saved.files[0].phase, TransferPhase::Kept);
            assert_eq!(
                saved.files[0].content_blake3.as_deref(),
                Some("a".repeat(64).as_str())
            );
            assert_eq!(saved.files[0].published_at, Some(at));
            assert_eq!(saved.files[0].interrupted_at, None);
            assert_eq!(saved.files[1].phase, TransferPhase::PublishPending);
            assert_eq!(saved.files[1].interrupted_at, Some(at + 1));
            {
                let tx = ledger.connection.transaction().unwrap();
                assert!(
                    catalogue::apply_update(
                        &tx,
                        &historical,
                        &hash,
                        second_id,
                        TransferUpdate::Published,
                        at + 2
                    )
                    .is_err()
                );
            }
            let recycled_model = ledger
                .transfer_catalogue(&human, &recycling.id, None, 100)
                .unwrap();
            assert_eq!(recycled_model.state, State::PartiallyCompleted);
            assert_eq!(recycled_model.files[0].phase, TransferPhase::Recycled);
            assert_eq!(
                recycled_model.files[1].phase,
                TransferPhase::OriginalPending
            );
            assert_eq!(recycled_model.files[1].interrupted_at, Some(at + 1));
            assert!(service.plan_execute(&historical.id, &hash).is_err());
            assert_eq!(std::fs::read(&original).unwrap(), b"owned dispatch fixture");
            assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 0);
            drop(pin);
            destination.close().unwrap();
            assert_eq!(
                ledger
                    .transfer_catalogue(&human, &historical.id, None, 100)
                    .unwrap()
                    .files
                    .len(),
                2
            );
            human.close();
            assert!(
                ledger
                    .transfer_catalogue(&human, &historical.id, None, 100)
                    .is_err()
            );
            println!(
                "TRANSFER_CATALOGUE_PROBE {{\"migration\":true,\"rollback\":true,\"offline_read\":true,\"partial_recovery\":true,\"real_transfer\":false}}"
            );
        }
        println!(
            "OFFLOAD_PLANNING_PROBE {{\"native_destination\":true,\"same_volume_rejected\":true,\"real_cross_volume_transfer\":false}}"
        );
    }
    {
        use crate::{
            plans::TransferUpdate,
            plans::copying::{Control, CopyPublication, copy_verified},
            transfer::{Phase, copy_bytes, verify_bytes},
            windows::{self, cleanup_io, owned_file::OwnedFile},
        };
        use std::{
            ffi::OsStr,
            fs::File,
            io::{Cursor, Write},
            sync::atomic::Ordering,
        };
        use windows_sys::Win32::Storage::FileSystem::*;
        // Internal producer fixtures only; public offload admission and original handling remain closed.
        let destination = tempfile::tempdir().unwrap();
        let input = source.path().join("copy-fixture.data");
        let payload: Vec<u8> = (0..262_177).map(|i| (i % 251) as u8).collect();
        std::fs::write(&input, &payload).unwrap();
        let metadata_handle = windows::open_metadata(&input).unwrap();
        assert_eq!(
            windows::security::granted_access(&metadata_handle).unwrap() & FILE_READ_DATA,
            0
        );
        let metadata = crate::transfer::metadata::BasicMetadata::observe(&metadata_handle).unwrap();
        let copy_index = crate::scanner::scan(
            source.path(),
            &Default::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        let copy_id = copy_index
            .entries
            .iter()
            .find(|entry| entry.name == "copy-fixture.data")
            .unwrap()
            .id;
        let copy_plan = crate::cleanup::Plan::manual(&copy_index, &[copy_id]).unwrap();
        let pin = crate::agents::pin_directory(destination.path()).unwrap();
        let parent = cleanup_io::reopen_directory(
            &windows::open_metadata(pin.path()).unwrap(),
            FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | FILE_ADD_FILE | FILE_TRAVERSE,
        )
        .unwrap();
        let cancelled = AtomicBool::new(false);
        let expected_hash = blake3::hash(&payload).to_hex().to_string();
        let mut events = Vec::new();
        let mut phases = Vec::new();
        let native = copy_verified(
            copy_plan.copy_source(copy_id).unwrap(),
            metadata.clone(),
            OwnedFile::create(&parent).unwrap(),
            OsStr::new("copy.data"),
            &cancelled,
            |control| {
                match control {
                    Control::Check => {}
                    Control::Record(update) => events.push(match update {
                        TransferUpdate::Private { name, identity } => {
                            assert!(name.starts_with(".voldiq-output-"));
                            assert_eq!(identity.len(), 48);
                            "private"
                        }
                        TransferUpdate::Verified { bytes, blake3 } => {
                            assert_eq!(bytes, 262_177);
                            assert_eq!(blake3, expected_hash);
                            "verified"
                        }
                        TransferUpdate::PublishIntent => "publish_intent",
                        TransferUpdate::Published => "published",
                        _ => panic!("Content producer attempted an original phase"),
                    }),
                }
                Ok(())
            },
            |value| phases.push(value),
        )
        .unwrap();
        native.check().unwrap();
        assert_eq!(native.bytes(), 262_177);
        assert_eq!(native.blake3(), expected_hash);
        assert_eq!(
            events,
            ["private", "verified", "publish_intent", "published"]
        );
        assert!(
            phases
                .iter()
                .any(|value| value.phase == Phase::Readback && value.bytes == 262_177)
        );
        assert!(
            phases
                .iter()
                .any(|value| value.phase == Phase::SourceRecheck && value.bytes == 262_177)
        );
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&input)
                .is_err()
        );
        let output = destination.path().join("copy.data");
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&output)
                .is_err()
        );
        drop(native);
        assert_eq!(std::fs::read(&input).unwrap(), payload);
        assert_eq!(std::fs::read(&output).unwrap(), payload);
        assert!(std::fs::OpenOptions::new().write(true).open(&input).is_ok());
        {
            // Owned same-volume content fixtures, not an approved offload or source-removal path.
            use super::super::batch::copy_all;
            let nodes = [crate::NodeId(1), crate::NodeId(2), crate::NodeId(3)];
            let copies = copy_all(&nodes, |node| {
                copy_verified(
                    copy_plan.copy_source(copy_id).unwrap(),
                    metadata.clone(),
                    OwnedFile::create(&parent).unwrap(),
                    OsStr::new(&format!("batch-complete-{}.data", node.0)),
                    &cancelled,
                    |_| Ok(()),
                    |_| {},
                )
            })
            .ok()
            .unwrap();
            assert_eq!(copies.len(), 3);
            assert_eq!(copies.iter().map(|copy| copy.bytes()).sum::<u64>(), 786531);
            for copy in &copies {
                copy.check().unwrap();
                assert_eq!(copy.blake3(), expected_hash);
            }
            drop(copies);
            let mut attempted = Vec::new();
            let stopped = copy_all(&nodes, |node| {
                attempted.push(node);
                copy_verified(
                    copy_plan.copy_source(copy_id).unwrap(),
                    metadata.clone(),
                    OwnedFile::create(&parent).unwrap(),
                    OsStr::new(&format!("batch-partial-{}.data", node.0)),
                    &cancelled,
                    |control| {
                        if node == crate::NodeId(2)
                            && matches!(control, Control::Record(TransferUpdate::Verified { .. }))
                        {
                            return Err(crate::Error::Invalid(
                                "Fixture second copy receipt failed".into(),
                            ));
                        }
                        Ok(())
                    },
                    |_| {},
                )
            })
            .err()
            .unwrap();
            assert_eq!(attempted, [crate::NodeId(1), crate::NodeId(2)]);
            assert_eq!(stopped.copies.len(), 1);
            assert_eq!(stopped.node, crate::NodeId(2));
            assert_eq!(stopped.error.publication(), CopyPublication::NotAttempted);
            assert!(stopped.error.cleanup_error().is_none());
            stopped.copies[0].check().unwrap();
            drop(stopped);
            assert_eq!(
                std::fs::read(destination.path().join("batch-partial-1.data")).unwrap(),
                payload
            );
            assert!(!destination.path().join("batch-partial-2.data").exists());
            assert!(!destination.path().join("batch-partial-3.data").exists());
            assert_eq!(std::fs::read(&input).unwrap(), payload);
            println!(
                "NATIVE_COPY_BATCH_PROBE {{\"completed_copies\":3,\"verified_bytes\":786531,\"partial_copies\":1,\"stopped_before_third\":true,\"failed_private_removed\":true,\"original_unchanged\":true,\"real_offload\":false}}"
            );
        }
        let failure = copy_verified(
            copy_plan.copy_source(copy_id).unwrap(),
            metadata.clone(),
            OwnedFile::create(&parent).unwrap(),
            OsStr::new("not-published.data"),
            &cancelled,
            |control| {
                if matches!(control, Control::Record(TransferUpdate::Verified { .. })) {
                    return Err(crate::Error::Invalid(
                        "Fixture catalogue write failed".into(),
                    ));
                }
                Ok(())
            },
            |_| {},
        )
        .err()
        .unwrap();
        assert_eq!(failure.publication(), CopyPublication::NotAttempted);
        assert!(failure.cleanup_error().is_none());
        assert!(!destination.path().join("not-published.data").exists());
        let failure = copy_verified(
            copy_plan.copy_source(copy_id).unwrap(),
            metadata.clone(),
            OwnedFile::create(&parent).unwrap(),
            OsStr::new("published-unrecorded.data"),
            &cancelled,
            |control| {
                if matches!(control, Control::Record(TransferUpdate::Published)) {
                    return Err(crate::Error::Invalid(
                        "Fixture publication receipt failed".into(),
                    ));
                }
                Ok(())
            },
            |_| {},
        )
        .err()
        .unwrap();
        assert_eq!(failure.publication(), CopyPublication::Confirmed);
        assert!(failure.cleanup_error().is_none());
        assert_eq!(
            std::fs::read(destination.path().join("published-unrecorded.data")).unwrap(),
            payload
        );
        let failure = copy_verified(
            copy_plan.copy_source(copy_id).unwrap(),
            metadata.clone(),
            OwnedFile::create(&parent).unwrap(),
            OsStr::new("copy.data"),
            &cancelled,
            |_| Ok(()),
            |_| {},
        )
        .err()
        .unwrap();
        assert_eq!(failure.publication(), CopyPublication::Uncertain);
        assert!(failure.cleanup_error().is_none());
        assert_eq!(std::fs::read(&output).unwrap(), payload);
        let failure = copy_verified(
            copy_plan.copy_source(copy_id).unwrap(),
            metadata.clone(),
            OwnedFile::create(&parent).unwrap(),
            OsStr::new("cancelled.data"),
            &cancelled,
            |_| Ok(()),
            |value| {
                if value.phase == Phase::Copying && value.bytes >= 65_536 {
                    cancelled.store(true, Ordering::Relaxed);
                }
            },
        )
        .err()
        .unwrap();
        assert_eq!(failure.publication(), CopyPublication::NotAttempted);
        assert!(failure.cleanup_error().is_none());
        cancelled.store(false, Ordering::Relaxed);
        let mut terminal = OwnedFile::create(&parent).unwrap();
        terminal
            .file()
            .write_all(b"owned terminal fixture")
            .unwrap();
        for name in [
            "../escape.data",
            "nested/copy.data",
            "copy.data/",
            "CON.txt",
            "copy.data:stream",
        ] {
            assert!(terminal.publish(OsStr::new(name)).is_err());
        }
        terminal.publish(OsStr::new("terminal.data")).unwrap();
        assert!(terminal.publish(OsStr::new("renamed.data")).is_err());
        terminal.abort().unwrap();
        drop(terminal);
        assert_eq!(
            std::fs::read(destination.path().join("terminal.data")).unwrap(),
            b"owned terminal fixture"
        );
        let mut aborted = OwnedFile::create(&parent).unwrap();
        aborted.abort().unwrap();
        assert!(aborted.publish(OsStr::new("resurrected.data")).is_err());
        drop(aborted);
        let mut bytes = Vec::new();
        let copied = copy_bytes(
            &mut Cursor::new(&payload),
            &mut bytes,
            262_177,
            &cancelled,
            || Ok(()),
            |_| {},
        )
        .unwrap();
        assert_eq!(bytes, payload);
        let mut corrupt = payload.clone();
        corrupt[65_536] ^= 1;
        assert!(
            verify_bytes(
                &mut Cursor::new(corrupt),
                &copied,
                &cancelled,
                || Ok(()),
                |_| {}
            )
            .is_err()
        );
        assert!(
            copy_bytes(
                &mut Cursor::new(b"short"),
                &mut Vec::new(),
                6,
                &cancelled,
                || Ok(()),
                |_| {}
            )
            .is_err()
        );
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
        let mut remaining = std::fs::read_dir(destination.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        remaining.sort();
        assert_eq!(
            remaining,
            [
                "batch-complete-1.data",
                "batch-complete-2.data",
                "batch-complete-3.data",
                "batch-partial-1.data",
                "copy.data",
                "published-unrecorded.data",
                "terminal.data"
            ]
        );
        {
            use crate::transfer::metadata::{BasicMetadata, DOS_FLAGS};
            use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
            let profiled = source.path().join("metadata-flags.data");
            std::fs::write(&profiled, b"owned basic metadata fixture").unwrap();
            let attributes = std::fs::OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .open(&profiled)
                .unwrap();
            let wanted = FILE_BASIC_INFO {
                CreationTime: 132_000_000_000_000_000,
                LastWriteTime: 133_000_000_000_000_000,
                FileAttributes: DOS_FLAGS,
                ..Default::default()
            };
            assert_ne!(
                unsafe {
                    SetFileInformationByHandle(
                        attributes.as_raw_handle(),
                        FileBasicInfo,
                        (&wanted as *const FILE_BASIC_INFO).cast(),
                        size_of::<FILE_BASIC_INFO>() as u32,
                    )
                },
                0
            );
            let observed = BasicMetadata::observe(&attributes).unwrap();
            assert_eq!(observed.created_windows_100ns(), Some("132000000000000000"));
            assert_eq!(observed.modified_windows_100ns(), "133000000000000000");
            assert_eq!(observed.dos_attributes(), DOS_FLAGS);
            let index = crate::scanner::scan(
                source.path(),
                &Default::default(),
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
            let id = index
                .entries
                .iter()
                .find(|entry| entry.name == "metadata-flags.data")
                .unwrap()
                .id;
            let planned = crate::cleanup::Plan::manual(&index, &[id]).unwrap();
            let evidence = copy_verified(
                planned.copy_source(id).unwrap(),
                observed.clone(),
                OwnedFile::create(&parent).unwrap(),
                OsStr::new("profiled.data"),
                &AtomicBool::new(false),
                |_| Ok(()),
                |_| {},
            )
            .unwrap();
            evidence.check().unwrap();
            drop(evidence);
            let output = windows::open_metadata(&destination.path().join("profiled.data")).unwrap();
            observed.verify_destination(&output).unwrap();
            assert_eq!(
                std::fs::read(destination.path().join("profiled.data")).unwrap(),
                b"owned basic metadata fixture"
            );
            assert_eq!(
                std::fs::read(&profiled).unwrap(),
                b"owned basic metadata fixture"
            );
            let mut stale = serde_json::to_value(&observed).unwrap();
            stale["modified_windows_100ns"] = serde_json::json!("133000000000000001");
            let stale: BasicMetadata = serde_json::from_value(stale).unwrap();
            let failed = copy_verified(
                planned.copy_source(id).unwrap(),
                stale,
                OwnedFile::create(&parent).unwrap(),
                OsStr::new("stale-metadata.data"),
                &AtomicBool::new(false),
                |_| Ok(()),
                |_| {},
            )
            .err()
            .unwrap();
            assert_eq!(failed.publication(), CopyPublication::NotAttempted);
            assert!(!destination.path().join("stale-metadata.data").exists());
            drop(output);
            // Reset only owned fixture attributes so ordinary temporary-directory cleanup remains possible.
            let normal = FILE_BASIC_INFO {
                FileAttributes: FILE_ATTRIBUTE_NORMAL,
                ..Default::default()
            };
            assert_ne!(
                unsafe {
                    SetFileInformationByHandle(
                        attributes.as_raw_handle(),
                        FileBasicInfo,
                        (&normal as *const FILE_BASIC_INFO).cast(),
                        size_of::<FILE_BASIC_INFO>() as u32,
                    )
                },
                0
            );
            let output = cleanup_io::open_component(
                &parent,
                OsStr::new("profiled.data"),
                FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            )
            .unwrap();
            assert_ne!(
                unsafe {
                    SetFileInformationByHandle(
                        output.as_raw_handle(),
                        FileBasicInfo,
                        (&normal as *const FILE_BASIC_INFO).cast(),
                        size_of::<FILE_BASIC_INFO>() as u32,
                    )
                },
                0
            );
            let streams = source.path().join("metadata-stream.data");
            std::fs::write(&streams, b"default fixture").unwrap();
            std::fs::write(
                format!("{}:VOLDIQ_METADATA_FIXTURE", streams.display()),
                b"owned alternate fixture",
            )
            .unwrap();
            assert!(BasicMetadata::observe(&windows::open_metadata(&streams).unwrap()).is_err());
            assert_eq!(std::fs::read(&streams).unwrap(), b"default fixture");
            let extended = source.path().join("metadata-ea.data");
            std::fs::write(&extended, b"owned extended attribute fixture").unwrap();
            let writer = std::fs::OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_EA)
                .open(&extended)
                .unwrap();
            let name = b"VOLDIQ_TEST";
            let mut encoded = vec![0u8; 8 + name.len() + 2];
            encoded[5] = name.len() as u8;
            encoded[6..8].copy_from_slice(&1u16.to_le_bytes());
            encoded[8..8 + name.len()].copy_from_slice(name);
            encoded[9 + name.len()] = 1;
            let mut aligned = [0u64; 4];
            unsafe {
                std::ptr::copy_nonoverlapping(
                    encoded.as_ptr(),
                    aligned.as_mut_ptr().cast::<u8>(),
                    encoded.len(),
                );
            }
            let mut status = windows_sys::Win32::System::IO::IO_STATUS_BLOCK::default();
            assert_eq!(
                unsafe {
                    windows_sys::Wdk::Storage::FileSystem::NtSetEaFile(
                        writer.as_raw_handle(),
                        &mut status,
                        aligned.as_ptr().cast(),
                        encoded.len() as u32,
                    )
                },
                0
            );
            assert!(BasicMetadata::observe(&windows::open_metadata(&extended).unwrap()).is_err());
            assert_eq!(
                std::fs::read(&extended).unwrap(),
                b"owned extended attribute fixture"
            );
            println!(
                "NTFS_METADATA_PROBE {{\"metadata_only_handle\":true,\"created_modified_and_dos_flags\":true,\"verified_after_close\":true,\"stale_metadata_refused\":true,\"named_stream_refused\":true,\"extended_attributes_refused\":true,\"real_offload\":false}}"
            );
        }
        {
            use crate::cleanup::recycle::batch::{Batch, Outcome};
            cancelled.store(false, Ordering::Release);
            let recovery = tempfile::tempdir().unwrap();
            let mut journal = Journal::open(&recovery.path().join("cleanup.db")).unwrap();
            let apartment = ShellApartment::new().unwrap();
            // Actual retained-handle fixtures; no offload grant, UAC launch or original admission.
            let kept = copy_verified(
                copy_plan.copy_source(copy_id).unwrap(),
                metadata.clone(),
                OwnedFile::create(&parent).unwrap(),
                OsStr::new("handoff-keep.data"),
                &cancelled,
                |_| Ok(()),
                |_| {},
            )
            .ok()
            .unwrap();
            let (source_guard, destination_guard) = kept.into_parts();
            assert_eq!(
                windows::security::granted_access(&source_guard.file).unwrap() & DELETE,
                0
            );
            assert!(
                Batch::prepare_transferred(
                    crate::cleanup::Plan::manual(&copy_index, &[copy_id]).unwrap(),
                    vec![(copy_id, source_guard)],
                    &mut journal,
                    &cancelled,
                    &apartment,
                    None,
                )
                .is_err()
            );
            destination_guard.check().unwrap();
            assert!(journal.relocations(0, 100).unwrap().is_empty());
            drop(destination_guard);
            let source_guard = copy_plan.copy_recycling_source(copy_id).unwrap();
            assert_eq!(
                windows::security::granted_access(&source_guard.file).unwrap()
                    & (DELETE | FILE_READ_DATA),
                DELETE | FILE_READ_DATA
            );
            source_guard.check_read_access().unwrap();
            let copied = copy_verified(
                source_guard,
                metadata.clone(),
                OwnedFile::create(&parent).unwrap(),
                OsStr::new("handoff-recycle.data"),
                &cancelled,
                |_| Ok(()),
                |_| {},
            )
            .ok()
            .unwrap();
            copied.check().unwrap();
            assert_eq!(copied.blake3(), expected_hash);
            let (source_guard, destination_guard) = copied.into_parts();
            let batch = Batch::prepare_transferred(
                crate::cleanup::Plan::manual(&copy_index, &[copy_id]).unwrap(),
                vec![(copy_id, source_guard)],
                &mut journal,
                &cancelled,
                &apartment,
                None,
            )
            .unwrap();
            assert!(batch.ready_for_admission());
            assert!(
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&input)
                    .is_err()
            );
            let copied_path = destination.path().join("handoff-recycle.data");
            assert!(
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&copied_path)
                    .is_err()
            );
            assert!(
                std::fs::rename(
                    &copied_path,
                    destination.path().join("replaced-handoff.data")
                )
                .is_err()
            );
            destination_guard.check().unwrap();
            let report = batch.abort(&mut journal, &AtomicBool::new(true), |_| {});
            assert!(report.cancelled);
            assert!(report.journal_error.is_none());
            assert_eq!(report.items.len(), 1);
            assert_eq!(report.items[0].outcome, Outcome::NotStarted);
            assert!(report.items[0].recorded && report.items[0].cleanup_error.is_none());
            let rows = journal.relocations(0, 100).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].state, "cancelled");
            assert_eq!(std::fs::read(&input).unwrap(), payload);
            destination_guard.check().unwrap();
            drop(destination_guard);
            assert_eq!(std::fs::read(&copied_path).unwrap(), payload);
            assert!(
                Batch::prepare_transferred(
                    crate::cleanup::Plan::manual(&copy_index, &[copy_id]).unwrap(),
                    vec![],
                    &mut journal,
                    &cancelled,
                    &apartment,
                    None,
                )
                .is_err()
            );
            assert!(
                Batch::prepare_transferred(
                    crate::cleanup::Plan::manual(&copy_index, &[copy_id]).unwrap(),
                    vec![(
                        crate::NodeId(u32::MAX),
                        copy_plan.copy_recycling_source(copy_id).unwrap()
                    )],
                    &mut journal,
                    &cancelled,
                    &apartment,
                    None,
                )
                .is_err()
            );
            assert_eq!(journal.relocations(0, 100).unwrap().len(), 1);
            assert_eq!(std::fs::read(&input).unwrap(), payload);
            assert!(!std::fs::read_dir(source.path()).unwrap().any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(crate::cleanup::STAGE_DIRECTORY_PREFIX)
            }));
            println!(
                "TRANSFER_ORIGINAL_HANDOFF_PROBE {{\"same_source_handle\":true,\"keep_without_delete_refused\":true,\"exact_selection_required\":true,\"destination_pinned_through_abort\":true,\"cancelled_before_admission\":true,\"original_unchanged\":true,\"real_offload\":false}}"
            );
        }
        println!(
            "NATIVE_CONTENT_COPY_PROBE {{\"copied_bytes\":262177,\"destination_readback\":true,\"source_recheck\":true,\"retained_sealed_handles\":true,\"published_receipt_failure_preserved\":true,\"original_unchanged\":true,\"real_offload\":false}}"
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
    {
        // A real consumed recycle grant cannot become content-copy authority.
        cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        let recycle_only = service.plan_prepare(snapshot, &[selected]).unwrap();
        assert!(service.plan_request(&recycle_only.manifest.id).is_err());
        desktop
            .approve(&recycle_only.manifest.id, &recycle_only.hash)
            .unwrap();
        service
            .plan_execute(&recycle_only.manifest.id, &recycle_only.hash)
            .unwrap();
        let principal = registry.authenticate(issued.credential.expose()).unwrap();
        let context = registry
            .plan_context(&principal, Capability::ExecuteApproved)
            .unwrap();
        let mut plans = Store::open(state.path()).unwrap();
        let crate::plans::Begin::Started(lease) = plans
            .begin(
                &context,
                &recycle_only.manifest.id,
                &desktop.human,
                super::super::now().unwrap(),
            )
            .unwrap()
        else {
            panic!("Recycle fixture did not consume its real grant");
        };
        assert!(matches!(
            plans.check_execution(&context, &desktop.human, &lease, super::super::now),
            Ok(super::super::store::ExecutionControl::Proceed)
        ));
        assert!(
            plans
                .check_execution(&context, &desktop.human, &lease, || Ok(lease
                    .runtime
                    .window
                    .admission_expires))
                .is_err()
        );
        plans
            .connection
            .execute(
                "UPDATE agent_plans SET approval_expires=approval_expires+1 WHERE id=?1",
                [&lease.id],
            )
            .unwrap();
        assert!(
            plans
                .check_execution(&context, &desktop.human, &lease, super::super::now)
                .is_err()
        );
        plans
            .connection
            .execute(
                "UPDATE agent_plans SET approval_expires=approval_expires-1 WHERE id=?1",
                [&lease.id],
            )
            .unwrap();
        let failure = plans
            .copy_transfer_file(
                &context,
                &desktop.human,
                &lease,
                selected,
                crate::plans::TransferWorker::new(&operation, &cancel),
                |_| {},
            )
            .err()
            .unwrap();
        assert_eq!(
            failure.publication(),
            crate::plans::CopyPublication::NotAttempted
        );
        assert_eq!(std::fs::read(&original).unwrap(), b"owned dispatch fixture");
        plans
            .cancel_execution(
                &context,
                &recycle_only.manifest.id,
                &recycle_only.hash,
                super::super::now().unwrap(),
            )
            .unwrap();
        let failure = plans
            .copy_transfer_batch(
                &context,
                &desktop.human,
                lease,
                crate::plans::TransferWorker::new(&operation, &cancel),
                |_, _| {},
            )
            .err()
            .unwrap();
        assert_eq!(failure.recorded_state, Some(State::Cancelled));
        assert_eq!(failure.copied_files, 0);
        assert_eq!(failure.verified_bytes, 0);
        assert!(failure.finalization_error.is_none());
        assert!(cancel.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(
            failure.copy_failure().publication(),
            crate::plans::CopyPublication::NotAttempted
        );
        assert_eq!(std::fs::read(&original).unwrap(), b"owned dispatch fixture");
        // A real grant must not start using a timestamp captured before SQLite's writer wait.
        let delayed = service.plan_prepare(snapshot, &[selected]).unwrap();
        assert!(service.plan_request(&delayed.manifest.id).is_err());
        desktop
            .approve(&delayed.manifest.id, &delayed.hash)
            .unwrap();
        service
            .plan_execute(&delayed.manifest.id, &delayed.hash)
            .unwrap();
        let requested_at = super::super::now().unwrap();
        plans
            .connection
            .execute(
                "UPDATE agent_plans SET approval_expires=?1 WHERE id=?2",
                rusqlite::params![requested_at + 1, delayed.manifest.id],
            )
            .unwrap();
        plans
            .connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let blocker = rusqlite::Connection::open(state.path().join("agent-plans.db")).unwrap();
        let (ready, blocked) = std::sync::mpsc::channel();
        let release = std::thread::spawn(move || {
            blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
            ready.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2100));
            blocker.execute_batch("ROLLBACK").unwrap();
        });
        blocked.recv().unwrap();
        let started = plans.begin(&context, &delayed.manifest.id, &desktop.human, requested_at);
        release.join().unwrap();
        assert!(matches!(started, Err(crate::Error::Invalid(_))));
        let stored: (String, Option<String>) = plans
            .connection
            .query_row(
                "SELECT state,execution_nonce FROM agent_plans WHERE id=?1",
                [&delayed.manifest.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, ("approved".into(), None));
        assert_eq!(std::fs::read(&original).unwrap(), b"owned dispatch fixture");
        println!(
            "TRANSFER_ADMISSION_PROBE {{\"cancel_recorded_as_cancelled\":true,\"approval_expired_during_writer_wait_refused\":true,\"no_execution_nonce\":true,\"original_unchanged\":true}}"
        );
    }
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
