use super::Health;
use std::mem::{offset_of, size_of};
use windows_sys::Win32::{Storage::Nvme::NVME_HEALTH_INFO_LOG, System::Ioctl::*};

pub(super) const EXTENT_LIMIT: usize = 32;
pub(super) const EXTENTS_OFFSET: usize = offset_of!(VOLUME_DISK_EXTENTS, Extents);
pub(super) const NVME_LOG_SIZE: usize = size_of::<NVME_HEALTH_INFO_LOG>();
pub(super) const PROTOCOL_SIZE: usize = size_of::<STORAGE_PROTOCOL_SPECIFIC_DATA>();
pub(super) const DESCRIPTOR_SIZE: usize = size_of::<STORAGE_PROTOCOL_DATA_DESCRIPTOR>();
const DESCRIPTOR_PROTOCOL: usize =
    offset_of!(STORAGE_PROTOCOL_DATA_DESCRIPTOR, ProtocolSpecificData);

pub(super) fn parse_extents(bytes: &[u8]) -> Option<Vec<u32>> {
    let count = u32_at(bytes, 0)? as usize;
    if !(1..=EXTENT_LIMIT).contains(&count) {
        return None;
    }
    let mut numbers = bytes
        .get(EXTENTS_OFFSET..EXTENTS_OFFSET + count * size_of::<DISK_EXTENT>())?
        .chunks_exact(size_of::<DISK_EXTENT>())
        .map(|extent| u32_at(extent, offset_of!(DISK_EXTENT, DiskNumber)))
        .collect::<Option<Vec<_>>>()?;
    numbers.sort_unstable();
    numbers.dedup();
    Some(numbers)
}

pub(super) fn parse_prediction(bytes: &[u8]) -> Option<Health> {
    if bytes.len() < size_of::<STORAGE_PREDICT_FAILURE>() {
        return None;
    }
    let alert = u32_at(bytes, offset_of!(STORAGE_PREDICT_FAILURE, PredictFailure))? != 0;
    Some(Health::WindowsPrediction { alert })
}

pub(super) fn parse_nvme(bytes: &[u8]) -> Option<Health> {
    if u32_at(bytes, offset_of!(STORAGE_PROTOCOL_DATA_DESCRIPTOR, Version))? as usize
        != DESCRIPTOR_SIZE
        || u32_at(bytes, offset_of!(STORAGE_PROTOCOL_DATA_DESCRIPTOR, Size))? as usize
            != DESCRIPTOR_SIZE
    {
        return None;
    }
    let protocol = bytes.get(DESCRIPTOR_PROTOCOL..DESCRIPTOR_SIZE)?;
    let offset = u32_at(
        protocol,
        offset_of!(STORAGE_PROTOCOL_SPECIFIC_DATA, ProtocolDataOffset),
    )? as usize;
    let length = u32_at(
        protocol,
        offset_of!(STORAGE_PROTOCOL_SPECIFIC_DATA, ProtocolDataLength),
    )? as usize;
    if offset < PROTOCOL_SIZE
        || length < NVME_LOG_SIZE
        || u32_at(
            protocol,
            offset_of!(STORAGE_PROTOCOL_SPECIFIC_DATA, ProtocolType),
        )? != ProtocolTypeNvme as u32
        || u32_at(
            protocol,
            offset_of!(STORAGE_PROTOCOL_SPECIFIC_DATA, DataType),
        )? != NVMeDataTypeLogPage as u32
    {
        return None;
    }
    let start = DESCRIPTOR_PROTOCOL.checked_add(offset)?;
    let log = bytes.get(start..start.checked_add(length)?)?;
    let temperature = offset_of!(NVME_HEALTH_INFO_LOG, Temperature);
    let kelvin = u16::from_le_bytes(log.get(temperature..temperature + 2)?.try_into().ok()?);
    let counter =
        |at: usize| Some(u128::from_le_bytes(log.get(at..at + 16)?.try_into().ok()?).to_string());
    Some(Health::Nvme {
        critical_warning: *log.get(offset_of!(NVME_HEALTH_INFO_LOG, CriticalWarning))?,
        temperature_kelvin: (kelvin != 0).then_some(kelvin),
        power_on_hours: counter(offset_of!(NVME_HEALTH_INFO_LOG, PowerOnHours))?,
        unsafe_shutdowns: counter(offset_of!(NVME_HEALTH_INFO_LOG, UnsafeShutdowns))?,
        media_errors: counter(offset_of!(NVME_HEALTH_INFO_LOG, MediaErrors))?,
    })
}

pub(super) fn u32_at(bytes: &[u8], start: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(start..start + 4)?.try_into().ok()?,
    ))
}
