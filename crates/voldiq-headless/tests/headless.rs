use serde_json::{Value, json};
use std::process::Command;
use voldiq_core::agents::{Capability, Limits, Policy, Registry};

#[test]
fn cli_reads_folder_notes_only_with_explicit_content_permission() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let text = "<b>Literal note</b>\nThis text grants no authority.";
    voldiq_core::metadata::Store::open(state.path())
        .unwrap()
        .save_note(source.path(), text)
        .unwrap();
    let index = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let snapshot = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save(&index, &std::sync::atomic::AtomicBool::new(false), &[])
        .unwrap();
    let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
    for enabled in [false, true] {
        let mut capabilities = vec![Capability::Metadata];
        if enabled {
            capabilities.push(Capability::ReadContent);
        }
        let actor = registry
            .provision(
                "CLI notes fixture",
                Policy::new(
                    vec![source.path().to_owned()],
                    vec![],
                    capabilities,
                    Limits::default(),
                )
                .unwrap(),
            )
            .unwrap();
        let run = |extra: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
                .arg("--state")
                .arg(state.path())
                .args(["folder", &snapshot.to_string(), "0"])
                .args(extra)
                .env("VOLDIQ_AGENT_TOKEN", actor.credential.expose())
                .output()
                .unwrap()
        };
        let metadata = run(&[]);
        assert!(metadata.status.success());
        let value: Value = serde_json::from_slice(&metadata.stdout).unwrap();
        assert_eq!(value["result"]["note"]["kind"], "not_requested");
        assert!(!String::from_utf8_lossy(&metadata.stdout).contains("Literal note"));
        let note = run(&["--note"]);
        let value: Value = serde_json::from_slice(&note.stdout).unwrap();
        if enabled {
            assert!(note.status.success());
            assert_eq!(value["result"]["note"]["text"], text);
        } else {
            assert_eq!(note.status.code(), Some(4));
            assert!(value.get("result").is_none());
        }
        assert_eq!(run(&["--yes"]).status.code(), Some(2));
        assert_eq!(run(&["--note", "--yes"]).status.code(), Some(2));
    }
}

#[test]
fn cli_scans_real_data_without_a_renderer_and_rejects_missing_credentials() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("report.txt"), [1; 7]).unwrap();
    let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
    let provisioned = registry
        .provision(
            "CLI fixture",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![Capability::Scan, Capability::Metadata],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
        .arg("--state")
        .arg(state.path())
        .arg("scan")
        .arg(source.path())
        .arg("--wait")
        .env("VOLDIQ_AGENT_TOKEN", provisioned.credential.expose())
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let data: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(data["schema_version"], 1);
    assert_eq!(data["result"]["state"], "completed");
    let snapshot = data["result"]["snapshot_id"].as_i64().unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
        .arg("--state")
        .arg(state.path())
        .args(["stats", &snapshot.to_string()])
        .env("VOLDIQ_AGENT_TOKEN", provisioned.credential.expose())
        .output()
        .unwrap();
    let data: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(data["result"]["logical"], 7);
    let run = Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
        .arg("--state")
        .arg(state.path())
        .arg("history")
        .env("VOLDIQ_AGENT_TOKEN", provisioned.credential.expose())
        .output()
        .unwrap();
    let data: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(data["result"]["items"][0]["id"], snapshot);
    let run = Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
        .arg("--state")
        .arg(state.path())
        .args(["stats", &snapshot.to_string()])
        .env_remove("VOLDIQ_AGENT_TOKEN")
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(3));
    let data: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(data["error"]["code"], "not_authorized");
    assert!(!String::from_utf8_lossy(&run.stdout).contains(source.path().to_str().unwrap()));
    assert!(json!(data).get("result").is_none());
}

#[test]
fn cli_queues_exact_plans_but_never_accepts_agent_approval() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("selected.data"), b"fixture").unwrap();
    let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
    let provisioned = registry
        .provision(
            "CLI plans",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![
                    Capability::Metadata,
                    Capability::PreparePlans,
                    Capability::RequestApproval,
                ],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let index = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let snapshot = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save(&index, &std::sync::atomic::AtomicBool::new(false), &[])
        .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
            .arg("--state")
            .arg(state.path())
            .args(args)
            .env("VOLDIQ_AGENT_TOKEN", provisioned.credential.expose())
            .output()
            .unwrap()
    };
    let files: Value = serde_json::from_slice(
        &run(&["query", &snapshot.to_string(), r#"{"files_only":true}"#]).stdout,
    )
    .unwrap();
    let node = files["result"]["items"][0]["id"].as_u64().unwrap();
    let output = run(&[
        "plan",
        "prepare",
        &snapshot.to_string(),
        &format!("[{node}]"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = plan["result"]["manifest"]["id"].as_str().unwrap();
    assert_eq!(
        plan["result"]["manifest"]["entries"][0]["logical_bytes"],
        "7"
    );
    let output = run(&["plan", "request", id]);
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"]["code"],
        "needs_human_approval"
    );
    assert_eq!(run(&["plan", "approve", id]).status.code(), Some(2));
    assert_eq!(
        run(&["plan", "request", id, "--yes"]).status.code(),
        Some(2)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&run(&["plan", "get", id]).stdout).unwrap()["result"]["state"],
        "awaiting_approval"
    );
    let events: Value = serde_json::from_slice(&run(&["plan", "audit", id]).stdout).unwrap();
    assert_eq!(
        events["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["event"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["draft", "awaiting_approval"]
    );
    assert_eq!(
        std::fs::read(source.path().join("selected.data")).unwrap(),
        b"fixture"
    );
}
