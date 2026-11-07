use super::{Category, Library, Provider, Query, Report, Size};
use crate::{
    Entry, Kind, NodeId, Result,
    index::Index,
    inspection::read_indexed_prefix,
    libraries::scoped::{
        Locations, OUTSIDE, UNRESOLVED, measure_manual, observe, ownership, path_key,
    },
};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const SIGNATURE_LIMIT: usize = 1024;
const PROFILE_PREFIX: usize = 4096;
const SQLITE_HEADER: usize = 100;
const CATEGORIES: [Category; 6] = [
    Category::SourceImages,
    Category::Derivatives,
    Category::Previews,
    Category::Proxies,
    Category::Catalog,
    Category::EditProfiles,
];

type Sibling = (NodeId, String);
type Claim = (usize, Category);

struct Group {
    anchor: NodeId,
    folder: NodeId,
    provider: Provider,
    name: String,
    totals: [Option<u128>; CATEGORIES.len()],
    partial: bool,
}

impl Group {
    fn new(anchor: NodeId, folder: &Entry, provider: Provider, name: String) -> Self {
        Self {
            anchor,
            folder: folder.id,
            provider,
            name,
            totals: [None; CATEGORIES.len()],
            partial: folder.incomplete,
        }
    }
    fn total(&self) -> u128 {
        self.totals.iter().flatten().sum()
    }
}

enum Relation {
    CatalogCompanion {
        group: usize,
        category: Category,
        directory_only: bool,
    },
    Profile {
        id: NodeId,
        category: Category,
    },
}

fn claim(
    claims: &mut HashMap<NodeId, Option<Claim>>,
    id: NodeId,
    owner: Claim,
    groups: &mut [Group],
) {
    claims
        .entry(id)
        .and_modify(|prior| {
            if *prior == Some(owner) {
                return;
            }
            if let Some((other, _)) = *prior {
                groups[other].partial = true;
            }
            groups[owner.0].partial = true;
            *prior = None;
        })
        .or_insert(Some(owner));
}

fn parent<'a>(index: &'a Index, entry: &Entry) -> Option<&'a Entry> {
    index.entry(entry.parent?).ok()
}

fn named_folder(entry: &Entry, name: &str) -> bool {
    entry.kind == Kind::Directory && entry.name.eq_ignore_ascii_case(name)
}

fn darktable_directory(index: &Index, folder: &Entry) -> bool {
    named_folder(folder, "darktable")
        && parent(index, folder).is_some_and(|local| {
            named_folder(local, "Local")
                && parent(index, local).is_some_and(|appdata| named_folder(appdata, "AppData"))
        })
}

fn sqlite_header(bytes: &[u8]) -> bool {
    if bytes.len() < SQLITE_HEADER || !bytes.starts_with(b"SQLite format 3\0") {
        return false;
    }
    let page = u16::from_be_bytes([bytes[16], bytes[17]]);
    (page == 1 || (page >= 512 && page.is_power_of_two()))
        && matches!(bytes[18], 1 | 2)
        && matches!(bytes[19], 1 | 2)
        && bytes[21..24] == [64, 32, 32]
}

fn profile_target(name: &str) -> Option<(String, Category)> {
    let name = name.to_lowercase();
    let profile = name.strip_suffix(".pp3")?;
    let (image, category) = profile
        .strip_suffix(".out")
        .map_or((profile, Category::SourceImages), |image| {
            (image, Category::Derivatives)
        });
    let (stem, ext) = image.rsplit_once('.')?;
    let image_format = matches!(ext, "jpg" | "jpeg" | "png" | "tif" | "tiff");
    let raw_format = matches!(
        ext,
        "dng" | "nef" | "cr2" | "cr3" | "arw" | "raf" | "rw2" | "orf" | "pef" | "srw" | "raw"
    );
    (!stem.is_empty() && (image_format || (category == Category::SourceImages && raw_format)))
        .then(|| (image.to_owned(), category))
}

fn profile_header(bytes: &[u8]) -> bool {
    // Only the initial profile section is needed. A later long line can end mid-character.
    let text = String::from_utf8_lossy(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes));
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(['#', ';']));
    if lines.next() != Some("[Version]") {
        return false;
    }
    let mut app = false;
    let mut version = false;
    for line in lines.take_while(|line| !line.starts_with('[')) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "AppVersion" => {
                app = value
                    .split('.')
                    .next()
                    .is_some_and(|part| part.parse::<u32>().is_ok())
            }
            "Version" => version = value.parse::<u32>().is_ok_and(|value| value > 0),
            _ => {}
        }
    }
    app && version
}

