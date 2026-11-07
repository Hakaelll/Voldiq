use std::{path::Path, sync::atomic::AtomicBool};
use voldiq_core::{
    ScanOptions,
    index::{Index, Query},
    scanner::scan,
    snapshots::Store,
};

fn capture(path: &Path) -> Index {
    scan(
        path,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap()
}

#[test]
fn history_survives_restart_and_compares_actual_directory_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let db = database.path().join("history.db");
    let loading = AtomicBool::new(false);
    std::fs::create_dir(fixture.path().join("nested")).unwrap();
    std::fs::write(fixture.path().join("nested/growth"), [1; 12]).unwrap();
    std::fs::write(fixture.path().join("removed"), [2; 7]).unwrap();
    std::fs::write(fixture.path().join("UPPER.txt"), [5; 7]).unwrap();
    std::fs::write(fixture.path().join("Résumé.txt"), [6; 7]).unwrap();
    let first = capture(fixture.path());
    {
        use voldiq_core::applications::{Catalog, Query as AppQuery, Registration, Source};
        let owned = tempfile::tempdir().unwrap();
        let suite = owned.path().join("Suite");
        let module = suite.join("bin");
        std::fs::create_dir_all(&module).unwrap();
        std::fs::create_dir(owned.path().join("Suite-other")).unwrap();
        std::fs::write(suite.join("app.bin"), [1; 11]).unwrap();
        std::fs::write(module.join("module.bin"), [2; 7]).unwrap();
        std::fs::write(owned.path().join("unattributed"), [3; 3]).unwrap();
        std::fs::write(owned.path().join("Suite-other/file"), [4; 5]).unwrap();
        let index = capture(owned.path());
        let suite_record = Registration {
            source: Source::CurrentUser64,
            key: "Suite".into(),
            name: "Suite".into(),
            publisher: Some("Fixture publisher".into()),
            version: Some("1".into()),
            install_location: Some(suite.clone()),
            profile: None,
        };
        let mut duplicate = suite_record.clone();
        duplicate.source = Source::CurrentUser32;
        let mut machine_duplicate = suite_record.clone();
        machine_duplicate.source = Source::Machine64;
        machine_duplicate.key = "SUITE".into();
        let catalog = Catalog {
            started_at: 1000,
            observed_at: 1001,
            coverage: vec![],
            registrations: vec![
                suite_record,
                duplicate,
                machine_duplicate,
                Registration {
                    source: Source::Machine64,
                    key: "Module".into(),
                    name: "Module".into(),
                    publisher: None,
                    version: None,
                    install_location: Some(module.clone()),
                    profile: None,
                },
                Registration {
                    source: Source::SuppliedMetadata,
                    key: "Missing".into(),
                    name: "Missing".into(),
                    publisher: None,
                    version: None,
                    install_location: Some(owned.path().join("missing")),
                    profile: None,
                },
            ],
        };
        let report = voldiq_core::applications::query(
            &index,
            catalog.clone(),
            &AppQuery::default(),
            &loading,
        )
        .unwrap();
        assert_eq!(
            (&report.total.logical_bytes, report.total.files),
            (&"26".to_string(), 4)
        );
        assert_eq!(
            (&report.exclusive.logical_bytes, report.exclusive.files),
            (&"11".to_string(), 1)
        );
        assert_eq!(
            (&report.overlapping.logical_bytes, report.overlapping.files),
            (&"7".to_string(), 1)
        );
        assert_eq!(
            (
                &report.unattributed.logical_bytes,
                report.unattributed.files
            ),
            (&"8".to_string(), 2)
        );
        assert_eq!(
            (
                report.application_count,
                report.duplicate_records,
                report.unbound_records
            ),
            (2, 2, 1)
        );
        assert_eq!(report.applications[0].name, "Suite");
        assert_eq!(report.applications[0].sources.len(), 3);
        assert_ne!(report.applications[0].id, report.applications[1].id);
        let bin = index
            .entries
            .iter()
            .find(|entry| entry.name == "bin")
            .unwrap()
            .id;
        let scoped = voldiq_core::applications::query(
            &index,
            catalog.clone(),
            &AppQuery {
                scope: bin,
                ..Default::default()
            },
            &loading,
        )
        .unwrap();
        assert_eq!(
            (&scoped.total.logical_bytes, scoped.total.files),
            (&"7".to_string(), 1)
        );
        assert_eq!(scoped.overlapping.logical_bytes, "7");
        assert_eq!(scoped.exclusive.logical_bytes, "0");
        let narrow = capture(&module);
        let narrow_report = voldiq_core::applications::query(
            &narrow,
            catalog.clone(),
            &AppQuery::default(),
            &loading,
        )
        .unwrap();
        assert_eq!(narrow_report.total.logical_bytes, "7");
        assert_eq!(narrow_report.overlapping.logical_bytes, "7");
        assert_eq!(narrow_report.application_count, 2);
        assert!(
            narrow_report
                .applications
                .iter()
                .all(|app| app.root == voldiq_core::NodeId(0))
        );
        let page = voldiq_core::applications::query(
            &index,
            catalog.clone(),
            &AppQuery {
                offset: 1,
                limit: 1,
                ..Default::default()
            },
            &loading,
        )
        .unwrap();
        assert_eq!(
            (
                page.application_count,
                page.applications.len(),
                page.total.files
            ),
            (2, 1, 4)
        );
        assert_eq!(page.applications[0].name, "Module");
        let filtered = voldiq_core::applications::query(
            &index,
            catalog.clone(),
            &AppQuery {
                filters: Query {
                    expression: "ext:bin".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            &loading,
        )
        .unwrap();
        assert_eq!(
            (&filtered.total.logical_bytes, filtered.total.files),
            (&"18".to_string(), 2)
        );
        assert_eq!(filtered.unattributed.logical_bytes, "0");
        let application_filtered = voldiq_core::applications::query(
            &index,
            catalog.clone(),
            &AppQuery {
                filters: Query {
                    expression: "app:Suite".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            &loading,
        )
        .unwrap();
        assert_eq!(
            (
                application_filtered.total.logical_bytes.as_str(),
                application_filtered.total.files
            ),
            ("18", 2)
        );
        assert!(
            voldiq_core::applications::query(
                &index,
                catalog.clone(),
                &AppQuery {
                    filters: Query {
                        parent: Some(bin),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &loading
            )
            .is_err()
        );
        if std::env::var("VOLDIQ_KEEP_APPLICATION_FIXTURE").as_deref() == Ok("1") {
            let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/probes/application-fixture.json");
            std::fs::create_dir_all(output.parent().unwrap()).unwrap();
            std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
        assert!(
            voldiq_core::applications::query(
                &index,
                catalog.clone(),
                &AppQuery::default(),
                &AtomicBool::new(true)
            )
            .is_err()
        );
        assert!(
            voldiq_core::applications::query(
                &index,
                catalog.clone(),
                &AppQuery {
                    limit: 101,
                    ..Default::default()
                },
                &loading
            )
            .is_err()
        );
        let moved = tempfile::tempdir().unwrap();
        std::fs::rename(&suite, moved.path().join("original")).unwrap();
        std::fs::create_dir(&suite).unwrap();
        std::fs::write(suite.join("new-unscanned"), [5; 99]).unwrap();
        let changed =
            voldiq_core::applications::query(&index, catalog, &AppQuery::default(), &loading)
                .unwrap();
        assert_eq!(changed.application_count, 0);
        assert_eq!(
            (
                &changed.unattributed.logical_bytes,
                changed.unattributed.files
            ),
            (&"26".to_string(), 4)
        );
        #[cfg(windows)]
        {
            let actual = voldiq_core::applications::installed_programs(&loading).unwrap();
            assert_eq!(actual.coverage.len(), 4);
            assert!(actual.registrations.len() <= voldiq_core::applications::MAX_REGISTRATIONS);
            assert!(actual.observed_at > 0);
        }
        {
            use voldiq_core::applications::{BrowserLayout, Category, ProfileLocation};
            use voldiq_core::scanner;
            let profiles = tempfile::tempdir().unwrap();
            let local = profiles.path().join("local");
            let data = local.join("Google/Chrome/User Data");
            let cache = data.join("Default/Cache/Cache_Data");
            std::fs::create_dir_all(&cache).unwrap();
            std::fs::create_dir(data.with_file_name("sibling")).unwrap();
            std::fs::write(profiles.path().join("browser.exe"), [1; 11]).unwrap();
            std::fs::write(data.join("Local State"), [2; 5]).unwrap();
            std::fs::write(data.join("chrome_debug.log"), [3; 3]).unwrap();
            std::fs::write(cache.join("f_000001"), [4; 7]).unwrap();
            let review_now = voldiq_core::cleanup::rules::now().unwrap();
            std::fs::OpenOptions::new()
                .write(true)
                .open(cache.join("f_000001"))
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(review_now - 8 * 86400),
                ))
                .unwrap();
            std::fs::write(cache.parent().unwrap().join("legacy.data"), [6; 1]).unwrap();
            std::fs::create_dir_all(data.join("custom/Cache/Cache_Data")).unwrap();
            std::fs::write(data.join("custom/Cache/Cache_Data/private"), [5; 13]).unwrap();
            let browser = Registration {
                source: Source::SuppliedMetadata,
                key: "Google Chrome".into(),
                name: "Google Chrome".into(),
                publisher: Some("Google LLC".into()),
                version: Some("1".into()),
                install_location: Some(profiles.path().to_owned()),
                profile: Some(ProfileLocation {
                    path: data.clone(),
                    layout: BrowserLayout::ChromeStable,
                }),
            };
            let claims = Catalog {
                started_at: 1000,
                observed_at: 1001,
                registrations: vec![browser.clone()],
                coverage: vec![],
            };
            let mut indexed =
                scanner::scan(profiles.path(), &Default::default(), &loading, |_| {}).unwrap();
            let measured = voldiq_core::applications::query(
                &indexed,
                claims.clone(),
                &AppQuery::default(),
                &loading,
            )
            .unwrap();
            assert_eq!(
                (&measured.total.logical_bytes, measured.total.files),
                (&"40".to_string(), 6)
            );
            assert_eq!(measured.overlapping.files, 0);
            use voldiq_core::applications::review;
            let review_state = tempfile::tempdir().unwrap();
            let mut notes = voldiq_core::metadata::Store::open(review_state.path()).unwrap();
            let protections = notes.protections().unwrap();
            let environment = review::Environment {
                local_data: &local,
                now: review_now,
                protections: &protections,
            };
            let context = review::Context {
                scope: voldiq_core::NodeId(0),
                application_id: measured.applications[0].id.clone(),
            };
            let review_request = review::Request {
                context: context.clone(),
                offset: 0,
                limit: 50,
            };
            let candidates = review::query(
                &indexed,
                claims.clone(),
                &review_request,
                &environment,
                &loading,
            )
            .unwrap();
            assert_eq!(
                (candidates.candidates.total, candidates.candidates.logical),
                (1, 7)
            );
            assert_eq!(
                candidates.candidates.rule.id,
                "old-browser-http-cache-files"
            );
            let selected = [candidates.candidates.items[0].id];
            let planned = review::preview(
                &indexed,
                claims.clone(),
                &context,
                &environment,
                &selected,
                &loading,
            )
            .unwrap()
            .with_protections(review_state.path())
            .unwrap();
            assert_eq!(
                (planned.preview().items.len(), planned.preview().logical),
                (1, 7)
            );
            assert_eq!(std::fs::read(cache.join("f_000001")).unwrap(), [4; 7]);
            assert!(
                review::preview(
                    &indexed,
                    claims.clone(),
                    &context,
                    &environment,
                    &[selected[0], selected[0]],
                    &loading
                )
                .is_err()
            );
            let log = indexed
                .entries
                .iter()
                .find(|entry| entry.name == "chrome_debug.log")
                .unwrap()
                .id;
            assert!(
                review::preview(
                    &indexed,
                    claims.clone(),
                    &context,
                    &environment,
                    &[log],
                    &loading
                )
                .is_err()
            );
            assert!(
                review::query(
                    &indexed,
                    claims.clone(),
                    &review_request,
                    &environment,
                    &AtomicBool::new(true)
                )
                .is_err()
            );
            let mut changed_claims = claims.clone();
            changed_claims.registrations[0].version = Some("changed".into());
            assert!(
                review::preview(
                    &indexed,
                    changed_claims,
                    &context,
                    &environment,
                    &selected,
                    &loading
                )
                .is_err()
            );
            if std::env::var("VOLDIQ_KEEP_APPLICATION_FIXTURE").as_deref() == Ok("1") {
                std::fs::write(
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../target/probes/application-cache-fixture.json"),
                    serde_json::to_vec_pretty(&candidates).unwrap(),
                )
                .unwrap();
            }
            if std::env::var("VOLDIQ_KEEP_APPLICATION_FIXTURE").as_deref() == Ok("1") {
                let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/probes/application-profile-fixture.json");
                std::fs::write(output, serde_json::to_vec_pretty(&measured).unwrap()).unwrap();
            }
            let breakdown: Vec<_> = measured.applications[0]
                .breakdown
                .iter()
                .map(|row| {
                    (
                        row.category,
                        row.exclusive.logical_bytes.as_str(),
                        row.exclusive.files,
                        row.overlapping.files,
                    )
                })
                .collect();
            assert_eq!(
                breakdown,
                vec![
                    (Category::Installation, "11", 1, 0),
                    (Category::Data, "18", 2, 0),
                    (Category::Cache, "8", 2, 0),
                    (Category::Logs, "3", 1, 0)
                ]
            );
            let mut shared = claims.clone();
            shared.registrations.push(Registration {
                source: Source::SuppliedMetadata,
                key: "Cache consumer".into(),
                name: "Cache consumer".into(),
                publisher: None,
                version: None,
                install_location: Some(cache.clone()),
                profile: None,
            });
            assert!(
                review::query(
                    &indexed,
                    shared.clone(),
                    &review_request,
                    &environment,
                    &loading
                )
                .is_err()
            );
            for (expression, files) in [
                ("app:\"Cache consumer\" app-category:cache", 0),
                ("app:\"Cache consumer\" app-category:installation", 1),
            ] {
                let result = voldiq_core::applications::query(
                    &indexed,
                    shared.clone(),
                    &AppQuery {
                        filters: Query {
                            expression: expression.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    &loading,
                )
                .unwrap();
                assert_eq!(result.total.files, files, "{expression}");
            }
            let shared =
                voldiq_core::applications::query(&indexed, shared, &AppQuery::default(), &loading)
                    .unwrap();
            assert_eq!(
                (
                    &shared.exclusive.logical_bytes,
                    &shared.overlapping.logical_bytes
                ),
                (&"33".to_string(), &"7".to_string())
            );
            let browser_row = shared
                .applications
                .iter()
                .find(|app| app.name == "Google Chrome")
                .unwrap();
            let cache_row = browser_row
                .breakdown
                .iter()
                .find(|row| row.category == Category::Cache)
                .unwrap();
            assert_eq!(
                (
                    &cache_row.exclusive.logical_bytes,
                    &cache_row.overlapping.logical_bytes
                ),
                (&"1".to_string(), &"7".to_string())
            );
            let mut scoped = scanner::scan(&cache, &Default::default(), &loading, |_| {}).unwrap();
            let narrow = voldiq_core::applications::query(
                &scoped,
                claims.clone(),
                &AppQuery::default(),
                &loading,
            )
            .unwrap();
            assert_eq!(narrow.applications[0].breakdown.len(), 1);
            assert_eq!(
                narrow.applications[0].breakdown[0].category,
                Category::Cache
            );
            assert_eq!(narrow.applications[0].exclusive.logical_bytes, "7");
            let narrow_context = review::Context {
                scope: voldiq_core::NodeId(0),
                application_id: narrow.applications[0].id.clone(),
            };
            let narrow_page = review::query(
                &scoped,
                claims.clone(),
                &review::Request {
                    context: narrow_context,
                    offset: 0,
                    limit: 50,
                },
                &environment,
                &loading,
            )
            .unwrap();
            assert_eq!(
                (narrow_page.candidates.total, narrow_page.candidates.logical),
                (1, 7)
            );
            scoped.snapshot_id = Some(1);
            assert!(
                review::query(
                    &scoped,
                    claims.clone(),
                    &review_request,
                    &environment,
                    &loading
                )
                .is_err()
            );
            notes.protect_folder(&cache).unwrap();
            let protected = notes.protections().unwrap();
            let protected_environment = review::Environment {
                local_data: &local,
                now: review_now,
                protections: &protected,
            };
            assert_eq!(
                review::query(
                    &indexed,
                    claims.clone(),
                    &review_request,
                    &protected_environment,
                    &loading
                )
                .unwrap()
                .candidates
                .total,
                0
            );
            assert!(planned.validate_protections().is_err());
            assert!(
                review::preview(
                    &indexed,
                    claims.clone(),
                    &context,
                    &protected_environment,
                    &selected,
                    &loading
                )
                .is_err()
            );
            let data_index = scanner::scan(&data, &Default::default(), &loading, |_| {}).unwrap();
            let state = tempfile::tempdir().unwrap();
            use voldiq_core::agents::{Capability, Limits, Policy, Registry};
            let mut registry = Registry::open(&state.path().join("agents.db")).unwrap();
            let connection = registry
                .provision(
                    "Profile-only fixture",
                    Policy::new(
                        vec![data.clone()],
                        vec![],
                        vec![Capability::Metadata],
                        Limits::default(),
                    )
                    .unwrap(),
                )
                .unwrap();
            let principal = registry
                .authenticate(connection.credential.expose())
                .unwrap();
            assert!(
                registry
                    .authorize(&principal, Capability::Metadata, profiles.path())
                    .is_err()
            );
            let authorized = voldiq_core::applications::query_authorized(
                &data_index,
                claims.clone(),
                &AppQuery::default(),
                &loading,
                |path| registry.authorize(&principal, Capability::Metadata, path),
            )
            .unwrap();
            assert_eq!(authorized.applications.len(), 1);
            assert_eq!(authorized.applications[0].exclusive.logical_bytes, "29");
            assert_eq!(authorized.unattributed.files, 0);
            assert!(
                !authorized.applications[0]
                    .breakdown
                    .iter()
                    .any(|row| row.category == Category::Installation)
            );
            let mut missing = browser.clone();
            let mut profile_only = browser.clone();
            profile_only.install_location = None;
            let profile_only = voldiq_core::applications::query(
                &indexed,
                Catalog {
                    registrations: vec![profile_only],
                    ..claims.clone()
                },
                &AppQuery::default(),
                &loading,
            )
            .unwrap();
            assert_eq!(profile_only.exclusive.logical_bytes, "29");
            assert_eq!(profile_only.unattributed.logical_bytes, "11");
            assert!(
                !profile_only.applications[0]
                    .breakdown
                    .iter()
                    .any(|row| row.category == Category::Installation)
            );
            missing.profile.as_mut().unwrap().path = profiles.path().join("unavailable");
            let missing = voldiq_core::applications::query(
                &indexed,
                Catalog {
                    registrations: vec![missing],
                    ..claims.clone()
                },
                &AppQuery::default(),
                &loading,
            )
            .unwrap();
            assert_eq!(missing.applications[0].breakdown.len(), 1);
            assert_eq!(
                missing.applications[0].breakdown[0].category,
                Category::Installation
            );
            let mut conflict = browser.clone();
            conflict.profile.as_mut().unwrap().path = profiles.path().to_owned();
            let conflict = voldiq_core::applications::query(
                &indexed,
                Catalog {
                    registrations: vec![conflict],
                    ..claims.clone()
                },
                &AppQuery::default(),
                &loading,
            )
            .unwrap();
            assert!(
                conflict.applications[0]
                    .breakdown
                    .iter()
                    .any(|row| row.category == Category::Unknown)
            );
            let mut invalid = browser;
            let mut edge = invalid.clone();
            edge.key = "Microsoft Edge".into();
            edge.name = "Microsoft Edge".into();
            edge.publisher = Some("Microsoft Corporation".into());
            edge.profile.as_mut().unwrap().layout = BrowserLayout::EdgeStable;
            let edge = voldiq_core::applications::query(
                &indexed,
                Catalog {
                    registrations: vec![edge],
                    ..claims.clone()
                },
                &AppQuery::default(),
                &loading,
            )
            .unwrap();
            assert!(
                !edge.applications[0]
                    .breakdown
                    .iter()
                    .any(|row| row.category == Category::Logs)
            );
            assert_eq!(
                edge.applications[0]
                    .breakdown
                    .iter()
                    .find(|row| row.category == Category::Data)
                    .unwrap()
                    .exclusive
                    .logical_bytes,
                "21"
            );
            let captured_state = tempfile::tempdir().unwrap();
            let capture_db = captured_state.path().join("applications.db");
            let mut history = Store::open(&capture_db).unwrap();
            let legacy_id = history.save(&indexed, &loading, &[]).unwrap();
            let legacy = history.load(legacy_id, &loading).unwrap();
            assert!(
                legacy
                    .query(&Query {
                        expression: "app-category:cache".into(),
                        ..Default::default()
                    })
                    .is_err()
            );
            assert!(
                voldiq_core::applications::read_desktop(&legacy, &AppQuery::default(), &loading)
                    .is_err()
            );
            indexed.captured_applications = Some(
                voldiq_core::applications::capture(&indexed, claims.clone(), &loading).unwrap(),
            );
            let indexed_apps =
                voldiq_core::applications::read_desktop(&indexed, &AppQuery::default(), &loading)
                    .unwrap();
            assert!(matches!(
                indexed_apps.attribution,
                voldiq_core::applications::Attribution::Indexed
            ));
            if std::env::var("VOLDIQ_KEEP_APPLICATION_FIXTURE").as_deref() == Ok("1") {
                std::fs::write(
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../target/probes/application-search-indexed-fixture.json"),
                    serde_json::to_vec_pretty(&indexed_apps).unwrap(),
                )
                .unwrap();
            }
            for (expression, bytes, files) in [
                ("app:\"google chrome\" app-category:cache".to_string(), 8, 2),
                ("app-category:cache size>1B".into(), 7, 1),
                ("app-category:data".into(), 18, 2),
                ("app-category:logs".into(), 3, 1),
                ("app-category:unknown".into(), 0, 0),
                ("app-category:cache app-category:data".into(), 0, 0),
                ("app:unregistered".into(), 0, 0),
                (format!("app-id:{}", measured.applications[0].id), 40, 6),
            ] {
                let page = indexed
                    .query(&Query {
                        expression,
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(
                    (
                        page.items.iter().map(|entry| entry.logical).sum::<u64>(),
                        page.total
                    ),
                    (bytes, files)
                );
                assert!(
                    page.items
                        .iter()
                        .all(|entry| entry.kind == voldiq_core::Kind::File)
                );
            }
            for expression in ["app:", "app-id:abc", "app-category:temporary"] {
                assert!(
                    Query {
                        expression: expression.into(),
                        ..Default::default()
                    }
                    .compile()
                    .is_err()
                );
            }
            let translated = voldiq_core::search::translate(
                "application cache and application named \"Google Chrome\"",
            )
            .unwrap();
            assert!(translated.unparsed.is_empty());
            assert_eq!(
                indexed
                    .query(&Query {
                        expression: translated.expression,
                        ..Default::default()
                    })
                    .unwrap()
                    .total,
                2
            );
            let mut calls = 0;
            assert!(
                indexed
                    .query_cancellable(
                        &Query {
                            expression: "app-category:cache".into(),
                            ..Default::default()
                        },
                        || {
                            calls += 1;
                            calls > 3
                        }
                    )
                    .unwrap()
                    .is_none()
            );
            #[cfg(windows)]
            {
                use voldiq_core::agents::{Capability, Limits, Policy, Registry};
                let agent_state = tempfile::tempdir().unwrap();
                let mut child = capture(&data);
                let ancestor_claim = Catalog {
                    registrations: vec![Registration {
                        profile: None,
                        name: "Outside application claim".into(),
                        ..claims.registrations[0].clone()
                    }],
                    ..claims.clone()
                };
                child.captured_applications = Some(
                    voldiq_core::applications::capture(&child, ancestor_claim, &loading).unwrap(),
                );
                assert_eq!(
                    child
                        .query(&Query {
                            expression: "app:\"Outside application claim\"".into(),
                            ..Default::default()
                        })
                        .unwrap()
                        .total,
                    5,
                    "{}",
                    serde_json::to_string(&child.captured_applications).unwrap()
                );
                let child_id = Store::open(&agent_state.path().join("snapshots.db"))
                    .unwrap()
                    .save(&child, &loading, &[])
                    .unwrap();
                let mut registry = Registry::open(&agent_state.path().join("agents.db")).unwrap();
                for (root, files) in [(&data, 0), (&profiles.path().to_owned(), 5)] {
                    let reader = registry
                        .provision(
                            "Application scope",
                            Policy::new(
                                vec![root.clone()],
                                vec![],
                                vec![Capability::Metadata],
                                Limits::default(),
                            )
                            .unwrap(),
                        )
                        .unwrap();
                    let service = voldiq_core::service::AgentService::new(
                        agent_state.path().to_owned(),
                        registry.authenticate(reader.credential.expose()).unwrap(),
                    )
                    .unwrap();
                    for _ in 0..2 {
                        let page = service
                            .query(
                                child_id,
                                &Query {
                                    expression: "app:\"Outside application claim\"".into(),
                                    ..Default::default()
                                },
                                &loading,
                            )
                            .unwrap()
                            .unwrap();
                        assert_eq!(
                            page.total,
                            files,
                            "{}",
                            serde_json::to_string(
                                &service
                                    .applications(child_id, Default::default(), &loading)
                                    .unwrap()
                            )
                            .unwrap()
                        );
                    }
                    registry.revoke(reader.id).unwrap();
                    assert!(
                        service
                            .query(child_id, &Query::default(), &loading)
                            .is_err()
                    );
                }
            }
            let first_capture = history.save(&indexed, &loading, &[]).unwrap();
            let profile_id = indexed
                .entries
                .iter()
                .find(|entry| entry.name == "User Data")
                .unwrap()
                .id;
            let sibling_id = indexed
                .entries
                .iter()
                .find(|entry| entry.name == "sibling")
                .unwrap()
                .id;
            let remap = |id| {
                if id == profile_id {
                    sibling_id
                } else if id == sibling_id {
                    profile_id
                } else {
                    id
                }
            };
            for entry in &mut indexed.entries {
                entry.id = remap(entry.id);
                entry.parent = entry.parent.map(remap);
            }
            indexed.entries.sort_by_key(|entry| entry.id.0);
            indexed.native_names = indexed
                .native_names
                .drain()
                .map(|(id, name)| (remap(id), name))
                .collect();
            indexed.captured_applications = Some(
                voldiq_core::applications::capture(&indexed, claims.clone(), &loading).unwrap(),
            );
            let captured_id = history.save(&indexed, &loading, &[]).unwrap();
            drop(history);
            let mut history = Store::open(&capture_db).unwrap();
            let saved = history.load(captured_id, &loading).unwrap();
            assert!(saved.entries.iter().all(|entry| entry.file_state.is_none()));
            let retired = data.with_file_name("retired-profile");
            assert_eq!(data.parent(), retired.parent());
            assert!(
                profiles.path().is_absolute()
                    && data.starts_with(profiles.path())
                    && retired.starts_with(profiles.path())
                    && !retired.exists()
            );
            std::fs::rename(&data, &retired).unwrap();
            assert!(
                voldiq_core::applications::query(
                    &saved,
                    claims.clone(),
                    &AppQuery::default(),
                    &loading
                )
                .is_err()
            );
            let recorded =
                voldiq_core::applications::read_desktop(&saved, &AppQuery::default(), &loading)
                    .unwrap();
            assert!(matches!(
                recorded.attribution,
                voldiq_core::applications::Attribution::Captured
            ));
            assert_eq!(
                (&recorded.total.logical_bytes, recorded.total.files),
                (&"40".to_string(), 6)
            );
            assert_eq!(
                (
                    recorded.registrations_started_at,
                    recorded.registrations_observed_at
                ),
                (1000, 1001)
            );
            let loaded_profile = saved
                .entries
                .iter()
                .find(|entry| entry.name == "User Data")
                .unwrap()
                .id;
            assert_ne!(loaded_profile, remap(profile_id));
            let search = Query {
                expression: "app:\"Google Chrome\" app-category:cache".into(),
                ..Default::default()
            };
            assert_eq!(saved.query(&search).unwrap().total, 2);
            let metrics = saved
                .map_metrics_cancellable(
                    voldiq_core::NodeId(0),
                    &[loaded_profile],
                    None,
                    Some(&search.compile().unwrap()),
                    || false,
                )
                .unwrap()
                .unwrap();
            assert_eq!(metrics[0].matches.as_ref().unwrap().descendants, 2);
            let mut csv = Vec::new();
            let report = voldiq_core::exports::stream_csv(
                &saved,
                &voldiq_core::exports::ReportSelection {
                    items: voldiq_core::exports::ReportItems::Files,
                    filter: voldiq_core::exports::ReportFilter {
                        expression: search.expression.clone(),
                        ..Default::default()
                    },
                    include_paths: false,
                },
                &mut csv,
                &loading,
                |_| {},
            )
            .unwrap();
            assert!(
                matches!(report, voldiq_core::exports::StreamOutcome::Complete(progress) if progress.rows == 2)
            );
            assert!(!String::from_utf8(csv).unwrap().contains("browser.exe"));
            assert_eq!(recorded.applications[0].profile_scope, Some(loaded_profile));
            assert_eq!(
                recorded.applications[0]
                    .breakdown
                    .iter()
                    .map(|part| (part.category, part.exclusive.logical_bytes.as_str()))
                    .collect::<Vec<_>>(),
                vec![
                    (Category::Installation, "11"),
                    (Category::Data, "18"),
                    (Category::Cache, "8"),
                    (Category::Logs, "3")
                ]
            );
            let cache_node = saved
                .entries
                .iter()
                .find(|entry| entry.name == "Cache_Data" && entry.logical == 7)
                .unwrap()
                .id;
            let narrowed = voldiq_core::applications::query_captured(
                &saved,
                &AppQuery {
                    scope: cache_node,
                    ..Default::default()
                },
                &loading,
                |_| Ok(()),
            )
            .unwrap();
            assert_eq!(narrowed.total.logical_bytes, "7");
            assert!(
                voldiq_core::applications::read_desktop(
                    &saved,
                    &AppQuery::default(),
                    &AtomicBool::new(true)
                )
                .is_err()
            );
            assert!(
                review::preview(
                    &saved,
                    claims.clone(),
                    &context,
                    &environment,
                    &selected,
                    &loading
                )
                .is_err()
            );
            if std::env::var("VOLDIQ_KEEP_APPLICATION_FIXTURE").as_deref() == Ok("1") {
                std::fs::write(
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../target/probes/application-history-fixture.json"),
                    serde_json::to_vec_pretty(&recorded).unwrap(),
                )
                .unwrap();
            }
            std::fs::rename(&retired, &data).unwrap();
            indexed.captured_applications = Some(voldiq_core::applications::Capture::unavailable());
            assert!(indexed.query(&search).is_err());
            let unavailable_id = history.save(&indexed, &loading, &[]).unwrap();
            assert!(
                voldiq_core::applications::read_desktop(
                    &history.load(unavailable_id, &loading).unwrap(),
                    &AppQuery::default(),
                    &loading
                )
                .is_err()
            );
            let retention = history.retention_settings(captured_id).unwrap();
            history
                .set_retention(captured_id, &retention.version, Some(1))
                .unwrap();
            let preview = history
                .preview_retention(captured_id, &loading)
                .unwrap()
                .unwrap();
            history
                .apply_retention(captured_id, &preview.fingerprint, &loading)
                .unwrap();
            let database = rusqlite::Connection::open(&capture_db).unwrap();
            assert_eq!(
                database
                    .query_row(
                        "SELECT COUNT(*) FROM snapshot_applications WHERE snapshot_id IN (?1,?2)",
                        rusqlite::params![first_capture, captured_id],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
            assert_eq!(database.query_row("SELECT COUNT(*) FROM snapshot_application_roots WHERE snapshot_id IN (?1,?2)", rusqlite::params![first_capture, captured_id], |row| row.get::<_, i64>(0)).unwrap(), 0);
            invalid.publisher = Some("Unrecognized publisher".into());
            assert!(
                voldiq_core::applications::query(
                    &indexed,
                    Catalog {
                        registrations: vec![invalid],
                        ..claims
                    },
                    &AppQuery::default(),
                    &loading
                )
                .is_err()
            );
        }
    }
    for (stop, replace, concurrency, expected_entries) in [
        (false, false, 8, 5),
        (true, false, 1, 2),
        (false, true, 8, 5),
    ] {
        use std::sync::{Arc, atomic::Ordering, mpsc};
        use voldiq_core::scanner::{ScanControl, ScanControlState};
        let control = Arc::new(ScanControl::default());
        assert!(!control.request_pause().unwrap());
        let options = ScanOptions {
            mode: voldiq_core::ScanMode::Directory,
            concurrency: voldiq_core::ScanConcurrency::try_from(concurrency).unwrap(),
            profile: voldiq_core::ScanProfile::Fast,
            control: Some(control.clone()),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);
        let (send, receive) = mpsc::sync_channel(1);
        let nested = fixture.path().join("nested");
        let moved = database.path().join("moved");
        std::thread::scope(|scope| {
            let observed_control = control.clone();
            let observed_cancel = &cancel;
            let nested = &nested;
            let moved = &moved;
            let observer = scope.spawn(move || {
                let observed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let shared: voldiq_core::scanner::SharedIndex = receive
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    while observed_control.state().unwrap() != ScanControlState::Paused
                        && std::time::Instant::now() < deadline
                    {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    assert_eq!(observed_control.state().unwrap(), ScanControlState::Paused);
                    assert_eq!(shared.read().unwrap().entries.len(), expected_entries);
                    if replace {
                        std::fs::rename(nested, moved).unwrap();
                        std::fs::create_dir(nested).unwrap();
                        std::fs::write(nested.join("replacement-marker"), [9; 200]).unwrap();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    assert_eq!(shared.read().unwrap().entries.len(), expected_entries);
                }));
                if stop || observed.is_err() {
                    observed_cancel.store(true, Ordering::Relaxed);
                }
                if !stop || observed.is_err() {
                    observed_control.resume().unwrap();
                }
                observed
            });
            let result = voldiq_core::scanner::scan_live(
                fixture.path(),
                &options,
                &cancel,
                |progress| {
                    if progress.phase == voldiq_core::ScanPhase::Scanning && progress.entries == 2 {
                        control.request_pause().unwrap();
                    }
                },
                |shared| {
                    send.send(shared).unwrap();
                },
            );
            observer.join().unwrap().unwrap();
            let shared = result.unwrap();
            let index = shared.read().unwrap();
            assert_eq!(index.entries[0].incomplete, stop || replace);
            if stop {
                assert_eq!(index.entries.len(), 2);
            } else if replace {
                assert_eq!((index.entries[0].logical, index.entries[0].files), (21, 3));
                assert!(index.entries[0].incomplete);
                assert!(index.error_count > 0);
                assert!(
                    !index
                        .entries
                        .iter()
                        .any(|entry| entry.name == "replacement-marker")
                );
            } else {
                assert_eq!((index.entries[0].logical, index.entries[0].files), (33, 4));
            }
        });
        if replace {
            std::fs::remove_dir_all(&nested).unwrap();
            std::fs::rename(&moved, &nested).unwrap();
        }
        assert_eq!(control.state().unwrap(), ScanControlState::Finished);
        assert!(!control.resume().unwrap());
        assert!(!control.request_pause().unwrap());
        assert!(scan(fixture.path(), &options, &AtomicBool::new(false), |_| {}).is_err());
    }
    let raw_control = std::sync::Arc::new(voldiq_core::scanner::ScanControl::default());
    assert!(
        scan(
            fixture.path(),
            &ScanOptions {
                control: Some(raw_control.clone()),
                ..Default::default()
            },
            &loading,
            |_| {}
        )
        .is_err()
    );
    assert_eq!(
        raw_control.state().unwrap(),
        voldiq_core::scanner::ScanControlState::Idle
    );
    assert!(
        serde_json::from_str::<ScanOptions>(r#"{"mode":"directory","control":true}"#)
            .unwrap()
            .control
            .is_none()
    );
    for (profile, workers, buffer) in [
        (voldiq_core::ScanProfile::Eco, 1, 64 * 1024),
        (voldiq_core::ScanProfile::Balanced, 2, 256 * 1024),
        (voldiq_core::ScanProfile::Fast, 8, 1024 * 1024),
    ] {
        let options = ScanOptions {
            mode: voldiq_core::ScanMode::Directory,
            concurrency: voldiq_core::ScanConcurrency::try_from(8).unwrap(),
            profile,
            ..Default::default()
        };
        let resources = profile.resources(options.concurrency, fixture.path());
        assert_eq!(
            (
                resources.workers,
                resources.batch_entries,
                resources.ntfs_buffer_bytes
            ),
            (workers, workers * 16, buffer)
        );
        let measured = scan(fixture.path(), &options, &loading, |_| {}).unwrap();
        assert_eq!(
            (measured.entries[0].logical, measured.entries[0].files),
            (33, 4)
        );
        assert!(!measured.entries[0].incomplete);
        let cancelled = AtomicBool::new(false);
        let partial = scan(fixture.path(), &options, &cancelled, |_| {
            cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        })
        .unwrap();
        assert!(partial.entries[0].incomplete);
        assert_eq!(partial.entries[0].files, 0);
    }
    assert!(serde_json::from_str::<ScanOptions>(r#"{"profile":"turbo"}"#).is_err());
    assert_eq!(
        serde_json::from_str::<ScanOptions>(r#"{}"#)
            .unwrap()
            .profile,
        voldiq_core::ScanProfile::Auto
    );
    assert_eq!(
        voldiq_core::recovery::cim_datetime("20240229013000.123456+090"),
        Some(1709164800)
    );
    assert_eq!(
        voldiq_core::recovery::cim_datetime("20240228223000.000000-090"),
        Some(1709164800)
    );
    assert_eq!(
        voldiq_core::recovery::cim_datetime("19700101000000.000000+000"),
        Some(0)
    );
    for invalid in [
        "20230229000000.000000+000",
        "20240229000000.000000+***",
        "2024**********.******+000",
        "19700101000000.000000+060",
        "20240229000000.000000:000",
    ] {
        assert_eq!(voldiq_core::recovery::cim_datetime(invalid), None);
    }
    assert!(voldiq_core::recovery::read_desktop(&first, &AtomicBool::new(true)).is_none());
    #[cfg(windows)]
    {
        use voldiq_core::recovery::{Discovery, Unavailable};
        let report = voldiq_core::recovery::read_desktop(&first, &loading).unwrap();
        assert_eq!(report.root, first.root);
        assert_eq!(
            report.volume.as_ref().unwrap().serial,
            format!("{:016x}", first.root_identity.unwrap().0)
        );
        assert_eq!(report.copy_limit, 100);
        let evidence = match report.shadows {
            Discovery::Available { copies, coverage } => {
                assert!(copies.len() <= 100);
                serde_json::json!({"kind":"available", "copies":copies.len(), "coverage":coverage})
            }
            Discovery::Unavailable {
                reason: reason @ (Unavailable::AccessDenied | Unavailable::ProviderUnavailable),
            } => serde_json::json!({"kind":"unavailable", "reason":reason}),
            other => panic!("Unexpected owned-root recovery probe outcome: {other:?}"),
        };
        eprintln!("RECOVERY_DISCOVERY_PROBE {evidence}");
        let recovery_root = fixture.path().join("recovery-identity");
        std::fs::create_dir(&recovery_root).unwrap();
        let recorded_root = capture(&recovery_root);
        std::fs::rename(&recovery_root, fixture.path().join("recovery-original")).unwrap();
        std::fs::create_dir(&recovery_root).unwrap();
        assert!(matches!(
            voldiq_core::recovery::read_desktop(&recorded_root, &loading)
                .unwrap()
                .shadows,
            Discovery::Unavailable {
                reason: Unavailable::RootUnavailable
            }
        ));
        std::fs::remove_dir(&recovery_root).unwrap();
        std::fs::remove_dir(fixture.path().join("recovery-original")).unwrap();
    }
    let mut store = Store::open(&db).unwrap();
    let before = store.save(&first, &AtomicBool::new(false), &[]).unwrap();
    std::fs::write(fixture.path().join("nested/growth"), [3; 33]).unwrap();
    std::fs::remove_file(fixture.path().join("removed")).unwrap();
    std::fs::write(fixture.path().join("added"), [4; 14]).unwrap();
    let second = capture(fixture.path());
    let after = store.save(&second, &AtomicBool::new(false), &[]).unwrap();
    let preference = store.retention_settings(after).unwrap();
    assert_eq!(preference.keep_latest, None);
    let preference = store
        .set_retention(after, &preference.version, Some(1))
        .unwrap();
    let review = store.preview_retention(after, &loading).unwrap().unwrap();
    assert_eq!(
        review
            .captures
            .iter()
            .map(|capture| capture.id)
            .collect::<Vec<_>>(),
        [before]
    );
    assert_eq!(review.settings.keep_latest, Some(1));
    assert!(store.set_retention(after, "0", Some(2)).is_err());
    let preference = store
        .set_retention(after, &preference.version, None)
        .unwrap();
    assert!(
        store
            .apply_retention(after, &review.fingerprint, &loading)
            .is_err()
    );
    assert_eq!(
        store.retention_settings(after).unwrap().version,
        preference.version
    );
    assert!(store.save(&second, &AtomicBool::new(true), &[]).is_err());
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
    drop(store);
    let store = Store::open(&db).unwrap();
    let history = store.list(0, 10).unwrap();
    assert_eq!(history.len(), 2);
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        store.load(-1, &cancelled).err().unwrap().to_string(),
        "Snapshot loading cancelled."
    );
    assert_eq!(
        store.load(before, &cancelled).err().unwrap().to_string(),
        "Snapshot loading cancelled."
    );
    assert_eq!(history[0].id, after);
    let loaded = store.load(before, &loading).unwrap();
    assert_eq!(loaded.observation, first.observation);
    assert!(
        loaded
            .observation
            .as_ref()
            .is_some_and(|observation| observation.complete)
    );
    assert_eq!(loaded.entries[0].logical, 33);
    let frame = store.visual_capture(before, &loading).unwrap().unwrap();
    assert_eq!(frame.snapshot, before);
    assert_eq!(frame.logical.as_deref(), Some("33"));
    assert_eq!(frame.entries.len(), 4);
    assert_eq!(frame.observation, first.observation);
    let mut clip_request = voldiq_core::snapshots::ExportClipRequest {
        snapshot_ids: vec![after, before],
        include_paths: false,
    };
    let clip = store
        .export_clip(&clip_request, &loading, |_| {})
        .unwrap()
        .unwrap();
    assert_eq!(
        clip.frames
            .iter()
            .map(|frame| frame.snapshot)
            .collect::<Vec<_>>(),
        [before, after]
    );
    assert_eq!(clip.frames[0].logical.as_deref(), Some("33"));
    assert_eq!(clip.frames[1].logical.as_deref(), Some("61"));
    assert_eq!(clip.maximum_logical, "61");
    assert_eq!(clip.partial_frames, 0);
    assert_eq!(clip.legacy_frames, 0);
    let private = serde_json::to_value(&clip).unwrap();
    assert_eq!(private["root_path"], serde_json::Value::Null);
    assert_eq!(private["exclusions"], serde_json::Value::Null);
    assert_eq!(private["accounting"], "compatible");
    assert_eq!(
        clip.fingerprint,
        store
            .export_clip(&clip_request, &loading, |_| {})
            .unwrap()
            .unwrap()
            .fingerprint
    );
    // A concurrent committed metadata change must not split the clip's SQLite read view.
    let clip_connection = rusqlite::Connection::open(&db).unwrap();
    let recorded_identity: Vec<u8> = clip_connection
        .query_row(
            "SELECT identity FROM snapshot_roots WHERE id=?1",
            [clip.root_key],
            |row| row.get(0),
        )
        .unwrap();
    let mut changed_identity = recorded_identity.clone();
    changed_identity[15] ^= 1;
    clip_connection
        .execute(
            "UPDATE snapshot_roots SET identity=?1 WHERE id=?2",
            rusqlite::params![changed_identity, clip.root_key],
        )
        .unwrap();
    let changed_scope = store
        .export_clip(&clip_request, &loading, |_| {})
        .unwrap()
        .unwrap();
    assert_ne!(changed_scope.scope_fingerprint, clip.scope_fingerprint);
    assert_ne!(changed_scope.fingerprint, clip.fingerprint);
    clip_connection
        .execute(
            "UPDATE snapshot_roots SET identity=?1 WHERE id=?2",
            rusqlite::params![recorded_identity, clip.root_key],
        )
        .unwrap();
    let consistent = store
        .export_clip(&clip_request, &loading, |value| {
            if value.prepared_frames == 1 {
                clip_connection
                    .execute(
                        "UPDATE snapshots SET captured=captured+1 WHERE id=?1",
                        [after],
                    )
                    .unwrap();
            }
        })
        .unwrap()
        .unwrap();
    assert_eq!(consistent.fingerprint, clip.fingerprint);
    assert_ne!(
        store
            .export_clip(&clip_request, &loading, |_| {})
            .unwrap()
            .unwrap()
            .fingerprint,
        clip.fingerprint
    );
    clip_connection
        .execute(
            "UPDATE snapshots SET captured=?1 WHERE id=?2",
            rusqlite::params![i64::try_from(history[0].captured).unwrap(), after],
        )
        .unwrap();
    clip_request.include_paths = true;
    let disclosed = store
        .export_clip(&clip_request, &loading, |_| {})
        .unwrap()
        .unwrap();
    assert_eq!(
        disclosed.root_path.unwrap().path,
        first.root.to_str().unwrap()
    );
    assert_ne!(disclosed.fingerprint, clip.fingerprint);
    clip_request.include_paths = false;
    let stop_clip = AtomicBool::new(false);
    assert!(
        store
            .export_clip(&clip_request, &stop_clip, |value| {
                if value.prepared_frames == 1 {
                    stop_clip.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            })
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .export_clip(&clip_request, &AtomicBool::new(true), |_| {})
            .unwrap()
            .is_none()
    );
    clip_connection.execute("UPDATE snapshots SET complete=0,observed_at=NULL,observed_complete=NULL,engine_version=NULL,accounting_version=NULL WHERE id=?1", [before]).unwrap();
    let legacy = store
        .export_clip(&clip_request, &loading, |_| {})
        .unwrap()
        .unwrap();
    assert_eq!(legacy.partial_frames, 1);
    assert_eq!(legacy.legacy_frames, 1);
    assert_eq!(
        serde_json::to_value(&legacy).unwrap()["accounting"],
        "unknown"
    );
    let observation = first.observation.as_ref().unwrap();
    clip_connection.execute("UPDATE snapshots SET complete=1,observed_at=?1,observed_complete=?2,engine_version=?3,accounting_version=?4 WHERE id=?5",rusqlite::params![i64::try_from(observation.completed_at).unwrap(),observation.complete,observation.engine_version,observation.accounting_version,before]).unwrap();
    for ids in [
        vec![before],
        vec![before, before],
        vec![-1, after],
        vec![before; 61],
        vec![before, i64::MAX],
    ] {
        clip_request.snapshot_ids = ids;
        assert!(store.export_clip(&clip_request, &loading, |_| {}).is_err());
    }
    #[cfg(windows)]
    {
        use voldiq_core::exports::{GifStage, HistoryGifRequest, StreamOutcome, save_history_gif};
        let destination = tempfile::tempdir().unwrap();
        let export_state = tempfile::tempdir().unwrap();
        let lease = voldiq_core::operations::Lease::acquire(export_state.path()).unwrap();
        let make_png = |pixel: &[u8], rgba: bool| {
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
                encoder.set_color(if rgba {
                    png::ColorType::Rgba
                } else {
                    png::ColorType::Rgb
                });
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .add_itxt_chunk("Untrusted".into(), "source-client-metadata-marker".into())
                    .unwrap();
                let mut writer = encoder.write_header().unwrap();
                writer.write_image_data(&pixel.repeat(4)).unwrap();
                writer.finish().unwrap();
            }
            bytes
        };
        let first_png = make_png(&[33, 40, 50], false);
        let second_png = make_png(&[61, 40, 50, 255], true);
        let mut request = HistoryGifRequest {
            clip: voldiq_core::snapshots::ExportClipRequest {
                snapshot_ids: vec![before, after],
                include_paths: false,
            },
            fingerprint: clip.fingerprint.clone(),
            width: 2,
            height: 2,
            delay_centiseconds: 100,
            looped: true,
        };
        let path = destination.path().join("history.gif");
        let mut images = vec![first_png.clone(), second_png.clone()].into_iter();
        assert!(
            matches!(save_history_gif(&store, &request, &path, &lease, &loading, |_| {}, || Ok(images.next())).unwrap(), StreamOutcome::Complete(value) if value.stage == GifStage::Finalizing && value.completed_frames == 2)
        );
        let saved = std::fs::read(&path).unwrap();
        assert_eq!(&saved[..6], b"GIF89a");
        assert_eq!(saved.last(), Some(&0x3b));
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut reader = options.read_info(std::io::Cursor::new(&saved)).unwrap();
        assert_eq!((reader.width(), reader.height()), (2, 2));
        assert_eq!(reader.repeat(), gif::Repeat::Infinite);
        for color in [[33, 40, 50, 255], [61, 40, 50, 255]] {
            let frame = reader.read_next_frame().unwrap().unwrap();
            assert_eq!(frame.delay, 100);
            assert_eq!(frame.buffer.as_ref(), color.repeat(4));
        }
        assert!(reader.read_next_frame().unwrap().is_none());
        let mut images = vec![first_png.clone(), second_png.clone()].into_iter();
        assert!(
            save_history_gif(
                &store,
                &request,
                &path,
                &lease,
                &loading,
                |_| {},
                || Ok(images.next())
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        let rejected = destination.path().join("rejected.gif");
        let stop = AtomicBool::new(false);
        let mut images = vec![first_png.clone(), second_png.clone()].into_iter();
        assert!(matches!(
            save_history_gif(
                &store,
                &request,
                &rejected,
                &lease,
                &stop,
                |value| {
                    if value.stage == GifStage::Encoding && value.completed_frames == 1 {
                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                },
                || Ok(images.next())
            )
            .unwrap(),
            StreamOutcome::Cancelled(_)
        ));
        assert!(!rejected.exists());
        let mut empty = Vec::new();
        assert!(matches!(
            voldiq_core::exports::stream_history_gif(
                &store,
                &request,
                &mut empty,
                &AtomicBool::new(true),
                |_| {},
                || panic!("Cancelled export must not request an image")
            )
            .unwrap(),
            StreamOutcome::Cancelled(_)
        ));
        assert!(empty.is_empty());
        let mut images = vec![first_png.clone(), second_png.clone()].into_iter();
        let mut changed = false;
        assert!(
            save_history_gif(
                &store,
                &request,
                &rejected,
                &lease,
                &loading,
                |_| {},
                || {
                    if !changed {
                        clip_connection
                            .execute(
                                "UPDATE snapshots SET captured=captured+1 WHERE id=?1",
                                [after],
                            )
                            .unwrap();
                        changed = true;
                    }
                    Ok(images.next())
                }
            )
            .is_err()
        );
        assert!(!rejected.exists());
        clip_connection
            .execute(
                "UPDATE snapshots SET captured=?1 WHERE id=?2",
                rusqlite::params![i64::try_from(history[0].captured).unwrap(), after],
            )
            .unwrap();
        let mut malformed = vec![
            first_png.clone(),
            second_png[..second_png.len() / 2].to_vec(),
        ]
        .into_iter();
        assert!(
            save_history_gif(
                &store,
                &request,
                &rejected,
                &lease,
                &loading,
                |_| {},
                || Ok(malformed.next())
            )
            .is_err()
        );
        assert!(!rejected.exists());
        let mut transparent = vec![first_png.clone(), make_png(&[61, 40, 50, 0], true)].into_iter();
        assert!(
            save_history_gif(
                &store,
                &request,
                &rejected,
                &lease,
                &loading,
                |_| {},
                || Ok(transparent.next())
            )
            .is_err()
        );
        assert!(!rejected.exists());
        let mut missing = vec![first_png.clone()].into_iter();
        assert!(
            save_history_gif(
                &store,
                &request,
                &rejected,
                &lease,
                &loading,
                |_| {},
                || Ok(missing.next())
            )
            .is_err()
        );
        let mut extra = vec![first_png.clone(), second_png.clone(), first_png.clone()].into_iter();
        assert!(
            save_history_gif(
                &store,
                &request,
                &rejected,
                &lease,
                &loading,
                |_| {},
                || Ok(extra.next())
            )
            .is_err()
        );
        assert!(!rejected.exists());
        request.looped = false;
        request.delay_centiseconds = 50;
        let mut images = vec![first_png, second_png].into_iter();
        let once = destination.path().join("once.gif");
        assert!(matches!(
            save_history_gif(
                &store,
                &request,
                &once,
                &lease,
                &loading,
                |_| {},
                || Ok(images.next())
            )
            .unwrap(),
            StreamOutcome::Complete(_)
        ));
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 2);
        {
            use voldiq_core::exports::{HistoryWebmRequest, WebmProgress, save_history_webm};
            let videos = tempfile::tempdir().unwrap();
            let mut video_request = HistoryWebmRequest {
                clip: voldiq_core::snapshots::ExportClipRequest {
                    snapshot_ids: vec![before, after],
                    include_paths: false,
                },
                fingerprint: clip.fingerprint.clone(),
                width: 16,
                height: 16,
                delay_centiseconds: 50,
            };
            let vp8 = include_bytes!("fixtures/history-vp8.webm");
            for (name, input, codec) in [
                ("vp8.webm", vp8.as_slice(), "V_VP8"),
                (
                    "vp8-live.webm",
                    include_bytes!("fixtures/history-vp8-live.webm").as_slice(),
                    "V_VP8",
                ),
                (
                    "vp9-live.webm",
                    include_bytes!("fixtures/history-vp9-live.webm").as_slice(),
                    "V_VP9",
                ),
            ] {
                let output = videos.path().join(name);
                assert!(matches!(
                    save_history_webm(
                        &store,
                        &video_request,
                        input,
                        &output,
                        &lease,
                        &loading,
                        |_| {}
                    )
                    .unwrap(),
                    StreamOutcome::Complete(WebmProgress::Finalizing {
                        completed_frames: 2,
                        total_frames: 2
                    })
                ));
                let bytes = std::fs::read(&output).unwrap();
                assert_eq!(&bytes[..4], &[0x1a, 0x45, 0xdf, 0xa3]);
                let text = String::from_utf8_lossy(&bytes);
                assert!(text.contains("VOLDIQ_CONTEXT") && text.contains(codec));
                assert!(
                    !text.contains("input.invalid")
                        && !text.contains("source-client-metadata-marker")
                );
                let expected = format!("\"clip\":{}", serde_json::to_string(&clip).unwrap());
                assert!(text.contains(&expected));
                assert!(
                    save_history_webm(
                        &store,
                        &video_request,
                        input,
                        &output,
                        &lease,
                        &loading,
                        |_| {}
                    )
                    .is_err()
                );
                assert_eq!(std::fs::read(&output).unwrap(), bytes);
            }
            // An unknown-size cluster ends at the next segment-level element, not inside its blocks.
            let mut streaming = vp8.to_vec();
            let cluster = streaming
                .windows(4)
                .position(|v| v == [0x1f, 0x43, 0xb6, 0x75])
                .unwrap()
                + 4;
            let size_width = streaming[cluster].leading_zeros() as usize + 1;
            streaming[cluster] = 0xff >> (size_width - 1);
            streaming[cluster + 1..cluster + size_width].fill(0xff);
            assert!(matches!(
                save_history_webm(
                    &store,
                    &video_request,
                    &streaming,
                    &videos.path().join("unknown-cluster.webm"),
                    &lease,
                    &loading,
                    |_| {}
                )
                .unwrap(),
                StreamOutcome::Complete(_)
            ));
            let rejected = videos.path().join("rejected.webm");
            let stop = AtomicBool::new(false);
            assert!(matches!(
                save_history_webm(&store, &video_request, vp8, &rejected, &lease, &stop, |p| {
                    if matches!(p, WebmProgress::Validating { .. }) {
                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                })
                .unwrap(),
                StreamOutcome::Cancelled(_)
            ));
            assert!(!rejected.exists());
            let mut audio = vp8.to_vec();
            let track_type = audio.windows(3).position(|v| v == [0x83, 0x81, 1]).unwrap();
            audio[track_type + 2] = 2;
            for input in [
                &vp8[..vp8.len() / 2],
                audio.as_slice(),
                b"not webm".as_slice(),
            ] {
                assert!(
                    save_history_webm(
                        &store,
                        &video_request,
                        input,
                        &rejected,
                        &lease,
                        &loading,
                        |_| {}
                    )
                    .is_err()
                );
                assert!(!rejected.exists());
            }
            video_request.width = 17;
            assert!(
                save_history_webm(
                    &store,
                    &video_request,
                    vp8,
                    &rejected,
                    &lease,
                    &loading,
                    |_| {}
                )
                .is_err()
            );
            video_request.width = 16;
            let mut changed = false;
            assert!(save_history_webm(&store, &video_request, vp8, &rejected, &lease, &loading, |p| {
                if matches!(p, WebmProgress::Validating { inspected_bytes, total_bytes } if inspected_bytes == total_bytes) && !changed {
                    clip_connection.execute("UPDATE snapshots SET captured=captured+1 WHERE id=?1", [after]).unwrap();
                    changed = true;
                }
            }).is_err());
            assert!(!rejected.exists());
            clip_connection
                .execute(
                    "UPDATE snapshots SET captured=?1 WHERE id=?2",
                    rusqlite::params![i64::try_from(history[0].captured).unwrap(), after],
                )
                .unwrap();
            assert_eq!(std::fs::read_dir(videos.path()).unwrap().count(), 4);
            if std::env::var("VOLDIQ_KEEP_WEBM_FIXTURE").as_deref() == Ok("1") {
                println!("WebM fixture: {}", videos.keep().display());
            }
        }
        if std::env::var("VOLDIQ_KEEP_GIF_FIXTURE").as_deref() == Ok("1") {
            if let Ok(node) = std::env::var("VOLDIQ_CANVAS_NODE") {
                let modules = std::env::var("VOLDIQ_CANVAS_MODULES").unwrap();
                assert!(Path::new(&node).is_absolute() && Path::new(&modules).is_absolute());
                assert_eq!(
                    Path::new(&node)
                        .file_stem()
                        .unwrap()
                        .to_string_lossy()
                        .to_lowercase(),
                    "node"
                );
                let recorded = store
                    .export_clip(&request.clip, &loading, |_| {})
                    .unwrap()
                    .unwrap();
                assert_eq!(recorded.fingerprint, request.fingerprint);
                std::fs::write(
                    destination.path().join("clip.json"),
                    serde_json::to_vec(&recorded).unwrap(),
                )
                .unwrap();
                let script = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../scripts/render_history_fixture.mjs");
                let rendered = std::process::Command::new(node)
                    .arg("--experimental-strip-types")
                    .arg(script)
                    .arg(modules)
                    .arg(destination.path())
                    .output()
                    .unwrap();
                assert!(
                    rendered.status.success(),
                    "{}",
                    String::from_utf8_lossy(&rendered.stderr)
                );
                request.width = 1000;
                request.height = 560;
                let mut images = ["history-0.png", "history-1.png"]
                    .into_iter()
                    .map(|name| std::fs::read(destination.path().join(name)).unwrap());
                let painted = destination.path().join("painted-history.gif");
                assert!(matches!(
                    save_history_gif(
                        &store,
                        &request,
                        &painted,
                        &lease,
                        &loading,
                        |_| {},
                        || Ok(images.next())
                    )
                    .unwrap(),
                    StreamOutcome::Complete(_)
                ));
                println!("Painted history fixture: {}", painted.display());
            }
            println!("GIF fixture: {}", destination.keep().display());
        }
    }
    assert!(
        store
            .visual_capture(-1, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        loaded
            .query(&Query {
                files_only: true,
                ..Query::default()
            })
            .unwrap()
            .total,
        4
    );
    for (text, expected) in [
        ("GROWTH", vec!["growth"]),
        ("upper", vec!["UPPER.txt"]),
        ("RÉSUMÉ", vec!["Résumé.txt"]),
        ("absent", vec![]),
    ] {
        let query = Query {
            text: text.into(),
            files_only: true,
            ..Query::default()
        };
        for index in [&first, &loaded] {
            let page = index.query(&query).unwrap();
            let cancellable = index.query_cancellable(&query, || false).unwrap().unwrap();
            assert_eq!(page.total, expected.len());
            assert_eq!(cancellable.total, expected.len());
            for result in [page, cancellable] {
                assert_eq!(
                    result
                        .items
                        .iter()
                        .map(|entry| entry.name.as_str())
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
    }
    let comparison = store.compare(before, after, 0, 200).unwrap();
    assert!(comparison.reliable);
    assert!(
        store
            .compare_cancellable(-1, -2, 0, 10, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    assert_eq!(comparison.delta, 28);
    let diary = store.diary(after, 0, 10, 5, &loading).unwrap().unwrap();
    assert_eq!(diary.events.len(), 2);
    let recorded = serde_json::to_value(&diary).unwrap();
    assert_eq!(recorded["events"][0]["detail"]["kind"], "compared");
    assert_eq!(recorded["events"][0]["detail"]["delta"], "28");
    assert_eq!(recorded["events"][1]["detail"]["kind"], "baseline");
    let scope = recorded["events"][0]["detail"]["comparison"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"].as_str().unwrap().ends_with("growth"))
        .unwrap()["path_id"]
        .as_str()
        .unwrap();
    let context = store
        .visual_pair(before, after, Some(scope), None, false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(context.before.logical.as_deref(), Some("12"));
    assert_eq!(context.after.logical.as_deref(), Some("33"));
    assert!(
        store
            .diary(after, 0, 1, 5, &loading)
            .unwrap()
            .unwrap()
            .has_more
    );
    assert_eq!(comparison.file_changes.added.logical, 14);
    assert_eq!(comparison.file_changes.added.files, 1);
    assert_eq!(comparison.file_changes.changed.delta, 21);
    assert_eq!(comparison.file_changes.changed.files, 1);
    assert_eq!(comparison.file_changes.removed.logical, 7);
    assert_eq!(comparison.file_changes.removed.files, 1);
    assert!(
        comparison
            .changes
            .iter()
            .any(|c| c.path.ends_with("removed") && c.kind == "removed" && c.delta == -7)
    );
    assert!(
        comparison
            .changes
            .iter()
            .any(|c| c.path.ends_with("added") && c.kind == "added" && c.delta == 14)
    );
    assert!(
        comparison
            .changes
            .iter()
            .any(|c| c.path.ends_with("nested") && c.directory && c.delta == 21)
    );
    let page = store.compare(before, after, 1, 1).unwrap();
    assert_eq!(page.total, comparison.total);
    assert_eq!(page.changes.len(), 1);
    assert_eq!(page.file_changes, comparison.file_changes);
    assert_eq!(store.load(after, &loading).unwrap().entries[0].logical, 61);
    drop(store);
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch("DROP TABLE snapshot_application_roots; DROP TABLE snapshot_applications; DROP TABLE snapshot_retention; DROP TRIGGER history_snapshot_insert; DROP TRIGGER history_snapshot_update; DROP TRIGGER history_snapshot_delete; DROP TABLE snapshot_history_revision; DROP INDEX snapshots_observed; ALTER TABLE snapshots DROP COLUMN observed_at; ALTER TABLE snapshots DROP COLUMN observed_complete; ALTER TABLE snapshots DROP COLUMN engine_version; ALTER TABLE snapshots DROP COLUMN accounting_version; ALTER TABLE snapshot_versions DROP COLUMN cloud; DROP TABLE duplicate_hashes; PRAGMA user_version=4;")
        .unwrap();
    drop(connection);
    let mut store = Store::open(&db).unwrap();
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
    assert_eq!(store.load(after, &loading).unwrap().entries[0].logical, 61);
    assert_eq!(store.load(after, &loading).unwrap().observation, None);
    let duplicates = tempfile::tempdir().unwrap();
    for name in ["first", "second"] {
        std::fs::write(duplicates.path().join(name), [7_u8; 16_384]).unwrap();
    }
    let cancel = AtomicBool::new(false);
    let policy_state = tempfile::tempdir().unwrap();
    let protections = voldiq_core::metadata::Store::open(policy_state.path())
        .unwrap()
        .protections()
        .unwrap();
    let index = std::sync::Arc::new(std::sync::RwLock::new(capture(duplicates.path())));
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let cold = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(cold.errors, 0);
    assert_eq!(cold.groups.len(), 1);
    assert_eq!(cold.groups[0].members.len(), 2);
    assert_eq!(
        cold.groups[0].digest,
        blake3::hash(&[7_u8; 16_384]).to_hex().as_str()
    );
    assert_eq!(cold.bytes_read, 49_152);
    assert_eq!(cold.cache_hits, 0);
    store.save_hash_cache(&cache, &cancel).unwrap();
    drop(cache);
    drop(store);
    let mut store = Store::open(&db).unwrap();
    store
        .save_hash_cache(
            &voldiq_core::duplicates::HashCache::default(),
            &AtomicBool::new(true),
        )
        .unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let warm = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(warm.errors, 0);
    assert_eq!(warm.groups.len(), 1);
    assert_eq!(warm.groups[0].digest, cold.groups[0].digest);
    assert_eq!(warm.groups[0].members.len(), 2);
    assert_eq!(warm.bytes_read, 0);
    assert_eq!(warm.cache_hits, 2);
    drop(cache);
    drop(store);
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE duplicate_hashes SET sample=x'00' WHERE identity=(SELECT min(identity) FROM duplicate_hashes);").unwrap();
    drop(connection);
    let mut store = Store::open(&db).unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let repaired = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(repaired.errors, 0);
    assert_eq!(repaired.groups.len(), 1);
    assert_eq!(repaired.groups[0].digest, cold.groups[0].digest);
    assert_eq!(repaired.bytes_read, 24_576);
    assert_eq!(repaired.cache_hits, 1);
    store.save_hash_cache(&cache, &cancel).unwrap();
    let first_path = duplicates.path().join("first");
    let modified = std::fs::metadata(&first_path).unwrap().modified().unwrap();
    std::fs::write(&first_path, [8_u8; 16_384]).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&first_path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    drop(cache);
    drop(store);
    let store = Store::open(&db).unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let changed = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(changed.errors, 0);
    assert!(changed.groups.is_empty());
    assert_eq!(changed.bytes_read, 8192);
    drop(cache);
    drop(store);
    let mut store = Store::open(&db).unwrap();
    store
        .save_hash_cache(&voldiq_core::duplicates::HashCache::default(), &cancel)
        .unwrap();
    let mut cache = store.load_hash_cache(&cancel).unwrap();
    let cleared = voldiq_core::duplicates::find(
        &index,
        voldiq_core::NodeId(0),
        0,
        &cancel,
        &mut cache,
        |_| {},
        &protections,
    )
    .unwrap();
    assert_eq!(cleared.errors, 0);
    assert!(cleared.groups.is_empty());
    assert_eq!(cleared.bytes_read, 16_384);
    assert_eq!(store.list(0, 10).unwrap().len(), 2);
    budget_history_uses_measurements_and_real_scoped_image_samples();

    // Continue the historical comparison flow with a folder larger than either map/page budget.
    for n in 0..110 {
        std::fs::create_dir(fixture.path().join(format!("nested/dir{n:03}"))).unwrap();
    }
    for n in 0..170 {
        std::fs::write(fixture.path().join(format!("nested/file{n:03}")), [1]).unwrap();
    }
    let third = store.save(&capture(fixture.path()), &loading, &[]).unwrap();
    let root_pair = store
        .visual_pair(before, third, None, None, false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(root_pair.before.logical.as_deref(), Some("33"));
    assert_eq!(root_pair.after.logical.as_deref(), Some("231"));
    assert_eq!(root_pair.delta.as_deref(), Some("198"));
    assert_eq!(
        root_pair.accounting,
        voldiq_core::snapshots::Accounting::Unknown
    );
    let nested = &root_pair.folders.items[0].path_id;
    let scoped = store
        .visual_pair(before, third, Some(nested), None, false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(scoped.before.logical.as_deref(), Some("12"));
    assert_eq!(scoped.after.logical.as_deref(), Some("203"));
    assert_eq!(scoped.after.child_count, 281);
    assert_eq!(scoped.after.entries.len(), 160);
    assert_eq!(scoped.after.remainder.as_ref().unwrap().logical, "11");
    assert_eq!(scoped.after.remainder.as_ref().unwrap().entries, 121);
    assert_eq!(scoped.folders.items.len(), 100);
    assert!(scoped.folders.previous.is_none());
    let cursor = scoped.folders.next.as_deref().unwrap();
    let next = store
        .visual_pair(before, third, Some(nested), Some(cursor), false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(next.folders.items.len(), 10);
    assert!(next.folders.next.is_none());
    let previous = store
        .visual_pair(
            before,
            third,
            Some(nested),
            next.folders.previous.as_deref(),
            true,
            &loading,
        )
        .unwrap()
        .unwrap();
    assert_eq!(previous.folders.items[0].name, "dir000");
    assert_eq!(previous.folders.items.last().unwrap().name, "dir099");
    let empty = &scoped.folders.items[0].path_id;
    let missing = store
        .visual_pair(before, third, Some(empty), None, false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(missing.before.logical, None);
    assert_eq!(
        serde_json::to_value(&missing).unwrap()["before"]["presence"],
        "not_recorded"
    );
    assert_eq!(missing.after.logical.as_deref(), Some("0"));
    assert!(
        store
            .visual_pair(before, third, Some("01"), None, false, &loading)
            .is_err()
    );
    assert!(
        store
            .visual_pair(before, before, None, None, false, &loading)
            .is_err()
    );
    assert!(
        store
            .visual_pair(-1, -2, None, None, false, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute("UPDATE snapshots SET complete=0 WHERE id=?1", [before])
        .unwrap();
    let partial = store
        .visual_pair(before, third, Some(empty), None, false, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&partial).unwrap()["before"]["presence"],
        "not_observed"
    );
    assert!(
        !store
            .compare(before, third, 0, 10)
            .unwrap()
            .coverage_complete
    );
    connection
        .execute(
            "UPDATE snapshots SET accounting_version=99 WHERE id=?1",
            [third],
        )
        .unwrap();
    assert!(
        store
            .visual_pair(before, third, None, None, false, &loading)
            .is_err()
    );
    assert!(store.compare(before, third, 0, 10).is_err());
    assert!(
        store
            .export_clip(
                &voldiq_core::snapshots::ExportClipRequest {
                    snapshot_ids: vec![before, third],
                    include_paths: false
                },
                &loading,
                |_| {}
            )
            .is_err()
    );
    let incompatible = store.diary(third, 0, 10, 5, &loading).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&incompatible).unwrap()["events"][0]["detail"]["kind"],
        "incompatible"
    );
    let foreign = store
        .save(&capture(duplicates.path()), &loading, &[])
        .unwrap();
    assert!(
        store
            .export_clip(
                &voldiq_core::snapshots::ExportClipRequest {
                    snapshot_ids: vec![before, foreign],
                    include_paths: false
                },
                &loading,
                |_| {}
            )
            .is_err()
    );
    assert!(
        store
            .visual_pair(before, foreign, None, None, false, &loading)
            .is_err()
    );
    connection
        .execute(
            "UPDATE snapshots SET accounting_version=?1 WHERE id=?2",
            rusqlite::params![voldiq_core::index::ACCOUNTING_VERSION, third],
        )
        .unwrap();
    let current = capture(fixture.path());
    for _ in 0..104 {
        store.save(&current, &loading, &[]).unwrap();
    }
    assert!(store.set_retention(third, "0", Some(0)).is_err());
    assert!(store.set_retention(third, "0", Some(10001)).is_err());
    let preference = store.set_retention(third, "0", Some(1)).unwrap();
    connection
        .execute(
            "UPDATE snapshots SET observed_complete=0 WHERE id=?1",
            [after],
        )
        .unwrap();
    let review = store.preview_retention(third, &loading).unwrap().unwrap();
    assert_eq!(review.eligible, 106);
    assert_eq!(review.captures.len(), 100);
    assert!(
        !review
            .captures
            .iter()
            .find(|capture| capture.id == after)
            .unwrap()
            .complete
    );
    let mut concurrent = Store::open(&db).unwrap();
    assert_eq!(
        concurrent.retention_settings(third).unwrap().version,
        preference.version
    );
    let newest = concurrent.save(&current, &loading, &[]).unwrap();
    assert!(
        store
            .apply_retention(third, &review.fingerprint, &loading)
            .is_err()
    );
    let review = store.preview_retention(newest, &loading).unwrap().unwrap();
    assert_eq!(review.eligible, 107);
    assert!(
        store
            .apply_retention(newest, &review.fingerprint, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    connection.execute_batch(&format!("CREATE TRIGGER fail_retention_fixture BEFORE DELETE ON snapshots WHEN OLD.id={after} BEGIN SELECT RAISE(ABORT,'owned fixture failure'); END;")).unwrap();
    assert!(
        store
            .apply_retention(newest, &review.fingerprint, &loading)
            .is_err()
    );
    assert_eq!(store.load(before, &loading).unwrap().entries[0].logical, 33);
    connection
        .execute_batch("DROP TRIGGER fail_retention_fixture;")
        .unwrap();
    let result = store
        .apply_retention(newest, &review.fingerprint, &loading)
        .unwrap()
        .unwrap();
    assert_eq!((result.removed, result.remaining_eligible), (100, 7));
    assert!(
        store
            .apply_retention(newest, &review.fingerprint, &loading)
            .is_err()
    );
    let review = store.preview_retention(newest, &loading).unwrap().unwrap();
    assert_eq!(review.captures.len(), 7);
    assert_eq!(
        store
            .apply_retention(newest, &review.fingerprint, &loading)
            .unwrap()
            .unwrap()
            .remaining_eligible,
        0
    );
    drop(store);
    let store = Store::open(&db).unwrap();
    assert_eq!(store.retention_settings(newest).unwrap().total, 1);
    assert_eq!(
        store.load(newest, &loading).unwrap().entries[0].logical,
        231
    );
    assert_eq!(
        store.load(foreign, &loading).unwrap().entries[0].logical,
        32768
    );
    assert_eq!(
        std::fs::metadata(fixture.path().join("nested/growth"))
            .unwrap()
            .len(),
        33
    );
    let root = store.retention_settings(newest).unwrap().root_key;
    connection.execute("UPDATE snapshot_versions SET modified=1709164800 WHERE path_id IN(SELECT id FROM snapshot_paths WHERE root_id=?1)", [root]).unwrap();
    let candidates = store
        .calendar_entries(newest, "2024-02", 29, 0, 4, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(candidates.total, 174);
    for entry in &candidates.entries {
        let id = entry.path_id.parse::<i64>().unwrap();
        if entry.path.ends_with("UPPER.txt") {
            connection.execute("UPDATE snapshot_versions SET kind=2,cloud=2,modified=NULL,incomplete=1 WHERE path_id=?1", [id]).unwrap();
        }
        if entry.path.ends_with("Résumé.txt") {
            connection
                .execute(
                    "UPDATE snapshot_versions SET modified=1709251200 WHERE path_id=?1",
                    [id],
                )
                .unwrap();
        }
        if entry.path.ends_with("added") {
            connection
                .execute(
                    "UPDATE snapshot_versions SET modified=1706745600 WHERE path_id=?1",
                    [id],
                )
                .unwrap();
        }
    }
    connection
        .execute(
            "UPDATE snapshots SET complete=0,observed_complete=0 WHERE id=?1",
            [newest],
        )
        .unwrap();
    let calendar = store
        .calendar(newest, "2024-02", &loading)
        .unwrap()
        .unwrap();
    assert_eq!(calendar.days.len(), 29);
    assert!(!calendar.capture.complete);
    assert_eq!(
        (calendar.total.files, calendar.total.logical.as_str()),
        (174, "231")
    );
    assert_eq!(
        (
            calendar.days[0].totals.files,
            calendar.days[0].totals.logical.as_str()
        ),
        (1, "14")
    );
    assert_eq!(
        (
            calendar.days[28].totals.files,
            calendar.days[28].totals.logical.as_str()
        ),
        (171, "203")
    );
    assert_eq!(
        (calendar.undated.files, calendar.undated.logical.as_str()),
        (1, "7")
    );
    assert_eq!(
        (calendar.outside.files, calendar.outside.logical.as_str()),
        (1, "7")
    );
    let day = store
        .calendar_entries(newest, "2024-02", 29, 0, 1, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(day.entries[0].logical, "33");
    assert!(day.entries[0].path.ends_with("growth"));
    assert!(day.has_more);
    assert!(
        store
            .calendar_entries(newest, "2024-02", 30, 0, 1, &loading)
            .is_err()
    );
    assert!(store.calendar(newest, "2024-13", &loading).is_err());
    let epoch = candidates
        .entries
        .iter()
        .find(|entry| entry.path.ends_with("Résumé.txt"))
        .unwrap()
        .path_id
        .parse::<i64>()
        .unwrap();
    connection
        .execute(
            "UPDATE snapshot_versions SET modified=0 WHERE path_id=?1",
            [epoch],
        )
        .unwrap();
    let epoch = store
        .calendar(newest, "1970-01", &loading)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            epoch.days[0].totals.files,
            epoch.days[0].totals.logical.as_str()
        ),
        (1, "7")
    );
    assert_eq!(epoch.undated.files, 1);
    assert!(
        store
            .calendar(-1, "invalid", &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .diary(foreign, 0, 10, 5, &loading)
            .unwrap()
            .unwrap()
            .events
            .len(),
        1
    );
    let account_root = fixture.path().join("space-account");
    let account_excluded = account_root.join("excluded");
    std::fs::create_dir_all(&account_excluded).unwrap();
    let mut account_data = vec![0_u8; 8192];
    getrandom::fill(&mut account_data).unwrap();
    std::fs::write(account_root.join("data.bin"), &account_data).unwrap();
    std::fs::hard_link(
        account_root.join("data.bin"),
        account_root.join("alias.bin"),
    )
    .unwrap();
    std::fs::write(account_excluded.join("outside.bin"), [1; 1024]).unwrap();
    let mut account_index = scan(
        &account_root,
        &ScanOptions {
            mode: voldiq_core::ScanMode::Directory,
            exclusions: vec![account_excluded.to_str().unwrap().into()],
            ..Default::default()
        },
        &loading,
        |_| {},
    )
    .unwrap();
    let indexed_space = voldiq_core::space::summarize(&account_index, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(indexed_space.logical_paths.0, 16_384);
    assert_eq!(
        (
            indexed_space.file_paths,
            indexed_space.hardlink_paths,
            indexed_space.exclusions
        ),
        (2, 2, 1)
    );
    let voldiq_core::space::Allocation::Measured {
        known: known_allocation,
        complete: true,
        ..
    } = indexed_space.allocation
    else {
        panic!("Expected complete allocation for the owned hardlink fixture");
    };
    assert_eq!(
        known_allocation.0,
        voldiq_core::windows::identity(&account_root.join("data.bin"))
            .unwrap()
            .allocated
    );
    assert!(known_allocation.0 > 100);
    {
        use voldiq_core::cleanup::tray::{self, Group, Metric, Source, Target, TargetProgress};
        let alias = account_index
            .entries
            .iter()
            .find(|entry| entry.name == "alias.bin")
            .unwrap()
            .id;
        let data = account_index
            .entries
            .iter()
            .find(|entry| entry.name == "data.bin")
            .unwrap()
            .id;
        let groups = [Group {
            source: Source::Manual,
            selection: vec![alias, data],
        }];
        let target = Target {
            metric: Metric::Allocated,
            expression: format!("{}B", known_allocation.0),
        };
        let review = tray::summarize(&account_index, &groups, Some(&target), &loading).unwrap();
        assert_eq!(review.selected_logical, "16384");
        assert_eq!(review.known_allocated, known_allocation.0.to_string());
        assert_eq!(review.repeated_identity_paths, 1);
        assert_eq!(review.remaining_logical.as_deref(), Some("0"));
        let uncharged = [alias, data]
            .into_iter()
            .find(|id| account_index.entry(*id).unwrap().allocated == 0)
            .unwrap();
        let alias_only = tray::summarize(
            &account_index,
            &[Group {
                source: Source::Manual,
                selection: vec![uncharged],
            }],
            Some(&target),
            &loading,
        )
        .unwrap();
        assert_eq!(alias_only.selected_logical, "8192");
        assert_eq!(alias_only.known_allocated, known_allocation.0.to_string());
        assert_eq!(alias_only.remaining_logical.as_deref(), Some("8192"));
        assert!(matches!(
            review.target,
            Some(TargetProgress::Measured {
                complete: true,
                reached: true,
                ..
            })
        ));
        account_index.entries[alias.0 as usize].allocation_known = false;
        let partial = tray::summarize(&account_index, &groups, Some(&target), &loading).unwrap();
        account_index.entries[alias.0 as usize].allocation_known = true;
        assert!(!partial.allocation_complete);
        assert!(
            partial.groups[0]
                .items
                .iter()
                .any(|item| item.id == alias && item.allocated.is_none())
        );
        assert!(matches!(
            partial.target,
            Some(TargetProgress::Measured {
                complete: false,
                reached: false,
                ..
            })
        ));
        assert!(tray::summarize(&account_index, &groups, None, &AtomicBool::new(true)).is_err());
    }
    let account_volume = voldiq_core::space::VolumeIdentity {
        guid_root: "fixture-volume".into(),
        serial: indexed_space.volume_serial.clone().unwrap(),
    };
    let account_units = voldiq_core::space::AllocationUnits {
        total: known_allocation.0 + 1000,
        used: known_allocation.0 + 100,
        free: 200,
        caller_total: 500,
        caller_available: 50,
        reserved: 10,
        storage_reserve: 5,
        pool_unavailable: 0,
    };
    let positive_measurement = voldiq_core::space::Measurement::from_units(
        account_volume.clone(),
        Some(123),
        1,
        account_units,
    )
    .unwrap();
    let positive_report = voldiq_core::space::Report::compare(
        indexed_space.clone(),
        voldiq_core::space::VolumeContext::Observed {
            observation: voldiq_core::space::Observation::Measured {
                measurement: positive_measurement.clone(),
            },
        },
    );
    assert!(
        matches!(positive_report.residual, voldiq_core::space::Residual::Compared { ref bytes, against_complete_scope: true } if bytes == "100")
    );
    let negative_measurement = voldiq_core::space::Measurement::from_units(
        account_volume,
        Some(123),
        1,
        voldiq_core::space::AllocationUnits {
            used: known_allocation.0 - 100,
            ..account_units
        },
    )
    .unwrap();
    let negative_report = voldiq_core::space::Report::compare(
        indexed_space.clone(),
        voldiq_core::space::VolumeContext::Observed {
            observation: voldiq_core::space::Observation::Measured {
                measurement: negative_measurement,
            },
        },
    );
    assert!(
        matches!(negative_report.residual, voldiq_core::space::Residual::Compared { ref bytes, .. } if bytes == "-100")
    );
    let mut foreign_measurement = positive_measurement.clone();
    foreign_measurement.volume.serial = "foreign".into();
    assert!(matches!(
        voldiq_core::space::Report::compare(
            indexed_space.clone(),
            voldiq_core::space::VolumeContext::Observed {
                observation: voldiq_core::space::Observation::Measured {
                    measurement: foreign_measurement
                },
            }
        )
        .residual,
        voldiq_core::space::Residual::Unavailable {
            reason: voldiq_core::space::ResidualReason::VolumeMismatch
        }
    ));
    assert!(matches!(
        voldiq_core::space::Report::compare(
            indexed_space,
            voldiq_core::space::VolumeContext::ScopedOnly
        )
        .residual,
        voldiq_core::space::Residual::Unavailable {
            reason: voldiq_core::space::ResidualReason::ScopedOnly
        }
    ));
    assert!(
        voldiq_core::space::summarize(&account_index, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    #[cfg(windows)]
    {
        let desktop_space = voldiq_core::space::read_desktop(&account_index, &loading)
            .unwrap()
            .unwrap();
        match desktop_space.volume {
            voldiq_core::space::VolumeContext::Observed {
                observation: voldiq_core::space::Observation::Measured { .. },
            } => assert!(matches!(
                desktop_space.residual,
                voldiq_core::space::Residual::Compared { .. }
            )),
            voldiq_core::space::VolumeContext::Observed {
                observation:
                    voldiq_core::space::Observation::Unavailable {
                        reason: voldiq_core::space::Unavailable::Unsupported,
                        ..
                    },
            } => assert!(matches!(
                desktop_space.residual,
                voldiq_core::space::Residual::Unavailable {
                    reason: voldiq_core::space::ResidualReason::VolumeUnavailable
                }
            )),
            other => panic!("Unexpected volume context for the owned local fixture: {other:?}"),
        }
    }
    let alias_record = account_index
        .entries
        .iter_mut()
        .find(|entry| entry.kind == voldiq_core::Kind::File && entry.allocated == 0)
        .unwrap();
    alias_record.kind = voldiq_core::Kind::Link;
    alias_record.cloud = voldiq_core::cloud::CloudState::FileRecallRequired;
    alias_record.allocation_known = false;
    account_index.entries[0].incomplete = true;
    account_index.entries[0].allocation_known = false;
    account_index.observation.as_mut().unwrap().complete = false;
    let partial_space = voldiq_core::space::summarize(&account_index, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            partial_space.cloud_file_paths,
            partial_space.cloud_logical_paths.0,
            partial_space.recall_required_file_paths
        ),
        (1, 8192, 1)
    );
    assert!(matches!(
        partial_space.allocation,
        voldiq_core::space::Allocation::Measured {
            complete: false,
            unknown_file_records: 1,
            ..
        }
    ));
    assert!(
        matches!(voldiq_core::space::Report::compare(partial_space, voldiq_core::space::VolumeContext::Observed {
        observation: voldiq_core::space::Observation::Measured { measurement: positive_measurement },
    }).residual, voldiq_core::space::Residual::Compared { ref bytes, against_complete_scope: false } if bytes == "100")
    );
    for entry in account_index
        .entries
        .iter_mut()
        .filter(|entry| entry.is_metadata_file())
    {
        entry.allocation_known = false;
        entry.file_state = None;
    }
    assert!(matches!(
        voldiq_core::space::summarize(&account_index, &loading)
            .unwrap()
            .unwrap()
            .allocation,
        voldiq_core::space::Allocation::Unavailable {
            reason: voldiq_core::space::AllocationReason::NoEvidence
        }
    ));
    let mut recorded_account_observation = account_index.observation.take().unwrap();
    let legacy_space = voldiq_core::space::summarize(&account_index, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(legacy_space.coverage, voldiq_core::space::Coverage::Unknown);
    assert!(matches!(
        legacy_space.allocation,
        voldiq_core::space::Allocation::Unavailable {
            reason: voldiq_core::space::AllocationReason::AccountingUnknown
        }
    ));
    recorded_account_observation.accounting_version += 1;
    account_index.observation = Some(recorded_account_observation);
    assert!(matches!(
        voldiq_core::space::summarize(&account_index, &loading)
            .unwrap()
            .unwrap()
            .allocation,
        voldiq_core::space::Allocation::Unavailable {
            reason: voldiq_core::space::AllocationReason::AccountingUnsupported
        }
    ));

    let mut export_index = capture(fixture.path());
    export_index
        .entries
        .iter_mut()
        .find(|entry| entry.kind == voldiq_core::Kind::File)
        .unwrap()
        .name = "=1+1\",\n@x".into();
    let request = voldiq_core::exports::CsvRequest {
        items: voldiq_core::exports::ReportItems::Files,
        query: Query {
            text: "=1+1".into(),
            ..Default::default()
        },
        include_paths: false,
    };
    let exported = voldiq_core::exports::csv(&export_index, &request, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(exported.rows, 1);
    assert!(exported.csv.contains("\"text: =1+1\"\",\n@x\""));
    assert!(exported.scope_root.is_none());
    let path_request = voldiq_core::exports::CsvRequest {
        include_paths: true,
        query: request.query.clone(),
        items: request.items,
    };
    let with_paths = voldiq_core::exports::csv(&export_index, &path_request, &loading)
        .unwrap()
        .unwrap();
    assert_eq!(with_paths.scope_root.as_deref(), export_index.root.to_str());
    assert!(with_paths.paths_included);
    assert!(
        voldiq_core::exports::csv(&export_index, &request, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    let streaming = voldiq_core::exports::ReportSelection {
        items: voldiq_core::exports::ReportItems::Files,
        filter: voldiq_core::exports::ReportFilter {
            text: "=1+1".into(),
            ..Default::default()
        },
        include_paths: false,
    };
    let mut encoded = Vec::new();
    let result =
        voldiq_core::exports::stream_csv(&export_index, &streaming, &mut encoded, &loading, |_| {})
            .unwrap();
    assert!(
        matches!(result, voldiq_core::exports::StreamOutcome::Complete(value) if value.rows == 1)
    );
    let encoded = String::from_utf8(encoded).unwrap();
    assert!(encoded.starts_with("record_type,node_id,kind,name,"));
    assert!(encoded.contains("\"text: =1+1\"\",\n@x\""));
    assert!(encoded.contains("\"\"root_path\"\":null"));
    assert!(encoded.contains("\r\ncomplete,"));
    assert!(exported.query["text"].is_null());
    assert!(!exported.filter_text_included);
    assert_eq!(with_paths.query["text"], "=1+1");
    let private_filter = voldiq_core::exports::ReportSelection {
        items: streaming.items,
        filter: voldiq_core::exports::ReportFilter {
            text: "https://private.invalid/secret".into(),
            ..Default::default()
        },
        include_paths: false,
    };
    let mut private_report = Vec::new();
    voldiq_core::exports::stream_csv(
        &export_index,
        &private_filter,
        &mut private_report,
        &loading,
        |_| {},
    )
    .unwrap();
    assert!(
        !String::from_utf8(private_report)
            .unwrap()
            .contains("private.invalid")
    );
    #[cfg(windows)]
    {
        let destination = tempfile::tempdir().unwrap();
        let export_state = tempfile::tempdir().unwrap();
        let mut policy = voldiq_core::metadata::Store::open(export_state.path()).unwrap();
        let protected = destination.path().join("protected");
        std::fs::create_dir(&protected).unwrap();
        policy.protect_folder(&protected).unwrap();
        let lease = voldiq_core::operations::Lease::acquire(export_state.path()).unwrap();
        let report = destination.path().join("report.csv");
        assert!(matches!(
            voldiq_core::exports::save_report(
                &export_index, voldiq_core::exports::ReportFormat::Csv, &streaming, &report, &lease, &loading, |_| {}
            ).unwrap(),
            voldiq_core::exports::StreamOutcome::Complete(value) if value.rows == 1
        ));
        let saved = std::fs::read_to_string(&report).unwrap();
        assert!(saved.contains("\"text: =1+1\"\",\n@x\""));
        assert!(saved.contains("\r\ncomplete,"));
        let workbook = destination.path().join("report.xlsx");
        assert!(matches!(voldiq_core::exports::save_report(
            &export_index, voldiq_core::exports::ReportFormat::Xlsx, &streaming, &workbook, &lease, &loading, |_| {}
        ).unwrap(), voldiq_core::exports::StreamOutcome::Complete(value) if value.rows == 1));
        let bytes = std::fs::read(&workbook).unwrap();
        assert!(bytes.starts_with(b"PK"));
        assert_eq!(
            zip::ZipArchive::new(std::io::Cursor::new(&bytes))
                .unwrap()
                .len(),
            7
        );
        std::fs::remove_file(&workbook).unwrap();
        let workbook_stop = AtomicBool::new(false);
        assert!(matches!(
            voldiq_core::exports::save_report(
                &export_index,
                voldiq_core::exports::ReportFormat::Xlsx,
                &streaming,
                &workbook,
                &lease,
                &workbook_stop,
                |value| {
                    if value.rows > 0 {
                        workbook_stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            )
            .unwrap(),
            voldiq_core::exports::StreamOutcome::Cancelled(_)
        ));
        assert!(!workbook.exists());
        let collision = destination.path().join("collision.csv");
        let sentinel = b"Owned fixture: preserve these bytes";
        let unrelated = destination.path().join(".voldiq-export-unrelated.tmp");
        std::fs::write(&unrelated, sentinel).unwrap();
        assert!(
            voldiq_core::exports::save_report(
                &export_index,
                voldiq_core::exports::ReportFormat::Csv,
                &streaming,
                &collision,
                &lease,
                &loading,
                |value| {
                    if value.rows > 0 {
                        std::fs::write(&collision, sentinel).unwrap();
                    }
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&collision).unwrap(), sentinel);
        assert_eq!(std::fs::read(&unrelated).unwrap(), sentinel);
        let stopped = AtomicBool::new(false);
        let cancelled_path = destination.path().join("cancelled.csv");
        assert!(matches!(
            voldiq_core::exports::save_report(
                &export_index,
                voldiq_core::exports::ReportFormat::Csv,
                &streaming,
                &cancelled_path,
                &lease,
                &stopped,
                |value| {
                    if value.rows > 0 {
                        assert!(
                            std::fs::rename(
                                destination.path(),
                                destination.path().with_extension("moved")
                            )
                            .is_err()
                        );
                        assert!(policy.protect_folder(destination.path()).is_err());
                        stopped.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            )
            .unwrap(),
            voldiq_core::exports::StreamOutcome::Cancelled(_)
        ));
        assert!(!cancelled_path.exists());
        for denied in [
            protected.join("report.csv"),
            export_state.path().join("report.csv"),
            destination.path().join("credentials.json"),
            destination.path().join("report.csv:stream"),
        ] {
            assert!(
                voldiq_core::exports::save_report(
                    &export_index,
                    voldiq_core::exports::ReportFormat::Csv,
                    &streaming,
                    &denied,
                    &lease,
                    &loading,
                    |_| {}
                )
                .is_err()
            );
            assert!(!denied.exists());
        }
        let invalid = voldiq_core::exports::ReportSelection {
            items: streaming.items,
            filter: voldiq_core::exports::ReportFilter {
                expression: "size:invalid".into(),
                ..Default::default()
            },
            include_paths: false,
        };
        assert!(
            voldiq_core::exports::save_report(
                &export_index,
                voldiq_core::exports::ReportFormat::Csv,
                &invalid,
                &destination.path().join("invalid.csv"),
                &lease,
                &loading,
                |_| {}
            )
            .is_err()
        );
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 4);
    }
    let mut workbook_index = capture(fixture.path());
    let workbook_entry = workbook_index
        .entries
        .iter_mut()
        .find(|entry| entry.kind == voldiq_core::Kind::File)
        .unwrap();
    workbook_entry.name = "=1+1\"<&>\n_x000D_\u{1} _X000D_".into();
    workbook_entry.logical = u64::MAX;
    workbook_entry.allocation_known = false;
    workbook_entry.allocated = 0;
    workbook_entry.modified = None;
    let mut workbook = std::io::Cursor::new(Vec::new());
    assert!(
        matches!(voldiq_core::exports::stream_xlsx(&workbook_index, &streaming, &mut workbook, &loading, |_| {}).unwrap(), voldiq_core::exports::StreamOutcome::Complete(value) if value.rows == 1)
    );
    {
        use std::io::Read;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(workbook.get_ref())).unwrap();
        let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
        assert_eq!(names.len(), 7);
        let mut data = String::new();
        let mut metadata = String::new();
        for name in names {
            let mut xml = String::new();
            archive
                .by_name(&name)
                .unwrap()
                .read_to_string(&mut xml)
                .unwrap();
            let mut reader = quick_xml::Reader::from_str(&xml);
            loop {
                match reader.read_event().unwrap() {
                    quick_xml::events::Event::Eof => break,
                    quick_xml::events::Event::Start(element)
                    | quick_xml::events::Event::Empty(element) => {
                        assert_ne!(element.name().as_ref(), "f");
                        assert_ne!(element.name().as_ref(), "hyperlink");
                    }
                    _ => {}
                }
            }
            assert!(!xml.contains("TargetMode=\"External\""));
            if name == "xl/worksheets/sheet2.xml" {
                data = xml;
            } else if name == "xl/worksheets/sheet1.xml" {
                metadata = xml;
            }
        }
        assert!(data.contains("=1+1&quot;&lt;&amp;&gt;\n_x005F_x000D__x0001_"));
        assert!(data.contains(" _X000D_"));
        assert!(data.contains("r=\"D2\" t=\"inlineStr\""));
        assert!(data.contains("18446744073709551615"));
        assert!(data.contains("<c r=\"E2\"/>") && data.contains("<c r=\"G2\"/>"));
        assert!(!metadata.contains(workbook_index.root.to_str().unwrap()));
        assert!(metadata.contains("Withheld") && metadata.contains("filter_text_included"));
    }
    let mut stopped_workbook = std::io::Cursor::new(Vec::new());
    assert!(matches!(
        voldiq_core::exports::stream_xlsx(
            &workbook_index,
            &streaming,
            &mut stopped_workbook,
            &AtomicBool::new(true),
            |_| {}
        )
        .unwrap(),
        voldiq_core::exports::StreamOutcome::Cancelled(_)
    ));
    assert!(stopped_workbook.get_ref().is_empty());
    workbook_index
        .entries
        .iter_mut()
        .find(|entry| entry.kind == voldiq_core::Kind::File)
        .unwrap()
        .name = format!("=1+1{}", "x".repeat(32_768));
    assert!(
        voldiq_core::exports::stream_xlsx(
            &workbook_index,
            &streaming,
            std::io::Cursor::new(Vec::new()),
            &loading,
            |_| {}
        )
        .is_err()
    );
    struct FailingSink;
    impl std::io::Write for FailingSink {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("Owned fixture output failure"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(
        voldiq_core::exports::stream_csv(
            &export_index,
            &streaming,
            &mut FailingSink,
            &loading,
            |_| {}
        )
        .is_err()
    );
    let mut empty = Vec::new();
    assert!(matches!(
        voldiq_core::exports::stream_csv(
            &export_index,
            &streaming,
            &mut empty,
            &AtomicBool::new(true),
            |_| {}
        )
        .unwrap(),
        voldiq_core::exports::StreamOutcome::Cancelled(_)
    ));
    assert!(empty.is_empty());
    let mut model = export_index
        .entries
        .iter()
        .find(|entry| entry.kind == voldiq_core::Kind::File)
        .unwrap()
        .clone();
    model.file_state = None;
    model.logical = 1;
    model.allocation_known = false;
    model.allocated = 0;
    export_index.entries.truncate(1);
    export_index.native_names.clear();
    export_index.observation = None;
    for id in 1..=10_005 {
        let mut entry = model.clone();
        entry.id = voldiq_core::NodeId(id);
        entry.parent = Some(voldiq_core::NodeId(0));
        entry.name = format!("record-{id}");
        export_index.entries.push(entry);
    }
    let all = voldiq_core::exports::ReportSelection {
        items: request.items,
        filter: Default::default(),
        include_paths: false,
    };
    let mut full = Vec::new();
    assert_eq!(
        voldiq_core::exports::stream_csv(&export_index, &all, &mut full, &loading, |_| {}).unwrap(),
        voldiq_core::exports::StreamOutcome::Complete(voldiq_core::exports::StreamProgress {
            inspected: 10_005,
            rows: 10_005,
            total_entries: 10_005
        })
    );
    assert_eq!(
        String::from_utf8(full)
            .unwrap()
            .lines()
            .filter(|line| line.starts_with("item,"))
            .count(),
        10_005
    );
    let mut full_workbook = std::io::Cursor::new(Vec::new());
    assert!(
        matches!(voldiq_core::exports::stream_xlsx(&export_index, &all, &mut full_workbook, &loading, |_| {}).unwrap(), voldiq_core::exports::StreamOutcome::Complete(value) if value.rows == 10_005)
    );
    {
        use std::io::Read;
        let mut archive = zip::ZipArchive::new(full_workbook).unwrap();
        let mut sheet = String::new();
        archive
            .by_name("xl/worksheets/sheet2.xml")
            .unwrap()
            .read_to_string(&mut sheet)
            .unwrap();
        assert_eq!(sheet.matches("<row r=").count(), 10_006);
        assert!(sheet.contains("record-10005") && sheet.contains("autoFilter ref=\"A1:J10006\""));
    }
    for entry in export_index.entries.iter_mut().skip(1) {
        entry.modified = Some(0);
        entry.hard_link = false;
        entry.incomplete = false;
    }
    export_index.entries[10_000].logical = u64::MAX;
    export_index.entries[10_001].logical = u64::MAX;
    export_index.entries[10_001].hard_link = true;
    export_index.entries[10_001].incomplete = true;
    export_index.entries[10_001].modified = None;
    let summary = voldiq_core::exports::ranked_summary(&export_index, &all, &loading, |_| {})
        .unwrap()
        .unwrap();
    assert_eq!(
        (summary.progress.rows, summary.rows.len(), summary.omitted),
        (10_005, 50, 9_955)
    );
    assert_eq!(
        summary
            .rows
            .iter()
            .take(3)
            .map(|row| row.entry.id.0)
            .collect::<Vec<_>>(),
        [10_000, 10_001, 1]
    );
    assert!(summary.rows.iter().all(|row| row.path.is_none()));
    assert!(
        matches!(summary.logical, voldiq_core::exports::LogicalTotal::FilePaths { bytes } if bytes == 36_893_488_147_419_113_233)
    );
    assert_eq!(
        (
            summary.incomplete_matches,
            summary.unknown_allocation_matches,
            summary.unknown_date_matches,
            summary.recorded_hard_link_matches
        ),
        (1, 10_005, 1, 1)
    );
    assert_eq!(
        summary.scope["order"],
        "recorded_logical_bytes_descending_then_id"
    );
    assert_eq!(summary.scope["traversal_order"], "recorded_index");
    assert_eq!(summary.scope["coverage"], "unknown");
    #[cfg(windows)]
    {
        export_index.entries[10_000].name =
            "Espa\u{f1}ol - \u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629} - =1+1\n\u{202e}"
                .into();
        export_index.entries[10_001].name = format!("LONG-NAME-{}-END", "abcdef".repeat(900));
        let destination = tempfile::tempdir().unwrap();
        let export_state = tempfile::tempdir().unwrap();
        let lease = voldiq_core::operations::Lease::acquire(export_state.path()).unwrap();
        let pdf = destination.path().join("ranked-summary.pdf");
        assert!(matches!(voldiq_core::exports::save_report(
            &export_index, voldiq_core::exports::ReportFormat::Pdf, &all, &pdf, &lease, &loading, |_| {}
        ).unwrap(), voldiq_core::exports::StreamOutcome::Complete(value) if value.rows == 10_005));
        let bytes = std::fs::read(&pdf).unwrap();
        assert!(bytes.starts_with(b"%PDF-1."));
        assert!(bytes.ends_with(b"%%EOF"));
        let mut stopped_pdf = Vec::new();
        assert!(matches!(
            voldiq_core::exports::stream_pdf(
                &export_index,
                &all,
                &mut stopped_pdf,
                &AtomicBool::new(true),
                |_| {}
            )
            .unwrap(),
            voldiq_core::exports::StreamOutcome::Cancelled(_)
        ));
        assert!(stopped_pdf.is_empty());
        let cancel = AtomicBool::new(false);
        let cancelled_pdf = destination.path().join("cancelled.pdf");
        assert!(matches!(
            voldiq_core::exports::save_report(
                &export_index,
                voldiq_core::exports::ReportFormat::Pdf,
                &all,
                &cancelled_pdf,
                &lease,
                &cancel,
                |state| {
                    if state.rows > 0 {
                        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            )
            .unwrap(),
            voldiq_core::exports::StreamOutcome::Cancelled(_)
        ));
        assert!(!cancelled_pdf.exists());
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 1);
        // Explicit developer opt-in retains only this owned output fixture for independent PDF review.
        if std::env::var("VOLDIQ_KEEP_PDF_FIXTURE").as_deref() == Ok("1") {
            println!(
                "PDF fixture: {}",
                destination.keep().join("ranked-summary.pdf").display()
            );
        }
    }
    #[cfg(windows)]
    {
        use voldiq_core::{
            NodeId,
            exports::{GraphicCamera, GraphicView, StreamOutcome, SvgRequest},
            workbench::{Color, Metric, Representation},
        };
        let destination = tempfile::tempdir().unwrap();
        let export_state = tempfile::tempdir().unwrap();
        let lease = voldiq_core::operations::Lease::acquire(export_state.path()).unwrap();
        let mut request = SvgRequest {
            context: voldiq_core::exports::GraphicContext {
                scope: NodeId(0),
                cloud: None,
                include_paths: false,
                highlight: voldiq_core::exports::ReportFilter {
                    text: "https://private.invalid/?token=secret".into(),
                    ..Default::default()
                },
                view: GraphicView {
                    representation: Representation::Treemap,
                    metric: Metric::Logical,
                    color: Color::Type,
                    color_maximum: 70.0,
                    reference_time: 0.0,
                    dark: false,
                    depth: 3,
                    width: 1000.0,
                    height: 700.0,
                    camera: GraphicCamera {
                        x: 0.0,
                        y: 0.0,
                        scale: 1.0,
                    },
                    detail: true,
                    city_height: None,
                    lens: false,
                },
            },
            drawing: include_str!("fixtures/storage-map.svg").into(),
        };
        let report = destination.path().join("map.svg");
        assert!(
            matches!(voldiq_core::exports::save_svg(&export_index,&request,&report,&lease,&loading,|_|{}).unwrap(), StreamOutcome::Complete(value) if value.rows==10_005)
        );
        let saved = std::fs::read_to_string(&report).unwrap();
        assert!(
            !saved.contains("private.invalid") && !saved.contains("Export integration supplies")
        );
        assert!(
            saved.contains("&lt;script&gt;archive.zip&lt;/script&gt;") && !saved.contains("<image")
        );
        let mut reader = quick_xml::Reader::from_str(&saved);
        let mut inside = false;
        let mut metadata = String::new();
        loop {
            match reader.read_event().unwrap() {
                quick_xml::events::Event::Start(element)
                    if element.name().as_ref() == "metadata" =>
                {
                    inside = true
                }
                quick_xml::events::Event::Text(text) if inside => metadata.push_str(text.as_ref()),
                quick_xml::events::Event::GeneralRef(value) if inside => {
                    if let Some(character) = value.resolve_char_ref().unwrap() {
                        metadata.push(character);
                    } else {
                        metadata.push_str(
                            quick_xml::escape::resolve_predefined_entity(value.as_ref()).unwrap(),
                        );
                    }
                }
                quick_xml::events::Event::End(element) if element.name().as_ref() == "metadata" => {
                    break;
                }
                quick_xml::events::Event::Eof => panic!("Published SVG lacks core context"),
                _ => {}
            }
        }
        let context: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(context["scope_file_matches"], 10_005);
        assert_eq!(context["scope_node"], 0);
        assert_eq!(context["highlight_filter"]["text"], serde_json::Value::Null);
        assert_eq!(context["root_path"], serde_json::Value::Null);
        assert_eq!(context["scope_path"], serde_json::Value::Null);
        assert_eq!(context["client_declared_view"]["representation"], "treemap");
        assert_eq!(context["coverage"], "unknown");
        assert!(
            voldiq_core::exports::save_svg(
                &export_index,
                &request,
                &report,
                &lease,
                &loading,
                |_| {}
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&report).unwrap(), saved);
        let original = request.drawing.clone();
        for attack in [
            original.replacen("<defs>", "<script>bad</script><defs>", 1),
            original.replacen(
                "fill=\"#fcfcf9\"",
                "fill=\"url(https://private.invalid/image)\"",
                1,
            ),
            format!("<!DOCTYPE svg [<!ENTITY stolen SYSTEM 'file:///private'>]>{original}"),
        ] {
            request.drawing = attack;
            assert!(
                voldiq_core::exports::save_svg(
                    &export_index,
                    &request,
                    &destination.path().join("unsafe.svg"),
                    &lease,
                    &loading,
                    |_| {}
                )
                .is_err()
            );
            assert!(!destination.path().join("unsafe.svg").exists());
        }
        request.drawing = original;
        for (representation, drawing, filename) in [
            (
                Representation::Sunburst,
                include_str!("fixtures/storage-sunburst.svg"),
                "sunburst.svg",
            ),
            (
                Representation::Circles,
                include_str!("fixtures/storage-circles.svg"),
                "circles.svg",
            ),
            (
                Representation::Voronoi,
                include_str!("fixtures/storage-voronoi.svg"),
                "voronoi.svg",
            ),
            (
                Representation::City,
                include_str!("fixtures/storage-city.svg"),
                "city.svg",
            ),
        ] {
            request.context.view.representation = representation;
            request.context.view.city_height = (representation == Representation::City).then_some(
                voldiq_core::exports::GraphicHeight {
                    metric: voldiq_core::exports::GraphicHeightMetric::Modified,
                    relief: 1.0,
                    maximum: 70.0,
                },
            );
            request.drawing = drawing.into();
            assert!(
                matches!(voldiq_core::exports::save_svg(&export_index,&request,&destination.path().join(filename),&lease,&loading,|_|{}).unwrap(),StreamOutcome::Complete(value) if value.rows==10_005)
            );
        }
        request.drawing = include_str!("fixtures/storage-map.svg").into();
        request.context.view.representation = Representation::Treemap;
        request.context.view.city_height = None;
        let stop = AtomicBool::new(false);
        assert!(matches!(
            voldiq_core::exports::save_svg(
                &export_index,
                &request,
                &destination.path().join("stopped.svg"),
                &lease,
                &stop,
                |value| {
                    if value.rows > 0 {
                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            )
            .unwrap(),
            StreamOutcome::Cancelled(_)
        ));
        assert!(!destination.path().join("stopped.svg").exists());
        let mut pre_cancelled = Vec::new();
        assert!(matches!(
            voldiq_core::exports::stream_svg(
                &export_index,
                &request,
                &mut pre_cancelled,
                &AtomicBool::new(true),
                |_| {}
            )
            .unwrap(),
            StreamOutcome::Cancelled(_)
        ));
        assert!(pre_cancelled.is_empty());
        request.context.include_paths = true;
        let mut opted_in = Vec::new();
        voldiq_core::exports::stream_svg(&export_index, &request, &mut opted_in, &loading, |_| {})
            .unwrap();
        assert!(
            String::from_utf8(opted_in)
                .unwrap()
                .contains("private.invalid")
        );
        let png_destination = tempfile::tempdir().unwrap();
        let mut raster = voldiq_core::exports::PngRequest {
            context: request.context.clone(),
            scale: 1,
            image: include_bytes!("fixtures/storage-treemap.png").to_vec(),
        };
        raster.context.include_paths = false;
        for (representation, scale, image, filename) in [
            (
                Representation::Treemap,
                1,
                include_bytes!("fixtures/storage-treemap.png").as_slice(),
                "treemap.png",
            ),
            (
                Representation::Sunburst,
                1,
                include_bytes!("fixtures/storage-sunburst.png").as_slice(),
                "sunburst.png",
            ),
            (
                Representation::Circles,
                1,
                include_bytes!("fixtures/storage-circles.png").as_slice(),
                "circles.png",
            ),
            (
                Representation::Voronoi,
                1,
                include_bytes!("fixtures/storage-voronoi.png").as_slice(),
                "voronoi.png",
            ),
            (
                Representation::City,
                1,
                include_bytes!("fixtures/storage-city.png").as_slice(),
                "city.png",
            ),
            (
                Representation::Treemap,
                2,
                include_bytes!("fixtures/storage-treemap-2x.png").as_slice(),
                "treemap-2x.png",
            ),
        ] {
            raster.context.view.representation = representation;
            raster.context.view.city_height = (representation == Representation::City).then_some(
                voldiq_core::exports::GraphicHeight {
                    metric: voldiq_core::exports::GraphicHeightMetric::Modified,
                    relief: 1.0,
                    maximum: 70.0,
                },
            );
            raster.scale = scale;
            raster.image = image.to_vec();
            let path = png_destination.path().join(filename);
            assert!(
                matches!(voldiq_core::exports::save_png(&export_index, &raster, &path, &lease, &loading, |_| {}).unwrap(), StreamOutcome::Complete(value) if value.rows == 10_005)
            );
            let bytes = std::fs::read(&path).unwrap();
            let mut decoded = png::Decoder::new(std::io::Cursor::new(&bytes))
                .read_info()
                .unwrap();
            let context: serde_json::Value =
                serde_json::from_str(&decoded.info().utf8_text[0].get_text().unwrap()).unwrap();
            assert_eq!(context["format"], "png");
            assert_eq!(context["scope_file_matches"], 10_005);
            assert_eq!(context["coverage"], "unknown");
            assert_eq!(context["highlight_filter"]["text"], serde_json::Value::Null);
            assert_eq!(context["scope_path"], serde_json::Value::Null);
            assert_eq!(context["pixel_ratio"], scale);
            assert_eq!(decoded.info().width, 1000 * u32::from(scale));
            assert_eq!(decoded.info().height, 700 * u32::from(scale));
            let mut original = png::Decoder::new(std::io::Cursor::new(image))
                .read_info()
                .unwrap();
            for _ in 0..decoded.info().height {
                assert_eq!(
                    decoded.next_row().unwrap().unwrap().data(),
                    original.next_row().unwrap().unwrap().data()
                );
            }
            decoded.finish().unwrap();
            assert!(
                voldiq_core::exports::save_png(
                    &export_index,
                    &raster,
                    &path,
                    &lease,
                    &loading,
                    |_| {}
                )
                .is_err()
            );
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let invalid_png = png_destination.path().join("invalid.png");
        let original = raster.image.clone();
        raster.image.truncate(raster.image.len() / 2);
        assert!(
            voldiq_core::exports::save_png(
                &export_index,
                &raster,
                &invalid_png,
                &lease,
                &loading,
                |_| {}
            )
            .is_err()
        );
        assert!(!invalid_png.exists());
        raster.image = original;
        raster.scale = 1; // The actual 2x image must not pass a 1x declaration.
        assert!(
            voldiq_core::exports::save_png(
                &export_index,
                &raster,
                &invalid_png,
                &lease,
                &loading,
                |_| {}
            )
            .is_err()
        );
        raster.scale = 2;
        let stopped = AtomicBool::new(false);
        assert!(matches!(
            voldiq_core::exports::save_png(
                &export_index,
                &raster,
                &invalid_png,
                &lease,
                &stopped,
                |value| {
                    if value.rows > 0 {
                        stopped.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            )
            .unwrap(),
            StreamOutcome::Cancelled(_)
        ));
        assert!(!invalid_png.exists());
        // A writer-triggered stop proves encoding cancellation after the source walk has completed.
        struct StopOutput<'a>(&'a AtomicBool);
        impl std::io::Write for StopOutput<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.store(true, std::sync::atomic::Ordering::Relaxed);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let during = AtomicBool::new(false);
        assert!(matches!(
            voldiq_core::exports::stream_png(
                &export_index,
                &raster,
                &mut StopOutput(&during),
                &during,
                |_| {}
            )
            .unwrap(),
            StreamOutcome::Cancelled(_)
        ));
        let mut empty = Vec::new();
        assert!(matches!(
            voldiq_core::exports::stream_png(
                &export_index,
                &raster,
                &mut empty,
                &AtomicBool::new(true),
                |_| {}
            )
            .unwrap(),
            StreamOutcome::Cancelled(_)
        ));
        assert!(empty.is_empty());
        // Client text chunks must never survive into the published native metadata.
        let mut injected = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut injected, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .add_itxt_chunk("Voldiq".into(), "private-client-context".into())
                .unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[1, 2, 3]).unwrap();
            writer.finish().unwrap();
        }
        raster.image = injected;
        raster.scale = 1;
        raster.context.view.width = 1.0;
        raster.context.view.height = 1.0;
        raster.context.include_paths = true;
        let mut bytes = Vec::new();
        voldiq_core::exports::stream_png(&export_index, &raster, &mut bytes, &loading, |_| {})
            .unwrap();
        let decoded = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        assert_eq!(decoded.info().utf8_text.len(), 1);
        let text = decoded.info().utf8_text[0].get_text().unwrap();
        assert!(!text.contains("private-client-context") && text.contains("private.invalid"));
        assert_eq!(
            std::fs::read_dir(png_destination.path()).unwrap().count(),
            6
        );
        if std::env::var("VOLDIQ_KEEP_PNG_FIXTURE").as_deref() == Ok("1") {
            println!("PNG fixture: {}", png_destination.keep().display());
        }
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 5);
        if std::env::var("VOLDIQ_KEEP_SVG_FIXTURE").as_deref() == Ok("1") {
            println!(
                "SVG fixture: {}",
                destination.keep().join("map.svg").display()
            );
        }
    }
    let stopped_summary = AtomicBool::new(false);
    assert!(
        voldiq_core::exports::ranked_summary(&export_index, &all, &stopped_summary, |value| {
            if value.rows > 0 {
                stopped_summary.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .unwrap()
        .is_none()
    );
    let folder_summary = voldiq_core::exports::ranked_summary(
        &first,
        &voldiq_core::exports::ReportSelection {
            items: voldiq_core::exports::ReportItems::Folders,
            filter: Default::default(),
            include_paths: false,
        },
        &loading,
        |_| {},
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        folder_summary.logical,
        voldiq_core::exports::LogicalTotal::OverlappingFolders
    ));
    export_index.entries[10_000].logical = 1;
    export_index.entries[10_001].logical = 1;
    let no_matches = voldiq_core::exports::ReportSelection {
        items: request.items,
        filter: voldiq_core::exports::ReportFilter {
            min_size: 2,
            ..Default::default()
        },
        include_paths: false,
    };
    let mut no_rows = Vec::new();
    assert!(
        matches!(voldiq_core::exports::stream_csv(&export_index, &no_matches, &mut no_rows, &loading, |_| {}).unwrap(), voldiq_core::exports::StreamOutcome::Complete(value) if value.rows == 0)
    );
    let no_rows = String::from_utf8(no_rows).unwrap();
    assert!(no_rows.contains("\r\nscope,") && no_rows.contains("\r\ncomplete,"));
    assert!(!no_rows.lines().any(|line| line.starts_with("item,")));
    let stopped = AtomicBool::new(false);
    let mut prefix = Vec::new();
    assert!(matches!(
        voldiq_core::exports::stream_csv(&export_index, &all, &mut prefix, &stopped, |value| {
            if value.rows > 0 {
                stopped.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .unwrap(),
        voldiq_core::exports::StreamOutcome::Cancelled(_)
    ));
    assert!(!String::from_utf8(prefix).unwrap().contains("\r\ncomplete,"));
    export_index.entries[1].name = "=1+1".into();
    export_index
        .entries
        .iter_mut()
        .find(|entry| entry.kind == voldiq_core::Kind::File)
        .unwrap()
        .name = format!("=1+1{}", "x".repeat(262_144));
    assert!(voldiq_core::exports::csv(&export_index, &request, &loading).is_err());
    let volume_identity = voldiq_core::space::VolumeIdentity {
        guid_root: "fixture-volume".into(),
        serial: "fixture".into(),
    };
    let volume_units = voldiq_core::space::AllocationUnits {
        total: 1000,
        free: 250,
        used: 700,
        caller_total: 100,
        caller_available: 10,
        reserved: 100,
        storage_reserve: 20,
        pool_unavailable: 5,
    };
    let volume_space = voldiq_core::space::Measurement::from_units(
        volume_identity.clone(),
        None,
        4096,
        volume_units,
    )
    .unwrap();
    assert_eq!(
        (
            volume_space.total.0,
            volume_space.free.0,
            volume_space.used.0,
            volume_space.caller_total.0,
            volume_space.caller_available.0,
            volume_space.reserved.0,
            volume_space.storage_reserve.0,
            volume_space.pool_unavailable.0
        ),
        (
            4_096_000, 1_024_000, 2_867_200, 409_600, 40_960, 409_600, 81_920, 20_480
        )
    );
    assert_eq!(
        serde_json::to_value(&volume_space).unwrap()["used"],
        "2867200"
    );
    assert_eq!(
        serde_json::to_value(voldiq_core::space::Bytes(9_007_199_254_740_993)).unwrap(),
        "9007199254740993"
    );
    assert!(
        voldiq_core::space::Measurement::from_units(volume_identity.clone(), None, 0, volume_units)
            .is_err()
    );
    let excessive = voldiq_core::space::AllocationUnits {
        total: u64::MAX,
        ..volume_units
    };
    assert!(
        voldiq_core::space::Measurement::from_units(volume_identity.clone(), None, 4096, excessive)
            .is_err()
    );
    let invalid = voldiq_core::space::AllocationUnits {
        storage_reserve: 101,
        ..volume_units
    };
    assert!(
        voldiq_core::space::Measurement::from_units(volume_identity, None, 4096, invalid).is_err()
    );
    #[cfg(windows)]
    {
        use voldiq_core::{
            ScanConcurrency,
            live::{Control, Reason, Session},
        };
        let mut store = store;
        let observed_volume = voldiq_core::space::observe_volume(fixture.path());
        let kernel = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(
                voldiq_core::windows::wide(Path::new("kernel32.dll")).as_ptr(),
            )
        };
        let physical_api_present = !kernel.is_null()
            && unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    kernel,
                    c"GetDiskSpaceInformationW".as_ptr().cast(),
                )
            }
            .is_some();
        if physical_api_present {
            let voldiq_core::space::Observation::Measured { measurement } = observed_volume else {
                panic!(
                    "Expected actual volume counters for the owned local fixture: {observed_volume:?}"
                );
            };
            assert!(measurement.total.0 > 0);
            assert!(measurement.used.0 <= measurement.total.0);
            assert!(measurement.free.0 <= measurement.total.0);
            assert!(measurement.volume.guid_root.starts_with(r"\\?\Volume{"));
            assert_eq!(
                measurement.volume.serial,
                format!(
                    "{:016x}",
                    voldiq_core::windows::identity(fixture.path())
                        .unwrap()
                        .key
                        .0
                )
            );
        } else {
            assert!(matches!(
                observed_volume,
                voldiq_core::space::Observation::Unavailable {
                    reason: voldiq_core::space::Unavailable::Unsupported,
                    caller: Some(_)
                }
            ));
        }
        let watched = fixture.path().join("live");
        let excluded = watched.join("ignored");
        std::fs::create_dir_all(&excluded).unwrap();
        std::fs::write(watched.join("payload"), [1; 4]).unwrap();
        std::fs::write(excluded.join("secret"), [2; 1024]).unwrap();
        let options = ScanOptions {
            mode: voldiq_core::ScanMode::Directory,
            exclusions: vec![excluded.to_str().unwrap().to_owned()],
            ..Default::default()
        };
        std::fs::create_dir(watched.join("nested")).unwrap();
        let baseline = scan(&watched, &options, &loading, |_| {}).unwrap();
        let control = Control::new().unwrap();
        let mut live = Session::start(
            &baseline,
            ScanConcurrency::default(),
            voldiq_core::ScanProfile::Eco,
            control.clone(),
        )
        .unwrap();
        let initial = live.observe().unwrap().unwrap();
        assert_eq!(initial.report.reason, Reason::Initial);
        assert_eq!(initial.report.after.logical, "4");
        assert_eq!(initial.report.net_change.as_deref(), Some("0"));
        let saved_initial = store.save(&initial.index, &loading, &[]).unwrap();

        std::fs::write(watched.join("payload"), [3; 11]).unwrap();
        std::fs::rename(watched.join("payload"), watched.join("renamed")).unwrap();
        std::fs::rename(watched.join("nested"), watched.join("moved")).unwrap();
        std::fs::write(watched.join("moved/extra"), [4; 3]).unwrap();
        std::fs::write(excluded.join("secret"), [5; 9999]).unwrap();
        let changed = live.observe().unwrap().unwrap();
        assert_eq!(changed.report.reason, Reason::Changes);
        assert_eq!(changed.report.after.logical, "14");
        assert_eq!(changed.report.net_change.as_deref(), Some("10"));
        assert!(changed.report.after.complete);
        let previous_folder = initial
            .index
            .entries
            .iter()
            .find(|entry| entry.name == "nested")
            .unwrap()
            .id;
        let rebased = voldiq_core::live::corresponding_directory(
            &initial.index,
            &changed.index,
            previous_folder,
        );
        assert_eq!(changed.index.entries[rebased.0 as usize].name, "moved");
        assert_eq!(
            voldiq_core::live::corresponding_directory(
                &initial.index,
                &changed.index,
                voldiq_core::NodeId(u32::MAX)
            ),
            voldiq_core::NodeId(0)
        );
        let saved_changed = store.save(&changed.index, &loading, &[]).unwrap();
        assert_eq!(
            store
                .compare(saved_initial, saved_changed, 0, 10)
                .unwrap()
                .delta,
            10
        );

        // Leave the pending notification unread while exceeding its bounded kernel buffer.
        for ordinal in 0..640 {
            let name = format!("{ordinal:04}-{}", "x".repeat(90));
            std::fs::write(watched.join(name), [6]).unwrap();
        }
        let recovered = live.observe().unwrap().unwrap();
        assert_eq!(recovered.report.reason, Reason::LostEvents);
        assert_eq!(recovered.report.after.logical, "654");
        assert_eq!(recovered.report.net_change.as_deref(), Some("640"));
        control.resynchronize().unwrap();
        let refreshed = live.observe().unwrap().unwrap();
        assert_eq!(refreshed.report.reason, Reason::Requested);
        assert_eq!(refreshed.report.after.logical, "654");
        assert_eq!(refreshed.report.net_change.as_deref(), Some("0"));
        assert!(
            Session::start(
                &store.load(saved_changed, &loading).unwrap(),
                ScanConcurrency::default(),
                voldiq_core::ScanProfile::Eco,
                Control::new().unwrap()
            )
            .is_err()
        );

        let mut partial = changed.index;
        partial.observation.as_mut().unwrap().complete = false;
        let partial_control = Control::new().unwrap();
        let mut partial_live = Session::start(
            &partial,
            ScanConcurrency::default(),
            voldiq_core::ScanProfile::Eco,
            partial_control.clone(),
        )
        .unwrap();
        assert_eq!(
            partial_live.observe().unwrap().unwrap().report.net_change,
            None
        );
        partial_control.stop().unwrap();
        drop(partial_live);
        partial.root_identity = Some((0, 0));
        assert!(
            Session::start(
                &partial,
                ScanConcurrency::default(),
                voldiq_core::ScanProfile::Eco,
                Control::new().unwrap()
            )
            .is_err()
        );

        let stopping = control.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            stopping.stop().unwrap();
        });
        assert!(live.observe().unwrap().is_none());
        stopper.join().unwrap();
        drop(live);
        assert!(
            Session::start(
                &refreshed.index,
                ScanConcurrency::default(),
                voldiq_core::ScanProfile::Eco,
                control
            )
            .is_err()
        );
        let restart_control = Control::new().unwrap();
        let mut restarted = Session::start(
            &refreshed.index,
            ScanConcurrency::default(),
            voldiq_core::ScanProfile::Eco,
            restart_control.clone(),
        )
        .unwrap();
        assert_eq!(
            restarted
                .observe()
                .unwrap()
                .unwrap()
                .report
                .net_change
                .as_deref(),
            Some("0")
        );
        restart_control.stop().unwrap();
        assert!(restarted.observe().unwrap().is_none());
        drop(restarted);
        std::fs::rename(&watched, fixture.path().join("live-stopped")).unwrap();
    }
}

fn budget_history_uses_measurements_and_real_scoped_image_samples() {
    use voldiq_core::{
        NodeId,
        budgets::{Forecast, ForecastReason, Status, Store as Budgets},
    };
    let state = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let folder = source.path().join("photos");
    std::fs::create_dir(&folder).unwrap();
    let steady = source.path().join("steady");
    std::fs::create_dir(&steady).unwrap();
    std::fs::write(steady.join("unchanged.bin"), [0; 16]).unwrap();
    std::fs::write(folder.join("a.jpg"), [1; 100]).unwrap();
    std::fs::write(folder.join("b.png"), [2; 200]).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut history = Store::open(&state.path().join("snapshots.db")).unwrap();
    let mut budgets = Budgets::open(state.path()).unwrap();
    let cancel = AtomicBool::new(false);
    let mut ids = Vec::new();
    for day in 0..8 {
        std::fs::write(folder.join("growth.bin"), vec![3; 100 * (day + 1)]).unwrap();
        let mut index = capture(source.path());
        index.observation.as_mut().unwrap().completed_at = now - (7 - day as u64) * 86400;
        ids.push(history.save(&index, &cancel, &[]).unwrap());
    }
    let mut index = history.load(*ids.last().unwrap(), &cancel).unwrap();
    let node = index
        .entries
        .iter()
        .find(|entry| entry.name == "photos")
        .unwrap()
        .id;
    // Saving a limit uses a fresh indexed identity; historical reading may use only recorded evidence.
    let fresh = capture(source.path());
    let fresh_node = fresh
        .entries
        .iter()
        .find(|entry| entry.name == "photos")
        .unwrap()
        .id;
    budgets.save(&fresh, fresh_node, "0", "2KB").unwrap();
    let steady_node = fresh
        .entries
        .iter()
        .find(|entry| entry.name == "steady")
        .unwrap()
        .id;
    budgets.save(&fresh, steady_node, "0", "1KB").unwrap();
    assert!(matches!(
        budgets
            .read(&fresh, steady_node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::NotGrowing
        }
    ));
    let report = budgets
        .read(&index, node, &history, true, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(report.logical_bytes, 1100);
    let sample = report.image_equivalent.unwrap();
    assert_eq!(
        (
            sample.sample_count,
            sample.matched_images,
            sample.mean_logical_bytes,
            sample.approximate_images
        ),
        (2, 2, 150, 7)
    );
    match report.forecast {
        Forecast::Estimate {
            as_of,
            estimated_at,
            earliest_at,
            latest_at,
            samples,
            span_days,
            ..
        } => {
            assert_eq!(as_of, now);
            assert_eq!(estimated_at, now + 9 * 86400);
            assert_eq!(
                (earliest_at, latest_at, samples, span_days),
                (estimated_at, estimated_at, 8, 7)
            );
        }
        _ => panic!("Expected a qualified, measured estimate"),
    }
    index.entries[node.0 as usize].logical = 900;
    assert!(matches!(
        budgets
            .read(&index, node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::Volatile
        }
    ));
    index.entries[node.0 as usize].logical = 1100;
    let far = budgets
        .save(&fresh, fresh_node, "1", "18446744073709551615B")
        .unwrap();
    assert!(matches!(
        budgets
            .read(&index, node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::BeyondHorizon
        }
    ));
    budgets
        .save(&fresh, fresh_node, &far.version, "2KB")
        .unwrap();
    index.observation.as_mut().unwrap().accounting_version = 99;
    assert!(matches!(
        budgets
            .read(&index, node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::Incompatible
        }
    ));
    index.observation.as_mut().unwrap().accounting_version = voldiq_core::index::ACCOUNTING_VERSION;
    index.entries[node.0 as usize].incomplete = true;
    let partial = budgets
        .read(&index, node, &history, true, &cancel)
        .unwrap()
        .unwrap();
    assert!(matches!(partial.status, Status::Unknown));
    assert!(matches!(
        partial.forecast,
        Forecast::Unavailable {
            reason: ForecastReason::Partial
        }
    ));
    index.entries[node.0 as usize].logical = 2500;
    assert!(matches!(
        budgets
            .read(&index, node, &history, false, &cancel)
            .unwrap()
            .unwrap()
            .status,
        Status::Exceeded
    ));
    index.entries[node.0 as usize].logical = 1100;
    index.entries[node.0 as usize].incomplete = false;
    index.observation.as_mut().unwrap().completed_at = now - 2 * 86400;
    assert!(matches!(
        budgets
            .read(&index, node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::Stale
        }
    ));
    index.observation = None;
    assert!(matches!(
        budgets
            .read(&index, node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::Unmeasured
        }
    ));
    assert!(
        budgets
            .read(&index, NodeId(0), &history, true, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    for id in ids.iter().take(7) {
        history.delete(*id).unwrap();
    }
    let mut fresh = capture(source.path());
    fresh.observation.as_mut().unwrap().completed_at = now;
    for _ in 0..7 {
        history.save(&fresh, &cancel, &[]).unwrap();
    }
    assert!(matches!(
        budgets
            .read(&fresh, fresh_node, &history, true, &cancel)
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Unavailable {
            reason: ForecastReason::InsufficientHistory
        }
    ));
}
