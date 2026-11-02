use std::sync::atomic::AtomicBool;
use voldiq_core::{NodeId, ScanOptions, cloud::CloudState, index::Query, scanner};

#[test]
fn syntax_inspection_is_structured_bounded_and_does_not_require_an_index() {
    let valid = serde_json::to_value(
        Query {
            expression: "glob:*.bin size>1KiB".into(),
            ..Query::default()
        }
        .inspect(),
    )
    .unwrap();
    assert_eq!(valid, serde_json::json!({"kind":"valid","conditions":2}));
    let invalid = serde_json::to_value(
        Query {
            expression: "app:private-secret".into(),
            ..Query::default()
        }
        .inspect(),
    )
    .unwrap();
    assert_eq!(invalid["kind"], "invalid");
    assert_eq!(invalid["offset"], 0);
    assert!(!invalid.to_string().contains("private-secret"));
}

fn fixture() -> (tempfile::TempDir, voldiq_core::index::Index) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("Archive")).unwrap();
    std::fs::write(root.path().join("Report Final.BIN"), vec![0; 1600]).unwrap();
    std::fs::write(root.path().join("report.txt"), vec![0; 1000]).unwrap();
    std::fs::write(
        root.path().join("Archive").join("old movie.mp4"),
        vec![0; 2048],
    )
    .unwrap();
    std::fs::write(root.path().join("readme"), []).unwrap();
    let mut index = scanner::scan(
        root.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    for entry in &mut index.entries {
        entry.cloud = CloudState::NotPlaceholder;
        if entry.is_metadata_file() {
            entry.modified = match entry.name.as_str() {
                "Report Final.BIN" => Some(1709164800), // 2024-02-29 UTC
                "report.txt" => Some(1709251199),
                "old movie.mp4" => Some(1709251200), // 2024-03-01 UTC
                _ => None,
            };
        }
    }
    (root, index)
}

fn names(index: &voldiq_core::index::Index, expression: &str) -> Vec<String> {
    index
        .query(&Query {
            expression: expression.into(),
            sort: "name".into(),
            ..Query::default()
        })
        .unwrap()
        .items
        .into_iter()
        .map(|e| e.name)
        .collect()
}

#[test]
fn expressions_share_scope_paging_metadata_filters_and_literal_search() {
    let (_root, index) = fixture();
    assert_eq!(
        names(&index, "report type:file size>=1KB size<2KiB"),
        ["Report Final.BIN", "report.txt"]
    );
    assert_eq!(
        names(&index, "\"report final\" ext:bin"),
        ["Report Final.BIN"]
    );
    assert_eq!(names(&index, "glob:*.MP4 size=2KiB"), ["old movie.mp4"]);
    assert_eq!(
        names(&index, "size>1.5KB size<1.6KiB"),
        ["Report Final.BIN"]
    );
    assert_eq!(names(&index, "size>=0.1KiB size<1.5KiB"), ["report.txt"]);
    assert_eq!(names(&index, "ext:none type:file"), ["readme"]);
    assert_eq!(
        names(&index, "cloud:not_placeholder glob:report*"),
        ["Report Final.BIN", "report.txt"]
    );
    assert!(names(&index, "cloud:unknown").is_empty());
    let archive = index
        .entries
        .iter()
        .find(|e| e.name == "Archive")
        .unwrap()
        .id;
    let query = Query {
        expression: "type:file size>0".into(),
        scope: Some(archive),
        ..Query::default()
    };
    assert_eq!(index.query(&query).unwrap().items[0].name, "old movie.mp4");
    let page = index
        .query(&Query {
            expression: "glob:report*".into(),
            offset: 1,
            limit: 1,
            sort: "name".into(),
            ..Query::default()
        })
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.items[0].name, "report.txt");
    assert!(
        index
            .query(&Query {
                expression: "size>1KiB".into(),
                extension: "txt".into(),
                ..Query::default()
            })
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        index
            .query(&Query {
                text: "size>1KiB".into(),
                ..Query::default()
            })
            .unwrap()
            .items
            .is_empty()
    );
    assert!(index.query_cancellable(&query, || true).unwrap().is_none());
    assert!(
        index
            .query(&Query {
                scope: Some(NodeId(u32::MAX)),
                ..query
            })
            .is_err()
    );
}