pub(super) fn query(index: &Index, query: &Query, cancel: &AtomicBool) -> Result<Option<Report>> {
    let stopped = || cancel.load(Ordering::Relaxed);
    let mut groups = Vec::new();
    let mut claims = HashMap::new();
    let mut darktable = HashMap::new();
    let mut lookup = Vec::<(Sibling, Relation)>::new();
    let mut inspected = 0;
    let mut omitted = 0;
    for entry in &index.entries {
        if stopped() {
            return Ok(None);
        }
        if !entry.is_metadata_file() {
            continue;
        }
        let Some((stem, extension)) = entry.name.rsplit_once('.') else {
            continue;
        };
        if !["lrcat", "db", "pp3"]
            .iter()
            .any(|kind| extension.eq_ignore_ascii_case(kind))
        {
            continue;
        }
        let Some(directory) = parent(index, entry).filter(|node| node.kind == Kind::Directory)
        else {
            continue;
        };
        let lightroom = !stem.is_empty() && extension.eq_ignore_ascii_case("lrcat");
        let darktable_db = (entry.name.eq_ignore_ascii_case("library.db")
            || entry.name.eq_ignore_ascii_case("data.db"))
            && darktable_directory(index, directory);
        let profile = extension
            .eq_ignore_ascii_case("pp3")
            .then(|| profile_target(&entry.name))
            .flatten();
        if (!lightroom && !darktable_db && profile.is_none())
            || !index.in_scope(entry.id, query.scope)
        {
            continue;
        }
        if inspected == SIGNATURE_LIMIT {
            omitted += 1;
            continue;
        }
        inspected += 1;
        let prefix = if profile.is_some() {
            PROFILE_PREFIX
        } else {
            SQLITE_HEADER
        };
        let Ok(bytes) = read_indexed_prefix(index, entry.id, prefix) else {
            omitted += 1;
            continue;
        };
        if let Some((target, category)) = profile {
            if profile_header(&bytes) {
                lookup.push((
                    (directory.id, target),
                    Relation::Profile {
                        id: entry.id,
                        category,
                    },
                ));
            } else {
                omitted += 1;
            }
        } else if !sqlite_header(&bytes) {
            omitted += 1;
        } else if lightroom {
            let group = groups.len();
            groups.push(Group::new(
                entry.id,
                directory,
                Provider::LightroomClassic,
                stem.to_owned(),
            ));
            claim(
                &mut claims,
                entry.id,
                (group, Category::Catalog),
                &mut groups,
            );
            for (suffix, category, directory_only) in [
                (".lrcat-data", Category::Catalog, false),
                (" Previews.lrdata", Category::Previews, true),
                (" Smart Previews.lrdata", Category::Proxies, true),
            ] {
                lookup.push((
                    (directory.id, format!("{stem}{suffix}").to_lowercase()),
                    Relation::CatalogCompanion {
                        group,
                        category,
                        directory_only,
                    },
                ));
            }
        } else {
            let group = *darktable.entry(directory.id).or_insert_with(|| {
                let group = groups.len();
                groups.push(Group::new(
                    directory.id,
                    directory,
                    Provider::Darktable,
                    "darktable catalog".into(),
                ));
                group
            });
            claim(
                &mut claims,
                entry.id,
                (group, Category::Catalog),
                &mut groups,
            );
        }
    }

    let manuals: Vec<_> = query
        .manual
        .iter()
        .map(|path| path_key(Path::new(path)))
        .collect();
    let absolute: HashSet<_> = manuals.iter().flatten().collect();
    let basenames: HashSet<_> = absolute
        .iter()
        .filter_map(|path| path.rsplit('\\').next())
        .collect();
    let wanted: HashSet<_> = lookup
        .iter()
        .map(|((parent, name), _)| (*parent, name.as_str()))
        .collect();
    let parents: HashSet<_> = wanted.iter().map(|(id, _)| *id).collect();
    let mut siblings = Locations::new();
    let mut manual_locations = Locations::new();
    for entry in &index.entries {
        if stopped() {
            return Ok(None);
        }
        let relevant_parent = entry.parent.filter(|parent| parents.contains(parent));
        let may_be_manual = entry.kind == Kind::Directory && !absolute.is_empty();
        if relevant_parent.is_none() && !may_be_manual {
            continue;
        }
        let name = entry.name.to_lowercase();
        if let Some(parent) = relevant_parent
            && wanted.contains(&(parent, name.as_str()))
            && index.in_scope(entry.id, query.scope)
        {
            observe(&mut siblings, (parent, name.clone()), entry.id);
        }
        if may_be_manual
            && (entry.id == NodeId(0) || basenames.contains(name.as_str()))
            && index.in_scope(entry.id, query.scope)
            && let Some(path) = path_key(&index.path(entry.id)?)
            && absolute.contains(&path)
        {
            observe(&mut manual_locations, path, entry.id);
        }
    }

    let mut rawtherapee = HashMap::new();
    for (key, relation) in lookup {
        if stopped() {
            return Ok(None);
        }
        let target = siblings
            .get(&key)
            .copied()
            .flatten()
            .and_then(|id| index.entry(id).ok());
        match relation {
            Relation::CatalogCompanion {
                group,
                category,
                directory_only,
            } => {
                if let Some(target) = target.filter(|entry| {
                    entry.kind == Kind::Directory || (!directory_only && entry.is_metadata_file())
                }) {
                    claim(&mut claims, target.id, (group, category), &mut groups);
                } else if siblings.contains_key(&key) {
                    groups[group].partial = true;
                    omitted += 1;
                }
            }
            Relation::Profile { id, category } => {
                let Some(target) = target.filter(|entry| entry.is_metadata_file()) else {
                    omitted += 1;
                    continue;
                };
                let group = if let Some(&group) = rawtherapee.get(&key.0) {
                    group
                } else {
                    let folder = index.entry(key.0)?;
                    let group = groups.len();
                    groups.push(Group::new(
                        folder.id,
                        folder,
                        Provider::RawTherapee,
                        folder.name.clone(),
                    ));
                    rawtherapee.insert(folder.id, group);
                    group
                };
                claim(&mut claims, target.id, (group, category), &mut groups);
                claim(
                    &mut claims,
                    id,
                    (group, Category::EditProfiles),
                    &mut groups,
                );
            }
        }
    }

    let boundaries: HashMap<_, _> = claims
        .into_iter()
        .map(|(id, claim)| {
            let owner = if let Some((group, category)) = claim {
                let category = category as usize;
                groups[group].totals[category].get_or_insert(0);
                (2 + group * CATEGORIES.len() + category) as u16
            } else {
                1
            };
            (id, owner)
        })
        .collect();
    let mut owners = vec![UNRESOLVED; index.entries.len()];
    let mut pending = Vec::new();
    let mut shared = 0_u128;
    let mut unattributed = 0_u128;
    for entry in &index.entries {
        if stopped() {
            return Ok(None);
        }
        let owner = ownership(
            index,
            query.scope,
            &boundaries,
            &mut owners,
            &mut pending,
            entry.id,
        );
        let bytes = if entry.is_metadata_file() {
            u128::from(entry.logical)
        } else {
            0
        };
        match owner {
            OUTSIDE => {}
            0 => unattributed += bytes,
            1 => shared += bytes,
            code => {
                let owner = usize::from(code - 2);
                let group = &mut groups[owner / CATEGORIES.len()];
                group.partial |= entry.incomplete;
                *group.totals[owner % CATEGORIES.len()].get_or_insert(0) += bytes;
            }
        }
    }

    let manual = measure_manual(index, query.scope, manuals, &manual_locations);
    groups.sort_unstable_by_key(|group| {
        (Reverse(group.total()), group.anchor.0, group.provider as u8)
    });
    let total = groups.len();
    let mut items = Vec::new();
    for group in groups.into_iter().skip(query.offset).take(query.limit) {
        if stopped() {
            return Ok(None);
        }
        let sizes = CATEGORIES
            .into_iter()
            .zip(group.totals)
            .filter_map(|(category, value)| {
                Some(Size {
                    category,
                    logical_bytes: value?.to_string(),
                })
            })
            .collect();
        items.push(Library {
            anchor: group.anchor,
            folder: group.folder,
            provider: group.provider,
            name: group.name,
            path: index.path(group.folder)?.to_string_lossy().into_owned(),
            sizes,
            partial: group.partial,
        });
    }
    if stopped() {
        return Ok(None);
    }
    Ok(Some(Report::Available {
        items,
        total,
        shared_bytes: shared.to_string(),
        unattributed_bytes: unattributed.to_string(),
        omitted_signatures: omitted,
        coverage_partial: omitted > 0
            || shared > 0
            || index.entry(query.scope)?.incomplete
            || !index.exclusions.is_empty()
            || index
                .observation
                .as_ref()
                .is_none_or(|observation| !observation.complete),
        manual,
    }))
}
