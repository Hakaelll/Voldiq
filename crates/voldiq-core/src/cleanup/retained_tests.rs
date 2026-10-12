use super::*;
use crate::{
    ScanOptions,
    duplicates::{Analysis, Group, Member},
    scanner,
};
use std::sync::atomic::AtomicBool;

fn duplicate_plan(root: &Path) -> Plan {
    let index = scanner::scan(
        root,
        &ScanOptions::default(),
        &AtomicBool::new(false),
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
    let analysis = Analysis {
        groups: vec![Group {
            digest: blake3::hash(b"same content").to_hex().to_string(),
            logical: 12,
            recoverable: 0,
            members: [id("keep.bin"), id("remove.bin")]
                .into_iter()
                .map(|id| Member {
                    id,
                    allocated: 0,
                    links: 1,
                })
                .collect(),
        }],
        ..Analysis::default()
    };
    Plan::duplicates(
        &index,
        &analysis,
        &[id("remove.bin")],
        &AtomicBool::new(false),
    )
    .unwrap()
}

fn fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    for name in ["keep.bin", "remove.bin"] {
        std::fs::write(fixture.path().join(name), b"same content").unwrap();
    }
    fixture
}

#[test]
fn retained_guard_pins_content_and_names_until_released_without_delete_access() {
    use std::io::Read;
    let fixture = fixture();
    let plan = duplicate_plan(fixture.path());
    let keep = fixture.path().join("keep.bin");
    let guard = plan.verify_retained(&AtomicBool::new(false)).unwrap();
    assert_eq!(guard._files.len(), 1);
    assert!(std::fs::write(&keep, b"replacement!").is_err());
    assert!(std::fs::rename(&keep, fixture.path().join("moved.bin")).is_err());
    assert!(std::fs::remove_file(&keep).is_err());
    let mut reader = std::fs::File::open(&keep).unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"same content");
    assert!(guard._files[0].file.set_len(0).is_err());
    drop(reader);
    drop(guard);
    std::fs::write(&keep, b"replacement!").unwrap();
    assert!(plan.verify_retained(&AtomicBool::new(false)).is_err());
    assert_eq!(
        std::fs::read(fixture.path().join("remove.bin")).unwrap(),
        b"same content"
    );
}

#[test]
fn retained_verification_rejects_wrong_digest_replacement_and_cancellation() {
    let fixture = fixture();
    let mut plan = duplicate_plan(fixture.path());
    plan.retained[0].1 = blake3::hash(b"different").to_hex().to_string();
    assert!(plan.verify_retained(&AtomicBool::new(false)).is_err());
    // A failed content check must release every acquired handle.
    let keep = fixture.path().join("keep.bin");
    std::fs::rename(&keep, fixture.path().join("original.bin")).unwrap();
    std::fs::write(&keep, b"same content").unwrap();
    assert!(plan.verify_retained(&AtomicBool::new(false)).is_err());
    assert!(plan.verify_retained(&AtomicBool::new(true)).is_err());
    assert_eq!(std::fs::read(&keep).unwrap(), b"same content");
    assert_eq!(
        std::fs::read(fixture.path().join("original.bin")).unwrap(),
        b"same content"
    );
}

#[test]
fn writable_mapping_with_closed_file_handle_blocks_retained_acquisition() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
        PAGE_READWRITE, UnmapViewOfFile,
    };
    struct View(MEMORY_MAPPED_VIEW_ADDRESS);
    impl Drop for View {
        fn drop(&mut self) {
            unsafe {
                UnmapViewOfFile(self.0);
            }
        }
    }
    let fixture = fixture();
    let plan = duplicate_plan(fixture.path());
    let keep = fixture.path().join("keep.bin");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&keep)
        .unwrap();
    // The owned fixture is mapped using its exact file handle; RAII releases both mapping resources.
    let raw = unsafe {
        CreateFileMappingW(
            file.as_raw_handle(),
            std::ptr::null(),
            PAGE_READWRITE,
            0,
            0,
            std::ptr::null(),
        )
    };
    assert!(!raw.is_null());
    let mapping = unsafe { OwnedHandle::from_raw_handle(raw) };
    let view = View(unsafe { MapViewOfFile(mapping.as_raw_handle(), FILE_MAP_WRITE, 0, 0, 12) });
    assert!(!view.0.Value.is_null());
    drop(file);
    match plan.verify_retained(&AtomicBool::new(false)) {
        Err(Error::Io(error)) => assert_eq!(error.raw_os_error(), Some(32)),
        _ => panic!("Writable mapping must reject acquisition with ERROR_SHARING_VIOLATION"),
    }
    drop(view);
    drop(mapping);
    drop(plan.verify_retained(&AtomicBool::new(false)).unwrap());
    assert_eq!(std::fs::read(&keep).unwrap(), b"same content");
    assert_eq!(
        std::fs::read(fixture.path().join("remove.bin")).unwrap(),
        b"same content"
    );
}
