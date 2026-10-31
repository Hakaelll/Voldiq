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
    assert_eq!(
        std::fs::read(source.path().join("measured.txt")).unwrap(),
        [1; 11]
    );
    registry.revoke(provisioned.id).unwrap();
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
