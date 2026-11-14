use super::{
    AllocationUnits, Bytes, CallerSpace, Measurement, Observation, Unavailable, VolumeIdentity,
};
use crate::{Error, Result, agents::pin_directory, windows};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Storage::FileSystem::*,
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
};

type QuerySpace = unsafe extern "system" fn(*const u16, *mut DISK_SPACE_INFORMATION) -> i32;

pub(super) fn observe(path: &Path, expected: Option<(u64, u64)>) -> Result<Observation> {
    let pin = pin_directory(path)?;
    if expected.is_some_and(|identity| identity != pin.identity()) {
        return Err(Error::Invalid("Volume scope changed.".into()));
    }
    let file = windows::open_metadata(pin.path())?;
    let metadata = file.metadata()?;
    if windows::is_reparse(&metadata)
        || windows::requires_recall(&metadata)
        || windows::file_identity(&file)?.key != pin.identity()
    {
        return Err(Error::Invalid("Volume scope changed.".into()));
    }
    let (native_path, guid_root) = windows::volume_paths(&file)?;
    let guid_root = guid_root.to_string_lossy().into_owned();
    let identity = VolumeIdentity {
        guid_root: guid_root.clone(),
        serial: format!("{:016x}", pin.identity().0),
    };
    let full = windows::wide(Path::new(&native_path));
    let root = windows::wide(Path::new(&guid_root));
    let now = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|time| time.as_secs())
    };
    let (mut caller_total, mut caller_available) = (0, 0);
    let caller = if unsafe {
        GetDiskFreeSpaceExW(
            full.as_ptr(),
            &mut caller_available,
            &mut caller_total,
            std::ptr::null_mut(),
        )
    } != 0
        && caller_available <= caller_total
    {
        Some(CallerSpace {
            volume: identity.clone(),
            observed_at: now(),
            total: Bytes(caller_total),
            available: Bytes(caller_available),
        })
    } else {
        None
    };

    // Kernel32 lives for the process lifetime. Optional lookup preserves startup on pre-17763 Windows.
    let kernel = unsafe { GetModuleHandleW(windows::wide(Path::new("kernel32.dll")).as_ptr()) };
    let symbol = if kernel.is_null() {
        None
    } else {
        unsafe { GetProcAddress(kernel, c"GetDiskSpaceInformationW".as_ptr().cast()) }
    };
    let result = if let Some(symbol) = symbol {
        // The signature and SDK-owned structure match GetDiskSpaceInformationW exactly.
        let query: QuerySpace = unsafe { std::mem::transmute(symbol) };
        let mut info = DISK_SPACE_INFORMATION::default();
        if unsafe { query(root.as_ptr(), &mut info) } == 0 {
            let unit_bytes =
                u64::from(info.SectorsPerAllocationUnit) * u64::from(info.BytesPerSector);
            let units = AllocationUnits {
                total: info.ActualTotalAllocationUnits,
                free: info.ActualAvailableAllocationUnits,
                used: info.UsedAllocationUnits,
                caller_total: info.CallerTotalAllocationUnits,
                caller_available: info.CallerAvailableAllocationUnits,
                reserved: info.TotalReservedAllocationUnits,
                storage_reserve: info.VolumeStorageReserveAllocationUnits,
                pool_unavailable: info.ActualPoolUnavailableAllocationUnits,
            };
            match Measurement::from_units(identity, now(), unit_bytes, units) {
                Ok(measurement) => Observation::Measured { measurement },
                Err(_) => Observation::Unavailable {
                    reason: Unavailable::InvalidCounters,
                    caller,
                },
            }
        } else {
            Observation::Unavailable {
                reason: Unavailable::NativeFailure,
                caller,
            }
        }
    } else {
        Observation::Unavailable {
            reason: Unavailable::Unsupported,
            caller,
        }
    };
    let current = pin_directory(pin.path())?;
    if current.identity() != pin.identity()
        || current.ancestor_identities()? != pin.ancestor_identities()?
        || windows::file_identity(&file)?.key != pin.identity()
    {
        return Err(Error::Invalid(
            "Volume scope changed during observation.".into(),
        ));
    }
    Ok(result)
}
