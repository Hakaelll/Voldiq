use super::*;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;

#[test]
fn acquired_directory_component_keeps_its_name_until_release() {
    let fixture = tempfile::tempdir().unwrap();
    let held = fixture.path().join("held");
    let moved = fixture.path().join("moved");
    std::fs::create_dir(&held).unwrap();
    let mut name = native_root_name(&std::fs::canonicalize(fixture.path()).unwrap());
    let root = native_open(
        &mut name,
        None,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )
    .unwrap();
    let parent = open_child(&root, OsStr::new("held"), true).unwrap();
    assert!(
        std::fs::rename(&held, &moved).is_err(),
        "An acquired cleanup parent was renamed before its child was opened"
    );
    std::fs::write(held.join("sibling.bin"), b"allowed").unwrap();
    drop(parent);
    std::fs::rename(&held, &moved).unwrap();
    assert_eq!(
        std::fs::read(moved.join("sibling.bin")).unwrap(),
        b"allowed"
    );
}

fn junction_control(file: &File, code: u32, data: &[u8]) -> std::io::Result<()> {
    use windows_sys::Win32::System::IO::DeviceIoControl;
    let mut returned = 0;
    let success = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            code,
            data.as_ptr().cast(),
            data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn junction_data(target: &Path) -> Vec<u8> {
    let substitute = native_root_name(&std::fs::canonicalize(target).unwrap());
    let print: Vec<u16> = target.as_os_str().encode_wide().collect();
    let mut data = vec![0; 16];
    data[..4].copy_from_slice(&0xa0000003_u32.to_le_bytes());
    data[10..12].copy_from_slice(&(u16::try_from(substitute.len() * 2).unwrap()).to_le_bytes());
    data[12..14]
        .copy_from_slice(&(u16::try_from((substitute.len() + 1) * 2).unwrap()).to_le_bytes());
    data[14..16].copy_from_slice(&(u16::try_from(print.len() * 2).unwrap()).to_le_bytes());
    for unit in substitute
        .into_iter()
        .chain(Some(0))
        .chain(print)
        .chain(Some(0))
    {
        data.extend_from_slice(&unit.to_le_bytes());
    }
    let length = u16::try_from(data.len() - 8).unwrap();
    data[4..6].copy_from_slice(&length.to_le_bytes());
    data
}

struct JunctionGuard(File);
impl Drop for JunctionGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Ioctl::FSCTL_DELETE_REPARSE_POINT;
        let mut header = [0; 8];
        header[..4].copy_from_slice(&0xa0000003_u32.to_le_bytes());
        junction_control(&self.0, FSCTL_DELETE_REPARSE_POINT, &header)
            .expect("Restore fixture directory");
    }
}

#[test]
fn parent_handle_does_not_follow_a_junction_installed_after_acquisition() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Foundation::GENERIC_WRITE,
        Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT},
        System::Ioctl::FSCTL_SET_REPARSE_POINT,
    };
    let fixture = tempfile::tempdir().unwrap();
    let held = fixture.path().join("held");
    let target = fixture.path().join("target");
    std::fs::create_dir(&held).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("file.bin"), b"replacement").unwrap();
    let mut name = native_root_name(&std::fs::canonicalize(&held).unwrap());
    let parent = native_open(
        &mut name,
        None,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )
    .unwrap();
    assert!(!crate::windows::is_reparse(&parent.metadata().unwrap()));
    let mutator = std::fs::OpenOptions::new()
        .access_mode(GENERIC_WRITE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&held)
        .unwrap();
    junction_control(&mutator, FSCTL_SET_REPARSE_POINT, &junction_data(&target)).unwrap();
    let guard = JunctionGuard(mutator);
    assert!(crate::windows::is_reparse(&parent.metadata().unwrap()));
    assert_eq!(
        std::fs::read(held.join("file.bin")).unwrap(),
        b"replacement"
    );
    let child = open_child(&parent, OsStr::new("file.bin"), false);
    let detail = match &child {
        Ok(file) => format!(
            "opened identity={:?}; target identity={:?}",
            crate::windows::file_identity(file).unwrap().key,
            crate::windows::identity(&target.join("file.bin"))
                .unwrap()
                .key
        ),
        Err(error) => format!("relative open error: {error:?}"),
    };
    assert!(
        matches!(child, Err(Error::Io(ref error)) if error.raw_os_error() == Some(windows_sys::Win32::Foundation::ERROR_CANT_RESOLVE_FILENAME as i32)),
        "{detail}"
    );
    drop(guard);
    assert!(!crate::windows::is_reparse(&parent.metadata().unwrap()));
    drop(parent);
    assert_eq!(
        std::fs::read(target.join("file.bin")).unwrap(),
        b"replacement"
    );
}

