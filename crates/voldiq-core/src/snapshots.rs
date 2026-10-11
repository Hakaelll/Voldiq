use crate::{Entry, Error, Issue, Kind, NodeId, Result, index::Index};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub struct Store {
    connection: Connection,
    path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub id: i64,
    pub root_key: i64,
    pub root: String,
    pub captured: u64,
    pub logical: u64,
    pub allocated: u64,
    pub files: u64,
    pub entries: u64,
    pub complete: bool,
    pub errors: u64,
}

#[derive(Debug, Serialize)]
pub struct Change {
    pub path: String,
    pub kind: String,
    pub directory: bool,
    pub before: u64,
    pub after: u64,
    pub delta: i64,
}

#[derive(Debug, Serialize)]
pub struct Comparison {
    pub before: i64,
    pub after: i64,
    pub delta: i64,
    pub reliable: bool,
    pub total: usize,
    pub changes: Vec<Change>,
}

#[derive(Debug, PartialEq)]
struct Data {
    kind: i64,
    logical: u64,
    allocated: u64,
    modified: Option<u64>,
    files: u64,
    known: bool,
    incomplete: bool,
    hard_link: bool,
}

impl Data {
    fn from_entry(entry: &Entry) -> Self {
        Self {
            kind: match entry.kind {
                Kind::Directory => 0,
                Kind::File => 1,
                Kind::Link => 2,
            },
            logical: entry.logical,
            allocated: entry.allocated,
            modified: entry.modified,
            files: entry.files,
            known: entry.allocation_known,
            incomplete: entry.incomplete,
            hard_link: entry.hard_link,
        }
    }
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            kind: row.get(0)?,
            logical: unsigned(row, 1)?,
            allocated: unsigned(row, 2)?,
            modified: optional_unsigned(row, 3)?,
            files: unsigned(row, 4)?,
            known: row.get(5)?,
            incomplete: row.get(6)?,
            hard_link: row.get(7)?,
        })
    }
}

const SCHEMA: &str = "
CREATE TABLE snapshot_roots(id INTEGER PRIMARY KEY, path BLOB NOT NULL, identity BLOB NOT NULL, coverage BLOB NOT NULL, UNIQUE(path,identity,coverage));
CREATE TABLE snapshots(id INTEGER PRIMARY KEY AUTOINCREMENT, root_id INTEGER NOT NULL REFERENCES snapshot_roots(id) ON DELETE CASCADE,
 captured INTEGER NOT NULL, logical INTEGER NOT NULL, allocated INTEGER NOT NULL, files INTEGER NOT NULL,
 entries INTEGER NOT NULL, complete INTEGER NOT NULL, errors INTEGER NOT NULL);
CREATE INDEX snapshots_root ON snapshots(root_id,id);
CREATE TABLE snapshot_paths(id INTEGER PRIMARY KEY, root_id INTEGER NOT NULL REFERENCES snapshot_roots(id) ON DELETE CASCADE,
 parent_id INTEGER NOT NULL, name BLOB NOT NULL, UNIQUE(root_id,parent_id,name));
CREATE TABLE snapshot_versions(path_id INTEGER NOT NULL REFERENCES snapshot_paths(id) ON DELETE CASCADE,
 start_id INTEGER NOT NULL, end_id INTEGER, kind INTEGER NOT NULL, logical INTEGER NOT NULL, allocated INTEGER NOT NULL,
 modified INTEGER, files INTEGER NOT NULL, known INTEGER NOT NULL, incomplete INTEGER NOT NULL, hard_link INTEGER NOT NULL,
 PRIMARY KEY(path_id,start_id));
CREATE UNIQUE INDEX active_version ON snapshot_versions(path_id) WHERE end_id IS NULL;
CREATE TABLE snapshot_issues(snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE, path TEXT NOT NULL, message TEXT NOT NULL);
PRAGMA user_version=1;";

