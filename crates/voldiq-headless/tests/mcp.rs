use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use serde_json::{Value, json};
use voldiq_core::agents::{Capability, Limits, Policy, Registry};

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
    let stats = call(&client, "voldiq_stats", json!({"snapshot_id":snapshot})).await;
    assert_eq!(stats.structured_content.unwrap()["result"]["logical"], 11);
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
    let index = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let later = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save(&index, &std::sync::atomic::AtomicBool::new(false), &[])
        .unwrap();
    std::fs::remove_file(extra).unwrap();
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
        ("voldiq_space_account", json!({"snapshot_id":foreign})),
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
        "app:private-secret",
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
    let prepared = call(
        &client,
        "voldiq_plan_prepare",
        json!({"snapshot_id":snapshot,"node_ids":[selected]}),
    )
    .await;
    assert_ne!(prepared.is_error, Some(true));
    let prepared = prepared.structured_content.unwrap();
    let plan_id = prepared["result"]["manifest"]["id"].as_str().unwrap();
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
    let hash = prepared["result"]["hash"].as_str().unwrap();
    let execution = json!({"plan_id":plan_id,"manifest_hash":hash});
    assert!(tools.iter().any(|tool| tool.name == "voldiq_plan_execute"));
    assert!(tools.iter().any(|tool| tool.name == "voldiq_plan_cancel"));
    let no_human = call(&client, "voldiq_plan_execute", execution.clone()).await;
    assert_eq!(
        no_human.structured_content.unwrap()["error"]["code"],
        "needs_human_approval"
    );
    let desktop = voldiq_core::plans::Desktop::open(state.path()).unwrap();
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
    registry.revoke(provisioned.id).unwrap();
    for (name, args) in [
        (
            "voldiq_calendar",
            json!({"snapshot_id":snapshot,"month":"invalid"}),
        ),
        ("voldiq_diary", json!({"snapshot_id":snapshot})),
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
    assert_eq!(
        call(&client, "voldiq_plan_get", json!({"plan_id":plan_id}))
            .await
            .is_error,
        Some(true)
    );
    client.cancel().await.unwrap();
}
