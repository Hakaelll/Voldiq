use super::{Disk, Health, Unavailable};
use crate::{agents::pin_directory, windows};
use std::{
    fs::{File, OpenOptions},
    io,
    mem::{offset_of, size_of},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
    ptr::null_mut,
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_FUNCTION, ERROR_MORE_DATA, ERROR_NOT_SUPPORTED},
    Storage::{
        FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
        },
        Nvme::NVME_LOG_PAGE_HEALTH_INFO,
    },
    System::{IO::DeviceIoControl, Ioctl::*},
};

mod decode;
use decode::{
    DESCRIPTOR_SIZE, EXTENT_LIMIT, EXTENTS_OFFSET, NVME_LOG_SIZE, PROTOCOL_SIZE, parse_extents,
    parse_nvme, parse_prediction, u32_at,
};

pub(super) fn read(
    root: &Path,
    identity: (u64, u64),
    cancelled: &AtomicBool,
) -> Result<Vec<Disk>, Unavailable> {
    let pin = pin_directory(root).map_err(root_failure)?;
    if pin.identity() != identity {
        return Err(Unavailable::RootChanged);
    }
    let file = pin.directory_handle().map_err(root_failure)?;
    let (_, guid_root) = windows::volume_paths(file).map_err(|_| Unavailable::VolumeUnmapped)?;
    let device_path = guid_root
        .to_str()
        .and_then(|path| path.strip_suffix('\\'))
        .ok_or(Unavailable::VolumeUnmapped)?;
    let volume = open_device(device_path).map_err(driver_failure)?;
    let numbers = disk_numbers(&volume)?;
    let mut disks = Vec::with_capacity(numbers.len());
    for &number in &numbers {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(disks);
        }
        let health = match read_disk(number, cancelled) {
            Ok(value) => value,
            Err(reason) => Health::Unavailable { reason },
        };
        disks.push(Disk { number, health });
    }
    if cancelled.load(Ordering::Relaxed) {
        return Ok(disks);
    }
    if disk_numbers(&volume)? != numbers {
        return Err(Unavailable::MappingChanged);
    }
    let current = pin_directory(root).map_err(root_failure)?;
    if current.identity() != identity
        || current.ancestor_identities().map_err(root_failure)?
            != pin.ancestor_identities().map_err(root_failure)?
        || windows::volume_paths(current.directory_handle().map_err(root_failure)?)
            .map_err(|_| Unavailable::MappingChanged)?
            .1
            != guid_root
    {
        return Err(Unavailable::RootChanged);
    }
    Ok(disks)
}

fn open_device(path: &str) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)
}

fn disk_numbers(volume: &File) -> Result<Vec<u32>, Unavailable> {
    let mut buffer = [0; EXTENTS_OFFSET + EXTENT_LIMIT * size_of::<DISK_EXTENT>()];
    let length =
        ioctl(volume, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, 0, &mut buffer).map_err(|error| {
            match error.raw_os_error().map(|code| code as u32) {
                Some(ERROR_MORE_DATA) => Unavailable::ExtentLimit,
                _ => driver_failure(error),
            }
        })?;
    parse_extents(&buffer[..length]).ok_or(Unavailable::InvalidResponse)
}

fn read_disk(number: u32, cancelled: &AtomicBool) -> Result<Health, Unavailable> {
    let disk = open_device(&format!(r"\\.\PhysicalDrive{number}")).map_err(driver_failure)?;
    let mut identity = [0; size_of::<STORAGE_DEVICE_NUMBER>()];
    let length =
        ioctl(&disk, IOCTL_STORAGE_GET_DEVICE_NUMBER, 0, &mut identity).map_err(driver_failure)?;
    let identity = &identity[..length];
    let disk_type = IOCTL_DISK_GET_DRIVE_GEOMETRY >> 16;
    if u32_at(identity, offset_of!(STORAGE_DEVICE_NUMBER, DeviceType)) != Some(disk_type)
        || u32_at(identity, offset_of!(STORAGE_DEVICE_NUMBER, DeviceNumber)) != Some(number)
    {
        return Err(Unavailable::MappingChanged);
    }
    let nvme_problem = match read_nvme(&disk) {
        Ok(health) => return Ok(health),
        Err(problem) => problem,
    };
    if cancelled.load(Ordering::Relaxed) {
        return Err(nvme_problem);
    }
    let mut prediction = [0; size_of::<STORAGE_PREDICT_FAILURE>()];
    match ioctl(&disk, IOCTL_STORAGE_PREDICT_FAILURE, 0, &mut prediction) {
        Ok(length) => parse_prediction(&prediction[..length]).ok_or(Unavailable::InvalidResponse),
        Err(error) => {
            let fallback = driver_failure(error);
            Err(match (nvme_problem, fallback) {
                (Unavailable::AccessDenied, _) | (_, Unavailable::AccessDenied) => {
                    Unavailable::AccessDenied
                }
                (Unavailable::InvalidResponse, _) => Unavailable::InvalidResponse,
                (reason, Unavailable::Unsupported) => reason,
                (_, reason) => reason,
            })
        }
    }
}

