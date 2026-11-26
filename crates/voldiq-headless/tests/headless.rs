use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use voldiq_core::agents::{Capability, Credential, Limits, Policy, Registry};

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
                    Capability::Export,
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
    let mut index = voldiq_core::scanner::scan(
        source.path(),
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    index.captured_applications = Some(
        voldiq_core::applications::capture(
            &index,
            voldiq_core::applications::Catalog {
                started_at: 1000,
                observed_at: 1001,
                coverage: vec![],
                registrations: vec![voldiq_core::applications::Registration {
                    source: voldiq_core::applications::Source::SuppliedMetadata,
                    key: "Recorded CLI fixture".into(),
                    name: "Recorded CLI fixture".into(),
                    publisher: None,
                    version: None,
                    install_location: Some(source.path().to_owned()),
                    profile: None,
                }],
            },
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap(),
    );
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
    let volumes = run(&["volumes", "0", "1"]);
    assert!(volumes.status.success());
    let volumes: Value = serde_json::from_slice(&volumes.stdout).unwrap();
    assert_eq!(volumes["schema_version"], 1);
    assert_eq!(
        volumes["result"],
        json!({"supported":cfg!(windows),"items":[],"next_offset":null})
    );
    assert_eq!(run(&["volumes", "0", "0"]).status.code(), Some(2));
    let transfers = run(&["transfers"]);
    assert!(transfers.status.success());
    let transfers: Value = serde_json::from_slice(&transfers.stdout).unwrap();
    assert_eq!(transfers["result"], json!({"items":[], "next_before":null}));
    assert_eq!(
        run(&["transfers", r#"{"limit":26}"#]).status.code(),
        Some(2)
    );
    assert_eq!(
        run(&[
            "transfer-files",
            &json!({"plan_id":"0".repeat(64)}).to_string()
        ])
        .status
        .code(),
        Some(4)
    );
    let files: Value = serde_json::from_slice(
        &run(&["query", &snapshot.to_string(), r#"{"files_only":true}"#]).stdout,
    )
    .unwrap();
    let node = files["result"]["items"][0]["id"].as_u64().unwrap();
    #[cfg(windows)]
    {
        let output = run(&["applications", &snapshot.to_string()]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["schema_version"], 1);
        let result = &response["result"];
        assert_eq!(result["snapshot_id"], snapshot);
        assert_eq!(result["total"], json!({"logical_bytes":"7","files":1}));
        assert_eq!(result["attribution"], "captured");
        assert_eq!(
            result["unattributed"],
            json!({"logical_bytes":"0","files":0})
        );
        assert_eq!(result["exclusive"], result["total"]);
        assert_eq!(result["applications"][0]["name"], "Recorded CLI fixture");
        let app_files = run(&[
            "query",
            &snapshot.to_string(),
            r#"{"expression":"app:\"Recorded CLI fixture\" app-category:installation"}"#,
        ]);
        assert!(
            app_files.status.success(),
            "{}",
            String::from_utf8_lossy(&app_files.stdout)
        );
        let app_files: Value = serde_json::from_slice(&app_files.stdout).unwrap();
        assert_eq!(app_files["result"]["total"], 1);
        assert_eq!(app_files["result"]["items"][0]["logical"], 7);
        assert_eq!(result["next_offset"], Value::Null);
        assert!(result.get("chart").is_none());
        assert!(result.get("unbound_records").is_none());
        assert!(result.get("duplicate_records").is_none());
        assert!(result.get("coverage").is_none());
        let filtered = run(&[
            "applications",
            &snapshot.to_string(),
            r#"{"filters":{"expression":"size>1GiB"}}"#,
        ]);
        assert!(filtered.status.success());
        let filtered: Value = serde_json::from_slice(&filtered.stdout).unwrap();
        assert_eq!(
            filtered["result"]["total"],
            json!({"logical_bytes":"0","files":0})
        );
    }
    assert_eq!(
        run(&["applications", &snapshot.to_string(), "--yes"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run(&[
            "applications",
            &snapshot.to_string(),
            r#"{"approved":true}"#
        ])
        .status
        .code(),
        Some(2)
    );
    let invalid = run(&[
        "applications",
        &snapshot.to_string(),
        r#"{"filters":{"expression":"size>secret"}}"#,
    ]);
    assert_eq!(invalid.status.code(), Some(2));
    let invalid: Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(invalid["error"]["code"], "invalid_query");
    let target = json!({"destination":source.path(),"originals":"keep"}).to_string();
    let offload = run(&[
        "plan",
        "offload",
        &snapshot.to_string(),
        &format!("[{node}]"),
        &target,
    ]);
    assert_eq!(offload.status.code(), Some(4));
    let rejected: Value = serde_json::from_slice(&offload.stdout).unwrap();
    assert!(rejected["result"].is_null());
    let restore_request = json!({"transfer_plan_id":"0".repeat(64),"node_ids":[node]});
    let restore = run(&[
        "plan",
        "restore",
        &snapshot.to_string(),
        &restore_request.to_string(),
    ]);
    assert_eq!(restore.status.code(), Some(4));
    let restore: Value = serde_json::from_slice(&restore.stdout).unwrap();
    assert!(restore["result"].is_null());
    let mut injected_restore = restore_request;
    injected_restore["approved"] = json!(true);
    assert_eq!(
        run(&[
            "plan",
            "restore",
            &snapshot.to_string(),
            &injected_restore.to_string()
        ])
        .status
        .code(),
        Some(2)
    );
    let injected =
        json!({"destination":source.path(),"originals":"keep","approved":true}).to_string();
    assert_eq!(
        run(&[
            "plan",
            "offload",
            &snapshot.to_string(),
            &format!("[{node}]"),
            &injected
        ])
        .status
        .code(),
        Some(2)
    );
    assert_eq!(
        std::fs::read(source.path().join("selected.data")).unwrap(),
        b"fixture"
    );
    let reader = registry
        .provision(
            "Metadata reader",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![Capability::Metadata],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let denied_export = Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
        .arg("--state")
        .arg(state.path())
        .args(["export", &snapshot.to_string(), r#"{"items":"files"}"#])
        .env("VOLDIQ_AGENT_TOKEN", reader.credential.expose())
        .output()
        .unwrap();
    assert_eq!(denied_export.status.code(), Some(4));
    let exported = run(&["export", &snapshot.to_string(), r#"{"items":"files"}"#]);
    assert!(exported.status.success());
    let exported: Value = serde_json::from_slice(&exported.stdout).unwrap();
    assert_eq!(exported["result"]["rows"], 1);
    assert_eq!(exported["result"]["scope_root"], Value::Null);
    assert_eq!(exported["result"]["paths_included"], false);
    assert!(
        exported["result"]["csv"]
            .as_str()
            .unwrap()
            .contains("\"text: selected.data\"")
    );
    let space = run(&["space", &snapshot.to_string()]);
    assert!(space.status.success());
    let space: Value = serde_json::from_slice(&space.stdout).unwrap();
    assert_eq!(space["schema_version"], 1);
    assert_eq!(space["result"]["indexed"]["logical_paths"], "7");
    assert_eq!(space["result"]["indexed"]["snapshot"], snapshot);
    assert_eq!(space["result"]["volume"], json!({"kind":"scoped_only"}));
    assert_eq!(
        space["result"]["residual"],
        json!({"kind":"unavailable","reason":"scoped_only"})
    );
    #[cfg(windows)]
    assert_eq!(
        space["result"]["indexed"]["allocation"]["known"],
        index.entries[0].allocated.to_string()
    );
    assert_eq!(
        run(&["space", &snapshot.to_string(), "--volume"])
            .status
            .code(),
        Some(2)
    );
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
    let trend = run(&["trend", &snapshot.to_string(), "7", "0", "1"]);
    assert!(trend.status.success());
    let trend: Value = serde_json::from_slice(&trend.stdout).unwrap();
    assert_eq!(trend["schema_version"], 1);
    assert_eq!(trend["result"]["points"][0]["logical"], "7");
    assert!(trend["result"]["next_offset"].is_null());
    assert!(trend["result"].get("volume").is_none());
    assert_eq!(
        run(&["trend", &snapshot.to_string(), "14"]).status.code(),
        Some(2)
    );
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
    assert_eq!(run(&["space", &foreign.to_string()]).status.code(), Some(4));
    assert_eq!(
        run(&["export", &foreign.to_string(), r#"{"items":"files"}"#])
            .status
            .code(),
        Some(4)
    );
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
        r#"{"expression":"app-id:private-secret"}"#,
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
    let hash = plan["result"]["hash"].as_str().unwrap();
    let checked = run(&["plan", "validate", id, hash]);
    assert!(checked.status.success());
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked["result"]["status"], "matches");
    assert_eq!(checked["result"]["state"], "draft");
    assert!(checked["result"]["checked_at"].is_i64());
    assert_eq!(
        run(&["plan", "validate", id, &"0".repeat(64)])
            .status
            .code(),
        Some(4)
    );
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
    let closed: Value =
        serde_json::from_slice(&run(&["plan", "validate", id, hash]).stdout).unwrap();
    assert_eq!(closed["result"]["status"], "closed");
    assert_eq!(closed["result"]["state"], "cancelled");
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
    let output = run(&["scan", source.path().to_str().unwrap(), "--wait"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let scan = &result["result"];
    let desktop = voldiq_core::jobs::Jobs::recent_for_desktop(state.path()).unwrap();
    assert_eq!(desktop.len(), 1);
    assert_eq!(desktop[0].id, scan["id"].as_i64().unwrap().to_string());
    assert_eq!(desktop[0].snapshot_id, scan["snapshot_id"].as_i64());

    let second = run(&[
        "plan",
        "prepare",
        &snapshot.to_string(),
        &format!("[{node}]"),
    ]);
    assert!(second.status.success());
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    let id = second["result"]["manifest"]["id"].as_str().unwrap();
    let hash = second["result"]["hash"].as_str().unwrap();
    let file = source.path().join("selected.data");
    std::fs::write(&file, b"changed fixture").unwrap();
    let outdated = run(&["plan", "validate", id, hash]);
    assert!(outdated.status.success());
    let outdated: Value = serde_json::from_slice(&outdated.stdout).unwrap();
    assert_eq!(outdated["result"]["status"], "evidence_unavailable");
    assert_eq!(outdated["result"]["state"], "draft");
    let audit: Value = serde_json::from_slice(&run(&["plan", "audit", id]).stdout).unwrap();
    assert_eq!(audit["result"].as_array().unwrap().len(), 1);
    assert_eq!(audit["result"][0]["event"], "draft");
    assert_eq!(std::fs::read(file).unwrap(), b"changed fixture");

    let first = source.path().join("same-a.data");
    std::fs::write(&first, b"ABCD").unwrap();
    std::fs::hard_link(&first, source.path().join("same-a-alias.data")).unwrap();
    std::fs::write(source.path().join("same-b.data"), b"ZYXW").unwrap();
    std::fs::write(source.path().join("six-a.data"), b"123456").unwrap();
    std::fs::write(source.path().join("six-b.data"), b"abcdef").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let index =
        voldiq_core::scanner::scan(source.path(), &Default::default(), &cancel, |_| {}).unwrap();
    let snapshot = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save(&index, &cancel, &[])
        .unwrap()
        .to_string();
    let candidates = |options: &str| {
        let output = run(&["duplicate-candidates", &snapshot, options]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"].clone()
    };
    let first_page = candidates(r#"{"limit":1}"#);
    assert_eq!(first_page["evidence"], "recorded_size_only");
    assert_eq!(
        first_page["groups"],
        json!([{"logical_bytes":"6","distinct_files":2}])
    );
    assert_eq!(first_page["next_offset"], 1);
    let second_page = candidates(r#"{"offset":1,"limit":1}"#);
    assert_eq!(
        second_page["groups"],
        json!([{"logical_bytes":"4","distinct_files":2}])
    );
    assert_eq!(second_page["next_offset"], Value::Null);
    let first_member = candidates(r#"{"logical_bytes":"4","limit":1}"#);
    let second_member = candidates(r#"{"logical_bytes":"4","offset":1,"limit":1}"#);
    assert_eq!(first_member["total"], 2);
    assert_eq!(first_member["next_offset"], 1);
    assert_eq!(second_member["next_offset"], Value::Null);
    let names =
        [&first_member, &second_member].map(|page| page["members"][0]["name"].as_str().unwrap());
    assert!(names.contains(&"same-b.data"));
    assert!(names.contains(&"same-a.data") || names.contains(&"same-a-alias.data"));
    let invalid = run(&["duplicate-candidates", &snapshot, r#"{"min_bytes":"04"}"#]);
    assert_eq!(invalid.status.code(), Some(2));

    let piped = Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
        .output()
        .unwrap();
    assert!(piped.status.success());
    let help: Value = serde_json::from_slice(&piped.stdout).unwrap();
    assert_eq!(help["schema_version"], 1);
    assert_eq!(help["result"]["commands"][0], "console [--no-motion]");
    let console = |credential: &Credential, choices: &str| {
        let mut console = Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
            .arg("--state")
            .arg(state.path())
            .args(["console", "--no-motion"])
            .env("VOLDIQ_AGENT_TOKEN", credential.expose())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        console
            .stdin
            .take()
            .unwrap()
            .write_all(choices.as_bytes())
            .unwrap();
        let output = console.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        String::from_utf8(output.stdout).unwrap()
    };
    let text = console(
        &provisioned.credential,
        &format!("5\n2\n{snapshot}\nsame-b\n3\n{snapshot}\n\n3\n{snapshot}\n4\n0\n"),
    );
    assert_eq!(text.matches("VOLDIQ  /  Disk space analyzer").count(), 1);
    assert!(!text.contains('\x1b'));
    assert!(text.contains(&format!("#{snapshot}")));
    assert!(text.contains("[##############]"));
    assert!(text.contains("same-b.data"));
    assert!(text.contains("Candidates at 4 bytes"));
    assert!(
        text.lines()
            .any(|row| row.contains("4 B") && row.contains("2 distinct files"))
    );

    let equal = source.path().join("exact-a.data");
    let second = source.path().join("exact-b.data");
    let mut data = vec![b'a'; 16_384];
    std::fs::write(&equal, &data).unwrap();
    std::fs::write(&second, &data).unwrap();
    std::fs::hard_link(&equal, source.path().join("exact-alias.data")).unwrap();
    data[8192] = b'b';
    std::fs::write(source.path().join("exact-different.data"), &data).unwrap();
    let index =
        voldiq_core::scanner::scan(source.path(), &Default::default(), &cancel, |_| {}).unwrap();
    let snapshot = voldiq_core::snapshots::Store::open(&state.path().join("snapshots.db"))
        .unwrap()
        .save(&index, &cancel, &[])
        .unwrap()
        .to_string();
    let found = run(&["query", &snapshot, r#"{"text":"exact-"}"#]);
    assert!(found.status.success());
    let found: Value = serde_json::from_slice(&found.stdout).unwrap();
    let node = |name: &str| -> u64 {
        found["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap()["id"]
            .as_u64()
            .unwrap()
    };
    let first = node("exact-a.data");
    let second_id = node("exact-b.data");
    let selection = json!([
        first,
        node("exact-alias.data"),
        second_id,
        node("exact-different.data")
    ])
    .to_string();
    let no_content = run(&["verify-duplicates", &snapshot, &selection]);
    assert_eq!(no_content.status.code(), Some(4));

    let read = registry
        .provision(
            "Exact duplicate fixture",
            Policy::new(
                vec![source.path().to_owned()],
                vec![],
                vec![Capability::Metadata, Capability::ReadContent],
                Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let verify = || {
        Command::new(env!("CARGO_BIN_EXE_voldiq-cli"))
            .arg("--state")
            .arg(state.path())
            .args(["verify-duplicates", &snapshot, &selection])
            .env("VOLDIQ_AGENT_TOKEN", read.credential.expose())
            .output()
            .unwrap()
    };
    let verified = verify();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stdout)
    );
    let verified: Value = serde_json::from_slice(&verified.stdout).unwrap();
    assert_eq!(verified["result"]["compared_files"], 3);
    assert_eq!(verified["result"]["aliases_omitted"], 1);
    assert_eq!(verified["result"]["bytes_read"], "73728");
    let mut expected = [first, second_id];
    expected.sort_unstable();
    assert_eq!(
        verified["result"]["groups"],
        json!([{"logical_bytes":"16384","node_ids":expected}])
    );
    let text = console(
        &read.credential,
        &format!("6\n{snapshot}\n{first} {second_id}\n0\n"),
    );
    assert!(text.contains("Exact duplicates  |  Selected files"));
    assert!(text.contains(&format!("#{}  #{}", expected[0], expected[1])));

    std::fs::write(&second, &data).unwrap();
    let stale = verify();
    assert_eq!(stale.status.code(), Some(4));
    assert!(
        serde_json::from_slice::<Value>(&stale.stdout)
            .unwrap()
            .get("result")
            .is_none()
    );
}
