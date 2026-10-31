#[cfg(windows)]
mod probe {
    use std::{
        fs::File,
        io::{self, Read, Seek, Write},
        os::windows::{
            fs::OpenOptionsExt,
            io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
        },
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        ptr,
    };
    use voldiq_core::{
        cleanup::{Plan, helper::Session, journal::Journal},
        windows::{
            cleanup_channel::{PeerElevation, Server, current_elevation},
            cleanup_stage::{StageEntry, StageIdentity, StageName, StagePin},
            wide,
        },
    };
    use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*, System::Threading::*};

    fn process(id: u32) -> io::Result<OwnedHandle> {
        let access = PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE;
        let handle = unsafe { OpenProcess(access, 0, id) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    fn wait(process: BorrowedHandle<'_>) -> io::Result<u32> {
        match unsafe { WaitForSingleObject(process.as_raw_handle(), 10000) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Owned helper remains live; do not restart.",
                ));
            }
            _ => return Err(io::Error::last_os_error()),
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(code)
    }

    fn child(server: &Server) -> io::Result<Child> {
        use std::os::windows::process::CommandExt;
        Command::new(std::env::current_exe()?)
            .args(["helper", server.name(), &std::process::id().to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }

    fn helper(name: &str, parent: u32) -> Result<(), Box<dyn std::error::Error>> {
        voldiq_core::cleanup::helper::serve(name, parent, current_elevation()?)?;
        Ok(())
    }

    fn identity(file: &File) -> io::Result<StageIdentity> {
        let mut id: FILE_ID_INFO = unsafe { std::mem::zeroed() };
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
        Ok(StageIdentity {
            volume: id.VolumeSerialNumber,
            file: id.FileId.Identifier,
        })
    }

    fn plan(path: &Path) -> Result<(Plan, voldiq_core::NodeId), Box<dyn std::error::Error>> {
        let index = voldiq_core::scanner::scan(
            path,
            &voldiq_core::ScanOptions::default(),
            &std::sync::atomic::AtomicBool::new(false),
            |_| {},
        )?;
        let id = index
            .entries
            .iter()
            .find(|entry| entry.name == "owned.bin")
            .ok_or("Missing fixture source.")?
            .id;
        Ok((Plan::manual(&index, &[id])?, id))
    }

    fn replacement(path: &Path, share: u32) -> io::Result<File> {
        let name = wide(path);
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                DELETE | FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES,
                share,
                ptr::null(),
                CREATE_NEW,
                FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut file = unsafe { File::from_raw_handle(handle) };
        file.write_all(b"owned replacement")?;
        Ok(file)
    }

    fn remove_replacement(mut file: File) -> Result<(), Box<dyn std::error::Error>> {
        file.rewind()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if bytes != b"owned replacement" {
            return Err("Owned replacement changed.".into());
        }
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        drop(file);
        Ok(())
    }

    fn wrong_role() -> Result<(), Box<dyn std::error::Error>> {
        let server = Server::create()?;
        let mut child = child(&server)?;
        let mut channel = server.accept(child.as_handle())?;
        if channel
            .require_peer(PeerElevation::Elevated)
            .err()
            .ok_or("Ordinary helper accepted as elevated.")?
            .kind()
            != io::ErrorKind::PermissionDenied
        {
            return Err("Unexpected role rejection.".into());
        }
        if channel.receive().unwrap_err().kind() != io::ErrorKind::NotConnected {
            return Err("Rejected channel remained usable.".into());
        }
        if wait(child.as_handle())? == 0 {
            return Err("Rejected helper unexpectedly completed.".into());
        }
        child.wait()?;
        Ok(())
    }

    fn scenario(
        root: &Path,
        external: bool,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let source_path = root.join("owned.bin");
        let mut writer = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&source_path)?;
        writer.write_all(&[1, 2, 3, 4])?;
        let original = identity(&writer)?;
        drop(writer);
        let (planned, id) = plan(root)?;
        let source = planned.verify(id)?;
        let parent = source.stage_parent()?;
        let name = StageName::generate()?;
        let journal_path = root.join("admission.sqlite");
        let mut journal = Journal::open(&journal_path)?;
        let relocation = journal.prepare_relocation(&planned, id, &source, &name)?;
        let mut stage = StagePin::create(&parent, &name)?;
        journal.relocation_stage_ready(relocation, stage.identity())?;
        if source
            .prepare_recycling(&stage, StageEntry::parse("owned.bin")?)
            .is_ok()
        {
            return Err("Unadmitted source was accepted for recycling.".into());
        }
        let source = planned.verify(id)?;
        voldiq_core::cleanup::recycle::ShellApartment::new()?.check_stage(&stage)?;
        let image = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(std::env::current_exe()?)?;
        let server = Server::create()?;
        let mut ordinary = None;
        let external_process;
        let peer = if external {
            println!(
                "{}",
                serde_json::json!({"pipe":server.name(), "parent":std::process::id()})
            );
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            external_process = process(line.trim().parse()?)?;
            external_process.as_handle()
        } else {
            ordinary = Some(child(&server)?);
            ordinary
                .as_ref()
                .ok_or("Missing owned helper.")?
                .as_handle()
        };
        let mut session = Session::connect(
            server,
            peer,
            if external {
                PeerElevation::Elevated
            } else {
                PeerElevation::Ordinary
            },
            image,
        )?;
        let entry = StageEntry::parse("owned.bin")?;
        journal.start_relocation(relocation)?;
        let reply = session.admit(&source, &parent, &name, stage.identity(), &entry)?;
        let replay_rejected = external
            && session
                .admit(&source, &parent, &name, stage.identity(), &entry)
                .is_err();
        let staged_path = root.join(name.as_str()).join(entry.as_str());
        if external {
            if !reply.admitted || !replay_rejected {
                return Err("Elevated helper did not admit exactly once.".into());
            }
            if source_path.exists()
                || std::fs::read(&staged_path)? != [1, 2, 3, 4]
                || identity(&File::open(&staged_path)?)? != original
            {
                return Err("Admission changed source identity or contents.".into());
            }
            journal.record_staged(relocation)?;
            journal.start_return(relocation)?;
            let replaced = replacement(&source_path, FILE_SHARE_READ)?;
            let error = source
                .return_from_stage(&mut stage, &entry, &entry)
                .err()
                .ok_or("Rollback replaced an occupied name.")?;
            if !matches!(error, voldiq_core::Error::Io(ref io) if io.kind() == io::ErrorKind::AlreadyExists)
            {
                return Err(error.into());
            }
            if std::fs::read(&source_path)? != b"owned replacement"
                || std::fs::read(&staged_path)? != [1, 2, 3, 4]
            {
                return Err("Collision changed fixture contents.".into());
            }
            remove_replacement(replaced)?;
            source.return_from_stage(&mut stage, &entry, &entry)?;
        } else if reply.admitted || reply.native_error != Some(5) {
            return Err("Ordinary helper was not denied native admission.".into());
        }
        if std::fs::read(&source_path)? != [1, 2, 3, 4]
            || identity(&File::open(&source_path)?)? != original
            || staged_path.exists()
        {
            return Err("Original fixture was not preserved/restored.".into());
        }
        if external {
            journal.record_returned(relocation)?;
        } else {
            journal.record_unmoved(relocation)?;
        }
        drop(journal);
        let recovered = Journal::open(&journal_path)?;
        let records = recovered.relocations(0, 100)?;
        if records.len() != 1
            || records[0].source != original
            || records[0].path != source.path().to_str().ok_or("Inexact fixture path.")?
            || records[0].stage_name != name.as_str()
            || records[0].stage_file != Some(stage.identity().file)
            || records[0].state != if external { "returned" } else { "unmoved" }
        {
            return Err("Durable relocation evidence did not survive restart.".into());
        }
        drop(recovered);
        let metadata = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL)
            .open(&source_path)?;
        drop(source);
        if unsafe { WaitForSingleObject(peer.as_raw_handle(), 0) } != WAIT_TIMEOUT {
            return Err("Helper exited before its handle-release check.".into());
        }
        let reopened = unsafe {
            ReOpenFile(
                metadata.as_raw_handle(),
                DELETE | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ,
                FILE_FLAG_OPEN_REPARSE_POINT,
            )
        };
        if reopened == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        let release_probe = unsafe { File::from_raw_handle(reopened) };
        if identity(&release_probe)? != original {
            return Err("Release check changed source identity.".into());
        }
        drop(release_probe);
        drop(metadata);
        session.finish()?;
        if let Some(mut child) = ordinary {
            child.wait()?;
        }
        let (fresh, id) = plan(root)?;
        fresh.verify(id)?.remove_permanently()?;
        let other_name = StageName::generate()?;
        let other_path = root.join(other_name.as_str());
        std::fs::create_dir(&other_path)?;
        let marker_path = other_path.join("owned.bin");
        let marker = replacement(&marker_path, FILE_SHARE_READ | FILE_SHARE_DELETE)?;
        let expected_stage = stage.identity();
        let error = stage
            .remove_empty(&other_name)
            .err()
            .ok_or("Wrong stage name was removed.")?;
        if error.kind() != io::ErrorKind::InvalidData
            || std::fs::read(&marker_path)? != b"owned replacement"
        {
            return Err("Stage cleanup did not preserve the wrong directory.".into());
        }
        StagePin::acquire(&parent, &name, expected_stage)?.remove_empty(&name)?;
        remove_replacement(marker)?;
        std::fs::remove_dir(&other_path)?;
        drop(parent);
        std::fs::remove_file(&journal_path)?;
        std::fs::remove_dir(root)?;
        Ok(
            serde_json::json!({"helperElevated":external,"parentAuthorityReopenedAndNarrowed":true,"emptyStageRemovedByVerifiedHandle":true,"wrongCleanupTargetPreserved":true,"productionAdmissionProtocolVerified":true,"helperReleasedStrictSourceWhileLive":true,"unadmittedSourceRejectedForRecycling":true,"shellApartmentInitialized":true,"protectedStageShellBindingVerified":true,"journalIntentAndResultSurvivedRestart":true,"sameUserImageAndRoleVerified":true,"nativeAdmissionSucceeded":reply.admitted,"nativeError":reply.native_error,
            "reviewedSourceReplayRejected":replay_rejected,"rollbackCollisionPreservedBothFiles":external,"returnedByOrdinaryController":external,"originalIdentityAndContentsVerified":true,"ownedFixtureCleaned":true}),
        )
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<String> = std::env::args().collect();
        if args.len() == 4 && args[1] == "helper" {
            return helper(&args[2], args[3].parse()?);
        }
        let external = args.len() == 2 && args[1] == "--external-helper";
        if args.len() != 1 && !external {
            return Err("Unexpected fixture arguments.".into());
        }
        if current_elevation()? != PeerElevation::Ordinary {
            return Err("Controller must use its ordinary Windows token.".into());
        }
        let _image_pin = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(std::env::current_exe()?)?;
        wrong_role()?;
        let root: PathBuf = tempfile::Builder::new()
            .prefix("voldiq-admission-")
            .tempdir()?
            .keep();
        match scenario(&root, external) {
            Ok(result) => {
                println!("{result}");
                Ok(())
            }
            Err(error) => {
                eprintln!("Owned fixture retained for review: {}", root.display());
                Err(error)
            }
        }
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(code) = voldiq_core::cleanup::helper::run_cli() {
        std::process::exit(code);
    }
    probe::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This fixture requires Windows.");
    std::process::exit(1);
}
