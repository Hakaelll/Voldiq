use super::{Data, Store, encode, encode_paths, integer, read_observation, unsigned};
use crate::{
    NodeId, Result,
    index::{ACCOUNTING_VERSION, Index},
};
use rusqlite::{OptionalExtension, params};
use std::{
    ffi::OsStr,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) struct HistoryPoint {
    pub at: u64,
    pub logical: u64,
}
impl Store {
    pub(crate) fn indexed_folder_identity(
        &self,
        index: &Index,
        node: NodeId,
    ) -> Result<Option<(u64, u64)>> {
        let tx = self.connection.unchecked_transaction()?;
        let Some((root, path)) = folder_key(&tx, index, node)? else {
            return Ok(None);
        };
        recorded_identity(&tx, root, path, index.snapshot_id)
    }
    /// At most 30 daily snapshot seeks and scoped version lookups; no historical tree is loaded.
    pub(crate) fn budget_history(
        &self,
        index: &Index,
        node: NodeId,
        at: u64,
        cancel: &AtomicBool,
    ) -> Result<Option<Vec<HistoryPoint>>> {
        let tx = self.connection.unchecked_transaction()?;
        let Some((root, path)) = folder_key(&tx, index, node)? else {
            return Ok(Some(Vec::new()));
        };
        let expected = match index.entry(node)?.file_state {
            Some(state) => Some(state.identity),
            None => recorded_identity(&tx, root, path, index.snapshot_id)?,
        };
        let mut latest = tx.prepare("SELECT id,observed_at,observed_complete,engine_version,accounting_version,complete,logical FROM snapshots
            WHERE root_id=?1 AND observed_at>=?2 AND observed_at<=?3 AND id<=?4 ORDER BY observed_at DESC,id DESC LIMIT 1")?;
        let mut versions = tx.prepare("SELECT kind,logical,allocated,modified,files,known,incomplete,hard_link,native_state,cloud FROM snapshot_versions
            WHERE path_id=?1 AND start_id<=?2 AND (end_id IS NULL OR end_id>?2) LIMIT 2")?;
        let mut points = Vec::new();
        let day = at / 86400;
        for ago in (0..30).rev() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            if day < ago {
                continue;
            }
            let start = (day - ago) * 86400;
            let end = (start + 86399).min(at);
            let record = latest
                .query_row(
                    params![
                        root,
                        integer(start)?,
                        integer(end)?,
                        index.snapshot_id.unwrap_or(i64::MAX)
                    ],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            read_observation(r, 1)?,
                            r.get::<_, bool>(5)?,
                            unsigned(r, 6)?,
                        ))
                    },
                )
                .optional()?;
            let Some((snapshot, observation, complete, root_bytes)) = record else {
                continue;
            };
            let Some(observation) = observation
                .filter(|o| o.complete && o.accounting_version == ACCOUNTING_VERSION && complete)
            else {
                points.clear();
                continue;
            };
            let logical = if node == NodeId(0) {
                Some(root_bytes)
            } else {
                let mut rows = versions.query(params![path, snapshot])?;
                let data = rows.next()?.map(Data::read).transpose()?;
                if rows.next()?.is_some() {
                    points.clear();
                    continue;
                }
                data.filter(|data| {
                    data.kind == 0
                        && !data.incomplete
                        && expected.is_some()
                        && data.native_state.map(|state| state.identity) == expected
                })
                .map(|data| data.logical)
            };
            if let Some(logical) = logical {
                points.push(HistoryPoint {
                    at: observation.completed_at,
                    logical,
                });
            } else {
                points.clear();
            }
        }
        Ok(Some(points))
    }
}

fn recorded_identity(
    connection: &rusqlite::Connection,
    root: i64,
    path: i64,
    snapshot: Option<i64>,
) -> Result<Option<(u64, u64)>> {
    let Some(snapshot) = snapshot else {
        return Ok(None);
    };
    let mut statement = connection.prepare("SELECT v.kind,v.logical,v.allocated,v.modified,v.files,v.known,v.incomplete,v.hard_link,v.native_state,v.cloud FROM snapshot_versions v JOIN snapshots s ON s.id=?1
        WHERE s.root_id=?2 AND v.path_id=?3 AND v.start_id<=s.id AND (v.end_id IS NULL OR v.end_id>s.id) LIMIT 2")?;
    let mut rows = statement.query(params![snapshot, root, path])?;
    let data = rows.next()?.map(Data::read).transpose()?;
    if rows.next()?.is_some() {
        return Err(crate::Error::Invalid(
            "Ambiguous recorded folder identity.".into(),
        ));
    }
    Ok(data
        .filter(|data| data.kind == 0)
        .and_then(|data| data.native_state)
        .map(|state| state.identity))
}

fn folder_key(
    connection: &rusqlite::Connection,
    index: &Index,
    node: NodeId,
) -> Result<Option<(i64, i64)>> {
    let Some(identity) = index.root_identity else {
        return Ok(None);
    };

    let root: Option<i64> = connection
        .query_row(
            "SELECT id FROM snapshot_roots WHERE path=?1 AND identity=?2 AND coverage=?3",
            params![
                encode(index.root.as_os_str()),
                [identity.0.to_le_bytes(), identity.1.to_le_bytes()].concat(),
                encode_paths(&index.exclusions)?
            ],
            |r| r.get(0),
        )
        .optional()?;
    let Some(root) = root else {
        return Ok(None);
    };
    let mut path: Option<i64> = connection
        .query_row(
            "SELECT id FROM snapshot_paths WHERE root_id=?1 AND parent_id=0 AND name=?2",
            params![root, encode(OsStr::new(""))],
            |r| r.get(0),
        )
        .optional()?;
    let mut chain = Vec::new();
    let mut cursor = node;
    while cursor != NodeId(0) {
        let entry = index.entry(cursor)?;
        chain.push(
            index
                .native_names
                .get(&cursor)
                .map_or_else(|| OsStr::new(&entry.name), |n| n.as_os_str()),
        );
        let Some(parent) = entry.parent else {
            return Ok(None);
        };
        if parent.0 >= cursor.0 {
            return Ok(None);
        }
        cursor = parent;
    }
    for name in chain.into_iter().rev() {
        let Some(parent) = path else {
            return Ok(None);
        };
        path = connection
            .query_row(
                "SELECT id FROM snapshot_paths WHERE root_id=?1 AND parent_id=?2 AND name=?3",
                params![root, parent, encode(name)],
                |r| r.get(0),
            )
            .optional()?;
    }
    let Some(path) = path else {
        return Ok(None);
    };
    Ok(Some((root, path)))
}
