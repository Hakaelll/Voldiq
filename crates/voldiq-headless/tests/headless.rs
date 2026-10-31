use serde_json::{Value, json};
use std::process::Command;
use voldiq_core::agents::{Capability, Limits, Policy, Registry};

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