const DIFFERENCES: &str = "
FROM snapshot_paths p
LEFT JOIN snapshot_versions o ON o.path_id=p.id AND o.start_id<=?1 AND (o.end_id IS NULL OR o.end_id>?1)
LEFT JOIN snapshot_versions n ON n.path_id=p.id AND n.start_id<=?2 AND (n.end_id IS NULL OR n.end_id>?2)
WHERE p.root_id=?3 AND (o.path_id IS NOT NULL OR n.path_id IS NOT NULL)
AND (o.path_id IS NULL OR n.path_id IS NULL OR o.kind!=n.kind OR o.logical!=n.logical OR o.allocated!=n.allocated
 OR o.modified IS NOT n.modified OR o.files!=n.files OR o.known!=n.known OR o.incomplete!=n.incomplete OR o.hard_link!=n.hard_link)";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
        )?;
        let version: u32 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 0 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(SCHEMA)?;
            transaction.commit()?;
        } else if version != 1 {
            return Err(Error::Invalid(
                "Unsupported snapshot database version.".into(),
            ));
        }
        Ok(Self {
            connection,
            path: path.to_owned(),
        })
    }

    pub fn save(&mut self, index: &Index, cancel: &AtomicBool) -> Result<i64> {
        let root = index.entry(NodeId(0))?;
        let identity = index
            .root_identity
            .ok_or_else(|| Error::Invalid("The scan has no verified volume identity.".into()))?;
        let identity = [identity.0.to_le_bytes(), identity.1.to_le_bytes()].concat();
        let coverage = encode_paths(&index.exclusions)?;
        let captured = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::Invalid(e.to_string()))?
            .as_secs();
        self.connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS captured_paths(id INTEGER PRIMARY KEY); DELETE FROM captured_paths;")?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO snapshot_roots(path,identity,coverage) VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",
            params![encode(index.root.as_os_str()), identity, coverage],
        )?;
        let root_id: i64 = transaction.query_row(
            "SELECT id FROM snapshot_roots WHERE path=?1 AND identity=?2 AND coverage=?3",
            params![encode(index.root.as_os_str()), identity, coverage],
            |r| r.get(0),
        )?;
        transaction.execute("INSERT INTO snapshots(root_id,captured,logical,allocated,files,entries,complete,errors)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![root_id, integer(captured)?, integer(root.logical)?, integer(root.allocated)?,
                integer(root.files)?, position_integer(index.entries.len())?, !root.incomplete && index.error_count == 0, integer(index.error_count)?])?;
        let snapshot_id = transaction.last_insert_rowid();
        let mut paths = Vec::<i64>::with_capacity(index.entries.len());
        {
            let mut lookup_path = transaction.prepare(
                "SELECT id FROM snapshot_paths WHERE root_id=?1 AND parent_id=?2 AND name=?3",
            )?;
            let mut insert_path = transaction
                .prepare("INSERT INTO snapshot_paths(root_id,parent_id,name) VALUES(?1,?2,?3)")?;
            let mut lookup_data = transaction.prepare(
                "SELECT kind,logical,allocated,modified,files,known,incomplete,hard_link
                FROM snapshot_versions WHERE path_id=?1 AND end_id IS NULL",
            )?;
            let mut expire = transaction.prepare(
                "UPDATE snapshot_versions SET end_id=?1 WHERE path_id=?2 AND end_id IS NULL",
            )?;
            let mut insert = transaction.prepare("INSERT INTO snapshot_versions(path_id,start_id,kind,logical,allocated,modified,files,known,incomplete,hard_link)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)")?;
            let mut seen = transaction.prepare("INSERT INTO captured_paths(id) VALUES(?1)")?;
            for (position, entry) in index.entries.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Invalid("Snapshot capture cancelled.".into()));
                }
                if entry.id.0 as usize != position {
                    return Err(Error::Invalid("Invalid scan entry order.".into()));
                }
                let parent = match entry.parent {
                    Some(parent) => *paths
                        .get(parent.0 as usize)
                        .ok_or_else(|| Error::Invalid("Invalid scan parent.".into()))?,
                    None if position == 0 => 0,
                    None => return Err(Error::Invalid("The scan has multiple roots.".into())),
                };
                let native = if position == 0 {
                    OsStr::new("")
                } else {
                    index
                        .native_names
                        .get(&entry.id)
                        .map(|n| n.as_os_str())
                        .unwrap_or_else(|| OsStr::new(&entry.name))
                };
                if position != 0 {
                    validate_name(native)?;
                }
                let name = encode(native);
                let path_id = match lookup_path
                    .query_row(params![root_id, parent, name], |r| r.get::<_, i64>(0))
                    .optional()?
                {
                    Some(id) => id,
                    None => {
                        insert_path.execute(params![root_id, parent, name])?;
                        transaction.last_insert_rowid()
                    }
                };
                paths.push(path_id);
                seen.execute([path_id])?;
                let data = Data::from_entry(entry);
                let previous = lookup_data.query_row([path_id], Data::read).optional()?;
                if previous.as_ref() != Some(&data) {
                    expire.execute(params![snapshot_id, path_id])?;
                    insert.execute(params![
                        path_id,
                        snapshot_id,
                        data.kind,
                        integer(data.logical)?,
                        integer(data.allocated)?,
                        data.modified.map(integer).transpose()?,
                        integer(data.files)?,
                        data.known,
                        data.incomplete,
                        data.hard_link
                    ])?;
                }
            }
        }
        transaction.execute("UPDATE snapshot_versions SET end_id=?1 WHERE end_id IS NULL AND path_id IN
            (SELECT p.id FROM snapshot_paths p LEFT JOIN captured_paths s ON s.id=p.id WHERE p.root_id=?2 AND s.id IS NULL)", params![snapshot_id, root_id])?;
        for issue in &index.issues {
            transaction.execute(
                "INSERT INTO snapshot_issues(snapshot_id,path,message) VALUES(?1,?2,?3)",
                params![snapshot_id, issue.path, issue.message],
            )?;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Invalid("Snapshot capture cancelled.".into()));
        }
        transaction.commit()?;
        self.connection
            .execute_batch("DROP TABLE captured_paths;")?;
        Ok(snapshot_id)
    }

    pub fn list(&self, offset: usize, limit: usize) -> Result<Vec<Snapshot>> {
        let mut statement = self.connection.prepare("SELECT s.id,r.path,s.captured,s.logical,s.allocated,s.files,s.entries,s.complete,s.errors,s.root_id
            FROM snapshots s JOIN snapshot_roots r ON r.id=s.root_id ORDER BY s.id DESC LIMIT ?1 OFFSET ?2")?;
        let rows = statement.query_map(
            params![
                position_integer(limit.clamp(1, 100))?,
                position_integer(offset)?
            ],
            |row| {
                let path: Vec<u8> = row.get(1)?;
                Ok((
                    Snapshot {
                        id: row.get(0)?,
                        root_key: row.get(9)?,
                        root: String::new(),
                        captured: unsigned(row, 2)?,
                        logical: unsigned(row, 3)?,
                        allocated: unsigned(row, 4)?,
                        files: unsigned(row, 5)?,
                        entries: unsigned(row, 6)?,
                        complete: row.get(7)?,
                        errors: unsigned(row, 8)?,
                    },
                    path,
                ))
            },
        )?;
        rows.map(|row| {
            let (mut snapshot, path) = row?;
            snapshot.root = decode(&path)?.to_string_lossy().into_owned();
            Ok(snapshot)
        })
        .collect()
    }

    pub fn load(&self, id: i64) -> Result<Index> {
        let (root_id, path, identity, coverage, errors) = self.connection.query_row(
            "SELECT s.root_id,r.path,r.identity,r.coverage,s.errors FROM snapshots s JOIN snapshot_roots r ON r.id=s.root_id WHERE s.id=?1",
            [id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, Vec<u8>>(2)?, r.get::<_, Vec<u8>>(3)?, unsigned(r, 4)?)))?;
        if identity.len() != 16 {
            return Err(Error::Invalid("Invalid snapshot volume identity.".into()));
        }
        let number = |slice: &[u8]| -> Result<u64> {
            Ok(u64::from_le_bytes(slice.try_into().map_err(|_| {
                Error::Invalid("Invalid identity length.".into())
            })?))
        };
        let mut index = Index {
            root: PathBuf::from(decode(&path)?),
            entries: Vec::new(),
            issues: Vec::new(),
            error_count: errors,
            native_names: HashMap::new(),
            root_identity: Some((number(&identity[..8])?, number(&identity[8..])?)),
            exclusions: decode_paths(&coverage)?,
            snapshot_id: Some(id),
        };
        if !index.root.is_absolute() {
            return Err(Error::Invalid("Snapshot root is not absolute.".into()));
        }
        let mut paths = HashMap::<i64, NodeId>::new();
        let mut statement = self.connection.prepare("SELECT v.kind,v.logical,v.allocated,v.modified,v.files,v.known,v.incomplete,v.hard_link,p.id,p.parent_id,p.name
            FROM snapshot_paths p JOIN snapshot_versions v ON v.path_id=p.id WHERE p.root_id=?1 AND v.start_id<=?2 AND (v.end_id IS NULL OR v.end_id>?2) ORDER BY p.id")?;
        let mut rows = statement.query(params![root_id, id])?;
        while let Some(row) = rows.next()? {
            let data = Data::read(row)?;
            let path_id: i64 = row.get(8)?;
            let parent: i64 = row.get(9)?;
            let native = decode(&row.get::<_, Vec<u8>>(10)?)?;
            let node = NodeId(
                u32::try_from(index.entries.len())
                    .map_err(|_| Error::Invalid("Snapshot exceeds node limit.".into()))?,
            );
            let parent = if parent == 0 {
                if !index.entries.is_empty() {
                    return Err(Error::Invalid("Snapshot has multiple roots.".into()));
                }
                None
            } else {
                validate_name(&native)?;
                Some(
                    *paths
                        .get(&parent)
                        .ok_or_else(|| Error::Invalid("Snapshot parent is missing.".into()))?,
                )
            };
            let name = if parent.is_none() {
                index
                    .root
                    .file_name()
                    .unwrap_or(index.root.as_os_str())
                    .to_string_lossy()
                    .into_owned()
            } else {
                native.to_string_lossy().into_owned()
            };
            if parent.is_some() && native.to_str().is_none() {
                index.native_names.insert(node, native);
            }
            index.entries.push(Entry {
                id: node,
                parent,
                name,
                kind: match data.kind {
                    0 => Kind::Directory,
                    1 => Kind::File,
                    2 => Kind::Link,
                    _ => return Err(Error::Invalid("Invalid snapshot entry kind.".into())),
                },
                logical: data.logical,
                allocated: data.allocated,
                modified: data.modified,
                files: data.files,
                allocation_known: data.known,
                incomplete: data.incomplete,
                hard_link: data.hard_link,
            });
            paths.insert(path_id, node);
        }
        index.entry(NodeId(0))?;
        let mut issues = self
            .connection
            .prepare("SELECT path,message FROM snapshot_issues WHERE snapshot_id=?1 LIMIT 1000")?;
        index.issues = issues
            .query_map([id], |r| {
                Ok(Issue {
                    path: r.get(0)?,
                    message: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(index)
    }

    pub fn compare(
        &self,
        before: i64,
        after: i64,
        offset: usize,
        limit: usize,
    ) -> Result<Comparison> {
        let summary = |id| -> Result<(i64, u64, bool)> {
            Ok(self.connection.query_row(
                "SELECT root_id,logical,complete FROM snapshots WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, unsigned(r, 1)?, r.get(2)?)),
            )?)
        };
        let (root, old_size, old_complete) = summary(before)?;
        let (new_root, new_size, new_complete) = summary(after)?;
        if root != new_root {
            return Err(Error::Invalid(
                "Compare snapshots from the same root, volume and exclusions.".into(),
            ));
        }
        let total: i64 = self.connection.query_row(
            &format!("SELECT COUNT(*) {DIFFERENCES}"),
            params![before, after, root],
            |r| r.get(0),
        )?;
        let query = format!(
            "SELECT p.id,o.logical,n.logical,COALESCE(n.kind,o.kind) {DIFFERENCES}
            ORDER BY ABS(COALESCE(n.logical,0)-COALESCE(o.logical,0)) DESC,p.id LIMIT ?4 OFFSET ?5"
        );
        let mut statement = self.connection.prepare(&query)?;
        let records = statement.query_map(
            params![
                before,
                after,
                root,
                position_integer(limit.clamp(1, 200))?,
                position_integer(offset)?
            ],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    optional_unsigned(r, 1)?,
                    optional_unsigned(r, 2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )?;
        let mut changes = Vec::new();
        for row in records {
            let (path, old, new, kind) = row?;
            let change = match (old, new) {
                (None, Some(_)) => {
                    if old_complete {
                        "added"
                    } else {
                        "newly_observed"
                    }
                }
                (Some(_), None) => {
                    if new_complete {
                        "removed"
                    } else {
                        "unobserved"
                    }
                }
                _ => "changed",
            };
            changes.push(Change {
                path: self.path(root, path)?.to_string_lossy().into_owned(),
                kind: change.into(),
                directory: kind == 0,
                before: old.unwrap_or(0),
                after: new.unwrap_or(0),
                delta: signed_delta(old.unwrap_or(0), new.unwrap_or(0))?,
            });
        }
        Ok(Comparison {
            before,
            after,
            delta: signed_delta(old_size, new_size)?,
            reliable: old_complete && new_complete,
            total: usize::try_from(total)
                .map_err(|_| Error::Invalid("Invalid comparison count.".into()))?,
            changes,
        })
    }

    fn path(&self, root: i64, mut id: i64) -> Result<PathBuf> {
        let bytes: Vec<u8> = self.connection.query_row(
            "SELECT path FROM snapshot_roots WHERE id=?1",
            [root],
            |r| r.get(0),
        )?;
        let mut path = PathBuf::from(decode(&bytes)?);
        let mut names = Vec::new();
        let mut visited = HashSet::new();
        let mut statement = self
            .connection
            .prepare("SELECT parent_id,name FROM snapshot_paths WHERE root_id=?1 AND id=?2")?;
        while id != 0 {
            if !visited.insert(id) {
                return Err(Error::Invalid("Snapshot path contains a cycle.".into()));
            }
            let (parent, bytes): (i64, Vec<u8>) =
                statement.query_row(params![root, id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if parent != 0 {
                let name = decode(&bytes)?;
                validate_name(&name)?;
                names.push(name);
            }
            id = parent;
        }
        for name in names.into_iter().rev() {
            path.push(name);
        }
        Ok(path)
    }

    pub fn delete(&mut self, id: i64) -> Result<()> {
        let transaction = self.connection.transaction()?;
        if transaction.execute("DELETE FROM snapshots WHERE id=?1", [id])? != 1 {
            return Err(Error::Invalid("Snapshot no longer exists.".into()));
        }
        transaction.execute("DELETE FROM snapshot_roots WHERE NOT EXISTS(SELECT 1 FROM snapshots s WHERE s.root_id=snapshot_roots.id)", [])?;
        transaction.execute("DELETE FROM snapshot_versions WHERE NOT EXISTS
            (SELECT 1 FROM snapshots s JOIN snapshot_paths p ON p.root_id=s.root_id WHERE p.id=snapshot_versions.path_id
             AND s.id>=snapshot_versions.start_id AND (snapshot_versions.end_id IS NULL OR s.id<snapshot_versions.end_id))", [])?;
        while transaction.execute("DELETE FROM snapshot_paths WHERE NOT EXISTS(SELECT 1 FROM snapshot_versions v WHERE v.path_id=snapshot_paths.id)
            AND NOT EXISTS(SELECT 1 FROM snapshot_paths child WHERE child.parent_id=snapshot_paths.id)", [])? > 0 {}
        transaction.commit()?;
        Ok(())
    }

    pub fn disk_bytes(&self) -> Result<u64> {
        let mut size = std::fs::metadata(&self.path)?.len();
        for suffix in ["-wal", "-shm"] {
            let mut path = self.path.as_os_str().to_os_string();
            path.push(suffix);
            match std::fs::metadata(path) {
                Ok(metadata) => size += metadata.len(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(size)
    }

    pub fn compact(&self) -> Result<()> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        Ok(())
    }
}

fn integer(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::Invalid("Snapshot value exceeds SQLite integer range.".into()))
}

fn position_integer(value: usize) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::Invalid("Snapshot position exceeds SQLite integer range.".into()))
}

fn unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn optional_unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(column)?
        .map(|value| {
            u64::try_from(value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    column,
                    rusqlite::types::Type::Integer,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn signed_delta(before: u64, after: u64) -> Result<i64> {
    i64::try_from(i128::from(after) - i128::from(before))
        .map_err(|_| Error::Invalid("Size difference exceeds signed range.".into()))
}

fn validate_name(name: &OsStr) -> Result<()> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(part)) if part == name)
        || components.next().is_some()
    {
        return Err(Error::Invalid(
            "Snapshot contains an invalid filename component.".into(),
        ));
    }
    Ok(())
}

fn encode_paths(paths: &[PathBuf]) -> Result<Vec<u8>> {
    let mut names: Vec<_> = paths.iter().map(|p| encode(p.as_os_str())).collect();
    names.sort();
    names.dedup();
    let mut bytes = Vec::new();
    for name in names {
        bytes.extend(
            u64::try_from(name.len())
                .map_err(|_| Error::Invalid("Exclusion path is too long.".into()))?
                .to_le_bytes(),
        );
        bytes.extend(name);
    }
    Ok(bytes)
}

fn decode_paths(mut bytes: &[u8]) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    while !bytes.is_empty() {
        let header = bytes
            .get(..8)
            .ok_or_else(|| Error::Invalid("Invalid snapshot exclusions.".into()))?;
        let length = u64::from_le_bytes(
            header
                .try_into()
                .map_err(|_| Error::Invalid("Invalid exclusion length.".into()))?,
        );
        let length = usize::try_from(length)
            .map_err(|_| Error::Invalid("Invalid exclusion length.".into()))?;
        bytes = &bytes[8..];
        let name = bytes
            .get(..length)
            .ok_or_else(|| Error::Invalid("Invalid snapshot exclusion path.".into()))?;
        let path = PathBuf::from(decode(name)?);
        if !path.is_absolute() {
            return Err(Error::Invalid("Snapshot exclusion is not absolute.".into()));
        }
        paths.push(path);
        bytes = &bytes[length..];
    }
    Ok(paths)
}

fn encode(value: &OsStr) -> Vec<u8> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().flat_map(u16::to_le_bytes).collect()
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().to_vec()
    }
}

fn decode(bytes: &[u8]) -> Result<OsString> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        if !bytes.len().is_multiple_of(2) {
            return Err(Error::Invalid("Invalid UTF-16 snapshot filename.".into()));
        }
        Ok(OsString::from_wide(
            &bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(OsString::from_vec(bytes.to_vec()))
    }
}
