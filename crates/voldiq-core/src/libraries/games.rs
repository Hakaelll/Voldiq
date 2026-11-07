mod manifests;

use super::scoped::{
    Locations, OUTSIDE, UNRESOLVED, local_folder, measure_manual, observe, ownership, path_key,
};
use super::{Game, Provider, Query, Report};
use crate::{Entry, Kind, NodeId, Result, index::Index, inspection::read_indexed_manifest};
use manifests::{FILE_LIMIT, number};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const MANIFEST_LIMIT: usize = 512;
const READ_LIMIT: u64 = 16 * 1024 * 1024;
const AUXILIARY_LIMIT: usize = 4096;

pub(super) enum Install {
    Indexed(NodeId),
    Steam { steamapps: NodeId, folder: String },
    Absolute(String),
}

pub(super) struct Record {
    provider: Provider,
    product: String,
    title: String,
    main: Option<String>,
    install: Install,
}

enum Candidate<'a> {
    Steam(NodeId, &'a str),
    Epic,
    Gog(NodeId, &'a str),
    Itch(NodeId),
}

impl Candidate<'_> {
    fn parse(self, bytes: &[u8]) -> Option<Record> {
        match self {
            Self::Steam(parent, id) => manifests::steam(bytes, parent, id),
            Self::Epic => manifests::epic(bytes),
            Self::Gog(folder, id) => manifests::gog(bytes, folder, id),
            Self::Itch(folder) => manifests::itch(bytes, folder),
        }
    }
}

fn parent<'a>(index: &'a Index, entry: &Entry) -> Option<&'a Entry> {
    index.entry(entry.parent?).ok()
}

fn folder(entry: &Entry, name: &str) -> bool {
    entry.kind == Kind::Directory && entry.name.eq_ignore_ascii_case(name)
}

fn candidate<'a>(index: &Index, entry: &'a Entry) -> Option<Candidate<'a>> {
    if !entry.is_metadata_file() {
        return None;
    }
    let name = entry.name.as_str();
    let steam = manifest_id(name, "appmanifest_", ".acf").filter(|id| number(id));
    let gog = manifest_id(name, "goggame-", ".info").filter(|id| number(id));
    let itch = name.eq_ignore_ascii_case("receipt.json.gz");
    let epic = name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("item"));
    if steam.is_none() && gog.is_none() && !itch && !epic {
        return None;
    }
    let parent = parent(index, entry)?;
    if let Some(id) = steam
        && folder(parent, "steamapps")
    {
        return Some(Candidate::Steam(parent.id, id));
    }
    if let Some(id) = gog
        && parent.kind == Kind::Directory
    {
        return Some(Candidate::Gog(parent.id, id));
    }
    let grandparent = self::parent(index, parent)?;
    if itch && folder(parent, ".itch") && grandparent.kind == Kind::Directory {
        return Some(Candidate::Itch(grandparent.id));
    }
    if epic
        && folder(parent, "Manifests")
        && folder(grandparent, "Data")
        && self::parent(index, grandparent).is_some_and(|node| folder(node, "EpicGamesLauncher"))
    {
        return Some(Candidate::Epic);
    }
    None
}

fn manifest_id<'a>(name: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let end = name.len().checked_sub(suffix.len())?;
    if name.get(..prefix.len())?.eq_ignore_ascii_case(prefix)
        && name.get(end..)?.eq_ignore_ascii_case(suffix)
    {
        name.get(prefix.len()..end)
    } else {
        None
    }
}

const INSTALL: usize = 0;
const SHADERS: usize = 1;
const WORKSHOP: usize = 2;
const USER_DATA: usize = 3;
const DLC: usize = 4;
const CATEGORIES: usize = 5;

struct Measured {
    id: NodeId,
    record: Record,
    size: [Option<u128>; CATEGORIES],
    roots: [u16; CATEGORIES],
    partial: bool,
    dlc: HashSet<String>,
}

impl Measured {
    fn total(&self) -> u128 {
        self.size.iter().flatten().sum()
    }
}

fn assign(
    boundaries: &mut HashMap<NodeId, u16>,
    games: &mut [Measured],
    id: NodeId,
    game: usize,
    category: usize,
) {
    let owner = (2 + game * CATEGORIES + category) as u16;
    match boundaries.get(&id).copied() {
        None => {
            boundaries.insert(id, owner);
            games[game].roots[category] += 1;
            games[game].size[category].get_or_insert(0);
        }
        Some(prior) if prior == owner => {}
        Some(prior) => {
            if prior > 1 {
                let prior = usize::from(prior - 2);
                let (other, category) = (prior / CATEGORIES, prior % CATEGORIES);
                games[other].partial = true;
                games[other].roots[category] -= 1;
                if games[other].roots[category] == 0 {
                    games[other].size[category] = None;
                }
            }
            games[game].partial = true;
            boundaries.insert(id, 1);
        }
    }
}

