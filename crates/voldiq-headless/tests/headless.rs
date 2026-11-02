use serde_json::{Value, json};
use std::process::Command;
use voldiq_core::agents::{Capability, Limits, Policy, Registry};

#[test]
fn cli_queues_exact_plans_but_never_accepts_agent_approval() {
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("selected.data"), b"fixture").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(source.path().join("selected.data"))
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1709164800))
        .unwrap();
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
                    Capability::ExecuteApproved,
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
    let calendar = run(&["calendar", &snapshot.to_string(), "2024-02"]);
    assert!(calendar.status.success());
    let calendar: Value = serde_json::from_slice(&calendar.stdout).unwrap();
    assert_eq!(
        calendar["result"]["report"]["days"][28]["totals"],
        json!({"files":1,"logical":"7"})
    );
    let day = run(&["calendar", &snapshot.to_string(), "2024-02", "29", "0", "1"]);
    assert!(day.status.success());
    let day: Value = serde_json::from_slice(&day.stdout).unwrap();
    assert_eq!(day["result"]["page"]["entries"][0]["logical"], "7");
    assert_eq!(
        run(&["calendar", &snapshot.to_string(), "2024-02", "30"])
            .status
            .code(),
        Some(2)
    );
    let budget = run(&["budget", &snapshot.to_string(), "0"]);
    assert!(budget.status.success());
    let budget: Value = serde_json::from_slice(&budget.stdout).unwrap();
    assert_eq!(budget["result"]["logical_bytes"], "7");
    assert_eq!(budget["result"]["budget"], Value::Null);
    assert_eq!(
        run(&["budget", &snapshot.to_string(), "0", "--yes"])
            .status
            .code(),
        Some(2)
    );
    let extra = source.path().join("history-added.data");
    std::fs::write(&extra, [2; 5]).unwrap();
    let next_index = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let later = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save(&next_index, &std::sync::atomic::AtomicBool::new(false), &[])
        .unwrap();
    std::fs::remove_file(extra).unwrap();
    let diary = run(&["diary", &later.to_string()]);
    assert!(diary.status.success());
    let diary: Value = serde_json::from_slice(&diary.stdout).unwrap();
    assert_eq!(diary["result"]["events"][0]["detail"]["delta"], "5");
    let compared = run(&[
        "compare",
        &snapshot.to_string(),
        &later.to_string(),
        "0",
        "1",
    ]);
    assert!(compared.status.success());
    let compared: Value = serde_json::from_slice(&compared.stdout).unwrap();
    assert_eq!(compared["result"]["delta"], 5);
    assert_eq!(compared["result"]["file_changes"]["added"]["logical"], 5);
    assert_eq!(compared["result"]["changes"].as_array().unwrap().len(), 1);
    let reversed: Value = serde_json::from_slice(
        &run(&["compare", &later.to_string(), &snapshot.to_string()]).stdout,
    )
    .unwrap();
    assert_eq!(reversed["result"]["delta"], -5);
    assert_eq!(
        run(&[
            "compare",
            &snapshot.to_string(),
            &later.to_string(),
            "0",
            "201"
        ])
        .status
        .code(),
        Some(2)
    );
    assert_eq!(
        run(&[
            "compare",
            &snapshot.to_string(),
            &later.to_string(),
            "--yes"
        ])
        .status
        .code(),
        Some(2)
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
        assert_eq!(
            run(&["compare", &a.to_string(), &b.to_string()])
                .status
                .code(),
            Some(4)
        );
    }
    assert_eq!(
        run(&["calendar", &foreign.to_string(), "2024-02"])
            .status
            .code(),
        Some(4)
    );
    assert_eq!(run(&["diary", &foreign.to_string()]).status.code(), Some(4));
    let translated = run(&["translate", "archivos mayores de 1 GiB"]);
    assert!(translated.status.success());
    let translated: Value = serde_json::from_slice(&translated.stdout).unwrap();
    assert_eq!(translated["result"]["expression"], "type:file size>1GiB");
    assert_eq!(translated["result"]["unparsed"], json!([]));
    let partial = run(&["translate", "vídeos antiguos"]);
    assert!(partial.status.success());
    let partial: Value = serde_json::from_slice(&partial.stdout).unwrap();
    assert_eq!(partial["result"]["unparsed"][0]["text"], "antiguos");
    assert_eq!(
        run(&["translate", "modified before 2023-02-29"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(run(&["translate", "files", "--yes"]).status.code(), Some(2));
    let allocated = run(&[
        "query",
        &snapshot.to_string(),
        r#"{"files_only":true,"sort":"allocated"}"#,
    ]);
    assert!(allocated.status.success());
    let allocated: Value = serde_json::from_slice(&allocated.stdout).unwrap();
    assert_eq!(allocated["result"]["items"][0]["id"], node);
    let filter = if cfg!(windows) {
        "not_placeholder"
    } else {
        "unknown"
    };
    let cloud_query = format!(r#"{{"files_only":true,"cloud":"{filter}"}}"#);
    let cloud = run(&["query", &snapshot.to_string(), &cloud_query]);
    assert!(cloud.status.success());
    let cloud: Value = serde_json::from_slice(&cloud.stdout).unwrap();
    assert_eq!(cloud["result"]["items"][0]["cloud"], filter);
    assert_eq!(cloud["result"]["items"][0]["id"], node);
    let expression = run(&[
        "query",
        &snapshot.to_string(),
        r#"{"expression":"glob:*.DATA size=7B type:file"}"#,
    ]);
    assert!(expression.status.success());
    let expression: Value = serde_json::from_slice(&expression.stdout).unwrap();
    assert_eq!(expression["result"]["items"][0]["id"], node);
    for query in [
        r#"{"expression":"size>private-secret"}"#,
        r#"{"expression":"app:private-secret"}"#,
    ] {
        let invalid = run(&["query", &snapshot.to_string(), query]);
        assert_eq!(invalid.status.code(), Some(2));
        let value: Value = serde_json::from_slice(&invalid.stdout).unwrap();
        assert_eq!(value["error"]["code"], "invalid_query");
        assert!(!String::from_utf8_lossy(&invalid.stdout).contains("private-secret"));
    }
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
    let hash = plan["result"]["hash"].as_str().unwrap();
    assert_eq!(run(&["plan", "execute", id, hash]).status.code(), Some(5));
    assert_eq!(
        run(&["plan", "execute", id, hash, "--yes"]).status.code(),
        Some(2)
    );
    let desktop = voldiq_core::plans::Desktop::open(state.path()).unwrap();
    desktop.approve(id, hash).unwrap();
    let queued: Value =
        serde_json::from_slice(&run(&["plan", "execute", id, hash]).stdout).unwrap();
    assert_eq!(queued["result"]["dispatch"], "queued");
    assert_eq!(desktop.pending_execution(25).unwrap().len(), 1);
    let cancelled: Value =
        serde_json::from_slice(&run(&["plan", "cancel", id, hash]).stdout).unwrap();
    assert_eq!(cancelled["result"]["state"], "cancelled");
    let replay: Value =
        serde_json::from_slice(&run(&["plan", "execute", id, hash]).stdout).unwrap();
    assert_eq!(replay["result"]["dispatch"], "existing");
    let events: Value = serde_json::from_slice(&run(&["plan", "audit", id]).stdout).unwrap();
    assert_eq!(
        events["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["event"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "draft",
            "awaiting_approval",
            "approved",
            "execution_requested",
            "cancelled"
        ]
    );
    assert_eq!(
        std::fs::read(source.path().join("selected.data")).unwrap(),
        b"fixture"
    );
}
