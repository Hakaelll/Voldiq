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
                Limits::default(),
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
    assert_eq!(
        call(&client, "voldiq_plan_get", json!({"plan_id":plan_id}))
            .await
            .is_error,
        Some(true)
    );
    client.cancel().await.unwrap();
}
