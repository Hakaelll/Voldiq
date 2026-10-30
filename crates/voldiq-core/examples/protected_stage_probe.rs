#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        fs::File,
        io::{self, Read, Write},
        os::windows::io::{AsRawHandle, FromRawHandle},
        ptr,
    };
    use voldiq_core::windows::{
        cleanup_stage::{StageEntry, StageIdentity, StageName, StagePin},
        wide,
    };
    use windows_sys::Win32::{
        Foundation::{INVALID_HANDLE_VALUE, LocalFree},
        Security::{
            Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW},
            DACL_SECURITY_INFORMATION,
        },
        Storage::FileSystem::*,
    };

    fn directory(path: &std::path::Path) -> io::Result<File> {
        let name = wide(path);
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | READ_CONTROL,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    }
    fn native_identity(file: &File) -> io::Result<StageIdentity> {
        let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(StageIdentity {
            volume: info.VolumeSerialNumber,
            file: info.FileId.Identifier,
        })
    }

    if std::env::args_os().len() != 1 {
        return Err("This fixture accepts no arguments.".into());
    }
    let fixture = tempfile::tempdir()?;
    let parent = directory(fixture.path())?;
    assert!(StageName::parse("..").is_err());
    assert!(StageName::parse("C:\\Windows").is_err());
    let name = StageName::generate()?;
    let name = StageName::parse(name.as_str())?;
    let path = fixture.path().join(name.as_str());
    let creator = StagePin::create(&parent, &name)?;
    let expected = creator.identity();
    assert!(StagePin::create(&parent, &name).is_err());
    let mut consumer = StagePin::acquire(&parent, &name, expected)?;
    assert_eq!(consumer.identity(), expected);
    let mut wrong = expected;
    wrong.file[0] ^= 1;
    assert_eq!(
        StagePin::acquire(&parent, &name, wrong)
            .err()
            .ok_or("Wrong identity accepted.")?
            .kind(),
        io::ErrorKind::InvalidData
    );
    let child_error = std::fs::write(path.join("blocked.bin"), b"not admitted")
        .unwrap_err()
        .raw_os_error()
        .ok_or("Missing child-create error.")?;
    assert_eq!(child_error, 5);

    let mut descriptor = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let path_wide = wide(&path);
    let queried = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if queried != 0 {
        return Err(io::Error::from_raw_os_error(queried as i32).into());
    }
    let acl_error = unsafe {
        SetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    unsafe { LocalFree(descriptor) };
    assert_eq!(acl_error, 5);
    for invalid in [
        "", ".", "..", "../x", "x:y", "file.", "file ", "CON.txt", "CON .txt", "CONOUT$", "lpt¹",
        "x*",
    ] {
        assert!(
            StageEntry::parse(invalid).is_err(),
            "Unexpected accepted filename: {invalid}"
        );
    }
    assert_eq!(StageEntry::parse("Résumé.txt")?.as_str(), "Résumé.txt");
    let source_path = fixture.path().join("owned-source.bin");
    let mut writer = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&source_path)?;
    writer.write_all(&[1, 2, 3, 4])?;
    let source_identity = native_identity(&writer)?;
    drop(writer);
    let source_wide = wide(&source_path);
    let raw_source = unsafe {
        CreateFileW(
            source_wide.as_ptr(),
            DELETE | FILE_READ_DATA | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL,
            ptr::null_mut(),
        )
    };
    if raw_source == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    let mut source = unsafe { File::from_raw_handle(raw_source) };
    assert_eq!(native_identity(&source)?, source_identity);
    let source_entry = StageEntry::parse("owned-source.bin")?;
    let admission_error = consumer
        .admit(&source, &source_entry)
        .unwrap_err()
        .raw_os_error()
        .ok_or("Missing native admission error.")?;
    assert_eq!(admission_error, 5);
    assert!(source_path.is_file());
    assert!(!path.join(source_entry.as_str()).exists());
    let mut original_bytes = Vec::new();
    source.read_to_end(&mut original_bytes)?;
    assert_eq!(original_bytes, [1, 2, 3, 4]);
    assert_eq!(native_identity(&source)?, source_identity);
    drop(source);
    std::fs::remove_file(&source_path)?;
    drop(creator);
    let moved = fixture.path().join("released-stage");
    let rename_error = std::fs::rename(&path, &moved)
        .unwrap_err()
        .raw_os_error()
        .ok_or("Missing pinned-rename error.")?;
    assert!([5, 32].contains(&rename_error));
    drop(consumer);
    std::fs::rename(&path, &moved)?;
    let reopened = directory(&moved)?;
    assert_eq!(native_identity(&reopened)?, expected);
    drop(reopened);

    let ordinary_name = StageName::generate()?;
    let ordinary_path = fixture.path().join(ordinary_name.as_str());
    std::fs::create_dir(&ordinary_path)?;
    let ordinary = directory(&ordinary_path)?;
    let ordinary_identity = native_identity(&ordinary)?;
    assert_eq!(
        StagePin::acquire(&parent, &ordinary_name, ordinary_identity)
            .err()
            .ok_or("Unsealed directory accepted.")?
            .kind(),
        io::ErrorKind::InvalidData
    );
    drop(ordinary);
    drop(parent);
    std::fs::remove_dir(&ordinary_path)?;
    std::fs::remove_dir(&moved)?;
    fixture.close()?;
    println!(
        "{{\"createdWithSealedPermissions\":true,\"independentReadPinAcquired\":true,\"collisionRejected\":true,\"wrongIdentityRejected\":true,\"unsealedDirectoryRejected\":true,\"childCreateError\":{child_error},\"securityChangeError\":{acl_error},\"normalAdmissionError\":{admission_error},\"deniedAdmissionPreservedSource\":true,\"ambiguousEntryNamesRejected\":true,\"unicodeEntryNamePreserved\":true,\"renameWhileConsumerPinnedError\":{rename_error},\"renameAfterReleaseSucceeded\":true,\"renamedIdentityPreserved\":true,\"ownedFixtureCleaned\":true}}"
    );
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The protected stage probe requires Windows.");
    std::process::exit(1);
}
