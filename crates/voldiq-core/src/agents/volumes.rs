#[cfg(windows)]
use super::{AuthorizedPath, Policy, denied, scope::Root};
use super::{Capability, Principal, Registry};
use crate::{Error, Result, space::Observation};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeCatalogueRequest {
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

impl VolumeCatalogueRequest {
    pub fn valid(&self) -> bool {
        self.offset <= 32 && (1..=32).contains(&self.limit)
    }
}

fn default_limit() -> usize {
    32
}

#[derive(Serialize)]
pub struct VolumeCatalogue {
    pub supported: bool,
    pub items: Vec<GrantedVolume>,
    pub next_offset: Option<usize>,
}

#[derive(Serialize)]
pub struct GrantedVolume {
    pub root: PathBuf,
    pub label: Option<String>,
    pub filesystem: Option<String>,
    pub space: Observation,
}

impl Registry {
    pub fn volume_catalogue(
        &self,
        principal: &Principal,
        request: VolumeCatalogueRequest,
        cancel: &AtomicBool,
    ) -> Result<Option<VolumeCatalogue>> {
        if !request.valid() {
            return Err(Error::Invalid(
                "Volume catalogue requires offset up to 32 and limit from 1 to 32.".into(),
            ));
        }
        let policy = self.policy(principal, Capability::Metadata)?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        #[cfg(not(windows))]
        {
            let _ = policy;
            Ok(Some(VolumeCatalogue {
                supported: false,
                items: Vec::new(),
                next_offset: None,
            }))
        }
        #[cfg(windows)]
        {
            let Some(eligible) = policy.pinned_volumes(cancel) else {
                return Ok(None);
            };
            let limit = request.limit.min(policy.limits().page_size);
            let start = request.offset.min(eligible.len());
            let end = (start + limit).min(eligible.len());
            let selected = &eligible[start..end];
            let mut items = Vec::new();
            for volume in selected {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                items.push(volume.measure()?);
            }
            let current = self.policy(principal, Capability::Metadata)?;
            if items.len() > current.limits().page_size {
                return Err(denied());
            }
            let Some(current_pins) = current.pinned_volumes(cancel) else {
                return Ok(None);
            };
            if !eligible
                .iter()
                .map(PinnedVolume::scope)
                .eq(current_pins.iter().map(PinnedVolume::scope))
            {
                return Err(denied());
            }
            let result = (!cancel.load(Ordering::Relaxed)).then_some(VolumeCatalogue {
                supported: true,
                items,
                next_offset: (end < eligible.len()).then_some(end),
            });
            Ok(result)
        }
    }
}

#[cfg(windows)]
impl Policy {
    fn pinned_volumes(&self, cancel: &AtomicBool) -> Option<Vec<PinnedVolume>> {
        let mut eligible = Vec::new();
        for grant in &self.roots {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(volume) = PinnedVolume::for_grant(grant, self)
                && !eligible
                    .iter()
                    .any(|item: &PinnedVolume| item.identity == volume.identity)
            {
                eligible.push(volume);
            }
        }
        Some(eligible)
    }
}

#[cfg(windows)]
struct PinnedVolume {
    root: AuthorizedPath,
    guid: PathBuf,
    identity: Vec<u8>,
}

#[cfg(windows)]
impl PinnedVolume {
    fn for_grant(grant: &Root, policy: &Policy) -> Option<Self> {
        // Exclusions on the same physical volume also apply through a different granted mount.
        if policy.exclusions.iter().any(|excluded| {
            grant.contains(&excluded.path) || excluded.volume_serial() == grant.volume_serial()
        }) {
            return None;
        }
        let root = grant.reopen().ok()?;
        if root.path() != grant.path {
            return None;
        }
        let (native, guid) = crate::windows::volume_paths(root.directory_handle().ok()?).ok()?;
        // A drive alias for a directory can look like a drive root without being a volume root.
        if native != guid {
            return None;
        }
        let identity = root.identity_bytes().ok()?;
        Some(Self {
            root,
            guid,
            identity,
        })
    }

    fn scope(&self) -> (&std::path::Path, &std::path::Path, &[u8]) {
        (self.root.path(), &self.guid, &self.identity)
    }

    fn measure(&self) -> Result<GrantedVolume> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationByHandleW;

        let mut label = [0_u16; 261];
        let mut filesystem = [0_u16; 261];
        let described = unsafe {
            GetVolumeInformationByHandleW(
                self.root.directory_handle()?.as_raw_handle(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                filesystem.as_mut_ptr(),
                filesystem.len() as u32,
            )
        } != 0;
        let name = |buffer: &[u16]| {
            let end = buffer
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(buffer.len());
            (described && end > 0).then(|| String::from_utf16_lossy(&buffer[..end]))
        };
        Ok(GrantedVolume {
            root: self.root.path().to_owned(),
            label: name(&label),
            filesystem: name(&filesystem),
            space: crate::space::observe_pinned(&self.root)?,
        })
    }
}