#[test]
fn dates_use_utc_days_or_exact_seconds_and_unknown_dates_never_match() {
    let (_root, index) = fixture();
    assert_eq!(
        names(&index, "type:file modified=2024-02-29"),
        ["Report Final.BIN", "report.txt"]
    );
    assert_eq!(
        names(&index, "type:file modified>2024-02-29"),
        ["old movie.mp4"]
    );
    assert_eq!(
        names(&index, "type:file modified>=2024-03-01"),
        ["old movie.mp4"]
    );
    assert_eq!(
        names(&index, "type:file modified<2024-03-01"),
        ["Report Final.BIN", "report.txt"]
    );
    assert_eq!(
        names(&index, "type:file modified=2024-02-29T23:59:59Z"),
        ["report.txt"]
    );
    assert_eq!(
        names(&index, "type:file modified!=2024-02-29"),
        ["old movie.mp4"]
    );
    assert!(names(&index, "name:readme modified!=2024-02-29").is_empty());
}

#[test]
fn paths_and_globs_filter_only_indexed_metadata_without_changing_authority() {
    let (_root, index) = fixture();
    assert_eq!(names(&index, "path:Archive type:file"), ["old movie.mp4"]);
    assert_eq!(
        names(&index, "pathglob:*Archive*movie.mp4"),
        ["old movie.mp4"]
    );
    assert_eq!(names(&index, "glob:re?ort.*"), ["report.txt"]);
    assert!(names(&index, "glob:movie").is_empty());
    assert!(names(&index, "path:../outside").is_empty());
    assert_eq!(
        names(
            &index,
            &format!("path:\"{}\" type:file", index.root.display())
        )
        .len(),
        4
    );
    let compiled = Query {
        expression: "glob:report*".into(),
        ..Query::default()
    };
    assert_eq!(
        index
            .query_compiled(&compiled.compile().unwrap())
            .unwrap()
            .total,
        2
    );
}

#[test]
fn unknown_reparse_sizes_and_incomplete_folder_totals_are_not_measured_zero_or_upper_bounds() {
    let (_root, mut index) = fixture();
    let folder = index
        .entries
        .iter_mut()
        .find(|e| e.name == "Archive")
        .unwrap();
    folder.incomplete = true;
    assert_eq!(names(&index, "type:folder size>=2KiB"), ["Archive"]);
    assert!(names(&index, "type:folder size=2KiB").is_empty());
    assert!(names(&index, "type:folder size<3KiB").is_empty());
    assert_eq!(names(&index, "size=0 type:file"), ["readme"]);
    let id = index
        .entries
        .iter()
        .position(|e| e.name == "readme")
        .unwrap();
    index.entries[id].kind = voldiq_core::Kind::Link;
    index.entries[id].incomplete = true;
    index.entries[id].cloud = CloudState::Unknown;
    assert!(names(&index, "name:readme size>=0").is_empty());
    index.entries[id].cloud = CloudState::FileRecallRequired;
    index.entries[id].allocation_known = false;
    assert_eq!(names(&index, "name:readme size=0"), ["readme"]);
    index.entries[id].name = "read (me)".into();
    assert_eq!(names(&index, "name:\"read (me)\""), ["read (me)"]);
}

#[test]
fn invalid_conditions_are_typed_errors_without_echoing_user_values_or_discarding_terms() {
    let (_root, index) = fixture();
    for expression in [
        "size>nonsense",
        "size>1G",
        "size>18446744073709551616B",
        "size>-1B",
        "size>1..5KB",
        "modified=2023-02-29",
        "modified=2024-13-01",
        "modified=1969-01-01",
        "modified=2024-01-01T24:00:00Z",
        "modified=2024-01-01T00:00:00+01:00",
        "cloud:online_only",
        "type:device",
        "app:private-secret",
        "ext:",
        "name:",
        "glob:[ab]",
        "name~abc",
        "size>>1",
        "report OR movie",
        "\"unclosed",
        "()",
    ] {
        let error = index
            .query(&Query {
                expression: expression.into(),
                ..Query::default()
            })
            .err()
            .unwrap();
        assert!(
            matches!(error, voldiq_core::Error::Query(_)),
            "{expression}: {error}"
        );
        assert!(!error.to_string().contains("private-secret"));
    }
    assert!(
        index
            .query(&Query {
                expression: "a ".repeat(65),
                ..Query::default()
            })
            .is_err()
    );
    assert!(
        index
            .query(&Query {
                expression: "a".repeat(4097),
                ..Query::default()
            })
            .is_err()
    );
    assert!(
        index
            .query(&Query {
                sort: "unsupported".into(),
                ..Query::default()
            })
            .is_err()
    );
    let error = index
        .query(&Query {
            expression: "é size>bad".into(),
            ..Query::default()
        })
        .err()
        .unwrap();
    let voldiq_core::Error::Query(error) = error else {
        panic!("Expected syntax error")
    };
    assert_eq!(error.offset, 3);
}
