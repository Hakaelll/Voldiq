use crate::{Error, Result};
use rusqlite::{Connection, ErrorCode};
use std::time::{Duration, Instant};

pub(crate) fn configure(connection: &Connection) -> Result<()> {
    connection.busy_timeout(Duration::from_millis(100))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // Two first-openers can contend while switching journal modes, before migrations begin.
        match connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0)) {
            Ok(mode) if mode == "wal" || mode == "memory" => break,
            Ok(_) => {
                return Err(Error::Invalid(
                    "Persistent storage could not enter WAL mode.".into(),
                ));
            }
            Err(rusqlite::Error::SqliteFailure(error, _))
                if matches!(
                    error.code,
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                ) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error.into()),
        }
    }
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(())
}
