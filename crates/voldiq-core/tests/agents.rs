use std::path::PathBuf;
use voldiq_core::agents::{Capability, Limits, Policy, Registry};

fn policy(root: &std::path::Path, exclusions: Vec<PathBuf>) -> Policy {
    Policy::new(
        vec![root.to_owned()],
        exclusions,
        vec![Capability::Metadata, Capability::Scan],
        Limits::default(),
    )
    .unwrap()
}

#[test]
fn scopes_reject_siblings_exclusions_missing_paths_and_changed_roots_without_disclosure() {
    let fixture = tempfile::tempdir().unwrap();
    let allowed = fixture.path().join("allowed");
    let sibling = fixture.path().join("allowed-secret");
    let excluded = allowed.join("private");
    std::fs::create_dir_all(&excluded).unwrap();
    std::fs::create_dir(&sibling).unwrap();
    std::fs::write(allowed.join("report.txt"), "report").unwrap();
    std::fs::write(excluded.join("key.pem"), "secret").unwrap();
    let mut registry = Registry::open(&fixture.path().join("agents.db")).unwrap();
    let created = registry
        .provision("Reader", policy(&allowed, vec![excluded.clone()]))
        .unwrap();
    let principal = registry.authenticate(created.credential.expose()).unwrap();
    let authorized = registry
        .authorize(
            &principal,
            Capability::Metadata,
            &allowed.join("report.txt"),
        )
        .unwrap();
    assert_eq!(
        authorized.identity(),
        voldiq_core::windows::identity(&allowed.join("report.txt"))
            .unwrap()
            .key
    );
    drop(authorized);
    {
        use voldiq_core::{Kind, NodeId, security::SensitiveKind};
        let expected = [
            ("AUTH.JSON", SensitiveKind::CredentialFile),
            (".env.example", SensitiveKind::CredentialFile),
            ("id_ed25519", SensitiveKind::KeyMaterial),
            ("public-certificate.PEM", SensitiveKind::KeyMaterial),
            ("vault.kdbx", SensitiveKind::PasswordStore),
            (".SSH/nested/opaque.txt", SensitiveKind::CredentialLocation),
        ];
        for (name, _) in expected {
            let path = allowed.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"fictional-content-not-for-metadata").unwrap();
        }
        for name in [
            ".environment",
            "public.pem.txt",
            "key.pub",
            "secrets-guide.md",
        ] {
            std::fs::write(allowed.join(name), b"ordinary").unwrap();
        }
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let index = voldiq_core::scanner::scan(
            &allowed,
            &voldiq_core::ScanOptions {
                exclusions: vec![excluded.to_str().unwrap().to_owned()],
                ..Default::default()
            },
            &cancel,
            |_| {},
        )
        .unwrap();
        let safety_state = tempfile::tempdir().unwrap();
        let mut readers = Registry::open(&safety_state.path().join("agents.db")).unwrap();
        let reader = readers
            .provision(
                "Sensitive metadata",
                Policy::new(
                    vec![allowed.clone()],
                    vec![excluded.clone()],
                    vec![Capability::Metadata],
                    Limits::default(),
                )
                .unwrap(),
            )
            .unwrap();
        let service = voldiq_core::service::AgentService::new(
            safety_state.path().to_owned(),
            readers.authenticate(reader.credential.expose()).unwrap(),
        )
        .unwrap();
        let snapshot =
            voldiq_core::snapshots::Store::open(&safety_state.path().join("snapshots.db"))
                .unwrap()
                .save(&index, &cancel, &[])
                .unwrap();
        let protections = voldiq_core::metadata::Store::open(safety_state.path())
            .unwrap()
            .protections()
            .unwrap();
        let mask = protections.for_index(&index, &cancel).unwrap();
        for (name, kind) in expected {
            let path = std::fs::canonicalize(allowed.join(name)).unwrap();
            let id = index
                .entries
                .iter()
                .find(|e| index.path(e.id).unwrap() == path)
                .unwrap()
                .id;
            let details = service.details(snapshot, id).unwrap();
            assert_eq!(details.sensitive.unwrap().kind, kind);
            assert!(
                !serde_json::to_string(&details)
                    .unwrap()
                    .contains("fictional-content-not-for-metadata")
            );
            assert!(!mask.allows(id));
            assert!(
                voldiq_core::cleanup::Plan::manual(&index, &[id])
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("Sensitive")
            );
            assert_eq!(
                std::fs::read(path).unwrap(),
                b"fictional-content-not-for-metadata"
            );
        }
        for entry in &index.entries {
            if [
                "report.txt",
                ".environment",
                "public.pem.txt",
                "key.pub",
                "secrets-guide.md",
            ]
            .contains(&entry.name.as_str())
            {
                assert_eq!(entry.kind, Kind::File);
                assert!(index.details(entry.id).unwrap().sensitive.is_none());
                assert!(mask.allows(entry.id));
                assert!(voldiq_core::cleanup::Plan::manual(&index, &[entry.id]).is_ok());
            }
        }
        let missing = index
            .entries
            .iter()
            .find(|e| e.name == "id_ed25519")
            .unwrap()
            .id;
        std::fs::remove_file(allowed.join("id_ed25519")).unwrap();
        assert_eq!(
            index.details(missing).unwrap().sensitive.unwrap().kind,
            SensitiveKind::KeyMaterial
        );
        assert_eq!(
            service
                .details(snapshot, missing)
                .unwrap()
                .sensitive
                .unwrap()
                .kind,
            SensitiveKind::KeyMaterial
        );
        assert!(index.details(NodeId(0)).unwrap().sensitive.is_none());
    }
    let denied = registry
        .authorize(&principal, Capability::Metadata, &sibling)
        .err()
        .unwrap()
        .to_string();
    for path in [
        &excluded,
        &excluded.join("key.pem"),
        &sibling.join("absent"),
        &allowed.join("absent"),
    ] {
        assert_eq!(
            registry
                .authorize(&principal, Capability::Metadata, path)
                .err()
                .unwrap()
                .to_string(),
            denied
        );
    }
    assert!(!denied.contains("secret"));
    let renamed_exclusion = allowed.join("renamed-private");
    std::fs::rename(&excluded, &renamed_exclusion).unwrap();
    assert!(
        registry
            .authorize(
                &principal,
                Capability::Metadata,
                &renamed_exclusion.join("key.pem")
            )
            .is_err()
    );
    #[cfg(windows)]
    {
        let guard = registry
            .authorize(
                &principal,
                Capability::Metadata,
                &allowed.join("report.txt"),
            )
            .unwrap();
        assert!(std::fs::rename(&allowed, fixture.path().join("pinned-root")).is_err());
        drop(guard);
    }
    std::fs::rename(&allowed, fixture.path().join("old-root")).unwrap();
    std::fs::create_dir(&allowed).unwrap();
    assert_eq!(
        registry
            .authorize(&principal, Capability::Metadata, &allowed)
            .err()
            .unwrap()
            .to_string(),
        denied
    );
}
