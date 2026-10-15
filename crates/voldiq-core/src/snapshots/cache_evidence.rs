use super::*;
use crate::developer::{CACHE_TOOLS, CacheEvidence, Category};

pub(super) const SCHEMA: &str = "
CREATE TABLE snapshot_caches(
 snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
 path_id INTEGER NOT NULL REFERENCES snapshot_paths(id) ON DELETE CASCADE,
 tool TEXT NOT NULL, category INTEGER NOT NULL CHECK(category BETWEEN 0 AND 2),
 PRIMARY KEY(snapshot_id,path_id));
PRAGMA user_version=4;";

pub(super) fn write(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: i64,
    paths: &[i64],
    evidence: &[CacheEvidence],
) -> Result<()> {
    let mut insert = transaction.prepare(
        "INSERT INTO snapshot_caches(snapshot_id,path_id,tool,category) VALUES(?1,?2,?3,?4)",
    )?;
    for cache in evidence {
        let category = match cache.category {
            Category::Dependencies => 0,
            Category::Build => 1,
            Category::Cache => 2,
        };
        let path = paths
            .get(cache.id.0 as usize)
            .ok_or_else(|| Error::Invalid("Invalid captured cache path.".into()))?;
        insert.execute(params![snapshot, path, cache.tool, category])?;
    }
    Ok(())
}

impl Store {
    pub(super) fn load_caches(
        &self,
        snapshot: i64,
        paths: &HashMap<i64, NodeId>,
        index: &Index,
    ) -> Result<Vec<CacheEvidence>> {
        let mut statement = self.connection.prepare(
            "SELECT path_id,tool,category FROM snapshot_caches WHERE snapshot_id=?1 LIMIT 1025",
        )?;
        let mut rows = statement.query([snapshot])?;
        let mut evidence = Vec::new();
        while let Some(row) = rows.next()? {
            if evidence.len() >= 1024 {
                return Err(Error::Invalid("Too many saved cache roots.".into()));
            }
            let path: i64 = row.get(0)?;
            let id = *paths
                .get(&path)
                .ok_or_else(|| Error::Invalid("Saved cache is outside its capture.".into()))?;
            if index.entry(id)?.kind != Kind::Directory {
                return Err(Error::Invalid("Saved cache is not a directory.".into()));
            }
            let name: String = row.get(1)?;
            let tool = CACHE_TOOLS
                .iter()
                .copied()
                .find(|tool| *tool == name)
                .ok_or_else(|| Error::Invalid("Unknown saved cache tool.".into()))?;
            let category = match row.get::<_, i64>(2)? {
                0 => Category::Dependencies,
                1 => Category::Build,
                2 => Category::Cache,
                _ => return Err(Error::Invalid("Invalid saved cache category.".into())),
            };
            evidence.push(CacheEvidence { id, tool, category });
        }
        Ok(evidence)
    }
}
