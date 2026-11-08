use super::*;
use crate::libraries::{Capture, MAX_CAPTURE_BYTES, MAX_ROOTS};

pub(super) const SCHEMA: &str = "
CREATE TABLE snapshot_libraries(
 snapshot_id INTEGER PRIMARY KEY REFERENCES snapshots(id) ON DELETE CASCADE,
 evidence BLOB NOT NULL CHECK(length(evidence)<=1048576));
CREATE TABLE snapshot_library_nodes(
 snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
 source_node INTEGER NOT NULL CHECK(source_node BETWEEN 0 AND 4294967295),
 path_id INTEGER NOT NULL REFERENCES snapshot_paths(id) ON DELETE CASCADE,
 PRIMARY KEY(snapshot_id,source_node), UNIQUE(snapshot_id,path_id));
PRAGMA user_version=10;";

fn invalid() -> Error {
    Error::Invalid("Invalid captured library evidence.".into())
}

pub(super) fn write(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: i64,
    paths: &[i64],
    index: &Index,
    capture: &Capture,
    cancel: &AtomicBool,
) -> Result<()> {
    capture.validate(index)?;
    let evidence = serde_json::to_vec(capture).map_err(|_| invalid())?;
    if evidence.len() > MAX_CAPTURE_BYTES {
        return Err(invalid());
    }
    transaction.execute(
        "INSERT INTO snapshot_libraries(snapshot_id,evidence) VALUES(?1,?2)",
        params![snapshot, evidence],
    )?;
    let mut insert = transaction.prepare(
        "INSERT INTO snapshot_library_nodes(snapshot_id,source_node,path_id) VALUES(?1,?2,?3)",
    )?;
    for node in capture.roots() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Invalid("Snapshot capture cancelled.".into()));
        }
        let path = paths.get(node.0 as usize).ok_or_else(invalid)?;
        insert.execute(params![snapshot, node.0, path])?;
    }
    Ok(())
}

impl Store {
    pub(super) fn load_libraries(
        &self,
        snapshot: i64,
        paths: &HashMap<i64, NodeId>,
        index: &Index,
    ) -> Result<Option<Capture>> {
        let payload: Option<Option<Vec<u8>>> = self.connection.query_row(
            "SELECT CASE WHEN length(evidence)<=?2 THEN evidence END FROM snapshot_libraries WHERE snapshot_id=?1",
            params![snapshot, MAX_CAPTURE_BYTES as i64], |row| row.get(0),
        ).optional()?;
        let Some(payload) = payload else {
            return Ok(None);
        };
        let mut capture: Capture =
            serde_json::from_slice(&payload.ok_or_else(invalid)?).map_err(|_| invalid())?;
        let expected = capture.roots();
        if expected.len() > MAX_ROOTS {
            return Err(invalid());
        }
        let mut nodes = HashMap::new();
        let mut statement = self.connection.prepare(
            "SELECT source_node,path_id FROM snapshot_library_nodes WHERE snapshot_id=?1 LIMIT ?2",
        )?;
        let mut rows = statement.query(params![snapshot, MAX_ROOTS as i64 + 1])?;
        while let Some(row) = rows.next()? {
            let source = NodeId(row.get(0)?);
            let path: i64 = row.get(1)?;
            if !expected.contains(&source)
                || nodes
                    .insert(source, *paths.get(&path).ok_or_else(invalid)?)
                    .is_some()
            {
                return Err(invalid());
            }
        }
        if nodes.len() != expected.len() {
            return Err(invalid());
        }
        capture.remap(|id| nodes.get(&id).copied().ok_or_else(invalid))?;
        capture.validate(index)?;
        Ok(Some(capture))
    }
}
