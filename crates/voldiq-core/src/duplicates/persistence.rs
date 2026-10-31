use super::{CACHE_LIMIT, CachedHash, HashCache, Stamp};
use crate::Result;
use rusqlite::{Connection, Row, params, types::ValueRef};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, UNIX_EPOCH},
};

pub(crate) const SCHEMA: &str = "
CREATE TABLE duplicate_hashes(
 identity BLOB PRIMARY KEY CHECK(length(identity)=16),
 size BLOB NOT NULL CHECK(length(size)=8),
 modified_seconds BLOB NOT NULL CHECK(length(modified_seconds)=8),
 modified_nanos INTEGER NOT NULL CHECK(modified_nanos BETWEEN 0 AND 999999999),
 change_time BLOB NOT NULL CHECK(length(change_time)=8),
 sample BLOB NOT NULL CHECK(length(sample)=32),
 full BLOB CHECK(full IS NULL OR length(full)=32));
PRAGMA user_version=5;";

fn bytes<const N: usize>(row: &Row<'_>, column: usize) -> Option<[u8; N]> {
    row.get_ref(column).ok()?.as_blob().ok()?.try_into().ok()
}

fn decode(row: &Row<'_>) -> Option<CachedHash> {
    let identity = bytes::<16>(row, 0)?;
    let nanos = u32::try_from(row.get::<_, i64>(3).ok()?).ok()?;
    if nanos >= 1_000_000_000 {
        return None;
    }
    let full = match row.get_ref(6).ok()? {
        ValueRef::Null => None,
        ValueRef::Blob(value) => Some(blake3::Hash::from_bytes(value.try_into().ok()?)),
        _ => return None,
    };
    Some(CachedHash {
        stamp: Stamp {
            identity: (
                u64::from_le_bytes(identity[..8].try_into().ok()?),
                u64::from_le_bytes(identity[8..].try_into().ok()?),
            ),
            size: u64::from_le_bytes(bytes(row, 1)?),
            modified: UNIX_EPOCH
                .checked_add(Duration::new(u64::from_le_bytes(bytes(row, 2)?), nanos))?,
            change_time: u64::from_le_bytes(bytes(row, 4)?),
        },
        sample: blake3::Hash::from_bytes(bytes(row, 5)?),
        full,
    })
}

impl HashCache {
    pub(crate) fn read(connection: &Connection, cancel: &AtomicBool) -> Result<Self> {
        let mut statement = connection.prepare(
            "SELECT identity,size,modified_seconds,modified_nanos,change_time,sample,full
             FROM duplicate_hashes
             WHERE typeof(identity)='blob' AND length(identity)=16
               AND typeof(size)='blob' AND length(size)=8
               AND typeof(modified_seconds)='blob' AND length(modified_seconds)=8
               AND typeof(modified_nanos)='integer' AND modified_nanos BETWEEN 0 AND 999999999
               AND typeof(change_time)='blob' AND length(change_time)=8
               AND typeof(sample)='blob' AND length(sample)=32
               AND (full IS NULL OR (typeof(full)='blob' AND length(full)=32))
             ORDER BY identity LIMIT ?1",
        )?;
        let mut rows = statement.query([CACHE_LIMIT as i64])?;
        let mut cache = Self::default();
        while let Some(row) = rows.next()? {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if let Some(entry) = decode(row) {
                cache.0.insert(entry.stamp.identity, entry);
            }
        }
        Ok(cache)
    }

    pub(crate) fn write(&self, connection: &mut Connection, cancel: &AtomicBool) -> Result<()> {
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM duplicate_hashes", [])?;
        {
            let mut insert =
                transaction.prepare("INSERT INTO duplicate_hashes VALUES(?1,?2,?3,?4,?5,?6,?7)")?;
            for entry in self.0.values().take(CACHE_LIMIT) {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let stamp = &entry.stamp;
                let Ok(modified) = stamp.modified.duration_since(UNIX_EPOCH) else {
                    continue;
                };
                let mut identity = [0_u8; 16];
                identity[..8].copy_from_slice(&stamp.identity.0.to_le_bytes());
                identity[8..].copy_from_slice(&stamp.identity.1.to_le_bytes());
                insert.execute(params![
                    identity.as_slice(),
                    stamp.size.to_le_bytes().as_slice(),
                    modified.as_secs().to_le_bytes().as_slice(),
                    modified.subsec_nanos(),
                    stamp.change_time.to_le_bytes().as_slice(),
                    entry.sample.as_bytes().as_slice(),
                    entry.full.map(|hash| *hash.as_bytes())
                ])?;
            }
        }
        if !cancel.load(Ordering::Relaxed) {
            transaction.commit()?;
        }
        Ok(())
    }
}
