#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        fs::File,
        io::{self, Read, Write},
        os::windows::{
            io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
            process::CommandExt,
        },
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    use voldiq_core::windows::cleanup_channel::{Channel, MAX_MESSAGE, Server};
    use voldiq_core::windows::cleanup_stage::{StageIdentity, StageName, StagePin};
    use windows_sys::Win32::{
        Foundation::{
            ERROR_BROKEN_PIPE, ERROR_MORE_DATA, ERROR_NO_DATA, GENERIC_READ, GENERIC_WRITE,
            INVALID_HANDLE_VALUE,
        },
        Storage::FileSystem::{
            CREATE_NEW, CreateFileW, DELETE, FILE_ADD_SUBDIRECTORY, FILE_DELETE_CHILD,
            FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_NO_RECALL,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_OVERLAPPED, FILE_ID_INFO, FILE_LIST_DIRECTORY,
            FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_SHARE_WRITE,
            FileDispositionInfo, FileIdInfo, GetFileInformationByHandleEx, OPEN_EXISTING,
            READ_CONTROL, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, SYNCHRONIZE,
            SetFileInformationByHandle, WriteFile,
        },
        System::{
            Pipes::GetNamedPipeServerProcessId,
            Threading::{
                CREATE_NO_WINDOW, OpenProcess, PROCESS_DUP_HANDLE,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };

    fn source_frame(file: &File) -> io::Result<Vec<u8>> {
        use windows_sys::{
            Wdk::Foundation::{NtQueryObject, ObjectBasicInformation},
            Win32::System::WindowsProgramming::PUBLIC_OBJECT_BASIC_INFORMATION,
        };
        let mut id = FILE_ID_INFO::default();
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut basic: PUBLIC_OBJECT_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let mut returned = 0;
        if unsafe {
            NtQueryObject(
                file.as_raw_handle(),
                ObjectBasicInformation,
                (&mut basic as *mut PUBLIC_OBJECT_BASIC_INFORMATION).cast(),
                size_of::<PUBLIC_OBJECT_BASIC_INFORMATION>() as u32,
                &mut returned,
            )
        } < 0
            || returned < 8
        {
            return Err(io::Error::other("Cannot inspect fixture handle authority."));
        }
        let mut frame = b"VDQSRC01".to_vec();
        frame.extend_from_slice(&(file.as_raw_handle() as usize as u64).to_le_bytes());
        frame.extend_from_slice(&id.VolumeSerialNumber.to_le_bytes());
        frame.extend_from_slice(&id.FileId.Identifier);
        frame.extend_from_slice(&basic.GrantedAccess.to_le_bytes());
        Ok(frame)
    }

    fn process(id: u32) -> io::Result<OwnedHandle> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_DUP_HANDLE | SYNCHRONIZE,
                0,
                id,
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
    fn spawn(role: &str, name: &str, server: u32) -> io::Result<Child> {
        Command::new(std::env::current_exe()?)
            .args([role, name, &server.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
    }
    fn stage_parent(path: &std::path::Path, writable: bool) -> io::Result<File> {
        let name = voldiq_core::windows::wide(path);
        let mut access = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | READ_CONTROL;
        if writable {
            access |= FILE_ADD_SUBDIRECTORY | FILE_DELETE_CHILD;
        }
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS
                    | FILE_FLAG_OPEN_REPARSE_POINT
                    | FILE_FLAG_OPEN_NO_RECALL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    }
    fn finish(mut child: Child) -> Result<String, Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(8);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                return Err("Owned probe child timed out.".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut output = String::new();
        child
            .stdout
            .take()
            .ok_or("Missing child stdout.")?
            .read_to_string(&mut output)?;
        if !status.success() {
            return Err(format!("Owned child failed: {status}").into());
        }
        Ok(output)
    }
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() == 4 {
        let role = arguments[1].as_str();
        if role == "idle" {
            io::stdin().read_to_end(&mut Vec::new())?;
            return Ok(());
        }
        if role == "oversize" {
            let name = voldiq_core::windows::wide(&std::path::PathBuf::from(&arguments[2]));
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    std::ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error().into());
            }
            let pipe = unsafe { File::from_raw_handle(handle) };
            let mut server_pid = 0;
            if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut server_pid) } == 0 {
                return Err(io::Error::last_os_error().into());
            }
            assert_eq!(server_pid, arguments[3].parse::<u32>()?);
            let bytes = vec![42; MAX_MESSAGE + 1];
            let mut written = 0;
            if unsafe {
                WriteFile(
                    pipe.as_raw_handle(),
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                assert!(
                    [Some(ERROR_BROKEN_PIPE as i32), Some(ERROR_NO_DATA as i32)]
                        .contains(&io::Error::last_os_error().raw_os_error())
                );
            } else {
                assert_eq!(written as usize, bytes.len());
            }
            io::stdin().read_to_end(&mut Vec::new())?;
            println!("oversized_frame_completed_or_peer_closed");
            return Ok(());
        }
        let parent = process(arguments[3].parse()?)?;
        let result = Channel::connect(&arguments[2], parent.as_handle());
        if role == "wrong-server" {
            assert_eq!(
                result.err().ok_or("Unexpected server accepted.")?.kind(),
                io::ErrorKind::PermissionDenied
            );
            println!("wrong_server_rejected");
            return Ok(());
        }
        let mut channel = result?;
        match role {
            "accept-raw-source" => {
                let source = channel.receive_source()?;
                assert_eq!(source.metadata()?.len(), 11);
                channel.send(b"raw-source-accepted")?;
            }
            "reject-malformed-source" => {
                assert_eq!(
                    channel
                        .receive_source()
                        .err()
                        .ok_or("Malformed source accepted.")?
                        .kind(),
                    io::ErrorKind::InvalidData
                );
                assert_eq!(
                    channel.receive().unwrap_err().kind(),
                    io::ErrorKind::NotConnected
                );
                println!("malformed_source_rejected");
            }
            "echo" => {
                assert_eq!(channel.receive()?, vec![42; MAX_MESSAGE]);
                channel.send(b"ready")?;
            }
            "rejected" => {
                assert!(channel.receive().is_err());
                println!("disconnected_without_message");
            }
            "silent" => {
                io::stdin().read_to_end(&mut Vec::new())?;
            }
            "source" => {
                let mut file = channel.receive_source()?;
                assert_eq!(
                    file.write_all(b"x").unwrap_err().kind(),
                    io::ErrorKind::PermissionDenied
                );
                channel.send(b"held")?;
                assert_eq!(channel.receive()?, b"release");
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                assert_eq!(bytes, b"owned source transfer");
                drop(file);
                channel.send(b"released")?;
            }
            "source-metadata" => {
                let mut source = channel.receive_source()?;
                assert_eq!(
                    source.read_to_end(&mut Vec::new()).unwrap_err().kind(),
                    io::ErrorKind::PermissionDenied
                );
                channel.send(b"metadata-only")?;
            }
            "reject-source" => {
                assert_eq!(
                    channel
                        .receive_source()
                        .err()
                        .ok_or("Invalid source accepted.")?
                        .kind(),
                    io::ErrorKind::PermissionDenied
                );
                assert_eq!(
                    channel.receive().unwrap_err().kind(),
                    io::ErrorKind::NotConnected
                );
                println!("readonly_source_rejected");
            }
            "stage-parent" => {
                let parent = channel.receive_stage_parent()?;
                let name = channel.receive()?;
                let name = StageName::parse(std::str::from_utf8(&name)?)?;
                let evidence = channel.receive()?;
                assert_eq!(evidence.len(), 24);
                let expected = StageIdentity {
                    volume: u64::from_le_bytes(evidence[..8].try_into()?),
                    file: evidence[8..].try_into()?,
                };
                let pin = StagePin::acquire(&parent, &name, expected)?;
                channel.send(b"parent-held")?;
                assert_eq!(channel.receive()?, b"release-parent");
                drop(pin);
                drop(parent);
                channel.send(b"parent-released")?;
            }
            _ => return Err("Unknown probe role.".into()),
        }
        return Ok(());
    }
    if arguments.len() != 1 {
        return Err("Unexpected arguments.".into());
    }

    let server = Server::create()?;
    let child = spawn("echo", server.name(), std::process::id())?;
    let mut channel = server.accept(child.as_handle())?;
    assert_eq!(
        channel.send(&[]).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        channel.send(&vec![0; MAX_MESSAGE + 1]).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    channel.send(&vec![42; MAX_MESSAGE])?;
    assert_eq!(channel.receive()?, b"ready");
    finish(child)?;
    drop(channel);

    let server = Server::create()?;
    let mut attacker = spawn("oversize", server.name(), std::process::id())?;
    let mut channel = server.accept(attacker.as_handle())?;
    assert_eq!(
        channel.receive().unwrap_err().raw_os_error(),
        Some(ERROR_MORE_DATA as i32)
    );
    assert_eq!(
        channel.receive().unwrap_err().kind(),
        io::ErrorKind::NotConnected
    );
    attacker.stdin.take();
    assert_eq!(
        finish(attacker)?.trim(),
        "oversized_frame_completed_or_peer_closed"
    );

    let frame_fixture = tempfile::tempdir()?;
    let frame_path = frame_fixture.path().join("frame.bin");
    std::fs::write(&frame_path, b"frame-proof")?;
    use std::os::windows::fs::OpenOptionsExt;
    let frame_source = std::fs::OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL)
        .open(&frame_path)?;
    let valid_frame = source_frame(&frame_source)?;
    let server = Server::create()?;
    let child = spawn("accept-raw-source", server.name(), std::process::id())?;
    let mut channel = server.accept(child.as_handle())?;
    channel.send(&valid_frame)?;
    let mut acknowledgement = b"VDQACK01".to_vec();
    acknowledgement.extend_from_slice(&valid_frame[16..]);
    assert_eq!(channel.receive()?, acknowledgement);
    assert_eq!(channel.receive()?, b"raw-source-accepted");
    finish(child)?;
    for kind in ["truncated", "wrong-kind", "zero-handle", "negative-handle"] {
        let mut frame = valid_frame.clone();
        match kind {
            "truncated" => {
                frame.pop();
            }
            "wrong-kind" => frame[..8].copy_from_slice(b"VDQPAR01"),
            "zero-handle" => frame[8..16].fill(0),
            "negative-handle" => frame[8..16].copy_from_slice(&u64::MAX.to_le_bytes()),
            _ => unreachable!(),
        }
        let server = Server::create()?;
        let child = spawn("reject-malformed-source", server.name(), std::process::id())?;
        let mut channel = server.accept(child.as_handle())?;
        channel.send(&frame)?;
        assert!(channel.receive().is_err());
        assert_eq!(finish(child)?.trim(), "malformed_source_rejected");
        assert_eq!(source_frame(&frame_source)?, valid_frame);
        assert_eq!(std::fs::read(&frame_path)?, b"frame-proof");
    }
    drop(frame_source);
    frame_fixture.close()?;

    let server = Server::create()?;
    let mut expected = spawn("idle", "unused", std::process::id())?;
    let intruder = spawn("rejected", server.name(), std::process::id())?;
    assert_eq!(
        server
            .accept(expected.as_handle())
            .err()
            .ok_or("Wrong client accepted.")?
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    expected.stdin.take();
    finish(expected)?;
    assert_eq!(finish(intruder)?.trim(), "disconnected_without_message");

    let server = Server::create()?;
    let mut unrelated = spawn("idle", "unused", std::process::id())?;
    let peer = spawn("wrong-server", server.name(), unrelated.id())?;
    let _connection = server.accept(peer.as_handle());
    assert_eq!(finish(peer)?.trim(), "wrong_server_rejected");
    unrelated.stdin.take();
    finish(unrelated)?;
    drop(_connection);

    let server = Server::create()?;
    let mut silent = spawn("silent", server.name(), std::process::id())?;
    let mut channel = server.accept(silent.as_handle())?;
    assert_eq!(
        channel.receive().unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(
        channel.receive().unwrap_err().kind(),
        io::ErrorKind::NotConnected
    );
    silent.stdin.take();
    finish(silent)?;

    let fixture = tempfile::tempdir()?;
    let source = fixture.path().join("owned.bin");
    let moved = fixture.path().join("moved.bin");
    std::fs::write(&source, b"owned source transfer")?;
    let server = Server::create()?;
    let child = spawn("source", server.name(), std::process::id())?;
    let mut channel = server.accept(child.as_handle())?;
    let readonly = File::open(&source)?;
    assert_eq!(
        channel.offer_source(&readonly).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    drop(readonly);
    for (label, mode) in [
        ("pending-deletion.bin", 0),
        ("overlapped.bin", FILE_FLAG_OVERLAPPED),
    ] {
        let path = fixture.path().join(label);
        let wide = voldiq_core::windows::wide(&path);
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                DELETE | FILE_READ_DATA | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ,
                std::ptr::null(),
                CREATE_NEW,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL | mode,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        let candidate = unsafe { File::from_raw_handle(handle) };
        if mode == 0 {
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            if unsafe {
                SetFileInformationByHandle(
                    candidate.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error().into());
            }
        }
        assert_eq!(
            channel.offer_source(&candidate).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        if mode == FILE_FLAG_OVERLAPPED {
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            if unsafe {
                SetFileInformationByHandle(
                    candidate.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error().into());
            }
        }
        drop(candidate);
        assert!(!path.exists());
    }
    let wide = voldiq_core::windows::wide(&source);
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            DELETE | FILE_READ_DATA | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    let file = unsafe { File::from_raw_handle(raw) };
    channel.offer_source(&file)?;
    assert_eq!(channel.receive()?, b"held");
    drop(file);
    let rename_error = std::fs::rename(&source, &moved)
        .unwrap_err()
        .raw_os_error()
        .ok_or("Missing native rename error.")?;
    assert!([5, 32].contains(&rename_error));
    channel.send(b"release")?;
    assert_eq!(channel.receive()?, b"released");
    finish(child)?;
    std::fs::rename(&source, &moved)?;
    assert_eq!(std::fs::read(&moved)?, b"owned source transfer");

    let server = Server::create()?;
    let child = spawn("source-metadata", server.name(), std::process::id())?;
    let mut channel = server.accept(child.as_handle())?;
    let index = voldiq_core::scanner::scan(
        fixture.path(),
        &voldiq_core::ScanOptions::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )?;
    let id = index
        .entries
        .iter()
        .find(|entry| entry.name == "moved.bin")
        .ok_or("Missing fixture entry.")?
        .id;
    let plan = voldiq_core::cleanup::Plan::manual(&index, &[id])?;
    let metadata_only = plan.verify(id)?;
    metadata_only.offer_source(&mut channel)?;
    assert_eq!(channel.receive()?, b"metadata-only");
    finish(child)?;
    drop(metadata_only);

    let server = Server::create()?;
    let child = spawn("reject-source", server.name(), std::process::id())?;
    let mut channel = server.accept(child.as_handle())?;
    let mut readonly = File::open(&moved)?;
    let mut forged = vec![0u8; 44];
    forged[..8].copy_from_slice(b"VDQSRC01");
    forged[8..16].copy_from_slice(&(readonly.as_raw_handle() as usize as u64).to_le_bytes());
    channel.send(&forged)?;
    assert!(channel.receive().is_err());
    assert_eq!(finish(child)?.trim(), "readonly_source_rejected");
    let mut bytes = Vec::new();
    readonly.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"owned source transfer");
    drop(readonly);
    fixture.close()?;

    let stage_fixture = tempfile::tempdir()?;
    let parent_path = stage_fixture.path().join("parent");
    let parent_moved = stage_fixture.path().join("released-parent");
    std::fs::create_dir(&parent_path)?;
    let parent = stage_parent(&parent_path, true)?;
    let name = StageName::generate()?;
    let creator = StagePin::create(&parent, &name)?;
    let expected = creator.identity();
    let server = Server::create()?;
    let child = spawn("stage-parent", server.name(), std::process::id())?;
    let mut channel = server.accept(child.as_handle())?;
    let narrow = stage_parent(&parent_path, false)?;
    assert_eq!(
        channel.offer_stage_parent(&narrow).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    drop(narrow);
    assert_eq!(
        channel.offer_source(&parent).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    channel.offer_stage_parent(&parent)?;
    channel.send(name.as_str().as_bytes())?;
    let mut evidence = Vec::with_capacity(24);
    evidence.extend_from_slice(&expected.volume.to_le_bytes());
    evidence.extend_from_slice(&expected.file);
    channel.send(&evidence)?;
    assert_eq!(channel.receive()?, b"parent-held");
    drop(creator);
    drop(parent);
    let parent_rename_error = std::fs::rename(&parent_path, &parent_moved)
        .unwrap_err()
        .raw_os_error()
        .ok_or("Missing parent-rename error.")?;
    assert!([5, 32].contains(&parent_rename_error));
    let stage_path = parent_path.join(name.as_str());
    let stage_rename_error = std::fs::rename(&stage_path, parent_path.join("moved-stage"))
        .unwrap_err()
        .raw_os_error()
        .ok_or("Missing stage-rename error.")?;
    assert!([5, 32].contains(&stage_rename_error));
    channel.send(b"release-parent")?;
    assert_eq!(channel.receive()?, b"parent-released");
    finish(child)?;
    std::fs::rename(&parent_path, &parent_moved)?;
    std::fs::remove_dir(parent_moved.join(name.as_str()))?;
    std::fs::remove_dir(&parent_moved)?;
    stage_fixture.close()?;
    println!(
        "{{\"oversizedInboundRejected\":true,\"oversizedChannelClosed\":true,\"malformedSourceFramesRejected\":4,\"validRawSourceFrameAccepted\":true,\"senderSourceGuardPreservedAcrossMalformedFrames\":true,\"maximumMessageExchanged\":true,\"invalidOutboundSizesRejected\":true,\"wrongClientRejected\":true,\"wrongServerRejected\":true,\"idleReadCancelledAndClosed\":true,\"sourceHandleTransferred\":true,\"receiverWritesDenied\":true,\"senderClosePreservedReceiverGuard\":true,\"guardedRenameError\":{rename_error},\"receiverReleaseAllowedRename\":true,\"forgedReadonlyOfferRejected\":true,\"senderHandleSurvivedRejection\":true,\"pendingDeletionSourceRejected\":true,\"overlappedSourceRejected\":true,\"reviewedPlanSourceTransferred\":true,\"metadataOnlySourceTransferred\":true,\"stageParentTransferred\":true,\"narrowStageParentRejected\":true,\"directorySourceRoleRejected\":true,\"receiverPinnedStage\":true,\"parentRenameWhileReceiverHeldError\":{parent_rename_error},\"stageRenameWhileReceiverHeldError\":{stage_rename_error},\"receiverReleaseAllowedParentRename\":true,\"ownedFixtureCleaned\":true}}"
    );
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The cleanup channel probe requires Windows.");
    std::process::exit(1);
}
