use super::{Store, decode, read_observation, unsigned};
use crate::{
    Error, Kind, Result,
    index::{ACCOUNTING_VERSION, Observation},
};
use rusqlite::params;
use serde::Serialize;
use std::{
    cmp::Reverse,
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

const MAP_LIMIT: usize = 160;
const FOLDER_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Accounting {
    Compatible,
    Unknown,
}

pub(super) fn accounting(
    before: Option<&Observation>,
    after: Option<&Observation>,
) -> Result<Accounting> {
    if [before, after]
        .into_iter()
        .flatten()
        .any(|o| o.accounting_version != ACCOUNTING_VERSION)
    {
        return Err(Error::Invalid(
            "Snapshot accounting revisions are incompatible with this comparison.".into(),
        ));
    }
    Ok(if before.is_some() && after.is_some() {
        Accounting::Compatible
    } else {
        Accounting::Unknown
    })
}

#[derive(Serialize)]
pub struct VisualEntry {
    pub path_id: String,
    pub name: String,
    pub kind: Kind,
    pub logical: String,
    pub incomplete: bool,
}

#[derive(Serialize)]
pub struct VisualRemainder {
    pub logical: String,
    pub entries: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualPresence {
    Present,
    NotRecorded,
    NotObserved,
}

#[derive(Serialize)]
pub struct VisualFrame {
    pub snapshot: i64,
    pub captured: u64,
    pub observation: Option<Observation>,
    pub presence: VisualPresence,
    pub kind: Option<Kind>,
    pub logical: Option<String>,
    pub incomplete: bool,
    pub child_count: usize,
    pub entries: Vec<VisualEntry>,
    pub remainder: Option<VisualRemainder>,
}

#[derive(Serialize)]
pub struct FolderChoice {
    pub path_id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct FolderChoices {
    pub items: Vec<FolderChoice>,
    pub previous: Option<String>,
    pub next: Option<String>,
}

#[derive(Serialize)]
pub struct VisualPair {
    pub path_id: String,
    pub parent_id: Option<String>,
    pub path: String,
    pub accounting: Accounting,
    pub before: VisualFrame,
    pub after: VisualFrame,
    pub delta: Option<String>,
    pub relative_percent: Option<f64>,
    pub folders: FolderChoices,
}

struct Source {
    id: i64,
    root: i64,
    captured: u64,
    complete: bool,
    observation: Option<Observation>,
}

fn invalid() -> Error {
    Error::Invalid("Invalid historical folder evidence.".into())
}
fn path_id(value: &str) -> Result<i64> {
    let id: i64 = value.parse().map_err(|_| invalid())?;
    if id <= 0 || id.to_string() != value {
        return Err(invalid());
    }
    Ok(id)
}
fn kind(value: i64) -> Result<Kind> {
    match value {
        0 => Ok(Kind::Directory),
        1 => Ok(Kind::File),
        2 => Ok(Kind::Link),
        _ => Err(invalid()),
    }
}

impl Store {
    fn visual_source(&self, id: i64) -> Result<Source> {
        Ok(self.connection.query_row("SELECT root_id,captured,complete,observed_at,observed_complete,engine_version,accounting_version FROM snapshots WHERE id=?1", [id], |r| Ok(Source {
            id, root: r.get(0)?, captured: unsigned(r, 1)?, complete: r.get(2)?, observation: read_observation(r, 3)?,
        }))?)
    }

    /// Bounded map summaries and a keyset folder page; never loads or publishes an Index.
    pub fn visual_pair(
        &self,
        before: i64,
        after: i64,
        scope: Option<&str>,
        folder_cursor: Option<&str>,
        reverse: bool,
        cancel: &AtomicBool,
    ) -> Result<Option<VisualPair>> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if before == after {
            return Err(Error::Invalid("Choose two different snapshots.".into()));
        }
        let _read = self.connection.unchecked_transaction()?;
        let old = self.visual_source(before)?;
        let new = self.visual_source(after)?;
        if old.root != new.root {
            return Err(Error::Invalid(
                "Compare snapshots from the same root, volume and exclusions.".into(),
            ));
        }
        let accounting = accounting(old.observation.as_ref(), new.observation.as_ref())?;
        let scope = match scope {
            Some(value) => path_id(value)?,
            None => {
                let mut statement = self.connection.prepare(
                    "SELECT id FROM snapshot_paths WHERE root_id=?1 AND parent_id=0 LIMIT 2",
                )?;
                let mut roots = statement.query([old.root])?;
                let id: i64 = roots.next()?.ok_or_else(invalid)?.get(0)?;
                if id <= 0 || roots.next()?.is_some() {
                    return Err(invalid());
                }
                id
            }
        };
        let parent: i64 = self.connection.query_row(
            "SELECT parent_id FROM snapshot_paths WHERE root_id=?1 AND id=?2",
            params![old.root, scope],
            |r| r.get(0),
        )?;
        if parent < 0 || parent == scope {
            return Err(invalid());
        }
        let Some(before) = self.visual_frame(&old, scope, cancel)? else {
            return Ok(None);
        };
        let Some(after) = self.visual_frame(&new, scope, cancel)? else {
            return Ok(None);
        };
        if before.logical.is_none() && after.logical.is_none() {
            return Err(invalid());
        }
        let Some(folders) =
            self.visual_folders(&old, &new, scope, folder_cursor, reverse, cancel)?
        else {
            return Ok(None);
        };
        let sizes = before
            .logical
            .as_ref()
            .zip(after.logical.as_ref())
            .map(|(a, b)| -> Result<(u64, u64)> {
                Ok((
                    a.parse().map_err(|_| invalid())?,
                    b.parse().map_err(|_| invalid())?,
                ))
            })
            .transpose()?;
        Ok(Some(VisualPair {
            path_id: scope.to_string(),
            parent_id: (parent != 0).then(|| parent.to_string()),
            path: self.path(old.root, scope)?.to_string_lossy().into_owned(),
            accounting,
            delta: sizes.map(|(a, b)| (i128::from(b) - i128::from(a)).to_string()),
            relative_percent: sizes
                .filter(|(a, _)| *a != 0)
                .map(|(a, b)| (b as f64 - a as f64) / a as f64 * 100.0),
            before,
            after,
            folders,
        }))
    }

    fn visual_frame(
        &self,
        source: &Source,
        scope: i64,
        cancel: &AtomicBool,
    ) -> Result<Option<VisualFrame>> {
        let mut frame = VisualFrame {
            snapshot: source.id,
            captured: source.captured,
            incomplete: !source.complete
                || source.observation.as_ref().is_some_and(|o| !o.complete),
            observation: source.observation.clone(),
            presence: VisualPresence::Present,
            kind: None,
            logical: None,
            child_count: 0,
            entries: Vec::new(),
            remainder: None,
        };
        let mut statement = self.connection.prepare("SELECT v.kind,v.logical,v.incomplete,p.name FROM snapshot_paths p JOIN snapshot_versions v ON v.path_id=p.id WHERE p.root_id=?1 AND p.id=?2 AND v.start_id<=?3 AND (v.end_id IS NULL OR v.end_id>?3) LIMIT 2")?;
        let mut rows = statement.query(params![source.root, scope, source.id])?;
        let Some(row) = rows.next()? else {
            frame.presence = if frame.incomplete {
                VisualPresence::NotObserved
            } else {
                VisualPresence::NotRecorded
            };
            return Ok(Some(frame));
        };
        let entry_kind = kind(row.get(0)?)?;
        let logical = unsigned(row, 1)?;
        frame.incomplete |= row.get::<_, bool>(2)?;
        let name: Vec<u8> = row.get(3)?;
        frame.kind = Some(entry_kind);
        frame.logical = Some(logical.to_string());
        if rows.next()?.is_some() {
            return Err(invalid());
        }
        if entry_kind != Kind::Directory {
            frame.entries.push(VisualEntry {
                path_id: scope.to_string(),
                name: decode(&name)?.to_string_lossy().into_owned(),
                kind: entry_kind,
                logical: logical.to_string(),
                incomplete: frame.incomplete,
            });
            return Ok(Some(frame));
        }
        let mut statement = self.connection.prepare("SELECT p.id,p.name,v.kind,v.logical,v.incomplete,(SELECT COUNT(*) FROM snapshot_versions x WHERE x.path_id=p.id AND x.start_id<=?3 AND (x.end_id IS NULL OR x.end_id>?3)) FROM snapshot_paths p JOIN snapshot_versions v ON v.path_id=p.id WHERE p.root_id=?1 AND p.parent_id=?2 AND v.start_id<=?3 AND (v.end_id IS NULL OR v.end_id>?3)")?;
        let mut rows = statement.query(params![source.root, scope, source.id])?;
        let mut ranked = BTreeMap::new();
        let mut observed = 0_u128;
        while let Some(row) = rows.next()? {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let id: i64 = row.get(0)?;
            if id <= 0 || row.get::<_, i64>(5)? != 1 {
                return Err(invalid());
            }
            let value = unsigned(row, 3)?;
            let entry_kind = kind(row.get(2)?)?;
            observed += u128::from(value);
            if observed > u128::from(logical) {
                return Err(invalid());
            }
            frame.child_count += 1;
            let key = (value, Reverse(id));
            if ranked.len() < MAP_LIMIT
                || ranked
                    .first_key_value()
                    .is_some_and(|(lowest, _)| key > *lowest)
            {
                let name: Vec<u8> = row.get(1)?;
                ranked.insert(
                    key,
                    VisualEntry {
                        path_id: id.to_string(),
                        name: decode(&name)?.to_string_lossy().into_owned(),
                        kind: entry_kind,
                        logical: value.to_string(),
                        incomplete: row.get(4)?,
                    },
                );
                if ranked.len() > MAP_LIMIT {
                    ranked.pop_first();
                }
            }
        }
        let retained: u64 = ranked.keys().map(|(bytes, _)| *bytes).sum();
        let omitted = frame.child_count - ranked.len();
        if logical > retained || omitted > 0 {
            frame.remainder = Some(VisualRemainder {
                logical: (logical - retained).to_string(),
                entries: omitted,
            });
        }
        frame.entries = ranked.into_iter().rev().map(|(_, entry)| entry).collect();
        Ok(Some(frame))
    }

    fn visual_folders(
        &self,
        before: &Source,
        after: &Source,
        scope: i64,
        cursor: Option<&str>,
        reverse: bool,
        cancel: &AtomicBool,
    ) -> Result<Option<FolderChoices>> {
        if reverse && cursor.is_none() {
            return Err(invalid());
        }
        let cursor_name: Option<Vec<u8>> = cursor
            .map(|value| -> Result<Vec<u8>> {
                Ok(self.connection.query_row(
                    "SELECT name FROM snapshot_paths WHERE root_id=?1 AND parent_id=?2 AND id=?3",
                    params![before.root, scope, path_id(value)?],
                    |r| r.get(0),
                )?)
            })
            .transpose()?;
        let direction = if reverse { "DESC" } else { "ASC" };
        let comparison = if reverse { "<" } else { ">" };
        let cursor_clause = if cursor_name.is_some() {
            format!("p.name {comparison} ?5")
        } else {
            "?5 IS NULL".into()
        };
        let sql = format!(
            "SELECT p.id,p.name,(SELECT v.kind FROM snapshot_versions v WHERE v.path_id=p.id AND v.start_id<=?3 AND (v.end_id IS NULL OR v.end_id>?3)),(SELECT v.kind FROM snapshot_versions v WHERE v.path_id=p.id AND v.start_id<=?4 AND (v.end_id IS NULL OR v.end_id>?4)) FROM snapshot_paths p WHERE p.root_id=?1 AND p.parent_id=?2 AND {cursor_clause} ORDER BY p.name {direction}"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params![
            before.root,
            scope,
            before.id,
            after.id,
            cursor_name
        ])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            if row.get::<_, Option<i64>>(2)? != Some(0) && row.get::<_, Option<i64>>(3)? != Some(0)
            {
                continue;
            }
            let name: Vec<u8> = row.get(1)?;
            let id: i64 = row.get(0)?;
            if id <= 0 {
                return Err(invalid());
            }
            items.push(FolderChoice {
                path_id: id.to_string(),
                name: decode(&name)?.to_string_lossy().into_owned(),
            });
            if items.len() > FOLDER_LIMIT {
                break;
            }
        }
        let more = items.len() > FOLDER_LIMIT;
        items.truncate(FOLDER_LIMIT);
        if reverse {
            items.reverse();
        }
        let previous = (if reverse { more } else { cursor.is_some() })
            .then(|| items.first().map(|item| item.path_id.clone()))
            .flatten();
        let next = (if reverse { cursor.is_some() } else { more })
            .then(|| items.last().map(|item| item.path_id.clone()))
            .flatten();
        Ok(Some(FolderChoices {
            items,
            previous,
            next,
        }))
    }
}