pub(super) fn query(index: &Index, query: &Query, cancel: &AtomicBool) -> Result<Option<Report>> {
    let stopped = || cancel.load(Ordering::Relaxed);
    let mut records = Vec::new();
    let mut omitted = 0;
    let mut remaining = READ_LIMIT;
    let mut inspected = 0;
    for entry in &index.entries {
        if stopped() {
            return Ok(None);
        }
        let Some(candidate) = candidate(index, entry) else {
            continue;
        };
        if !index.in_scope(entry.id, query.scope) {
            continue;
        }
        if inspected == MANIFEST_LIMIT
            || entry.logical > remaining
            || entry.logical > FILE_LIMIT as u64
        {
            omitted += 1;
            continue;
        }
        inspected += 1;
        remaining -= entry.logical;
        match read_indexed_manifest(index, entry.id, FILE_LIMIT)
            .ok()
            .and_then(|bytes| candidate.parse(&bytes))
        {
            Some(record) => records.push(record),
            None => omitted += 1,
        }
    }

    let manuals: Vec<_> = query
        .manual
        .iter()
        .map(|path| path_key(Path::new(path)))
        .collect();
    let mut absolute: HashSet<_> = manuals.iter().flatten().cloned().collect();
    let mut steam = HashSet::new();
    for record in &records {
        match &record.install {
            Install::Absolute(path) => absolute.extend(path_key(Path::new(path))),
            Install::Steam { steamapps, folder } => {
                steam.insert((*steamapps, folder.to_lowercase()));
            }
            Install::Indexed(_) => {}
        }
    }
    let basenames: HashSet<_> = absolute
        .iter()
        .filter_map(|path| path.rsplit('\\').next())
        .collect();
    let mut absolute_locations = Locations::new();
    let mut steam_locations = Locations::new();
    for entry in &index.entries {
        if stopped() {
            return Ok(None);
        }
        if entry.kind != Kind::Directory {
            continue;
        }
        let basename = entry.name.to_lowercase();
        if (entry.id == NodeId(0) || basenames.contains(basename.as_str()))
            && index.in_scope(entry.id, query.scope)
            && let Some(path) = path_key(&index.path(entry.id)?)
            && absolute.contains(&path)
        {
            observe(&mut absolute_locations, path, entry.id);
        }
        if let Some(common) = parent(index, entry).filter(|node| folder(node, "common"))
            && let Some(steamapps) = common.parent
        {
            let key = (steamapps, basename);
            if steam.contains(&key) && index.in_scope(entry.id, query.scope) {
                observe(&mut steam_locations, key, entry.id);
            }
        }
    }
    let mut games = Vec::<Measured>::new();
    let mut base = HashMap::new();
    let mut dlc = Vec::new();
    let mut boundaries = HashMap::new();
    for record in records {
        if stopped() {
            return Ok(None);
        }
        let located = match &record.install {
            Install::Indexed(id) => Some(*id),
            Install::Absolute(path) => path_key(Path::new(path))
                .and_then(|key| absolute_locations.get(&key).copied().flatten()),
            Install::Steam { steamapps, folder } => steam_locations
                .get(&(*steamapps, folder.to_lowercase()))
                .copied()
                .flatten(),
        };
        let Some(id) = located.filter(|id| local_folder(index, *id, query.scope)) else {
            omitted += 1;
            continue;
        };
        if record.main.is_some() {
            dlc.push((id, record));
        } else {
            let key = (record.provider, record.product.clone(), id);
            if base.contains_key(&key) {
                continue;
            }
            let position = games.len();
            base.insert(key, position);
            games.push(Measured {
                id,
                record,
                size: [None; CATEGORIES],
                roots: [0; CATEGORIES],
                partial: index.entry(id)?.incomplete,
                dlc: HashSet::new(),
            });
            assign(&mut boundaries, &mut games, id, position, INSTALL);
        }
    }

    for (id, record) in dlc {
        let Some(main) = record.main else { continue };
        let direct = base.get(&(record.provider, main.clone(), id)).copied();
        let related = direct.or_else(|| {
            if record.provider != Provider::Epic {
                return None;
            }
            let mut matching = games.iter().enumerate().filter(|(_, game)| {
                game.record.provider == record.provider && game.record.product == main
            });
            let position = matching.next()?.0;
            matching.next().is_none().then_some(position)
        });
        if let Some(game) = related {
            if games[game].dlc.insert(record.product) && id != games[game].id {
                assign(&mut boundaries, &mut games, id, game, DLC);
            }
        } else {
            omitted += 1;
        }
    }

    let mut steam_aux = HashMap::<_, Vec<_>>::new();
    let mut steam_users = HashMap::<_, Vec<_>>::new();
    for (position, game) in games.iter().enumerate() {
        if let Install::Steam { steamapps, .. } = &game.record.install {
            steam_aux
                .entry((*steamapps, game.record.product.clone()))
                .or_default()
                .push(position);
            if let Some(root) = index.entry(*steamapps)?.parent {
                steam_users
                    .entry((root, game.record.product.clone()))
                    .or_default()
                    .push(position);
            }
        }
    }

    let mut limited = false;
    let mut auxiliary = 0;
    if !steam_aux.is_empty() {
        for entry in &index.entries {
            if stopped() {
                return Ok(None);
            }
            if entry.kind == Kind::Directory
                && number(&entry.name)
                && let Some((category, claims)) =
                    steam_scopes(index, entry, &steam_aux, &steam_users)
                && index.in_scope(entry.id, query.scope)
            {
                if auxiliary == AUXILIARY_LIMIT {
                    limited = true;
                    break;
                }
                auxiliary += 1;
                for &game in claims {
                    assign(&mut boundaries, &mut games, entry.id, game, category);
                }
            }
        }
    }
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
        if owner == OUTSIDE {
            continue;
        }
        if owner > 1 {
            let position = usize::from(owner - 2);
            let game = &mut games[position / CATEGORIES];
            game.partial |= entry.incomplete;
            if entry.is_metadata_file() {
                *game.size[position % CATEGORIES].get_or_insert(0) += u128::from(entry.logical);
            }
        } else if entry.is_metadata_file() {
            if owner == 1 {
                shared += u128::from(entry.logical);
            } else {
                unattributed += u128::from(entry.logical);
            }
        }
    }

    let manual = measure_manual(index, query.scope, manuals, &absolute_locations);
    games.sort_unstable_by(|left, right| {
        (Reverse(left.total()), left.id.0, &left.record.product).cmp(&(
            Reverse(right.total()),
            right.id.0,
            &right.record.product,
        ))
    });
    let total = games.len();
    let mut items = Vec::new();
    for game in games.into_iter().skip(query.offset).take(query.limit) {
        if stopped() {
            return Ok(None);
        }
        let [installation, shaders, workshop, user_data, dlc] =
            game.size.map(|size| size.map(|value| value.to_string()));
        items.push(Game {
            id: game.id,
            provider: game.record.provider,
            product: game.record.product,
            title: game.record.title,
            path: index.path(game.id)?.to_string_lossy().into_owned(),
            installation_bytes: installation,
            shaders_bytes: shaders,
            workshop_bytes: workshop,
            user_data_bytes: user_data,
            dlc_bytes: dlc,
            dlc_records: game.dlc.len(),
            partial: game.partial,
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
        omitted_manifests: omitted,
        coverage_partial: omitted > 0
            || limited
            || index.entry(query.scope)?.incomplete
            || !index.exclusions.is_empty()
            || index
                .observation
                .as_ref()
                .is_none_or(|observation| !observation.complete),
        manual,
    }))
}

type SteamOwners = HashMap<(NodeId, String), Vec<usize>>;

fn steam_scopes<'a>(
    index: &Index,
    entry: &Entry,
    steam: &'a SteamOwners,
    users: &'a SteamOwners,
) -> Option<(usize, &'a [usize])> {
    let directory = parent(index, entry)?;
    let grandparent = parent(index, directory)?;
    let (root, category, claims) =
        if folder(directory, "shadercache") && folder(grandparent, "steamapps") {
            (grandparent.id, SHADERS, steam)
        } else if folder(directory, "content") && folder(grandparent, "workshop") {
            let steamapps = parent(index, grandparent).filter(|node| folder(node, "steamapps"))?;
            (steamapps.id, WORKSHOP, steam)
        } else if number(&directory.name) && folder(grandparent, "userdata") {
            (grandparent.parent?, USER_DATA, users)
        } else {
            return None;
        };
    Some((category, claims.get(&(root, entry.name.clone()))?))
}
