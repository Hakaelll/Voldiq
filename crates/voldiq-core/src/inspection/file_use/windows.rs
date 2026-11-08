use super::{Application, Report};
use crate::{Entry, Error, NodeId, Result, agents::AuthorizedPath, index::Index, windows};
use std::{
    ffi::OsString,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::null,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::{ERROR_MORE_DATA, ERROR_SHARING_VIOLATION, ERROR_SUCCESS, MAX_PATH},
    Storage::FileSystem::{
        FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
    System::RestartManager::{
        CCH_RM_SESSION_KEY, RM_PROCESS_INFO, RmEndSession, RmGetList, RmRegisterResources,
        RmStartSession,
    },
};

const PROCESS_LIMIT: usize = 128;
static ACTIVE_CHECK: Mutex<()> = Mutex::new(());

pub(super) struct Target {
    parent: AuthorizedPath,
    name: OsString,
    entry: Entry,
}

impl Target {
    pub(super) fn from_index(index: &Index, id: NodeId) -> Result<Self> {
        let entry = index.entry(id)?;
        let path = index.path(id)?;
        let parent = super::super::native::checked_parent(index, entry, &path)?;
        let name = path.file_name().ok_or_else(changed)?.to_owned();
        Ok(Self {
            parent,
            name,
            entry: entry.clone(),
        })
    }

    pub(super) fn read(self, cancelled: &AtomicBool) -> Result<Option<Report>> {
        let Ok(_check) = ACTIVE_CHECK.try_lock() else {
            return Ok(Some(Report::Busy));
        };
        let path = crate::agents::ordinary_path(&self.parent.path().join(&self.name))?;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        if wide.len() > MAX_PATH as usize {
            return Ok(Some(Report::PathLimit));
        }
        // Data access retains namespace sharing for the query. No file content is read.
        let file = match windows::cleanup_io::open_component(
            self.parent.directory_handle()?,
            &self.name,
            FILE_READ_ATTRIBUTES | FILE_READ_DATA,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        ) {
            Ok(file) => file,
            Err(Error::Io(error))
                if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) =>
            {
                return Ok(Some(Report::SharingConflict));
            }
            Err(error) => return Err(error),
        };
        let verify = |path: &Path| {
            super::super::native::verify(&file, &self.entry, true)?;
            if crate::agents::ordinary_path(&windows::final_path(&file)?)? != path {
                return Err(changed());
            }
            Ok(())
        };
        verify(&path)?;
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let report = query(&wide);
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        verify(&path)?;
        Ok(Some(report))
    }
}

fn changed() -> Error {
    Error::Invalid("File name changed since analysis. Rescan and try again.".into())
}

struct Session(u32);

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { RmEndSession(self.0) };
    }
}

fn query(path: &[u16]) -> Report {
    let mut handle = 0;
    let mut key = [0; CCH_RM_SESSION_KEY as usize + 1];
    let status = unsafe { RmStartSession(&mut handle, 0, key.as_mut_ptr()) };
    if status != ERROR_SUCCESS {
        return Report::WindowsError { code: status };
    }
    let session = Session(handle);
    let status = unsafe { RmRegisterResources(session.0, 1, &path.as_ptr(), 0, null(), 0, null()) };
    if status != ERROR_SUCCESS {
        return Report::WindowsError { code: status };
    }
    let mut needed = 0;
    let mut count = PROCESS_LIMIT as u32;
    let mut reboot_reasons = 0;
    let mut records = vec![RM_PROCESS_INFO::default(); PROCESS_LIMIT];
    let status = unsafe {
        RmGetList(
            session.0,
            &mut needed,
            &mut count,
            records.as_mut_ptr(),
            &mut reboot_reasons,
        )
    };
    match status {
        ERROR_MORE_DATA => Report::ProcessLimit,
        ERROR_SUCCESS if count as usize <= PROCESS_LIMIT => {
            let own_pid = std::process::id();
            records.truncate(count as usize);
            let mut applications: Vec<_> = records
                .into_iter()
                .filter(|record| record.Process.dwProcessId != own_pid)
                .map(|record| {
                    let length = record
                        .strAppName
                        .iter()
                        .position(|&unit| unit == 0)
                        .unwrap_or(record.strAppName.len());
                    let mut name = String::from_utf16_lossy(&record.strAppName[..length]);
                    name.retain(|character| !character.is_control());
                    Application {
                        pid: record.Process.dwProcessId,
                        name,
                    }
                })
                .collect();
            applications.sort_unstable_by_key(|application| application.pid);
            Report::Observed { applications }
        }
        ERROR_SUCCESS => Report::ProcessLimit,
        code => Report::WindowsError { code },
    }
}
