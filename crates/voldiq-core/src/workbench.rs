//! Human-maintained view specifications and pinned folder shortcuts. Neither grants authority.
mod spec;
use crate::{
    Error, NodeId, Result,
    agents::{AuthorizedPath, pin_directory},
    index::Index,
    operations::Coordinator,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
pub use spec::{Color, Metric, Representation, Scope, View, ViewSpec};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_RECORDS: usize = 100;
const MAX_SPEC_BYTES: usize = 16_384;

pub struct Store {
    connection: Connection,
    directory: Coordinator,
}
#[derive(Serialize)]
pub struct Catalog {
    pub revision: String,
    pub views: Vec<SavedView>,
    pub favorites: Vec<Favorite>,
}
#[derive(Serialize)]
pub struct SavedView {
    pub id: i64,
    pub name: String,
    pub spec: ViewSpec,
    pub updated: i64,
}
#[derive(Serialize)]
pub struct Favorite {
    pub id: i64,
    pub name: String,
    pub path: PathBuf,
    pub updated: i64,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordRef {
    pub id: i64,
    pub revision: String,
}

fn changed() -> Error {
    Error::Invalid("Saved workspaces changed. Reload before continuing.".into())
}
fn invalid() -> Error {
    Error::Invalid("Saved workspace record is unavailable or unsupported.".into())
}
fn now() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| invalid())?
            .as_secs(),
    )
    .map_err(|_| invalid())
}
fn valid_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "Use a nonempty name of at most 128 UTF-8 bytes, without control characters.".into(),
        ));
    }
    Ok(name)
}
fn text(row: &rusqlite::Row<'_>, column: usize, max: usize) -> Result<String> {
    if let rusqlite::types::ValueRef::Text(value) = row.get_ref(column)?
        && value.len() <= max
    {
        return Ok(std::str::from_utf8(value)
            .map_err(|_| invalid())?
            .to_owned());
    }
    Err(invalid())
}
fn revision(connection: &Connection) -> Result<String> {
    Ok(connection
        .query_row("SELECT revision FROM catalogue WHERE id=1", [], |row| {
            row.get::<_, i64>(0)
        })?
        .to_string())
}
fn check(connection: &Connection, expected: &str) -> Result<()> {
    if revision(connection)? != expected {
        return Err(changed());
    }
    Ok(())
}
fn bump(connection: &Connection) -> Result<()> {
    if connection.execute(
        "UPDATE catalogue SET revision=revision+1 WHERE id=1 AND revision<9223372036854775807",
        [],
    )? != 1
    {
        return Err(invalid());
    }
    Ok(())
}
fn valid_id(id: i64) -> Result<()> {
    if !(1..=9_007_199_254_740_991).contains(&id) {
        return Err(invalid());
    }
    Ok(())
}
fn read_view(row: &rusqlite::Row<'_>) -> Result<SavedView> {
    let id = row.get(0)?;
    valid_id(id)?;
    let name = text(row, 1, 128)?;
    valid_name(&name)?;
    let spec: ViewSpec =
        serde_json::from_str(&text(row, 2, MAX_SPEC_BYTES)?).map_err(|_| invalid())?;
    spec.validate()?;
    Ok(SavedView {
        id,
        name,
        spec,
        updated: row.get(3)?,
    })
}
fn catalog(connection: &Connection) -> Result<Catalog> {
    let mut views = Vec::new();
    let mut statement = connection.prepare(
        "SELECT id,name,spec,updated FROM saved_views ORDER BY name COLLATE NOCASE,id LIMIT 101",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        views.push(read_view(row)?);
    }
    let mut favorites = Vec::new();
    let mut statement = connection.prepare(
        "SELECT id,name,path,updated FROM favorites ORDER BY name COLLATE NOCASE,id LIMIT 101",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id = row.get(0)?;
        valid_id(id)?;
        let name = text(row, 1, 128)?;
        valid_name(&name)?;
        favorites.push(Favorite {
            id,
            name,
            path: PathBuf::from(text(row, 2, 32767)?),
            updated: row.get(3)?,
        });
    }
    if views.len() > MAX_RECORDS || favorites.len() > MAX_RECORDS {
        return Err(invalid());
    }
    Ok(Catalog {
        revision: revision(connection)?,
        views,
        favorites,
    })
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        let directory = Coordinator::open(directory)?;
        let mut connection = Connection::open(directory.directory().join("workbench.db"))?;
        crate::database::configure(&connection)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: u32 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version == 0 {
            tx.execute_batch("CREATE TABLE catalogue(id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL CHECK(revision>0));
                INSERT INTO catalogue VALUES(1,1);
                CREATE TABLE saved_views(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL UNIQUE COLLATE NOCASE,spec TEXT NOT NULL,updated INTEGER NOT NULL);
                CREATE TABLE favorites(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL,path TEXT NOT NULL,identity BLOB NOT NULL UNIQUE,updated INTEGER NOT NULL);
                PRAGMA user_version=1;")?;
        } else if version != 1 {
            return Err(invalid());
        }
        tx.commit()?;
        Ok(Self {
            connection,
            directory,
        })
    }
    pub fn catalog(&self) -> Result<Catalog> {
        let tx = self.connection.unchecked_transaction()?;
        let value = catalog(&tx)?;
        tx.commit()?;
        Ok(value)
    }
    pub fn view(&self, reference: &RecordRef) -> Result<SavedView> {
        valid_id(reference.id)?;
        let tx = self.connection.unchecked_transaction()?;
        check(&tx, &reference.revision)?;
        let mut statement =
            tx.prepare("SELECT id,name,spec,updated FROM saved_views WHERE id=?1")?;
        let mut rows = statement.query([reference.id])?;
        let value = read_view(rows.next()?.ok_or_else(changed)?)?;
        drop(rows);
        drop(statement);
        tx.commit()?;
        Ok(value)
    }
    pub fn save_view(
        &mut self,
        id: Option<i64>,
        expected: &str,
        name: &str,
        spec: ViewSpec,
    ) -> Result<Catalog> {
        let name = valid_name(name)?;
        spec.validate()?;
        if let Some(id) = id {
            valid_id(id)?;
        }
        let json = serde_json::to_string(&spec).map_err(|_| invalid())?;
        if json.len() > MAX_SPEC_BYTES {
            return Err(invalid());
        }
        let _lease = self.directory.acquire()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check(&tx, expected)?;
        let named = tx
            .query_row(
                "SELECT id FROM saved_views WHERE name=?1 COLLATE NOCASE",
                [name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if named.is_some() && named != id {
            return Err(Error::Invalid(
                "A view with this name already exists. Choose another name or replace that view."
                    .into(),
            ));
        }
        if let Some(id) = id {
            if tx.execute(
                "UPDATE saved_views SET name=?1,spec=?2,updated=?3 WHERE id=?4",
                params![name, json, now()?, id],
            )? != 1
            {
                return Err(changed());
            }
        } else {
            if tx.query_row("SELECT COUNT(*) FROM saved_views", [], |row| {
                row.get::<_, i64>(0)
            })? >= 100
            {
                return Err(Error::Invalid("Save up to 100 views.".into()));
            }
            tx.execute(
                "INSERT INTO saved_views(name,spec,updated) VALUES(?1,?2,?3)",
                params![name, json, now()?],
            )?;
        }
        bump(&tx)?;
        let value = catalog(&tx)?;
        tx.commit()?;
        Ok(value)
    }
    pub fn remove_view(&mut self, reference: &RecordRef) -> Result<Catalog> {
        self.remove(reference, false)
    }
    pub fn remove_favorite(&mut self, reference: &RecordRef) -> Result<Catalog> {
        self.remove(reference, true)
    }
    fn remove(&mut self, reference: &RecordRef, favorite: bool) -> Result<Catalog> {
        valid_id(reference.id)?;
        let _lease = self.directory.acquire()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check(&tx, &reference.revision)?;
        let sql = if favorite {
            "DELETE FROM favorites WHERE id=?1"
        } else {
            "DELETE FROM saved_views WHERE id=?1"
        };
        if tx.execute(sql, [reference.id])? != 1 {
            return Err(changed());
        }
        bump(&tx)?;
        let value = catalog(&tx)?;
        tx.commit()?;
        Ok(value)
    }
    pub fn add_favorite(&mut self, expected: &str, index: &Index, node: NodeId) -> Result<Catalog> {
        let folder = index.pin_folder(node)?;
        let path = folder
            .path()
            .to_str()
            .filter(|path| path.len() <= 32767)
            .ok_or_else(invalid)?;
        let name = folder
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path);
        // Long native names stay exact in path; use a bounded display label.
        let name: String = name
            .chars()
            .take_while(|c| !c.is_control())
            .scan(0, |bytes, c| {
                *bytes += c.len_utf8();
                (*bytes <= 128).then_some(c)
            })
            .collect();
        let name = valid_name(&name)?;
        let identity = folder.identity_bytes()?;
        let _lease = self.directory.acquire()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check(&tx, expected)?;
        let existing = tx
            .query_row(
                "SELECT id FROM favorites WHERE identity=?1",
                [&identity],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if existing.is_none()
            && tx.query_row("SELECT COUNT(*) FROM favorites", [], |row| {
                row.get::<_, i64>(0)
            })? >= 100
        {
            return Err(Error::Invalid("Save up to 100 favorite folders.".into()));
        }
        tx.execute("INSERT INTO favorites(name,path,identity,updated) VALUES(?1,?2,?3,?4) ON CONFLICT(identity) DO UPDATE SET name=excluded.name,path=excluded.path,updated=excluded.updated", params![name,path,identity,now()?])?;
        bump(&tx)?;
        let value = catalog(&tx)?;
        tx.commit()?;
        Ok(value)
    }
    pub fn pin_favorite(&self, reference: &RecordRef) -> Result<AuthorizedPath> {
        valid_id(reference.id)?;
        let tx = self.connection.unchecked_transaction()?;
        check(&tx, &reference.revision)?;
        let mut statement = tx.prepare("SELECT path,identity FROM favorites WHERE id=?1")?;
        let mut rows = statement.query([reference.id])?;
        let row = rows.next()?.ok_or_else(changed)?;
        let path = text(row, 0, 32767)?;
        let identity = match row.get_ref(1)? {
            rusqlite::types::ValueRef::Blob(bytes) if matches!(bytes.len(), 16 | 24) => bytes,
            _ => return Err(invalid()),
        };
        let folder = pin_directory(Path::new(&path)).map_err(|_| {
            Error::Invalid("Favorite folder is missing or unavailable. Choose it again.".into())
        })?;
        if folder.identity_bytes()?.as_slice() != identity {
            return Err(Error::Invalid(
                "Favorite folder identity changed. Choose it again.".into(),
            ));
        }
        drop(rows);
        drop(statement);
        tx.commit()?;
        Ok(folder)
    }
}
