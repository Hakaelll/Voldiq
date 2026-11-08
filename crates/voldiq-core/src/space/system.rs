//! Labels recorded paths using current Windows location evidence. These rows are not deductions.

mod native;

use crate::{
    Kind, NodeId, Result,
    agents::ordinary_path,
    index::{ACCOUNTING_VERSION, Index},
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use super::Bytes;

pub struct Locations {
    pub scan_volume_root: Option<PathBuf>,
    pub windows_volume_root: Option<PathBuf>,
    pub windows_directory: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub program_data: Option<PathBuf>,
    pub wsl: WslLocations,
}

pub struct WslLocations {
    pub coverage: RegistrationCoverage,
    pub disks: Vec<RegisteredDisk>,
}

pub struct RegisteredDisk {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationCoverage {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    RecordedScan,
    Live {
        root: PathBuf,
        indexed_at: Option<u64>,
        scope_complete: bool,
        docker_defaults_available: bool,
        wsl_registrations: RegistrationCoverage,
        items: Vec<Item>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Paging,
    Hibernation,
    PreviousWindows,
    ComponentStore,
    Docker,
    Wsl,
}

#[derive(Debug, Serialize)]
pub struct Item {
    pub category: Category,
    pub label: String,
    pub observation: Observation,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Observation {
    Indexed {
        path: PathBuf,
        logical: Bytes,
        allocated: Option<Bytes>,
        incomplete: bool,
    },
    NotObserved,
    OutsideScope,
    LocationUnavailable,
}

struct Target {
    category: Category,
    label: String,
    path: Option<PathBuf>,
    kind: Kind,
}

impl Locations {
    fn targets(&self) -> Vec<Target> {
        let fixed = [
            (
                Category::Paging,
                "Paging file",
                &self.scan_volume_root,
                "pagefile.sys",
                Kind::File,
            ),
            (
                Category::Paging,
                "Swap file",
                &self.scan_volume_root,
                "swapfile.sys",
                Kind::File,
            ),
            (
                Category::Hibernation,
                "Hibernation file",
                &self.windows_volume_root,
                "hiberfil.sys",
                Kind::File,
            ),
            (
                Category::PreviousWindows,
                "Previous Windows installation",
                &self.windows_volume_root,
                "Windows.old",
                Kind::Directory,
            ),
            (
                Category::ComponentStore,
                "Windows component store",
                &self.windows_directory,
                "WinSxS",
                Kind::Directory,
            ),
        ];
        let mut targets: Vec<_> = fixed
            .into_iter()
            .map(|(category, label, base, relative, kind)| Target {
                category,
                label: label.into(),
                path: base.as_ref().map(|base| base.join(relative)),
                kind,
            })
            .collect();
        for (label, base, relative) in [
            (
                "Docker Desktop · WSL 2",
                &self.local_app_data,
                r"Docker\wsl\disk\docker_data.vhdx",
            ),
            (
                "Docker Desktop · Hyper-V",
                &self.program_data,
                r"DockerDesktop\vm-data\DockerDesktop.vhdx",
            ),
            (
                "Docker Desktop · Docker VMM",
                &self.local_app_data,
                r"Docker\vm-data\DockerDesktop.vhdx",
            ),
        ] {
            if let Some(base) = base {
                targets.push(Target {
                    category: Category::Docker,
                    label: label.into(),
                    path: Some(base.join(relative)),
                    kind: Kind::File,
                });
            }
        }
        for disk in &self.wsl.disks {
            let category = if disk.name.eq_ignore_ascii_case("docker-desktop-data")
                || disk.name.eq_ignore_ascii_case("docker-desktop")
            {
                Category::Docker
            } else {
                Category::Wsl
            };
            targets.push(Target {
                category,
                label: disk.name.clone(),
                path: Some(disk.path.clone()),
                kind: Kind::File,
            });
        }
        targets
    }
}

fn key(path: &Path) -> Option<PathBuf> {
    let path = ordinary_path(path).ok()?;
    Some(PathBuf::from(path.to_str()?.to_ascii_lowercase()))
}

enum Found {
    Missing,
    Unique(NodeId),
    Ambiguous,
}

pub fn read_desktop(index: &Index, cancel: &AtomicBool) -> Result<Option<Report>> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    if index.snapshot_id.is_some() {
        return Ok(Some(Report::RecordedScan));
    }
    let _root = index.pin_folder(NodeId(0))?;
    let locations = native::discover(&index.root, cancel);
    describe(index, &locations, cancel)
}

/// Interpret the supplied locations against one index, without reopening target paths.
pub fn describe(
    index: &Index,
    locations: &Locations,
    cancel: &AtomicBool,
) -> Result<Option<Report>> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    if index.snapshot_id.is_some() {
        return Ok(Some(Report::RecordedScan));
    }
    let root = key(&index.root).ok_or_else(|| {
        crate::Error::Invalid("System storage requires a local drive path.".into())
    })?;
    let targets: Vec<_> = locations
        .targets()
        .into_iter()
        .map(|target| {
            let path = target.path.as_deref().and_then(key);
            (target, path)
        })
        .collect();
    let mut paths: HashMap<_, _> = targets
        .iter()
        .filter_map(|(_, path)| {
            let path = path.as_ref().filter(|path| path.starts_with(&root))?;
            Some((path.clone(), Found::Missing))
        })
        .collect();
    let names: HashSet<_> = paths
        .keys()
        .filter_map(|path| path.file_name()?.to_str().map(str::to_owned))
        .collect();
    if !paths.is_empty() {
        for entry in &index.entries {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            if entry.id != NodeId(0)
                && !names.contains(entry.name.as_str())
                && !(entry.name.bytes().any(|byte| byte.is_ascii_uppercase())
                    && names.contains(&entry.name.to_ascii_lowercase()))
            {
                continue;
            }
            if let Some(path) = key(&index.path(entry.id)?)
                && let Some(found) = paths.get_mut(&path)
            {
                *found = match found {
                    Found::Missing => Found::Unique(entry.id),
                    _ => Found::Ambiguous,
                };
            }
        }
    }
    let mut items = Vec::new();
    let mut emitted = HashSet::new();
    let current_accounting = index
        .observation
        .as_ref()
        .is_some_and(|value| value.accounting_version == ACCOUNTING_VERSION);
    for (target, path) in targets {
        let dynamic = matches!(target.category, Category::Docker | Category::Wsl);
        if dynamic
            && path
                .as_ref()
                .is_none_or(|path| !path.starts_with(&root) || !emitted.insert(path.clone()))
        {
            continue;
        }
        let observation = match path {
            None => Observation::LocationUnavailable,
            Some(path) if !path.starts_with(&root) => Observation::OutsideScope,
            Some(path) => match paths.get(&path) {
                Some(Found::Unique(id)) => {
                    let entry = index.entry(*id)?;
                    if entry.kind != target.kind || entry.cloud.is_placeholder() {
                        Observation::LocationUnavailable
                    } else {
                        let allocated = (current_accounting
                            && entry.kind == Kind::File
                            && entry.allocation_known
                            && !entry.hard_link
                            && !entry.incomplete
                            && entry.file_state.is_some_and(|state| {
                                Some(state.identity.0)
                                    == index.root_identity.map(|identity| identity.0)
                            }))
                        .then_some(Bytes(entry.allocated));
                        Observation::Indexed {
                            path: index.path(*id)?,
                            logical: Bytes(entry.logical),
                            allocated,
                            incomplete: entry.incomplete,
                        }
                    }
                }
                Some(Found::Ambiguous) => Observation::LocationUnavailable,
                _ => Observation::NotObserved,
            },
        };
        if !dynamic
            || matches!(
                observation,
                Observation::Indexed { .. } | Observation::LocationUnavailable
            )
        {
            items.push(Item {
                category: target.category,
                label: target.label,
                observation,
            });
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some(Report::Live {
        root: index.root.clone(),
        indexed_at: index.observation.as_ref().map(|value| value.completed_at),
        scope_complete: index
            .observation
            .as_ref()
            .is_some_and(|value| value.complete),
        docker_defaults_available: locations.local_app_data.is_some()
            && locations.program_data.is_some(),
        wsl_registrations: locations.wsl.coverage,
        items,
    }))
}
