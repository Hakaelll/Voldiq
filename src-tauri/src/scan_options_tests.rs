use voldiq_core::{ScanMode, ScanOptions};

#[test]
fn scan_mode_defaults_to_auto_and_rejects_unknown_ipc_values() {
    let defaults: ScanOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(defaults.mode, ScanMode::Auto);
    let options: ScanOptions =
        serde_json::from_str(r#"{"mode":"directory","exclusions":["C:\\excluded"]}"#).unwrap();
    assert_eq!(options.mode, ScanMode::Directory);
    assert_eq!(options.exclusions, [r"C:\excluded"]);
    for value in [r#"{"mode":"ntfs"}"#, r#"{"mode":null}"#, r#"{"mode":1}"#] {
        assert!(serde_json::from_str::<ScanOptions>(value).is_err());
    }
}
