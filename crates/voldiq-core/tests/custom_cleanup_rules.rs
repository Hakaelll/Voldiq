#![cfg(windows)]

use std::{
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, UNIX_EPOCH},
};
use voldiq_core::{
    ScanOptions,
    cleanup::{
        rules::{
            Query,
            custom::{Draft, Environment, Store},
            now,
        },
        tray::{self, Group, Source},
    },
    scanner::scan,
};

#[test]
fn saved_rules_recheck_scope_and_revision_before_cleanup() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let project = root.join("Project");
    let protected = project.join("Keep");
    let state = project.join("Voldiq state");
    let excluded = project.join("Excluded");
    for folder in [&protected, &state, &excluded] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let measured = now().unwrap();
    let old = UNIX_EPOCH + Duration::from_secs(measured - 2 * 86_400);
    for (relative, size) in [
        ("Project/report.TMP", 9),
        ("Project/trace.tmp", 7),
        ("Project/tiny.tmp", 2),
        ("Project/notes.log", 15),
        ("Project/Keep/protected.tmp", 16),
        ("Project/Voldiq state/state.tmp", 17),
        ("Project/Excluded/excluded.tmp", 18),
        ("outside.tmp", 19),
    ] {
        let path = root.join(relative);
        std::fs::write(&path, vec![1u8; size]).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
    }
    std::fs::write(project.join("recent.tmp"), [2u8; 10]).unwrap();
    let mut metadata = voldiq_core::metadata::Store::open(&state).unwrap();
    metadata.protect_folder(&protected).unwrap();
    let protections = metadata.protections().unwrap();
    let cancel = AtomicBool::new(false);
    let index = scan(
        root,
        &ScanOptions {
            exclusions: vec![excluded.to_string_lossy().into_owned()],
            ..Default::default()
        },
        &cancel,
        |_| {},
    )
    .unwrap();
    let id = |name| {
        index
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
            .id
    };
    let scope = id("Project");
    let draft = Draft {
        name: "Old reports".into(),
        extension: ".TMP".into(),
        minimum_age_days: 1,
        minimum_logical: "5".into(),
        expression: "name:report".into(),
    };
    let mut rules = Store::open(&state).unwrap();
    let invalid = Draft {
        extension: "*".into(),
        ..draft.clone()
    };
    assert!(
        rules
            .save(&index, Some(scope), None, invalid, &cancel)
            .is_err()
    );
    let invalid_expression = Draft {
        expression: "condition-that-does-not-exist:tmp".into(),
        ..draft.clone()
    };
    assert!(
        rules
            .save(&index, Some(scope), None, invalid_expression, &cancel)
            .is_err()
    );
    let old_draft = Draft {
        expression: String::new(),
        ..draft.clone()
    };
    let legacy = rules
        .save(&index, Some(scope), None, old_draft, &cancel)
        .unwrap();
    assert_eq!(legacy.reference.revision, 1);
    drop(rules);
    {
        let previous = rusqlite::Connection::open(state.join("cleanup-rules.db")).unwrap();
        previous
            .execute_batch("ALTER TABLE rules DROP COLUMN expression; PRAGMA user_version=1;")
            .unwrap();
    }
    let mut rules = Store::open(&state).unwrap();
    let listed = rules.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].reference, legacy.reference);
    assert_eq!(listed[0].criteria.extension, "tmp");
    assert_eq!(listed[0].criteria.expression, "");
    let query = Query {
        offset: 0,
        limit: 1,
        weights: Default::default(),
    };
    let env = Environment {
        now: measured,
        protections: &protections,
        duplicates: None,
    };
    let old_page = rules
        .query(&index, legacy.reference, query, &env, &cancel)
        .unwrap();
    assert_eq!((old_page.total, old_page.logical.as_str()), (2, "16"));
    let created = rules
        .save(&index, None, Some(legacy.reference), draft.clone(), &cancel)
        .unwrap();
    assert_eq!(created.reference.revision, 2);
    assert_eq!(rules.list().unwrap()[0].criteria.expression, "name:report");
    let page = rules
        .query(&index, created.reference, query, &env, &cancel)
        .unwrap();
    assert_eq!((page.total, page.logical.as_str()), (1, "9"));
    assert_eq!(page.items[0].logical, "9");
    assert!(page.items[0].path.ends_with("report.TMP"));

    let tray_env = tray::Environment {
        temporary: root,
        local_data: root,
        caches: &[],
        state: &state,
        now: measured,
        duplicates: None,
    };
    let source = Source::Custom {
        reference: created.reference,
    };
    for wrong in [
        "outside.tmp",
        "protected.tmp",
        "state.tmp",
        "recent.tmp",
        "trace.tmp",
    ] {
        let group = Group {
            source: source.clone(),
            selection: vec![id(wrong)],
        };
        assert!(tray::prepare(&index, &[group], &tray_env, &cancel).is_err());
    }
    let group = Group {
        source,
        selection: vec![page.items[0].id],
    };
    let preview = tray::prepare(&index, std::slice::from_ref(&group), &tray_env, &cancel).unwrap();
    assert_eq!(preview.origins[0].reference, Some(created.reference));
    assert_eq!(preview.plan.preview().logical, 9);
    preview.plan.validate_protections().unwrap();
    let updated = rules
        .save(
            &index,
            None,
            Some(created.reference),
            Draft {
                minimum_logical: "10".into(),
                ..draft
            },
            &cancel,
        )
        .unwrap();
    assert_eq!(updated.reference.revision, 3);
    assert_eq!(updated.root, created.root);
    assert!(preview.plan.validate_protections().is_err());
    assert!(tray::prepare(&index, &[group], &tray_env, &cancel).is_err());
    assert!(rules.remove(created.reference).is_err());
    assert_eq!(
        rules
            .query(&index, updated.reference, query, &env, &cancel)
            .unwrap()
            .total,
        0
    );

    // Recreate the rule folder at the same path with a different native identity.
    drop(rules);
    drop(preview);
    drop(protections);
    drop(metadata);
    let relocated = root.join("Relocated");
    std::fs::rename(&project, &relocated).unwrap();
    std::fs::create_dir(&project).unwrap();
    let relocated_state = relocated.join(Path::new("Voldiq state"));
    let mut rules = Store::open(&relocated_state).unwrap();
    let protections = voldiq_core::metadata::Store::open(&relocated_state)
        .unwrap()
        .protections()
        .unwrap();
    let env = Environment {
        now: measured,
        protections: &protections,
        duplicates: None,
    };
    assert!(
        rules
            .query(&index, updated.reference, query, &env, &cancel)
            .is_err()
    );
    rules.remove(updated.reference).unwrap();
    assert!(rules.list().unwrap().is_empty());
}
