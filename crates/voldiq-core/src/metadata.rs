//! Human-maintained folder notes and protections shared by every adapter.
mod read;
mod suggestions;
use crate::{
    Error, Result,
    agents::{pin_directory, pin_path},
    operations::Coordinator,
};
pub use read::{FolderRead, NoteContent};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub use suggestions::{Eligibility, SuggestionMask};

const MAX_NOTES: i64 = 10_000;
const MAX_NOTE_BYTES: usize = 8192;

pub struct Store {
    connection: Connection,
    directory: Coordinator,
}

#[derive(Clone, Debug, Serialize)]
pub struct Note {
    pub id: Option<i64>,
    pub path: PathBuf,
    pub note: String,
    pub protected: bool,
    pub version: String,
    pub updated: Option<i64>,
}

pub struct Protections {
    fingerprint: String,
    paths: Vec<PathBuf>,
    identities: HashSet<(u64, u64)>,
    _directory: Coordinator,
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        let directory = Coordinator::open(directory)?;
        let mut connection = Connection::open(directory.directory().join("metadata.db"))?;
        crate::database::configure(&connection)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: u32 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version == 0 {
            tx.execute_batch("CREATE TABLE folder_notes(
                id INTEGER PRIMARY KEY AUTOINCREMENT, path TEXT NOT NULL, identity BLOB NOT NULL UNIQUE,
                volume BLOB NOT NULL, file BLOB NOT NULL, note TEXT NOT NULL,
                protected INTEGER NOT NULL CHECK(protected IN (0,1)),
                version INTEGER NOT NULL CHECK(version>0), updated INTEGER NOT NULL);
                CREATE TABLE protection_policy(id INTEGER PRIMARY KEY CHECK(id=1), fingerprint TEXT NOT NULL);
                PRAGMA user_version=1;")?;
            tx.execute(
                "INSERT INTO protection_policy VALUES(1,?1)",
                [fingerprint()?],
            )?;
        } else if version != 1 {
            return Err(Error::Invalid("Unsupported folder metadata schema.".into()));
        }
        tx.commit()?;
        Ok(Self {
            connection,
            directory,
        })
    }

    pub fn get(&self, folder: &Path) -> Result<Note> {
        let folder = pin_directory(folder)?;
        let mut note = self
            .connection
            .query_row(
                "SELECT id,path,note,protected,version,updated FROM folder_notes WHERE identity=?1",
                [folder.identity_bytes()?],
                read_note,
            )
            .optional()?
            .unwrap_or(Note {
                id: None,
                path: folder.path().to_owned(),
                note: String::new(),
                protected: false,
                version: "0".into(),
                updated: None,
            });
        // The caller's pinned path is authoritative after a rename; do not return the stale label.
        note.path = folder.path().to_owned();
        Ok(note)
    }

    pub fn save_note(&mut self, folder: &Path, note: &str) -> Result<Note> {
        if note.len() > MAX_NOTE_BYTES {
            return Err(Error::Invalid(
                "Folder notes are limited to 8192 UTF-8 bytes.".into(),
            ));
        }
        self.update(folder, Some(note))
    }

    pub fn protect_folder(&mut self, folder: &Path) -> Result<Note> {
        self.update(folder, None)
    }

    fn update(&mut self, folder: &Path, text: Option<&str>) -> Result<Note> {
        let _lease = self.directory.acquire()?;
        let folder = pin_directory(folder)?;
        let identity = folder.identity_bytes()?;
        let (volume, file) = folder.identity();
        let path = folder.path().to_str().ok_or_else(unavailable)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = tx
            .query_row(
                "SELECT id,path,note,protected,version,updated FROM folder_notes WHERE identity=?1",
                [&identity],
                read_note,
            )
            .optional()?;
        if previous.is_none()
            && tx.query_row("SELECT COUNT(*) FROM folder_notes", [], |row| {
                row.get::<_, i64>(0)
            })? >= MAX_NOTES
        {
            return Err(Error::Invalid(
                "Folder metadata limit reached (10000 folders).".into(),
            ));
        }
        let protected = previous.as_ref().is_some_and(|row| row.protected) || text.is_none();
        // Preserve a protected row's original path as a conservative replacement guard on rename.
        let stored_path = previous
            .as_ref()
            .filter(|row| row.protected)
            .and_then(|row| row.path.to_str())
            .unwrap_or(path);
        let note = text.unwrap_or_else(|| previous.as_ref().map_or("", |row| row.note.as_str()));
        tx.execute("INSERT INTO folder_notes(path,identity,volume,file,note,protected,version,updated)
            VALUES(?1,?2,?3,?4,?5,?6,1,?7) ON CONFLICT(identity) DO UPDATE SET
            path=excluded.path,note=excluded.note,protected=excluded.protected,version=folder_notes.version+1,updated=excluded.updated",
            params![stored_path,identity,volume.to_le_bytes().as_slice(),file.to_le_bytes().as_slice(),note,protected,now()?])?;
        if protected && !previous.as_ref().is_some_and(|row| row.protected) {
            tx.execute(
                "UPDATE protection_policy SET fingerprint=?1 WHERE id=1",
                [fingerprint()?],
            )?;
        }
        tx.commit()?;
        self.get(folder.path())
    }

    /// Remove only the exact version reviewed by the human, including missing or moved folders.
    pub fn remove_protection(&mut self, id: i64, version: &str) -> Result<()> {
        let version: i64 = version.parse().map_err(|_| unavailable())?;
        let _lease = self.directory.acquire()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute("UPDATE folder_notes SET protected=0,version=version+1,updated=?1 WHERE id=?2 AND version=?3 AND protected=1", params![now()?,id,version])? != 1 {
            return Err(Error::Invalid("Folder protection changed. Review it again before removing it.".into()));
        }
        tx.execute(
            "UPDATE protection_policy SET fingerprint=?1 WHERE id=1",
            [fingerprint()?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn protected_folders(&self, offset: usize, limit: usize) -> Result<Vec<Note>> {
        if offset > MAX_NOTES as usize || !(1..=100).contains(&limit) {
            return Err(unavailable());
        }
        let mut query = self.connection.prepare("SELECT id,path,note,protected,version,updated FROM folder_notes WHERE protected=1 ORDER BY id LIMIT ?1 OFFSET ?2")?;
        let notes = query
            .query_map(params![(limit + 1) as i64, offset as i64], read_note)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if notes.len() > MAX_NOTES as usize {
            return Err(unavailable());
        }
        Ok(notes)
    }

    pub fn protection(&self, id: i64, version: &str) -> Result<Note> {
        let version: i64 = version.parse().map_err(|_| unavailable())?;
        self.connection.query_row("SELECT id,path,note,protected,version,updated FROM folder_notes WHERE id=?1 AND version=?2 AND protected=1",
            params![id,version],read_note).optional()?.ok_or_else(unavailable)
    }

    pub fn protections(&mut self) -> Result<Protections> {
        let tx = self.connection.transaction()?;
        let fingerprint = tx.query_row(
            "SELECT fingerprint FROM protection_policy WHERE id=1",
            [],
            |row| row.get::<_, String>(0),
        )?;
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(unavailable());
        }
        let mut paths = vec![self.directory.directory().to_owned()];
        let mut identities = HashSet::from([self.directory.identity()]);
        {
            let mut query = tx.prepare(
                "SELECT path,volume,file FROM folder_notes WHERE protected=1 LIMIT 10001",
            )?;
            let mut rows = query.query([])?;
            while let Some(row) = rows.next()? {
                if paths.len() > MAX_NOTES as usize {
                    return Err(unavailable());
                }
                let path = PathBuf::from(row.get::<_, String>(0)?);
                if !path.is_absolute() {
                    return Err(unavailable());
                }
                paths.push(path);
                let volume: Vec<u8> = row.get(1)?;
                let file: Vec<u8> = row.get(2)?;
                identities.insert((
                    u64::from_le_bytes(volume.try_into().map_err(|_| unavailable())?),
                    u64::from_le_bytes(file.try_into().map_err(|_| unavailable())?),
                ));
            }
        }
        tx.commit()?;
        Ok(Protections {
            fingerprint,
            paths,
            identities,
            _directory: self.directory.clone(),
        })
    }
}

impl Protections {
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn check_path(&self, path: &Path) -> Result<()> {
        if self.protects(path)? {
            return Err(Error::Invalid(
                "This folder or one of its parents is protected from cleanup.".into(),
            ));
        }
        Ok(())
    }

    pub fn protects(&self, path: &Path) -> Result<bool> {
        let path = pin_path(path)?;
        Ok(self
            .check_evidence(path.path(), path.identity(), &path.ancestor_identities()?)
            .is_err())
    }

    pub(crate) fn check_evidence(
        &self,
        path: &Path,
        identity: (u64, u64),
        parents: &[(u64, u64)],
    ) -> Result<()> {
        if self.identities.contains(&identity)
            || parents
                .iter()
                .any(|parent| self.identities.contains(parent))
            || self.paths.iter().any(|protected| contains(protected, path))
        {
            return Err(Error::Invalid(
                "This folder or one of its parents is protected from cleanup.".into(),
            ));
        }
        Ok(())
    }
}

fn contains(root: &Path, path: &Path) -> bool {
    path_key(path).starts_with(path_key(root))
}

fn path_key(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let path = path.to_string_lossy().to_uppercase();
        PathBuf::from(path.strip_prefix(r"\\?\").unwrap_or(&path))
    }
    #[cfg(not(windows))]
    {
        path.to_owned()
    }
}

fn read_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: Some(row.get(0)?),
        path: PathBuf::from(row.get::<_, String>(1)?),
        note: row.get(2)?,
        protected: row.get(3)?,
        version: row.get::<_, i64>(4)?.to_string(),
        updated: Some(row.get(5)?),
    })
}

fn fingerprint() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| unavailable())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn now() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|time| i64::try_from(time.as_secs()).ok())
        .ok_or_else(unavailable)
}
fn unavailable() -> Error {
    Error::Invalid("Folder metadata is unavailable.".into())
}
