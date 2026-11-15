use super::{Summary, invalid};
use crate::{
    agents::{AuthorizedPath, pin_directory},
    space::{VolumeIdentity, measure_index_volume},
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
    root_identity: (u64, u64),
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
        require_local_drive(&root)?;
        let pin = pin_directory(&root).map_err(io::Error::other)?;
        let root_identity = pin.identity();
        let (identity, summary) = observe(&root, root_identity)?;
        Ok((
            Self {
                root,
                identity,
                root_identity,
            },
            summary,
        ))
    }

    pub fn refresh(&self) -> io::Result<Summary> {
        let (identity, summary) = observe(&self.root, self.root_identity)?;
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

    pub fn pin_scan(&self) -> io::Result<AuthorizedPath> {
        self.refresh()?;
        let pin = pin_directory(&self.root).map_err(io::Error::other)?;
        if pin.identity() != self.root_identity {
            return Err(invalid(
                "The shared drive changed. Stop sharing and select it again.",
            ));
        }
        Ok(pin)
    }

    pub fn path(&self) -> &Path {
        &self.root
    }
}

fn observe(path: &Path, root_identity: (u64, u64)) -> io::Result<(VolumeIdentity, Summary)> {
    require_local_drive(path)?;
    let Some(measurement) = measure_index_volume(path, root_identity) else {
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

fn require_local_drive(path: &Path) -> io::Result<()> {
    // The caller checked the normal drive-root form before storing this path.
    if matches!(
        unsafe { GetDriveTypeW(wide(path).as_ptr()) },
        DRIVE_FIXED | DRIVE_REMOVABLE
    ) {
        Ok(())
    } else {
        Err(invalid("Select a local or removable drive."))
    }
}
