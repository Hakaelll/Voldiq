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
