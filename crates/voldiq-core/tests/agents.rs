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
fn credentials_are_individual_persistent_revocable_and_never_stored_in_plaintext() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("allowed");
    std::fs::create_dir(&root).unwrap();
    let db = fixture.path().join("agents.db");
    let mut registry = Registry::open(&db).unwrap();
    let created = registry
        .provision("Local analysis", policy(&root, vec![]))
        .unwrap();
    let token = created.credential.expose().to_owned();
    let principal = registry.authenticate(&token).unwrap();
    assert_eq!(principal.id(), created.id);
    assert!(registry.authenticate("not-a-credential").is_err());
    assert!(
        registry
            .authorize(&principal, Capability::Scan, &root)
            .is_ok()
    );
    assert!(
        registry
            .authorize(&principal, Capability::PreparePlans, &root)
            .is_err()
    );
    let replacement = registry.rotate(created.id).unwrap();
    assert!(registry.authenticate(&token).is_err());
    assert!(
        registry
            .authorize(&principal, Capability::Scan, &root)
            .is_err()
    );
    let principal = registry.authenticate(replacement.expose()).unwrap();
    drop(registry);
    let mut registry = Registry::open(&db).unwrap();
    assert!(
        registry
            .authorize(&principal, Capability::Scan, &root)
            .is_ok()
    );
    registry.revoke(created.id).unwrap();
    assert!(registry.authenticate(replacement.expose()).is_err());
    assert!(
        registry
            .authorize(&principal, Capability::Metadata, &root)
            .is_err()
    );
    drop(registry);
    for path in [db.clone(), db.with_extension("db-wal")] {
        if let Ok(bytes) = std::fs::read(path) {
            assert!(
                !bytes
                    .windows(token.len())
                    .any(|window| window == token.as_bytes())
            );
            assert!(
                !bytes
                    .windows(replacement.expose().len())
                    .any(|window| window == replacement.expose().as_bytes())
            );
        }
    }
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

#[test]
fn registry_migrations_serialize_and_reject_future_schemas() {
    let fixture = tempfile::tempdir().unwrap();
    let db = fixture.path().join("agents.db");
    let gate = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let runs: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    gate.wait();
                    Registry::open(&db).unwrap();
                })
            })
            .collect();
        for run in runs {
            run.join().unwrap();
        }
    });
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection.execute_batch("PRAGMA user_version=999").unwrap();
    assert!(Registry::open(&db).is_err());
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        999
    );
}

#[test]
fn policies_require_explicit_bounded_capabilities_and_nonoverlapping_roots() {
    let fixture = tempfile::tempdir().unwrap();
    let nested = fixture.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    assert!(
        Policy::new(
            vec![],
            vec![],
            vec![Capability::Metadata],
            Limits::default()
        )
        .is_err()
    );
    assert!(Policy::new(vec![nested.clone()], vec![], vec![], Limits::default()).is_err());
    assert!(
        Policy::new(
            vec![fixture.path().to_owned(), nested.clone()],
            vec![],
            vec![Capability::Metadata],
            Limits::default()
        )
        .is_err()
    );
    assert!(
        Policy::new(
            vec![nested],
            vec![],
            vec![Capability::Metadata],
            Limits {
                page_size: 501,
                ..Limits::default()
            }
        )
        .is_err()
    );
}

#[cfg(windows)]
#[test]
fn windows_agent_paths_reject_ambiguous_names_namespaces_and_reparse_traversal() {
    let fixture = tempfile::tempdir().unwrap();
    let allowed = fixture.path().join("allowed");
    let outside = fixture.path().join("outside");
    std::fs::create_dir(&allowed).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let mut registry = Registry::open(&fixture.path().join("agents.db")).unwrap();
    let created = registry
        .provision("Reader", policy(&allowed, vec![]))
        .unwrap();
    let principal = registry.authenticate(created.credential.expose()).unwrap();
    for suffix in [
        "..\\outside",
        "file:stream",
        "CON",
        "NUL.txt",
        "trailing.",
        "trailing ",
    ] {
        assert!(
            registry
                .authorize(&principal, Capability::Metadata, &allowed.join(suffix))
                .is_err()
        );
    }
    for path in [
        r"\\server\share",
        r"\\.\C:",
        r"\\?\C:\Windows",
        r"C:Windows",
    ] {
        assert!(
            registry
                .authorize(&principal, Capability::Metadata, std::path::Path::new(path))
                .is_err()
        );
    }
    let escape = allowed.join("escape");
    junction(&escape, &outside);
    std::fs::write(outside.join("private.txt"), "outside").unwrap();
    assert!(
        registry
            .authorize(&principal, Capability::Metadata, &escape)
            .is_err()
    );
    assert!(
        registry
            .authorize(
                &principal,
                Capability::Metadata,
                &escape.join("private.txt")
            )
            .is_err()
    );
    assert!(
        Policy::new(
            vec![escape.clone()],
            vec![],
            vec![Capability::Metadata],
            Limits::default()
        )
        .is_err()
    );
    std::fs::remove_dir(escape).unwrap();
}

#[cfg(windows)]
fn junction(link: &std::path::Path, target: &std::path::Path) {
    use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::{Storage::FileSystem::*, System::IO::DeviceIoControl};
    std::fs::create_dir(link).unwrap();
    let print: Vec<u16> = target.as_os_str().encode_wide().collect();
    let substitute: Vec<u16> = std::ffi::OsStr::new(&format!(r"\??\{}", target.display()))
        .encode_wide()
        .collect();
    let mut data = Vec::new();
    data.extend_from_slice(&0xA000_0003u32.to_le_bytes());
    data.extend_from_slice(
        &u16::try_from(8 + (substitute.len() + print.len() + 2) * 2)
            .unwrap()
            .to_le_bytes(),
    );
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&u16::try_from(substitute.len() * 2).unwrap().to_le_bytes());
    data.extend_from_slice(
        &u16::try_from((substitute.len() + 1) * 2)
            .unwrap()
            .to_le_bytes(),
    );
    data.extend_from_slice(&u16::try_from(print.len() * 2).unwrap().to_le_bytes());
    for unit in substitute
        .iter()
        .chain([0].iter())
        .chain(print.iter())
        .chain([0].iter())
    {
        data.extend_from_slice(&unit.to_le_bytes());
    }
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_WRITE_DATA)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(link)
        .unwrap();
    let mut returned = 0;
    let result = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            0x0009_00A4,
            data.as_ptr().cast(),
            data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(result, 0, "{}", std::io::Error::last_os_error());
}
