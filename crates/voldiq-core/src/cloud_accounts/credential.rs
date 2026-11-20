use super::invalid;
use crate::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    ptr::null,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};
use zeroize::{Zeroize, Zeroizing};

pub(super) struct Store(PathBuf);

#[derive(Deserialize, Serialize)]
pub(super) struct Credential {
    pub(super) client_id: String,
    pub(super) drive_id: String,
    pub(super) refresh_token: String,
}

impl Drop for Credential {
    fn drop(&mut self) {
        self.refresh_token.zeroize();
    }
}

impl Store {
    pub(super) fn open(directory: &Path) -> Self {
        Self(directory.join("cloud-accounts.db"))
    }

    fn connect(&self) -> Result<Connection> {
        let database = Connection::open(&self.0)?;
        crate::database::configure(&database)?;
        database.execute_batch(
            "CREATE TABLE IF NOT EXISTS onedrive_session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                ciphertext BLOB NOT NULL
            );",
        )?;
        Ok(database)
    }

    pub(super) fn read(&self) -> Result<Option<Vec<u8>>> {
        if !self.0.try_exists()? {
            return Ok(None);
        }
        self.connect()?
            .query_row(
                "SELECT ciphertext FROM onedrive_session WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub(super) fn save(
        &self,
        existing: Option<&[u8]>,
        client_id: &str,
        drive_id: &str,
        refresh_token: Option<&str>,
    ) -> Result<Vec<u8>> {
        let refresh_token = refresh_token
            .filter(|token| !token.is_empty())
            .ok_or_else(|| invalid("Microsoft did not grant offline access for this account."))?;
        let credential = Credential {
            client_id: client_id.into(),
            drive_id: drive_id.into(),
            refresh_token: refresh_token.into(),
        };
        let mut plain = Zeroizing::new(
            serde_json::to_vec(&credential)
                .map_err(|_| invalid("The Microsoft credential could not be encoded."))?,
        );
        let sealed = dpapi(&mut plain, true)?;
        let database = self.connect()?;
        let changed = if let Some(existing) = existing {
            database.execute(
                "UPDATE onedrive_session SET ciphertext = ?1 WHERE id = 1 AND ciphertext = ?2",
                params![sealed, existing],
            )?
        } else {
            database.execute("INSERT INTO onedrive_session (id, ciphertext) VALUES (1, ?1) ON CONFLICT DO NOTHING", [&sealed])?
        };
        if changed != 1 {
            return Err(invalid(
                "The OneDrive connection changed in another window.",
            ));
        }
        Ok(sealed)
    }

    pub(super) fn forget(&self) -> Result<()> {
        if self.0.try_exists()? {
            let database = self.connect()?;
            database.execute("DELETE FROM onedrive_session WHERE id = 1", [])?;
        }
        Ok(())
    }
}

pub(super) fn unseal(sealed: &[u8]) -> Result<Credential> {
    let plain = Zeroizing::new(dpapi(&mut sealed.to_vec(), false)?);
    serde_json::from_slice(&plain).map_err(|_| {
        invalid("The saved OneDrive credential is invalid. Disconnect and sign in again.")
    })
}

fn dpapi(input: &mut [u8], seal: bool) -> Result<Vec<u8>> {
    let source = CRYPT_INTEGER_BLOB {
        cbData: input
            .len()
            .try_into()
            .map_err(|_| invalid("The OneDrive credential is too large."))?,
        pbData: input.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let transformed = unsafe {
        if seal {
            CryptProtectData(
                &source,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &source,
                std::ptr::null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if transformed == 0 || output.pbData.is_null() {
        return Err(invalid(
            "Windows could not access the OneDrive credential. Disconnect and sign in again.",
        ));
    }
    // DPAPI allocated this buffer. Clear bytes before returning it to Windows.
    let data = unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
    let transformed = data.to_vec();
    data.zeroize();
    unsafe { LocalFree(output.pbData.cast()) };
    Ok(transformed)
}