#[test]
fn populated_pinned_parent_cannot_be_converted_to_a_junction() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Foundation::{ERROR_DIR_NOT_EMPTY, GENERIC_WRITE},
        Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT},
        System::Ioctl::FSCTL_SET_REPARSE_POINT,
    };
    let fixture = tempfile::tempdir().unwrap();
    let held = fixture.path().join("held");
    let target = fixture.path().join("target");
    std::fs::create_dir(&held).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::write(held.join("file.bin"), b"original").unwrap();
    std::fs::write(target.join("file.bin"), b"replacement").unwrap();
    let expected = crate::windows::identity(&held.join("file.bin"))
        .unwrap()
        .key;
    let mut name = native_root_name(&std::fs::canonicalize(&held).unwrap());
    let parent = native_open(
        &mut name,
        None,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )
    .unwrap();
    let leaf = open_child(&parent, OsStr::new("file.bin"), false).unwrap();
    let mutator = std::fs::OpenOptions::new()
        .access_mode(GENERIC_WRITE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&held)
        .unwrap();
    let result = junction_control(&mutator, FSCTL_SET_REPARSE_POINT, &junction_data(&target));
    assert!(
        matches!(result, Err(ref error) if error.raw_os_error() == Some(ERROR_DIR_NOT_EMPTY as i32)),
        "{result:?}"
    );
    assert!(!crate::windows::is_reparse(&parent.metadata().unwrap()));
    assert_eq!(crate::windows::file_identity(&leaf).unwrap().key, expected);
    assert_eq!(std::fs::read(held.join("file.bin")).unwrap(), b"original");
    assert_eq!(
        std::fs::read(target.join("file.bin")).unwrap(),
        b"replacement"
    );
    drop(mutator);
    drop(leaf);
    drop(parent);
}

#[test]
fn child_open_keeps_directory_authority_after_name_replacement() {
    let fixture = tempfile::tempdir().unwrap();
    let held = fixture.path().join("held");
    let retained = fixture.path().join("retained");
    std::fs::create_dir(&held).unwrap();
    let original = held.join("file.bin");
    std::fs::write(&original, b"original").unwrap();
    let expected = crate::windows::identity(&original).unwrap().key;
    let canonical = std::fs::canonicalize(&held).unwrap();
    let mut native = native_root_name(&canonical);
    // Allow namespace substitution in this fixture; production directory handles deny delete sharing.
    let parent = native_open(
        &mut native,
        None,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
    .unwrap();
    std::fs::rename(&held, &retained).unwrap();
    std::fs::create_dir(&held).unwrap();
    std::fs::write(held.join("file.bin"), b"replacement").unwrap();
    let selected = open_child(&parent, OsStr::new("file.bin"), false).unwrap();
    assert_eq!(
        crate::windows::file_identity(&selected).unwrap().key,
        expected
    );
    assert_eq!(selected.metadata().unwrap().len(), 8);
    assert_ne!(
        crate::windows::identity(&held.join("file.bin"))
            .unwrap()
            .key,
        expected
    );
    for invalid in [
        "",
        ".",
        "..",
        "../file.bin",
        "file.bin:stream",
        "C:\\file.bin",
        "file\0.bin",
    ] {
        assert!(open_child(&parent, OsStr::new(invalid), false).is_err());
    }
    assert!(open_child(&parent, OsStr::new("missing.bin"), false).is_err());
    drop(selected);
    drop(parent);
    assert!(!retained.join("missing.bin").exists());
    assert_eq!(
        std::fs::read(retained.join("file.bin")).unwrap(),
        b"original"
    );
    assert_eq!(
        std::fs::read(held.join("file.bin")).unwrap(),
        b"replacement"
    );
}

#[test]
fn root_names_preserve_disk_and_unc_namespaces() {
    for (input, expected) in [
        (r"C:\", r"\??\C:\"),
        (r"\\?\C:\", r"\??\C:\"),
        (r"\\server\share\", r"\??\UNC\server\share\"),
        (r"\\?\UNC\server\share\", r"\??\UNC\server\share\"),
    ] {
        assert_eq!(
            native_root_name(Path::new(input)),
            expected.encode_utf16().collect::<Vec<_>>()
        );
    }
    for invalid in ["", "relative", r"C:\child", r"C:child", r"\child"] {
        assert!(open_root(Path::new(invalid)).is_err());
    }
}
