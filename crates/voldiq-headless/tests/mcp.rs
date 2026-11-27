use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use serde_json::{Value, json};
use voldiq_core::agents::{Capability, Limits, Policy, Registry};
use voldiq_core::{
    Progress, ScanPhase,
    jobs::{DesktopScan, JobState, Jobs, ScanOrigin},
    operations::Lease,
};

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    args: Value,
) -> rmcp::model::CallToolResult {
    client
        .call_tool(
            CallToolRequestParams::new(name.to_owned())
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn official_sdk_client_negotiates_stdio_and_checks_authorization_on_every_tool() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("measured.txt"), [1; 11]).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(source.path().join("measured.txt"))
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1709164800))
        .unwrap();
    let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
    let provisioned = registry
        .provision(
            "SDK fixture",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![
                    Capability::Metadata,
                    Capability::Export,
                    Capability::Scan,
                    Capability::PreparePlans,
                    Capability::RequestApproval,
                    Capability::ExecuteApproved,
                ],
                Limits {
                    page_size: 1,
                    ..Limits::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_voldiq-cli"));
    command
        .arg("--state")
        .arg(state.path())
        .args(["mcp", "serve", "--stdio"])
        .env("VOLDIQ_AGENT_TOKEN", provisioned.credential.expose());
    let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert!(tools.iter().any(|tool| tool.name == "voldiq_scan_start"));
    assert!(
        tools
            .iter()
            .any(|tool| tool.name == "voldiq_query_translate")
    );
    let translated = call(
        &client,
        "voldiq_query_translate",
        json!({"source":"archivos mayores de 1 GiB"}),
    )
    .await;
    assert_ne!(translated.is_error, Some(true));
    assert_eq!(
        translated.structured_content.unwrap()["result"]["expression"],
        "type:file size>1GiB"
    );
    let partial = call(
        &client,
        "voldiq_query_translate",
        json!({"source":"vídeos antiguos"}),
    )
    .await;
    assert_eq!(
        partial.structured_content.unwrap()["result"]["unparsed"][0]["text"],
        "antiguos"
    );
    let invalid = call(
        &client,
        "voldiq_query_translate",
        json!({"source":"modified before 2023-02-29"}),
    )
    .await;
    assert_eq!(
        invalid.structured_content.unwrap()["error"]["code"],
        "invalid_query"
    );
    assert!(!tools.iter().any(|tool| tool.name.contains("approve")
        || tool.name.contains("delete")
        || tool.name.contains("shell")));
    let status = call(&client, "voldiq_capabilities", json!({})).await;
    assert_ne!(status.is_error, Some(true));
    assert_eq!(
        status.structured_content.unwrap()["result"]["volume_catalogue_available"],
        cfg!(windows)
    );
    assert!(tools.iter().any(|tool| tool.name == "voldiq_volumes"));
    let volumes = call(&client, "voldiq_volumes", json!({})).await;
    assert_ne!(volumes.is_error, Some(true));
    assert_eq!(
        volumes.structured_content.unwrap()["result"],
        json!({"supported":cfg!(windows),"items":[],"next_offset":null})
    );
    for args in [json!({"root":state.path()}), json!({"limit":33})] {
        let invalid = call(&client, "voldiq_volumes", args).await;
        assert_eq!(
            invalid.structured_content.unwrap()["error"]["code"],
            "invalid_arguments"
        );
    }
    assert!(Jobs::recent_for_desktop(state.path()).unwrap().is_empty());
    assert!(tools.iter().any(|tool| tool.name == "voldiq_transfers"));
    assert!(
        tools
            .iter()
            .any(|tool| tool.name == "voldiq_duplicate_candidates")
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool.name == "voldiq_cleanup_candidates")
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool.name == "voldiq_verify_duplicates")
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool.name == "voldiq_transfer_files")
    );
    let transfers = call(&client, "voldiq_transfers", json!({})).await;
    assert_eq!(
        transfers.structured_content.unwrap()["result"],
        json!({"items":[],"next_before":null})
    );
    let hidden = call(
        &client,
        "voldiq_transfer_files",
        json!({"plan_id":"0".repeat(64)}),
    )
    .await;
    assert_eq!(
        hidden.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let invalid_files = call(
        &client,
        "voldiq_transfer_files",
        json!({"plan_id":"0".repeat(64),"path":source.path()}),
    )
    .await;
    assert_eq!(
        invalid_files.structured_content.unwrap()["error"]["code"],
        "invalid_arguments"
    );
    assert!(!state.path().join("jobs.db").exists());
    let start = call(&client, "voldiq_scan_start", json!({"root": source.path()})).await;
    assert_ne!(start.is_error, Some(true));
    let id = start.structured_content.unwrap()["result"]["job_id"]
        .as_i64()
        .unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    let snapshot = loop {
        let job = call(&client, "voldiq_job_get", json!({"job_id":id}))
            .await
            .structured_content
            .unwrap();
        if job["result"]["state"] == "completed" {
            break job["result"]["snapshot_id"].as_i64().unwrap();
        }
        assert!(tokio::time::Instant::now() < deadline, "{job}");
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    };
    let desktop = Jobs::recent_for_desktop(state.path()).unwrap();
    assert_eq!(desktop.len(), 1);
    assert_eq!(desktop[0].id, id.to_string());
    assert_eq!(desktop[0].state, JobState::Completed);
    assert_eq!(desktop[0].origin, ScanOrigin::Headless);
    assert_eq!(desktop[0].snapshot_id, Some(snapshot));
    assert_eq!(desktop[0].progress.as_ref().unwrap().logical, "11");
    let visible = serde_json::to_value(&desktop[0]).unwrap();
    assert!(visible.get("owner").is_none() && visible.get("snapshot_id").is_none());
    let history = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db")).unwrap();
    assert!(history.contains(snapshot).unwrap());
    assert_eq!(history.summary(snapshot).unwrap().unwrap().logical, 11);
    assert!(!history.contains(i64::MAX).unwrap());
    assert!(history.summary(i64::MAX).unwrap().is_none());
    drop(history);

    let candidates = call(
        &client,
        "voldiq_duplicate_candidates",
        json!({"snapshot_id":snapshot,"options":{"limit":1}}),
    )
    .await;
    assert_ne!(candidates.is_error, Some(true));
    let candidates = candidates.structured_content.unwrap();
    assert_eq!(candidates["result"]["evidence"], "recorded_size_only");
    assert_eq!(candidates["result"]["groups"], json!([]));
    #[cfg(windows)]
    {
        let cleanup = call(
            &client,
            "voldiq_cleanup_candidates",
            json!({
                "snapshot_id":snapshot,"rule":"old_files","options":{"limit":100}
            }),
        )
        .await;
        assert_ne!(cleanup.is_error, Some(true), "{cleanup:?}");
        let cleanup = cleanup.structured_content.unwrap();
        let result = &cleanup["result"];
        assert_eq!(result["availability"], "recorded");
        assert_eq!(result["rule_version"], 1);
        assert_eq!(result["logical_bytes"], "11");
        assert_eq!(result["items"][0]["name"], "measured.txt");
        assert!(result["items"][0].get("path").is_none());
        assert!(result.get("reclaimable").is_none());
        assert!(result["items"][0]["priority"]["signals"][3]["value"].is_null());
    }
    for args in [
        json!({"snapshot_id":snapshot,"rule":"delete","options":{}}),
        json!({"snapshot_id":snapshot,"rule":"old_files","options":{"root":source.path()}}),
        json!({"snapshot_id":snapshot,"rule":"old_files","options":{"limit":101}}),
        json!({"snapshot_id":snapshot,"rule":"old_files","options":{"weights":{
            "size":0,"age":0,"regeneration":0,"duplication":0,"provenance":0,"recovery":0
        }}}),
    ] {
        let invalid = call(&client, "voldiq_cleanup_candidates", args).await;
        assert_eq!(
            invalid.structured_content.unwrap()["error"]["code"],
            "invalid_arguments"
        );
    }
    let injected = call(
        &client,
        "voldiq_duplicate_candidates",
        json!({"snapshot_id":snapshot,"options":{"root":source.path()}}),
    )
    .await;
    assert_eq!(
        injected.structured_content.unwrap()["error"]["code"],
        "invalid_arguments"
    );

    let isolated = tempfile::tempdir().unwrap();
    let lease = Lease::acquire(isolated.path()).unwrap();
    let mut journal = Jobs::open(isolated.path()).unwrap();
    let running = journal.begin(&lease, 17, source.path()).unwrap();
    assert!(
        !journal
            .progress(
                17,
                running,
                &Progress {
                    previous: false,
                    phase: ScanPhase::Scanning,
                    entries: 1,
                    files: 1,
                    logical: u64::MAX,
                    elapsed_ms: 1,
                    errors: 0,
                    method: "Owned fixture".into(),
                }
            )
            .unwrap()
    );
    let desktop = Jobs::recent_for_desktop(isolated.path()).unwrap();
    assert_eq!(desktop[0].state, JobState::Running);
    assert_eq!(
        desktop[0].progress.as_ref().unwrap().logical,
        "18446744073709551615"
    );
    assert!(Jobs::snapshot_for_desktop(&lease, running).is_err());
    assert!(journal.get(18, running).is_err());
    Jobs::cancel_for_desktop(isolated.path(), running).unwrap();
    assert!(journal.cancellation_requested(17, running).unwrap());
    drop(lease);
    let desktop = Jobs::recent_for_desktop(isolated.path()).unwrap();
    assert_eq!(desktop[0].state, JobState::Interrupted);
    assert!(desktop[0].finished.is_some());
    assert!(Jobs::cancel_for_desktop(isolated.path(), running).is_err());
    let lease = Lease::acquire(isolated.path()).unwrap();
    let completed = journal.begin(&lease, 18, source.path()).unwrap();
    journal
        .finish(18, completed, JobState::Completed, Some(snapshot))
        .unwrap();
    assert_eq!(
        Jobs::snapshot_for_desktop(&lease, completed).unwrap(),
        snapshot
    );
    assert!(Jobs::snapshot_for_desktop(&lease, running).is_err());
    drop(lease);

    let lease = Lease::acquire(isolated.path()).unwrap();
    let mut local = DesktopScan::begin(&lease, source.path()).unwrap();
    let previous = Progress {
        previous: true,
        phase: ScanPhase::Scanning,
        entries: 10,
        files: 9,
        logical: 200,
        elapsed_ms: 0,
        errors: 0,
        method: "Owned fixture".into(),
    };
    local.observe(&previous).unwrap();
    let activity = Jobs::recent_for_desktop(isolated.path()).unwrap();
    let local_id = activity[0].id.parse::<i64>().unwrap();
    assert_eq!(activity[0].origin, ScanOrigin::Desktop);
    assert_eq!(activity[0].state, JobState::Running);
    assert!(activity[0].progress.is_none());
    assert!(journal.get(18, local_id).is_err());
    assert!(Jobs::cancel_for_desktop(isolated.path(), local_id).is_err());
    let measured = Progress {
        previous: false,
        phase: ScanPhase::Complete,
        entries: 2,
        files: 1,
        logical: 11,
        ..previous
    };
    let capture = local
        .finish(JobState::Completed, Some(&measured))
        .unwrap()
        .unwrap();
    assert!(Jobs::snapshot_for_desktop(&lease, local_id).is_err());
    drop(lease);

    // Saving a capture reacquires the profile lease after the scan worker leaves.
    let stop = std::sync::atomic::AtomicBool::new(false);
    let mut store =
        voldiq_core::snapshots::Store::open(&isolated.path().join("snapshots.db")).unwrap();
    let index =
        voldiq_core::scanner::scan(source.path(), &Default::default(), &stop, |_| {}).unwrap();
    let lease = Lease::acquire(isolated.path()).unwrap();
    let saved = store.save(&index, &stop, &[]).unwrap();
    let other = tempfile::tempdir().unwrap();
    assert!(
        capture
            .link_snapshot(&Lease::acquire(other.path()).unwrap(), saved)
            .is_err()
    );
    capture.link_snapshot(&lease, saved).unwrap();
    assert_eq!(Jobs::snapshot_for_desktop(&lease, local_id).unwrap(), saved);
    assert_eq!(store.summary(saved).unwrap().unwrap().logical, 11);
    let activity = Jobs::recent_for_desktop(isolated.path()).unwrap();
    assert_eq!(activity[0].snapshot_id, Some(saved));
    assert_eq!(activity[0].progress.as_ref().unwrap().logical, "11");
    drop(lease);

    let stats = call(&client, "voldiq_stats", json!({"snapshot_id":snapshot})).await;
    assert!(tools.iter().any(|tool| tool.name == "voldiq_applications"));
    #[cfg(windows)]
    {
        let applications = call(
            &client,
            "voldiq_applications",
            json!({"snapshot_id":snapshot}),
        )
        .await;
        assert_ne!(applications.is_error, Some(true));
        let response = applications.structured_content.unwrap();
        assert_eq!(response["schema_version"], 1);
        let result = &response["result"];
        assert_eq!(result["total"], json!({"logical_bytes":"11","files":1}));
        assert_eq!(result["unattributed"], result["total"]);
        assert_eq!(result["attribution"], "captured");
        assert_eq!(result["applications"], json!([]));
        assert_eq!(result["next_offset"], Value::Null);
        assert!(result.get("chart").is_none());
        assert!(result.get("unbound_records").is_none());
        assert!(result.get("coverage").is_none());
        for source in result["registration_sources"].as_array().unwrap() {
            assert_eq!(source.as_object().unwrap().len(), 2);
        }
    }
    let injected = call(
        &client,
        "voldiq_applications",
        json!({"snapshot_id":snapshot,"approved":true}),
    )
    .await;
    assert_eq!(injected.is_error, Some(true));
    let invalid = call(
        &client,
        "voldiq_applications",
        json!({"snapshot_id":snapshot,"query":{"limit":0}}),
    )
    .await;
    assert_eq!(invalid.is_error, Some(true));
    let readonly = registry
        .provision(
            "Read without export",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![Capability::Metadata],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let reader = voldiq_core::service::AgentService::new(
        state.path().to_owned(),
        registry.authenticate(readonly.credential.expose()).unwrap(),
    )
    .unwrap();
    assert_eq!(reader.stats(snapshot).unwrap().logical, 11);
    assert!(
        reader
            .export_csv(
                snapshot,
                serde_json::from_value(json!({"items":"files"})).unwrap(),
                &std::sync::atomic::AtomicBool::new(false)
            )
            .is_err()
    );
    assert_eq!(stats.structured_content.unwrap()["result"]["logical"], 11);
    let labelled = call(
        &client,
        "voldiq_stats",
        json!({"snapshot_id":snapshot,"display_units":"decimal"}),
    )
    .await;
    let labelled = &labelled.structured_content.unwrap()["result"];
    assert_eq!(labelled["logical"], 11);
    assert_eq!(labelled["display"]["units"], "decimal");
    assert_eq!(labelled["display"]["logical"], "11 B");
    let exported = call(
        &client,
        "voldiq_export_csv",
        json!({"snapshot_id":snapshot,"request":{"items":"files","query":{"limit":100}}}),
    )
    .await;
    assert_ne!(exported.is_error, Some(true));
    let exported = exported.structured_content.unwrap();
    assert_eq!(exported["result"]["rows"], 1);
    assert_eq!(exported["result"]["query"]["limit"], 1);
    assert_eq!(exported["result"]["scope_root"], Value::Null);
    assert!(
        exported["result"]["csv"]
            .as_str()
            .unwrap()
            .contains("\"text: measured.txt\"")
    );
    assert!(tools.iter().any(|tool| tool.name == "voldiq_space_account"));
    let space = call(
        &client,
        "voldiq_space_account",
        json!({"snapshot_id":snapshot}),
    )
    .await;
    assert_ne!(space.is_error, Some(true));
    let space = space.structured_content.unwrap();
    assert_eq!(space["result"]["indexed"]["logical_paths"], "11");
    assert_eq!(space["result"]["indexed"]["snapshot"], snapshot);
    assert_eq!(space["result"]["volume"], json!({"kind":"scoped_only"}));
    assert_eq!(
        space["result"]["residual"],
        json!({"kind":"unavailable","reason":"scoped_only"})
    );
    let invalid_space = call(
        &client,
        "voldiq_space_account",
        json!({"snapshot_id":snapshot,"whole_volume":true}),
    )
    .await;
    assert_eq!(
        invalid_space.structured_content.unwrap()["error"]["code"],
        "invalid_arguments"
    );
    assert!(tools.iter().any(|tool| tool.name == "voldiq_calendar"));
    assert!(tools.iter().any(|tool| tool.name == "voldiq_diary"));
    let calendar = call(
        &client,
        "voldiq_calendar",
        json!({"snapshot_id":snapshot,"month":"2024-02"}),
    )
    .await;
    assert_ne!(calendar.is_error, Some(true));
    assert_eq!(
        calendar.structured_content.unwrap()["result"]["report"]["days"][28]["totals"],
        json!({"files":1,"logical":"11"})
    );
    let invalid_calendar = call(
        &client,
        "voldiq_calendar",
        json!({"snapshot_id":snapshot,"month":"2024-02","offset":0}),
    )
    .await;
    assert_eq!(
        invalid_calendar.structured_content.unwrap()["error"]["code"],
        "invalid_arguments"
    );
    let budget = call(
        &client,
        "voldiq_folder_budget",
        json!({"snapshot_id":snapshot,"node_id":0}),
    )
    .await;
    assert_ne!(budget.is_error, Some(true));
    assert_eq!(
        budget.structured_content.unwrap()["result"]["logical_bytes"],
        "11"
    );
    assert!(
        !tools
            .iter()
            .any(|tool| tool.name.contains("save_budget") || tool.name.contains("remove_budget"))
    );
    let history = call(&client, "voldiq_history", json!({})).await;
    assert_eq!(
        history.structured_content.unwrap()["result"]["items"][0]["id"],
        snapshot
    );
    let denied = call(&client, "voldiq_scan_start", json!({"root":state.path()})).await;
    assert!(tools.iter().any(|tool| tool.name == "voldiq_compare"));
    assert!(tools.iter().any(|tool| tool.name == "voldiq_trend"));
    let private = source.path().join("excluded");
    std::fs::create_dir(&private).unwrap();
    let extra = source.path().join("history-added.data");
    std::fs::write(&extra, [2; 5]).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&extra)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1709164800))
        .unwrap();
    let mut index = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let mut store =
        voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db")).unwrap();
    let first_observed = store
        .summary(snapshot)
        .unwrap()
        .unwrap()
        .observation
        .unwrap()
        .completed_at;
    index.observation.as_mut().unwrap().completed_at = first_observed + 1;
    let later = store
        .save(&index, &std::sync::atomic::AtomicBool::new(false), &[])
        .unwrap();
    drop(store);
    std::fs::remove_file(extra).unwrap();
    let trend = call(
        &client,
        "voldiq_trend",
        json!({"snapshot_id":later,"limit":100}),
    )
    .await;
    assert_ne!(trend.is_error, Some(true));
    let trend = &trend.structured_content.unwrap()["result"];
    assert_eq!(trend["points"].as_array().unwrap().len(), 1);
    assert_eq!(trend["points"][0]["snapshot"], snapshot);
    assert_eq!(trend["next_offset"], 1);
    assert!(trend["forecast"].is_null() && trend.get("volume").is_none());
    let second = call(
        &client,
        "voldiq_trend",
        json!({"snapshot_id":later,"offset":1}),
    )
    .await;
    let second = &second.structured_content.unwrap()["result"];
    assert_eq!(second["points"][0]["delta"], "5");
    assert_eq!(second["points"][0]["previous"], snapshot);
    assert!(second["next_offset"].is_null());
    let anchored = call(&client, "voldiq_trend", json!({"snapshot_id":snapshot})).await;
    let anchored = &anchored.structured_content.unwrap()["result"];
    assert_eq!(anchored["points"][0]["logical"], "11");
    assert!(anchored["next_offset"].is_null());
    for args in [
        json!({"snapshot_id":later,"days":14}),
        json!({"snapshot_id":later,"volume":true}),
    ] {
        let invalid = call(&client, "voldiq_trend", args).await;
        assert_eq!(
            invalid.structured_content.unwrap()["error"]["code"],
            "invalid_arguments"
        );
    }
    let day = call(
        &client,
        "voldiq_calendar",
        json!({"snapshot_id":later,"month":"2024-02","day":29,"limit":100}),
    )
    .await;
    assert_ne!(day.is_error, Some(true));
    let day = day.structured_content.unwrap();
    assert_eq!(day["result"]["page"]["total"], 2);
    assert_eq!(
        day["result"]["page"]["entries"].as_array().unwrap().len(),
        1
    );
    let diary = call(
        &client,
        "voldiq_diary",
        json!({"snapshot_id":later,"limit":10}),
    )
    .await;
    assert_ne!(diary.is_error, Some(true));
    let diary = diary.structured_content.unwrap();
    assert_eq!(diary["result"]["events"].as_array().unwrap().len(), 1);
    assert_eq!(diary["result"]["events"][0]["detail"]["delta"], "5");
    assert_eq!(
        diary["result"]["events"][0]["detail"]["comparison"]["changes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(diary["result"]["has_more"], true);
    let compared = call(
        &client,
        "voldiq_compare",
        json!({"before":snapshot,"after":later,"limit":200}),
    )
    .await;
    assert_ne!(compared.is_error, Some(true));
    let compared = compared.structured_content.unwrap();
    assert_eq!(compared["result"]["delta"], 5);
    assert_eq!(compared["result"]["accounting"], "compatible");
    assert_eq!(compared["result"]["coverage_complete"], true);
    assert_eq!(compared["result"]["file_changes"]["added"]["logical"], 5);
    assert_eq!(compared["result"]["changes"].as_array().unwrap().len(), 1);
    let restricted = registry
        .provision(
            "Restricted comparison",
            Policy::new(
                vec![source.path().to_owned()],
                vec![private],
                vec![Capability::Metadata],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let reader = voldiq_core::service::AgentService::new(
        state.path().to_owned(),
        registry
            .authenticate(restricted.credential.expose())
            .unwrap(),
    )
    .unwrap();
    assert!(
        reader
            .compare_snapshots(
                snapshot,
                later,
                0,
                10,
                &std::sync::atomic::AtomicBool::new(false)
            )
            .is_err()
    );
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    let scan_only = registry
        .provision(
            "Scan without metadata",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![Capability::Scan],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let scan_only = voldiq_core::service::AgentService::new(
        state.path().to_owned(),
        registry
            .authenticate(scan_only.credential.expose())
            .unwrap(),
    )
    .unwrap();
    assert!(
        scan_only
            .applications(snapshot, Default::default(), &cancelled)
            .is_err()
    );
    assert!(
        reader
            .applications(
                snapshot,
                Default::default(),
                &std::sync::atomic::AtomicBool::new(false)
            )
            .is_err()
    );
    assert!(
        reader
            .calendar(
                voldiq_core::snapshots::CalendarRequest::Month {
                    snapshot,
                    month: "2024-02".into()
                },
                &std::sync::atomic::AtomicBool::new(false)
            )
            .is_err()
    );
    assert!(
        reader
            .diary(snapshot, 0, 5, &std::sync::atomic::AtomicBool::new(false))
            .is_err()
    );
    assert!(
        reader
            .space_account(snapshot, &std::sync::atomic::AtomicBool::new(false))
            .is_err()
    );
    let reader = voldiq_core::service::AgentService::new(
        state.path().to_owned(),
        registry
            .authenticate(provisioned.credential.expose())
            .unwrap(),
    )
    .unwrap();
    assert!(
        reader
            .compare_snapshots(snapshot, later, 0, 10, &cancelled)
            .unwrap()
            .is_none()
    );
    assert!(
        reader
            .applications(snapshot, Default::default(), &cancelled)
            .unwrap()
            .is_none()
    );
    assert!(
        reader
            .space_account(snapshot, &cancelled)
            .unwrap()
            .is_none()
    );
    let outside = tempfile::tempdir().unwrap();
    let foreign_index = voldiq_core::scanner::scan(
        outside.path(),
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let foreign = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save(
            &foreign_index,
            &std::sync::atomic::AtomicBool::new(false),
            &[],
        )
        .unwrap();
    let unavailable = call(
        &client,
        "voldiq_applications",
        json!({"snapshot_id":foreign}),
    )
    .await;
    assert_eq!(
        unavailable.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    for (a, b) in [(snapshot, foreign), (foreign, snapshot)] {
        let rejected = call(&client, "voldiq_compare", json!({"before":a,"after":b})).await;
        assert_eq!(
            rejected.structured_content.unwrap()["error"]["code"],
            "not_authorized_or_unavailable"
        );
    }
    for (name, args) in [
        (
            "voldiq_calendar",
            json!({"snapshot_id":foreign,"month":"2024-02"}),
        ),
        ("voldiq_diary", json!({"snapshot_id":foreign})),
        ("voldiq_trend", json!({"snapshot_id":foreign})),
        ("voldiq_space_account", json!({"snapshot_id":foreign})),
        (
            "voldiq_export_csv",
            json!({"snapshot_id":foreign,"request":{"items":"files"}}),
        ),
    ] {
        let rejected = call(&client, name, args).await;
        assert_eq!(
            rejected.structured_content.unwrap()["error"]["code"],
            "not_authorized_or_unavailable"
        );
    }
    let invalid = call(
        &client,
        "voldiq_compare",
        json!({"before":snapshot,"after":later,"approved":true}),
    )
    .await;
    assert_eq!(
        invalid.structured_content.unwrap()["error"]["code"],
        "invalid_arguments"
    );
    assert_eq!(denied.is_error, Some(true));
    let unknown = call(
        &client,
        "voldiq_stats",
        json!({"snapshot_id":snapshot,"approved":true}),
    )
    .await;
    assert_eq!(unknown.is_error, Some(true));
    let files = call(
        &client,
        "voldiq_query",
        json!({"snapshot_id":snapshot,"query":{"files_only":true}}),
    )
    .await;
    let selected = files.structured_content.unwrap()["result"]["items"][0]["id"]
        .as_u64()
        .unwrap();
    let allocated = call(
        &client,
        "voldiq_query",
        json!({"snapshot_id":snapshot,"query":{"files_only":true,"sort":"allocated"}}),
    )
    .await;
    assert_ne!(allocated.is_error, Some(true));
    assert_eq!(
        allocated.structured_content.unwrap()["result"]["items"][0]["id"],
        selected
    );
    let filter = if cfg!(windows) {
        "not_placeholder"
    } else {
        "unknown"
    };
    let cloud = call(
        &client,
        "voldiq_query",
        json!({"snapshot_id":snapshot,"query":{"files_only":true,"cloud":filter}}),
    )
    .await;
    assert_ne!(cloud.is_error, Some(true));
    let cloud = cloud.structured_content.unwrap();
    assert_eq!(cloud["result"]["items"][0]["cloud"], filter);
    assert_eq!(cloud["result"]["items"][0]["id"], selected);
    let expression = call(
        &client,
        "voldiq_query",
        json!({"snapshot_id":snapshot,"query":{"expression":"type:file size=11B"}}),
    )
    .await;
    assert_ne!(expression.is_error, Some(true));
    assert_eq!(
        expression.structured_content.unwrap()["result"]["items"][0]["id"],
        selected
    );
    for expression in [
        "size>private-secret",
        "app-id:private-secret",
        "modified=2023-02-29",
    ] {
        let invalid = call(
            &client,
            "voldiq_query",
            json!({"snapshot_id":snapshot,"query":{"expression":expression}}),
        )
        .await;
        assert_eq!(invalid.is_error, Some(true));
        let value = invalid.structured_content.unwrap();
        assert_eq!(value["error"]["code"], "invalid_query");
        assert!(!value.to_string().contains("private-secret"));
    }
    assert!(
        tools
            .iter()
            .any(|tool| tool.name == "voldiq_plan_prepare_offload")
    );
    let offload = call(&client, "voldiq_plan_prepare_offload",
        json!({"snapshot_id":snapshot,"node_ids":[selected],"destination":source.path(),"originals":"keep"})).await;
    assert_eq!(offload.is_error, Some(true));
    assert!(offload.structured_content.unwrap()["result"].is_null());
    let injected_offload = call(&client, "voldiq_plan_prepare_offload",
        json!({"snapshot_id":snapshot,"node_ids":[selected],"destination":source.path(),"originals":"keep","approved":true})).await;
    assert_eq!(injected_offload.is_error, Some(true));
    let restore_tool = tools
        .iter()
        .find(|tool| tool.name == "voldiq_plan_prepare_restore")
        .unwrap();
    assert_eq!(
        restore_tool.input_schema["properties"]["on_collision"]["enum"],
        json!(["fail", "keep_both"])
    );
    let restore = call(
        &client,
        "voldiq_plan_prepare_restore",
        json!({"snapshot_id":snapshot,"transfer_plan_id":"0".repeat(64),"node_ids":[selected],"on_collision":"keep_both"}),
    )
    .await;
    assert_eq!(restore.is_error, Some(true));
    assert!(restore.structured_content.unwrap()["result"].is_null());
    let injected_restore = call(&client, "voldiq_plan_prepare_restore", json!({"snapshot_id":snapshot,"transfer_plan_id":"0".repeat(64),"node_ids":[selected],"approved":true})).await;
    assert_eq!(injected_restore.is_error, Some(true));
    assert_eq!(
        std::fs::read(source.path().join("measured.txt")).unwrap(),
        [1; 11]
    );
    let prepared = call(
        &client,
        "voldiq_plan_prepare",
        json!({"snapshot_id":snapshot,"node_ids":[selected]}),
    )
    .await;
    assert_ne!(prepared.is_error, Some(true));
    let prepared = prepared.structured_content.unwrap();
    let plan_id = prepared["result"]["manifest"]["id"].as_str().unwrap();
    let hash = prepared["result"]["hash"].as_str().unwrap();
    let execution = json!({"plan_id":plan_id,"manifest_hash":hash});
    assert!(tools.iter().any(|tool| tool.name == "voldiq_plan_validate"));
    let validation = call(&client, "voldiq_plan_validate", execution.clone()).await;
    assert_ne!(validation.is_error, Some(true));
    let validation = validation.structured_content.unwrap();
    assert_eq!(validation["result"]["status"], "matches");
    assert_eq!(validation["result"]["state"], "draft");
    let injected_validation = call(
        &client,
        "voldiq_plan_validate",
        json!({"plan_id":plan_id,"manifest_hash":hash,"approved":true}),
    )
    .await;
    assert_eq!(
        injected_validation.structured_content.unwrap()["error"]["code"],
        "invalid_arguments"
    );
    assert_eq!(
        prepared["result"]["manifest"]["entries"][0]["logical_bytes"],
        "11"
    );
    let injected = call(
        &client,
        "voldiq_plan_request_approval",
        json!({"plan_id":plan_id,"approved":true}),
    )
    .await;
    assert_eq!(injected.is_error, Some(true));
    let requested = call(
        &client,
        "voldiq_plan_request_approval",
        json!({"plan_id":plan_id}),
    )
    .await;
    assert_eq!(requested.is_error, Some(true));
    assert_eq!(
        requested.structured_content.unwrap()["error"]["code"],
        "needs_human_approval"
    );
    let forbidden = call(&client, "voldiq_plan_approve", json!({"plan_id":plan_id})).await;
    assert_eq!(forbidden.is_error, Some(true));
    let status = call(&client, "voldiq_plan_get", json!({"plan_id":plan_id})).await;
    assert_eq!(
        status.structured_content.unwrap()["result"]["state"],
        "awaiting_approval"
    );
    let audit = call(&client, "voldiq_plan_audit", json!({"plan_id":plan_id})).await;
    assert!(
        audit.structured_content.unwrap()["result"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| event["event"] != "approved")
    );
    assert!(tools.iter().any(|tool| tool.name == "voldiq_plan_execute"));
    assert!(tools.iter().any(|tool| tool.name == "voldiq_plan_cancel"));
    let no_human = call(&client, "voldiq_plan_execute", execution.clone()).await;
    assert_eq!(
        no_human.structured_content.unwrap()["error"]["code"],
        "needs_human_approval"
    );
    let desktop = voldiq_core::plans::Desktop::open(state.path()).unwrap();
    let validation = desktop.validate(plan_id, hash).unwrap();
    assert_eq!(
        validation.status,
        voldiq_core::plans::ValidationStatus::Matches
    );
    assert_eq!(
        validation.state,
        voldiq_core::plans::State::AwaitingApproval
    );
    desktop.approve(plan_id, hash).unwrap();
    let invalid = call(
        &client,
        "voldiq_plan_execute",
        json!({"plan_id":plan_id,"manifest_hash":hash,"approved":true}),
    )
    .await;
    assert_eq!(invalid.is_error, Some(true));
    let queued = call(&client, "voldiq_plan_execute", execution.clone()).await;
    assert_eq!(
        queued.structured_content.unwrap()["result"]["dispatch"],
        "queued"
    );
    assert_eq!(desktop.pending_execution(25).unwrap().len(), 1);
    let cancelled = call(&client, "voldiq_plan_cancel", execution.clone()).await;
    assert_eq!(
        cancelled.structured_content.unwrap()["result"]["state"],
        "cancelled"
    );
    assert!(desktop.pending_execution(25).unwrap().is_empty());
    let replay = call(&client, "voldiq_plan_execute", execution).await;
    assert_eq!(
        replay.structured_content.unwrap()["result"]["dispatch"],
        "existing"
    );
    assert_eq!(
        std::fs::read(source.path().join("measured.txt")).unwrap(),
        [1; 11]
    );
    #[cfg(windows)]
    {
        let recorded_root = tempfile::tempdir_in(source.path()).unwrap();
        std::fs::write(recorded_root.path().join("app-measurement.data"), [1; 11]).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut recorded =
            voldiq_core::scanner::scan(recorded_root.path(), &Default::default(), &cancel, |_| {})
                .unwrap();
        recorded.captured_applications = Some(
            voldiq_core::applications::capture(
                &recorded,
                voldiq_core::applications::Catalog {
                    started_at: 1000,
                    observed_at: 1001,
                    coverage: vec![],
                    registrations: vec![voldiq_core::applications::Registration {
                        source: voldiq_core::applications::Source::SuppliedMetadata,
                        key: "Recorded SDK fixture".into(),
                        name: "Recorded SDK fixture".into(),
                        publisher: None,
                        version: None,
                        install_location: Some(recorded_root.path().to_owned()),
                        profile: None,
                    }],
                },
                &cancel,
            )
            .unwrap(),
        );
        let recorded_id = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
            .unwrap()
            .save(&recorded, &cancel, &[])
            .unwrap();
        let recorded_reply = call(
            &client,
            "voldiq_applications",
            json!({"snapshot_id":recorded_id}),
        )
        .await;
        assert_ne!(recorded_reply.is_error, Some(true));
        let recorded_reply = recorded_reply.structured_content.unwrap();
        assert_eq!(recorded_reply["result"]["attribution"], "captured");
        assert_eq!(
            recorded_reply["result"]["applications"][0]["name"],
            "Recorded SDK fixture"
        );
        assert_eq!(
            recorded_reply["result"]["exclusive"],
            json!({"logical_bytes":"11","files":1})
        );
        assert_eq!(recorded_reply["result"]["registrations_observed_at"], 1001);
        let files = call(&client, "voldiq_query", json!({"snapshot_id":recorded_id,"query":{"expression":"app:\"Recorded SDK fixture\" app-category:installation"}})).await;
        assert_ne!(files.is_error, Some(true));
        let files = files.structured_content.unwrap();
        assert_eq!(files["result"]["total"], 1);
        assert_eq!(files["result"]["items"][0]["logical"], 11);
        verify_recorded_libraries(
            &client,
            &mut registry,
            state.path(),
            recorded_root.path(),
            recorded_id,
        )
        .await;
    }
    #[cfg(windows)]
    {
        let empty_folder = source.path().join("rpc-empty-review/rpc-empty");
        std::fs::create_dir_all(&empty_folder).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let index = voldiq_core::scanner::scan(source.path(), &Default::default(), &cancel, |_| {})
            .unwrap();
        let observation = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
            .unwrap()
            .save(&index, &cancel, &[])
            .unwrap();
        let folder = call(
            &client,
            "voldiq_query",
            json!({
                "snapshot_id":observation,"query":{"text":"rpc-empty-review","kind":"directory"}
            }),
        )
        .await
        .structured_content
        .unwrap();
        let scope = folder["result"]["items"][0]["id"].as_u64().unwrap();
        let empty = call(
            &client,
            "voldiq_empty_folders",
            json!({"snapshot_id":observation,"options":{"scope":scope,"limit":100}}),
        )
        .await;
        assert_ne!(empty.is_error, Some(true), "{empty:?}");
        let empty = empty.structured_content.unwrap();
        assert_eq!(empty["result"]["availability"], "observed");
        assert_eq!(empty["result"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(empty["result"]["items"][0]["relative_path"], "rpc-empty");
        assert!(empty["result"]["items"][0].get("path").is_none());
        std::fs::write(empty_folder.join("added.txt"), b"added after capture").unwrap();
        let changed = call(
            &client,
            "voldiq_empty_folders",
            json!({"snapshot_id":observation,"options":{"scope":scope}}),
        )
        .await;
        assert_eq!(
            changed.structured_content.unwrap()["result"]["items"],
            json!([])
        );
        let invalid = call(
            &client,
            "voldiq_empty_folders",
            json!({"snapshot_id":observation,"options":{"path":source.path()}}),
        )
        .await;
        assert_eq!(
            invalid.structured_content.unwrap()["error"]["code"],
            "invalid_arguments"
        );
    }
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let measured =
        voldiq_core::scanner::scan(source.path(), &Default::default(), &cancel, |_| {}).unwrap();
    let summary = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save_summary(&measured, &cancel)
        .unwrap();
    let history = call(&client, "voldiq_history", json!({}))
        .await
        .structured_content
        .unwrap();
    assert_eq!(history["result"]["items"][0]["id"], summary);
    assert_eq!(history["result"]["items"][0]["tier"], "summary");
    let detail = call(
        &client,
        "voldiq_compare",
        json!({"before":snapshot,"after":summary}),
    )
    .await;
    assert_eq!(
        detail.structured_content.unwrap()["error"]["code"],
        "snapshot_details_unavailable",
    );

    registry.revoke(provisioned.id).unwrap();
    for (name, args) in [
        (
            "voldiq_applications",
            json!({"snapshot_id":snapshot,"query":{"filters":{"expression":"size>secret"}}}),
        ),
        (
            "voldiq_calendar",
            json!({"snapshot_id":snapshot,"month":"invalid"}),
        ),
        ("voldiq_diary", json!({"snapshot_id":snapshot})),
        (
            "voldiq_calendar",
            json!({"snapshot_id":summary,"month":"2026-01"}),
        ),
    ] {
        let rejected = call(&client, name, args).await;
        assert_eq!(
            rejected.structured_content.unwrap()["error"]["code"],
            "not_authorized_or_unavailable"
        );
    }
    let revoked_comparison = call(
        &client,
        "voldiq_compare",
        json!({"before":snapshot,"after":later}),
    )
    .await;
    assert_eq!(
        revoked_comparison.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let revoked_translation = call(
        &client,
        "voldiq_query_translate",
        json!({"source":"files larger than secret"}),
    )
    .await;
    assert_eq!(
        revoked_translation.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let revoked = call(&client, "voldiq_stats", json!({"snapshot_id":snapshot})).await;
    assert_eq!(
        call(
            &client,
            "voldiq_export_csv",
            json!({"snapshot_id":snapshot,"request":{"items":"files"}})
        )
        .await
        .is_error,
        Some(true)
    );
    assert_eq!(revoked.is_error, Some(true));
    let revoked_space = call(
        &client,
        "voldiq_space_account",
        json!({"snapshot_id":snapshot}),
    )
    .await;
    assert_eq!(
        revoked_space.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let revoked_trend = call(&client, "voldiq_trend", json!({"snapshot_id":snapshot})).await;
    assert_eq!(
        revoked_trend.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let revoked_volumes = call(&client, "voldiq_volumes", json!({})).await;
    assert_eq!(
        revoked_volumes.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let revoked_transfers = call(&client, "voldiq_transfers", json!({})).await;
    assert_eq!(
        revoked_transfers.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let revoked_candidates = call(
        &client,
        "voldiq_duplicate_candidates",
        json!({"snapshot_id":snapshot}),
    )
    .await;
    assert_eq!(
        revoked_candidates.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let revoked_cleanup = call(
        &client,
        "voldiq_cleanup_candidates",
        json!({"snapshot_id":snapshot,"rule":"old_files"}),
    )
    .await;
    assert_eq!(
        revoked_cleanup.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let revoked_folders = call(
        &client,
        "voldiq_empty_folders",
        json!({"snapshot_id":snapshot}),
    )
    .await;
    assert_eq!(
        revoked_folders.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    assert_eq!(
        call(&client, "voldiq_plan_get", json!({"plan_id":plan_id}))
            .await
            .is_error,
        Some(true)
    );
    let revoked_plan = call(
        &client,
        "voldiq_plan_validate",
        json!({"plan_id":plan_id,"manifest_hash":hash}),
    )
    .await;
    assert_eq!(
        revoked_plan.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    client.cancel().await.unwrap();
}

#[cfg(windows)]
async fn verify_recorded_libraries(
    metadata_client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    registry: &mut Registry,
    state: &std::path::Path,
    root: &std::path::Path,
    legacy: i64,
) {
    let manifests = root.join("steamapps");
    let game = manifests.join("common/Owned Game");
    std::fs::create_dir_all(&game).unwrap();
    let manifest = manifests.join("appmanifest_120.acf");
    let profile = root.join("Owned.NEF.pp3");
    std::fs::write(
        &manifest,
        br#""AppState" { "appid" "120" "name" "Owned Game" "installdir" "Owned Game" }"#,
    )
    .unwrap();
    std::fs::write(game.join("game.bin"), [0; 13]).unwrap();
    std::fs::write(root.join("Owned.NEF"), [0; 17]).unwrap();
    std::fs::write(&profile, b"[Version]\nAppVersion=5.12\nVersion=360\n").unwrap();
    let duplicate = root.join("duplicate-first.data");
    std::fs::write(&duplicate, b"matches").unwrap();
    std::fs::write(root.join("duplicate-second.data"), b"matches").unwrap();
    std::fs::write(root.join("duplicate-third.data"), b"another!").unwrap();
    std::fs::write(root.join("duplicate-fourth.data"), b"another!").unwrap();

    let provisioned = registry
        .provision(
            "Library evidence fixture",
            Policy::new(
                vec![root.to_owned()],
                vec![],
                vec![
                    Capability::Metadata,
                    Capability::Scan,
                    Capability::ReadContent,
                ],
                Limits {
                    page_size: 1,
                    ..Limits::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    let run = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
            .arg("--state")
            .arg(state)
            .args(args)
            .env("VOLDIQ_AGENT_TOKEN", provisioned.credential.expose())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let job = run(&["scan", root.to_str().unwrap(), "--wait"]);
    let snapshot = job["result"]["snapshot_id"].as_i64().unwrap();
    std::fs::remove_file(manifest).unwrap();
    std::fs::remove_file(profile).unwrap();
    let games = run(&["libraries", &snapshot.to_string(), "games"]);
    assert_eq!(games["result"]["recorded"], true);
    assert_eq!(games["result"]["items"][0]["installation_bytes"], "13");
    let scope = games["result"]["items"][0]["id"].as_u64().unwrap();

    let denied = call(
        metadata_client,
        "voldiq_libraries",
        json!({"snapshot_id":snapshot,"kind":"games"}),
    )
    .await;
    assert_eq!(
        denied.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );

    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_voldiq-cli"));
    command
        .arg("--state")
        .arg(state)
        .args(["mcp", "serve", "--stdio"])
        .env("VOLDIQ_AGENT_TOKEN", provisioned.credential.expose());
    let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    assert!(
        client
            .list_all_tools()
            .await
            .unwrap()
            .iter()
            .any(|tool| tool.name == "voldiq_libraries")
    );
    let media = call(
        &client,
        "voldiq_libraries",
        json!({"snapshot_id":snapshot,"kind":"media"}),
    )
    .await
    .structured_content
    .unwrap();
    assert_eq!(media["result"]["recorded"], true);
    assert_eq!(
        media["result"]["items"][0]["sizes"][0],
        json!({"category":"source_images","logical_bytes":"17"})
    );
    let narrowed = call(
        &client,
        "voldiq_libraries",
        json!({"snapshot_id":snapshot,"kind":"games","query":{"scope":scope}}),
    )
    .await
    .structured_content
    .unwrap();
    assert_eq!(narrowed["result"]["items"], json!([]));
    assert_eq!(narrowed["result"]["unattributed_bytes"], "13");
    let older = call(
        &client,
        "voldiq_libraries",
        json!({"snapshot_id":legacy,"kind":"media"}),
    )
    .await
    .structured_content
    .unwrap();
    assert_eq!(older["result"]["kind"], "recorded_scan");
    let invalid = call(
        &client,
        "voldiq_libraries",
        json!({"snapshot_id":snapshot,"kind":"games","query":{"manual":[root]}}),
    )
    .await;
    assert_eq!(invalid.is_error, Some(true));

    let mut ids = Vec::new();
    for offset in 0..2 {
        let page = call(
            &client,
            "voldiq_duplicate_candidates",
            json!({"snapshot_id":snapshot,"options":{"logical_bytes":"7","offset":offset}}),
        )
        .await
        .structured_content
        .unwrap();
        ids.push(page["result"]["members"][0]["node_id"].as_u64().unwrap());
    }
    ids.sort_unstable();
    let request = json!({"snapshot_id":snapshot,"node_ids":ids});
    let denied = call(metadata_client, "voldiq_verify_duplicates", request.clone()).await;
    assert_eq!(
        denied.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let verified = call(&client, "voldiq_verify_duplicates", request.clone()).await;
    assert_ne!(verified.is_error, Some(true), "{verified:?}");
    let result = &verified.structured_content.as_ref().unwrap()["result"];
    assert_eq!(
        result["groups"],
        json!([{"logical_bytes":"7","node_ids":ids}])
    );
    assert_eq!(result["bytes_read"], "14");

    let started = call(
        &client,
        "voldiq_duplicate_analysis_start",
        json!({"snapshot_id":snapshot,"options":{"scope":0,"min_bytes":"7"}}),
    )
    .await;
    assert_ne!(started.is_error, Some(true), "{started:?}");
    let duplicate_job = started.structured_content.unwrap()["result"]["job_id"]
        .as_i64()
        .unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let status = call(&client, "voldiq_job_get", json!({"job_id":duplicate_job}))
            .await
            .structured_content
            .unwrap();
        if status["result"]["state"] != "running" {
            assert_eq!(status["result"]["state"], "completed", "{status}");
            assert_eq!(status["result"]["snapshot_id"], snapshot);
            assert_eq!(status["result"]["progress"]["sampled"], 4);
            assert_eq!(status["result"]["duplicate_analysis"]["scope"], 0);
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{status}");
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let groups = run(&["duplicate-results", &duplicate_job.to_string()]);
    assert_eq!(groups["result"]["total"], 2);
    assert_eq!(groups["result"]["next_offset"], 1);
    assert_eq!(
        groups["result"]["groups"],
        json!([{"group":1,"logical_bytes":"8","distinct_files":2}])
    );
    let members = call(
        &client,
        "voldiq_duplicate_analysis_results",
        json!({"job_id":duplicate_job,"options":{"group":2,"offset":1}}),
    )
    .await
    .structured_content
    .unwrap();
    assert_eq!(members["result"]["node_ids"], json!([ids[1]]));
    let foreign = call(
        metadata_client,
        "voldiq_job_get",
        json!({"job_id":duplicate_job}),
    )
    .await;
    assert_eq!(
        foreign.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );

    std::fs::write(&duplicate, b"changed").unwrap();
    let stale = call(&client, "voldiq_verify_duplicates", request.clone()).await;
    assert_eq!(
        stale.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );

    registry.revoke(provisioned.id).unwrap();
    let after_revoke = call(
        &client,
        "voldiq_duplicate_analysis_results",
        json!({"job_id":duplicate_job}),
    )
    .await;
    assert_eq!(
        after_revoke.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let revoked_duplicate = call(&client, "voldiq_verify_duplicates", request).await;
    assert_eq!(
        revoked_duplicate.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    let revoked = call(
        &client,
        "voldiq_libraries",
        json!({"snapshot_id":snapshot,"kind":"media"}),
    )
    .await;
    assert_eq!(
        revoked.structured_content.unwrap()["error"]["code"],
        "not_authorized_or_unavailable"
    );
    client.cancel().await.unwrap();
}
