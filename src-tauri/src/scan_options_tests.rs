use voldiq_core::{ScanMode, ScanOptions};

#[test]
fn scan_mode_defaults_to_auto_and_rejects_unknown_ipc_values() {
    let defaults: ScanOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(defaults.mode, ScanMode::Auto);
    assert_eq!(defaults.concurrency.get(), 1);
    let options: ScanOptions =
        serde_json::from_str(r#"{"mode":"directory","exclusions":["C:\\excluded"]}"#).unwrap();
    assert_eq!(options.mode, ScanMode::Directory);
    assert_eq!(options.exclusions, [r"C:\excluded"]);
    for value in [r#"{"mode":"ntfs"}"#, r#"{"mode":null}"#, r#"{"mode":1}"#] {
        assert!(serde_json::from_str::<ScanOptions>(value).is_err());
    }
}

#[test]
fn concurrency_is_bounded_and_omitted_requests_use_one_worker() {
    for workers in 1..=8 {
        let options: ScanOptions =
            serde_json::from_value(serde_json::json!({"concurrency": workers})).unwrap();
        assert_eq!(options.concurrency.get(), workers as usize);
    }
    for value in [
        serde_json::json!(0),
        serde_json::json!(9),
        serde_json::json!(256),
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!(null),
        serde_json::json!("2"),
    ] {
        assert!(
            serde_json::from_value::<ScanOptions>(serde_json::json!({"concurrency": value}))
                .is_err()
        );
    }
}
