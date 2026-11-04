use super::*;
use crate::{Error, metadata, windows};
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::{
    Storage::FileSystem::*,
    System::{
        SystemServices::FILE_READ_ONLY_VOLUME,
        WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE},
    },
};

pub(super) fn volume(pin: &AuthorizedPath) -> Result<VolumeBinding> {
    let file = windows::open_metadata(pin.path())?;
    let info = file.metadata()?;
    if !info.is_dir()
        || windows::is_reparse(&info)
        || windows::requires_recall(&info)
        || windows::file_identity(&file)?.key != pin.identity()
    {
        return Err(invalid());
    }
    let (_, root) = windows::volume_paths(&file)?;
    Ok(VolumeBinding {
        guid_root: root.to_str().ok_or_else(invalid)?.to_ascii_lowercase(),
        serial: format!("{:016x}", pin.identity().0),
    })
}

impl Destination {
    pub(in crate::plans) fn observe(pin: &AuthorizedPath, state: &Path) -> Result<Self> {
        let ancestors = pin.ancestor_identities()?;
        let system = crate::cleanup::protected_identities()?;
        let state = crate::agents::pin_directory(state)?;
        metadata::Store::open(state.path())?
            .protections()?
            .check_evidence(pin.path(), pin.identity(), &ancestors)?;
        if crate::cleanup::record::sensitive(pin.path())
            || ancestors
                .iter()
                .any(|key| *key == state.identity() || system.contains(key))
        {
            return Err(invalid());
        }
        let binding = volume(pin)?;
        let file = windows::open_metadata(pin.path())?;
        let _access = windows::cleanup_io::reopen_directory(
            &file,
            FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | FILE_ADD_FILE | FILE_TRAVERSE,
        )?;
        if windows::file_identity(&file)?.key != pin.identity() {
            return Err(invalid());
        }
        let mut filesystem = [0_u16; 261];
        let (mut flags, mut maximum) = (0, 0);
        // The handle, not a drive-letter lookup, supplies filesystem compatibility.
        if unsafe {
            GetVolumeInformationByHandleW(
                file.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut maximum,
                &mut flags,
                filesystem.as_mut_ptr(),
                filesystem.len() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let end = filesystem
            .iter()
            .position(|c| *c == 0)
            .ok_or_else(invalid)?;
        let filesystem = String::from_utf16(&filesystem[..end])
            .map_err(|_| invalid())?
            .to_ascii_uppercase();
        let root = windows::wide(Path::new(&binding.guid_root));
        if !matches!(filesystem.as_str(), "NTFS" | "EXFAT" | "FAT32")
            || flags & FILE_READ_ONLY_VOLUME != 0
            || !matches!(
                unsafe { GetDriveTypeW(root.as_ptr()) },
                DRIVE_FIXED | DRIVE_REMOVABLE
            )
        {
            return Err(Error::Invalid(
                "Transfer destination filesystem or volume type is unsupported or read-only."
                    .into(),
            ));
        }
        let (mut sectors, mut bytes, mut free, mut total) = (0, 0, 0, 0);
        if unsafe {
            GetDiskFreeSpaceW(
                root.as_ptr(),
                &mut sectors,
                &mut bytes,
                &mut free,
                &mut total,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let allocation_unit_bytes = u64::from(sectors)
            .checked_mul(u64::from(bytes))
            .ok_or_else(invalid)?;
        let (available, observed_at, observed_volume) =
            match crate::space::observe_volume(pin.path()) {
                crate::space::Observation::Measured { measurement } => (
                    measurement.caller_available.0,
                    measurement.observed_at,
                    measurement.volume,
                ),
                crate::space::Observation::Unavailable {
                    caller: Some(caller),
                    ..
                } => (caller.available.0, caller.observed_at, caller.volume),
                _ => {
                    return Err(Error::Invalid(
                        "Transfer destination capacity is unavailable.".into(),
                    ));
                }
            };
        if observed_volume.guid_root.to_ascii_lowercase() != binding.guid_root
            || observed_volume.serial != binding.serial
            || allocation_unit_bytes == 0
            || allocation_unit_bytes > 16 * 1024 * 1024
            || maximum == 0
            || maximum > 255
        {
            return Err(invalid());
        }
        let fresh = crate::agents::pin_directory(pin.path())?;
        if fresh.identity_bytes()? != pin.identity_bytes()?
            || fresh.ancestor_identities()? != ancestors
        {
            return Err(invalid());
        }
        Ok(Self {
            path: pin.path().to_owned(),
            identity: identity(pin.identity()),
            native_identity: pin
                .identity_bytes()?
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            ancestors: ancestors.into_iter().map(identity).collect(),
            volume: binding,
            filesystem,
            filesystem_flags: flags,
            maximum_name_units: maximum,
            allocation_unit_bytes,
            caller_available_bytes: available.to_string(),
            observed_at: observed_at.ok_or_else(invalid)?,
        })
    }
}
