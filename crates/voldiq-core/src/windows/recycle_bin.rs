use crate::{Error, Result};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Summary {
    pub drive: String,
    pub items: u64,
    pub size: u64,
}

fn checked_summary(drive: String, items: i64, size: i64) -> Result<Summary> {
    const MAX_EXACT: i64 = 9_007_199_254_740_991;
    if !(0..=MAX_EXACT).contains(&items) || !(0..=MAX_EXACT).contains(&size) {
        return Err(Error::Invalid("Invalid Recycle Bin totals.".into()));
    }
    Ok(Summary {
        drive,
        items: items as u64,
        size: size as u64,
    })
}

pub fn query(drive: &str) -> Result<Summary> {
    let volume = super::volumes()
        .into_iter()
        .find(|volume| volume.path == drive && volume.accessible && volume.drive_type == "Local")
        .ok_or_else(|| Error::Invalid("Select an accessible local drive.".into()))?;
    query_native(volume.path)
}

#[cfg(windows)]
fn query_native(drive: String) -> Result<Summary> {
    use windows_sys::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};
    let root = super::wide(std::path::Path::new(&drive));
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    // The SDK-owned structure and terminated root remain valid throughout this read-only call.
    let status = unsafe { SHQueryRecycleBinW(root.as_ptr(), &mut info) };
    if status != 0 {
        return Err(Error::Invalid(format!(
            "Recycle Bin unavailable ({status:#010x})."
        )));
    }
    checked_summary(drive, info.i64NumItems, info.i64Size)
}

#[cfg(not(windows))]
fn query_native(_drive: String) -> Result<Summary> {
    Err(Error::Invalid("Recycle Bin requires Windows.".into()))
}