fn read_nvme(disk: &File) -> Result<Health, Unavailable> {
    // The same buffer carries the property query in and the protocol descriptor out.
    let mut buffer = [0; DESCRIPTOR_SIZE + NVME_LOG_SIZE];
    let protocol = offset_of!(STORAGE_PROPERTY_QUERY, AdditionalParameters);
    let mut put = |at: usize, value: u32| buffer[at..at + 4].copy_from_slice(&value.to_le_bytes());
    put(
        offset_of!(STORAGE_PROPERTY_QUERY, PropertyId),
        StorageDeviceProtocolSpecificProperty as u32,
    );
    put(
        offset_of!(STORAGE_PROPERTY_QUERY, QueryType),
        PropertyStandardQuery as u32,
    );
    put(
        protocol + offset_of!(STORAGE_PROTOCOL_SPECIFIC_DATA, ProtocolType),
        ProtocolTypeNvme as u32,
    );
    put(
        protocol + offset_of!(STORAGE_PROTOCOL_SPECIFIC_DATA, DataType),
        NVMeDataTypeLogPage as u32,
    );
    put(
        protocol + offset_of!(STORAGE_PROTOCOL_SPECIFIC_DATA, ProtocolDataRequestValue),
        NVME_LOG_PAGE_HEALTH_INFO as u32,
    );
    put(
        protocol + offset_of!(STORAGE_PROTOCOL_SPECIFIC_DATA, ProtocolDataOffset),
        PROTOCOL_SIZE as u32,
    );
    put(
        protocol + offset_of!(STORAGE_PROTOCOL_SPECIFIC_DATA, ProtocolDataLength),
        NVME_LOG_SIZE as u32,
    );
    // Retain controller asynchronous events while reading the log.
    put(
        protocol + offset_of!(STORAGE_PROTOCOL_SPECIFIC_DATA, ProtocolDataRequestSubValue4),
        1,
    );
    let length = ioctl(
        disk,
        IOCTL_STORAGE_QUERY_PROPERTY,
        buffer.len() as u32,
        &mut buffer,
    )
    .map_err(driver_failure)?;
    parse_nvme(&buffer[..length]).ok_or(Unavailable::InvalidResponse)
}

fn ioctl(file: &File, code: u32, length: u32, buffer: &mut [u8]) -> io::Result<usize> {
    let output = buffer.as_mut_ptr().cast::<std::ffi::c_void>();
    let input = if length == 0 {
        std::ptr::null()
    } else {
        output.cast_const()
    };
    let mut returned = 0;
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            code,
            input,
            length,
            output,
            buffer.len() as u32,
            &mut returned,
            null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let returned = returned as usize;
    if returned > buffer.len() {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok(returned)
}

fn root_failure(error: crate::Error) -> Unavailable {
    match error {
        crate::Error::Io(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            Unavailable::AccessDenied
        }
        _ => Unavailable::RootChanged,
    }
}

fn driver_failure(error: io::Error) -> Unavailable {
    match error.kind() {
        io::ErrorKind::PermissionDenied => Unavailable::AccessDenied,
        io::ErrorKind::InvalidData => Unavailable::InvalidResponse,
        io::ErrorKind::Unsupported => Unavailable::Unsupported,
        _ if matches!(
            error.raw_os_error().map(|code| code as u32),
            Some(ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED)
        ) =>
        {
            Unavailable::Unsupported
        }
        _ => Unavailable::DriverError,
    }
}
