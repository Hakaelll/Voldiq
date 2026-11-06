use super::*;
use crate::applications::{Capture, capture::MAX_CAPTURE_BYTES};

pub(super) const SCHEMA: &str = "
CREATE TABLE snapshot_applications(
 snapshot_id INTEGER PRIMARY KEY REFERENCES snapshots(id) ON DELETE CASCADE,
 evidence BLOB NOT NULL CHECK(length(evidence)<=8388608));
CREATE TABLE snapshot_application_roots(
 snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
 source_node INTEGER NOT NULL CHECK(source_node BETWEEN 0 AND 4294967295),
 path_id INTEGER NOT NULL REFERENCES snapshot_paths(id) ON DELETE CASCADE,
 PRIMARY KEY(snapshot_id,source_node));
PRAGMA user_version=9;";

pub(super) fn write(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: i64,
    paths: &[i64],
    index: &Index,
    cancel: &AtomicBool,
) -> Result<()> {
    let Some(capture) = &index.captured_applications else {
        return Ok(());
    };
    capture.validate(index)?;
    let payload = serde_json::to_vec(capture).map_err(|error| Error::Invalid(error.to_string()))?;
    if payload.len() > MAX_CAPTURE_BYTES {
        return Err(Error::Invalid(
            "Captured application metadata exceeds its size limit.".into(),
        ));
    }
    transaction.execute(
        "INSERT INTO snapshot_applications(snapshot_id,evidence) VALUES(?1,?2)",
        params![snapshot, payload],
    )?;
    let mut insert = transaction.prepare(
        "INSERT INTO snapshot_application_roots(snapshot_id,source_node,path_id) VALUES(?1,?2,?3)",
    )?;
    for root in capture.roots() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Invalid("Snapshot capture cancelled.".into()));
        }
        let path = paths
            .get(root.0 as usize)
            .ok_or_else(|| Error::Invalid("Invalid captured application root.".into()))?;
        insert.execute(params![snapshot, root.0, path])?;
    }
    Ok(())
}

impl Store {
    pub(super) fn load_applications(
        &self,
        snapshot: i64,
        paths: &HashMap<i64, NodeId>,
        index: &Index,
    ) -> Result<Option<Capture>> {
        let payload: Option<Option<Vec<u8>>> = self.connection.query_row(
            "SELECT CASE WHEN length(evidence)<=?2 THEN evidence END FROM snapshot_applications WHERE snapshot_id=?1",
            params![snapshot, i64::try_from(MAX_CAPTURE_BYTES).map_err(|_| Error::Invalid("Invalid application capture limit.".into()))?], |row| row.get(0)).optional()?;
        let Some(payload) = payload else {
            return Ok(None);
        };
        let payload = payload.ok_or_else(|| {
            Error::Invalid("Captured application metadata exceeds its size limit.".into())
        })?;
        let mut capture: Capture = serde_json::from_slice(&payload)
            .map_err(|_| Error::Invalid("Invalid captured application metadata.".into()))?;
        let expected = capture.roots();
        let mut roots = HashMap::new();
        let mut statement = self.connection.prepare("SELECT source_node,path_id FROM snapshot_application_roots WHERE snapshot_id=?1 LIMIT 2049")?;
        let mut rows = statement.query([snapshot])?;
        while let Some(row) = rows.next()? {
            let node = NodeId(row.get(0)?);
            let path: i64 = row.get(1)?;
            let current = *paths.get(&path).ok_or_else(|| {
                Error::Invalid("Captured application root is outside its snapshot.".into())
            })?;
            if roots.len() >= 2048
                || !expected.contains(&node)
                || roots.insert(node, current).is_some()
            {
                return Err(Error::Invalid(
                    "Invalid captured application root map.".into(),
                ));
            }
        }
        if roots.len() != expected.len() {
            return Err(Error::Invalid(
                "Missing captured application root map.".into(),
            ));
        }
        capture.remap_roots(|node| {
            roots
                .get(&node)
                .copied()
                .ok_or_else(|| Error::Invalid("Missing captured application root.".into()))
        })?;
        capture.validate(index)?;
        Ok(Some(capture))
    }
}
