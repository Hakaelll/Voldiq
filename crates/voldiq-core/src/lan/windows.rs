use super::{Summary, invalid};
use crate::{
    space::{Observation, VolumeIdentity, observe_volume},
    windows::wide,
};
use std::{
    collections::BTreeSet,
    io,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA},
    NetworkManagement::{
        IpHelper::{
            GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_FRIENDLY_NAME,
            GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
        },
        Ndis::IfOperStatusUp,
    },
    Networking::WinSock::{AF_INET, SOCKADDR_IN},
    Storage::FileSystem::GetDriveTypeW,
    System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE},
};

pub fn local_addresses() -> io::Result<Vec<Ipv4Addr>> {
    let mut bytes = 15_000u32;
    for _ in 0..3 {
        if bytes > 1024 * 1024 {
            break;
        }
        let mut buffer = vec![0u64; (bytes as usize).div_ceil(size_of::<u64>())];
        let mut adapter = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // The system returns a linked list backed by this aligned allocation.
        let status = unsafe {
            GetAdaptersAddresses(
                AF_INET.into(),
                GAA_FLAG_SKIP_ANYCAST
                    | GAA_FLAG_SKIP_MULTICAST
                    | GAA_FLAG_SKIP_DNS_SERVER
                    | GAA_FLAG_SKIP_FRIENDLY_NAME,
                ptr::null(),
                adapter,
                &mut bytes,
            )
        };
        match status {
            ERROR_BUFFER_OVERFLOW => continue,
            ERROR_NO_DATA => return Ok(Vec::new()),
            0 => {}
            code => return Err(io::Error::from_raw_os_error(code as i32)),
        }
        let mut addresses = BTreeSet::new();
        // The pointers remain valid while the successful API buffer is retained.
        unsafe {
            while let Some(entry) = adapter.as_ref() {
                adapter = entry.Next;
                if entry.OperStatus != IfOperStatusUp {
                    continue;
                }
                let mut unicast = entry.FirstUnicastAddress;
                while let Some(item) = unicast.as_ref() {
                    unicast = item.Next;
                    let address = item.Address;
                    if !address.lpSockaddr.is_null()
                        && address.iSockaddrLength >= size_of::<SOCKADDR_IN>() as i32
                        && (*address.lpSockaddr).sa_family == AF_INET
                    {
                        let internet = &*address.lpSockaddr.cast::<SOCKADDR_IN>();
                        let ip = Ipv4Addr::from(internet.sin_addr.S_un.S_addr.to_ne_bytes());
                        if ip.is_private() {
                            addresses.insert(ip);
                        }
                    }
                }
            }
        }
        return Ok(addresses.into_iter().collect());
    }
    Err(io::Error::other(
        "Local addresses changed during enumeration. Try again.",
    ))
}

pub struct LocalVolume {
    root: PathBuf,
    identity: VolumeIdentity,
}

impl LocalVolume {
    pub fn open(root: &str) -> io::Result<(Self, Summary)> {
        let [drive, b':', b'\\'] = root.as_bytes() else {
            return Err(invalid("Select a local drive."));
        };
        if !drive.is_ascii_alphabetic() {
            return Err(invalid("Select a local drive."));
        }
        let root = PathBuf::from(root);
        let (identity, summary) = observe(&root)?;
        Ok((Self { root, identity }, summary))
    }

    pub fn refresh(&self) -> io::Result<Summary> {
        let (identity, summary) = observe(&self.root)?;
        if !identity
            .guid_root
            .eq_ignore_ascii_case(&self.identity.guid_root)
            || identity.serial != self.identity.serial
        {
            return Err(invalid(
                "The shared drive changed. Stop sharing and select it again.",
            ));
        }
        Ok(summary)
    }

    pub fn path(&self) -> &Path {
        &self.root
    }
}

fn observe(path: &Path) -> io::Result<(VolumeIdentity, Summary)> {
    // The caller checked the normal drive-root form before storing this path.
    if !matches!(
        unsafe { GetDriveTypeW(wide(path).as_ptr()) },
        DRIVE_FIXED | DRIVE_REMOVABLE
    ) {
        return Err(invalid("Select a local or removable drive."));
    }
    let Observation::Measured { measurement } = observe_volume(path) else {
        return Err(io::Error::other(
            "Cannot measure this drive's whole-volume storage.",
        ));
    };
    let summary = Summary {
        total: measurement.total.0,
        free: measurement.free.0,
        used: measurement.used.0,
        observed_at: measurement.observed_at,
    }
    .validate()?;
    Ok((measurement.volume, summary))
}
