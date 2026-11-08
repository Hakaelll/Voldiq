use std::{fs, path::Path, sync::atomic::AtomicBool};
use voldiq_core::space::{
    Bytes,
    system::{
        Category, Locations, Observation, RegisteredDisk, RegistrationCoverage, Report,
        WslLocations, describe, read_desktop,
    },
};

fn write(root: &Path, relative: &str, size: usize) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![0x42; size]).unwrap();
}

pub(super) fn verify() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("observed");
    let local = root.join("Local");
    let program_data = root.join("ProgramData");
    write(&root, "Windows/WinSxS/component.bin", 13);
    write(&root, "Windows.old/preserved.bin", 17);
    write(&root, "pagefile.sys", 19);
    write(&root, "HIBERFIL.SYS", 23);
    write(&root, "Local/Docker/wsl/disk/docker_data.vhdx", 29);
    write(&root, "Distros/linux/custom.vhdx", 31);
    write(&root, "Distros/docker/ext4.vhdx", 37);
    write(&root, "lookalike/WinSxS/ignored.bin", 41);
    write(&root, "lookalike/ext4.vhdx", 43);
    fs::create_dir(root.join("swapfile.sys")).unwrap();
    fs::hard_link(
        root.join("Distros/linux/custom.vhdx"),
        root.join("copy.vhdx"),
    )
    .unwrap();
    let outside = temporary.path().join("observed-other");
    write(&outside, "external.vhdx", 47);
    let locations = Locations {
        scan_volume_root: Some(root.clone()),
        windows_volume_root: Some(root.clone()),
        windows_directory: Some(root.join("Windows")),
        local_app_data: Some(local.clone()),
        program_data: Some(program_data),
        wsl: WslLocations {
            coverage: RegistrationCoverage::Partial,
            disks: vec![
                RegisteredDisk {
                    name: "Local distro".into(),
                    path: root.join("Distros/linux/custom.vhdx"),
                },
                RegisteredDisk {
                    name: "docker-desktop-data".into(),
                    path: root.join("Distros/docker/ext4.vhdx"),
                },
                RegisteredDisk {
                    name: "Duplicate registration".into(),
                    path: local.join("Docker/wsl/disk/docker_data.vhdx"),
                },
                RegisteredDisk {
                    name: "Outside private distro".into(),
                    path: outside.join("external.vhdx"),
                },
            ],
        },
    };
    let cancel = AtomicBool::new(false);
    let mut index = super::capture(&root);
    let report = describe(&index, &locations, &cancel).unwrap().unwrap();
    let serialized = serde_json::to_string(&report).unwrap();
    assert!(!serialized.contains("Outside private distro") && !serialized.contains("lookalike"));
    let Report::Live {
        items,
        scope_complete,
        docker_defaults_available,
        wsl_registrations,
        ..
    } = report
    else {
        panic!("Expected live report")
    };
    assert!(scope_complete && docker_defaults_available);
    assert_eq!(wsl_registrations, RegistrationCoverage::Partial);
    let logical = |label: &str| match &items
        .iter()
        .find(|item| item.label == label)
        .unwrap()
        .observation
    {
        Observation::Indexed {
            logical: Bytes(bytes),
            ..
        } => *bytes,
        other => panic!("Expected indexed bytes for {label}: {other:?}"),
    };
    assert_eq!(items.len(), 8);
    assert_eq!(logical("Paging file"), 19);
    assert_eq!(logical("Hibernation file"), 23);
    assert_eq!(logical("Windows component store"), 13);
    assert_eq!(logical("Previous Windows installation"), 17);
    assert_eq!(logical("Docker Desktop · WSL 2"), 29);
    assert_eq!(logical("Local distro"), 31);
    assert_eq!(logical("docker-desktop-data"), 37);
    assert!(items.iter().any(|item| item.label == "Swap file"
        && matches!(item.observation, Observation::LocationUnavailable)));
    assert!(items.iter().any(|item| item.category == Category::Wsl
        && matches!(
            item.observation,
            Observation::Indexed {
                allocated: None,
                ..
            }
        )));
    assert!(
        items
            .iter()
            .any(|item| item.category == Category::Docker && item.label == "docker-desktop-data")
    );

    let nested = super::capture(&root.join("lookalike"));
    let Some(Report::Live { items, .. }) = describe(&nested, &locations, &cancel).unwrap() else {
        panic!("Expected scoped report")
    };
    assert_eq!(items.len(), 5);
    assert!(
        items
            .iter()
            .all(|item| matches!(item.observation, Observation::OutsideScope))
    );
    let missing = Locations {
        scan_volume_root: Some(root.join("not-indexed")),
        ..locations
    };
    let Some(Report::Live { items, .. }) = describe(&index, &missing, &cancel).unwrap() else {
        panic!("Expected an unobserved location")
    };
    assert!(
        items.iter().any(|item| item.label == "Paging file"
            && matches!(item.observation, Observation::NotObserved))
    );
    let Some(Report::Live { items, .. }) = read_desktop(&index, &cancel).unwrap() else {
        panic!("Expected native location check")
    };
    assert!(
        items
            .iter()
            .take(5)
            .all(|item| matches!(item.observation, Observation::OutsideScope))
    );
    assert!(
        describe(&index, &missing, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    index.snapshot_id = Some(1);
    assert!(matches!(
        read_desktop(&index, &cancel).unwrap(),
        Some(Report::RecordedScan)
    ));
}
