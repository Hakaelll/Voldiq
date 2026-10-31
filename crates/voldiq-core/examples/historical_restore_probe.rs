#[cfg(windows)]
mod probe {
    use ::windows::{
        Win32::{
            System::Com::CoTaskMemFree,
            UI::Shell::{
                BHID_Transfer, FOLDERID_RecycleBinFolder, IShellItem, ITransferSource,
                KF_FLAG_DEFAULT, SHCreateItemFromParsingName, SHGetKnownFolderItem,
                SIGDN_FILESYSPATH,
            },
        },
        core::PCWSTR,
    };
    use std::{
        fs::{File, OpenOptions},
        os::windows::{ffi::OsStringExt, fs::OpenOptionsExt, io::AsRawHandle},
        path::{Path, PathBuf},
        sync::atomic::AtomicBool,
    };
    use voldiq_core::{
        NodeId, ScanOptions,
        cleanup::{
            Plan,
            journal::{Journal, relocation::RecycledVersion},
            recycle::{RestorationReview, ShellApartment},
        },
        windows::{
            self,
            cleanup_channel::{PeerElevation, current_elevation},
            cleanup_stage::{StageIdentity, StageName},
        },
    };
    use windows_sys::Win32::Storage::FileSystem::*;

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    fn identity(file: &File) -> Result<StageIdentity> {
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
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(StageIdentity {
            volume: id.VolumeSerialNumber,
            file: id.FileId.Identifier,
        })
    }

    fn shell_item(path: &Path) -> Result<IShellItem> {
        let name = windows::wide(path);
        Ok(unsafe { SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None) }?)
    }

    fn locator(item: &IShellItem) -> Result<PathBuf> {
        let text = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }?;
        let path = unsafe { std::ffi::OsString::from_wide(text.as_wide()) };
        unsafe { CoTaskMemFree(Some(text.0.cast())) };
        Ok(path.into())
    }

    fn plan(root: &Path) -> Result<(Plan, NodeId)> {
        let index = voldiq_core::scanner::scan(
            root,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )?;
        let node = index
            .entries
            .iter()
            .find(|entry| entry.name == "owned.bin")
            .ok_or("Missing owned source")?
            .id;
        Ok((Plan::manual(&index, &[node])?, node))
    }

    pub fn run() -> Result<()> {
        if current_elevation()? != PeerElevation::Ordinary {
            return Err("Run this fixture with an ordinary Windows token.".into());
        }
        let root = tempfile::Builder::new()
            .prefix("voldiq-historical-bin-")
            .tempdir()?
            .keep();
        eprintln!("Owned fixture preserved on failure: {}", root.display());
        let original = root.join("owned.bin");
        std::fs::write(&original, b"own!")?;
        let (plan, node) = plan(&root)?;
        let verified = plan.verify(node)?;
        let database = root.join("cleanup.sqlite");
        let mut journal = Journal::open(&database)?;
        let id = journal.prepare_relocation(&plan, node, &verified, &StageName::generate()?)?;
        let record = journal
            .relocation(id.value())?
            .ok_or("Missing prepared receipt")?;
        let guard = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL)
            .open(&original)?;
        assert_eq!(identity(&guard)?, record.source);
        let apartment = ShellApartment::new()?;
        let parent = shell_item(&root)?;
        let transfer: ITransferSource = unsafe { parent.BindToHandler(None, &BHID_Transfer) }?;
        let bin: IShellItem =
            unsafe { SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None) }?;
        let item = shell_item(&original)?;
        // The ordinary owned source directory substitutes for admission only in this recovery fixture.
        journal.relocation_stage_ready(id, record.parent)?;
        journal.start_relocation(id)?;
        journal.record_staged(id)?;
        journal.start_recycling(id, record.source, record.parent)?;
        drop(verified);
        let recycled = unsafe { transfer.RecycleItem(&item, &bin, 0) }?;
        let bin_path = locator(&recycled)?;
        eprintln!("Owned recycled locator: {}", bin_path.display());
        let in_bin = File::open(&bin_path)?;
        assert_eq!(identity(&in_bin)?, record.source);
        assert_eq!(identity(&guard)?, record.source);
        assert_eq!(std::fs::read(&bin_path)?, b"own!");
        assert!(!original.exists());
        let metadata = guard.metadata()?;
        let version = RecycledVersion {
            change_time: windows::file_identity(&guard)?.change_time,
            size: metadata.len(),
            modified: metadata.modified()?,
        };
        journal.record_recycled(id, record.source, &bin_path, version)?;
        drop(in_bin);
        drop(guard);
        drop(recycled);
        drop(item);
        drop(bin);
        drop(transfer);
        drop(parent);
        drop(apartment);
        drop(journal);

        let mut journal = Journal::open(&database)?;
        assert_eq!(
            journal.relocation(id.value())?.unwrap().recycled_version,
            Some(version)
        );
        let review = RestorationReview::prepare(&journal, id.value())?;
        assert_eq!(review.size(), 4);
        assert_eq!(review.destination(), Path::new(&record.path));
        assert!(std::fs::write(&bin_path, b"fail").is_err());
        std::fs::write(&original, b"keep")?;
        assert!(review.restore(&mut journal).is_err());
        assert_eq!(std::fs::read(&original)?, b"keep");
        assert_eq!(std::fs::read(&bin_path)?, b"own!");
        assert_eq!(journal.relocation(id.value())?.unwrap().state, "recycled");
        std::fs::remove_file(&original)?;
        let restored = RestorationReview::prepare(&journal, id.value())?.restore(&mut journal)?;
        assert!(restored.recorded());
        assert_eq!(restored.identity(), record.source);
        assert_eq!(identity(&File::open(&original)?)?, record.source);
        assert_eq!(std::fs::read(&original)?, b"own!");
        assert!(!bin_path.exists());
        assert!(RestorationReview::prepare(&journal, id.value()).is_err());
        drop(journal);
        let journal = Journal::open(&database)?;
        let receipt = journal
            .relocation(id.value())?
            .ok_or("Missing restored receipt")?;
        assert_eq!(receipt.state, "restored");
        assert_eq!(
            receipt.restore_path.as_deref(),
            Some(Path::new(&record.path))
        );
        assert!(RestorationReview::prepare(&journal, id.value()).is_err());
        drop(journal);
        let (cleanup, node) = self::plan(&root)?;
        cleanup.verify(node)?.remove_permanently()?;
        std::fs::remove_file(database)?;
        std::fs::remove_dir(&root)?;
        println!(
            "{}",
            serde_json::json!({
                "reopenedBinRestored": true, "observedVersionSurvivedRestart": true,
                "collisionPreservedBoth": true, "sourceWritesDeniedDuringReview": true,
                "originalIdentityAndBytesRestored": true, "durableRestorationSurvivedRestart": true,
                "replayRejected": true, "ownedFixtureRemoved": true,
                "uac": false, "protectedAdmissionExercised": false,
                "physicalBinMetadataRetirementClaimed": false,
                "binLocator": bin_path, "sourceVolume": record.source.volume,
                "sourceFile": record.source.file
            })
        );
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    probe::run()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows-only historical restoration fixture");
}
