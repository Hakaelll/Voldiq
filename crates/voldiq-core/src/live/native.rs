use super::{Measurement, Reason, Report, Update};
use crate::{
    Error, Result, ScanConcurrency, ScanMode, ScanOptions,
    agents::{AuthorizedPath, pin_directory, pin_path},
    index::{ACCOUNTING_VERSION, Index},
    scanner, windows,
};
use std::{
    fs::File,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{IO::*, Threading::*},
};

const GROUP_WINDOW: Duration = Duration::from_millis(300);

struct Signals {
    stop: OwnedHandle,
    resync: OwnedHandle,
    cancelled: AtomicBool,
}

/// Cloneable control for another thread. Cancellation is permanent; restart needs a new control.
#[derive(Clone)]
pub struct Control(Arc<Signals>);

impl Control {
    pub fn new() -> Result<Self> {
        Ok(Self(Arc::new(Signals {
            stop: event()?,
            resync: event()?,
            cancelled: AtomicBool::new(false),
        })))
    }

    pub fn stop(&self) -> Result<()> {
        self.0.cancelled.store(true, Ordering::Relaxed);
        signal(&self.0.stop)
    }

    pub fn resynchronize(&self) -> Result<()> {
        signal(&self.0.resync)
    }

    pub fn is_stopped(&self) -> bool {
        self.0.cancelled.load(Ordering::Relaxed)
    }
}

/// Construct, wait and drop on the owning worker. No background thread or idle timer is created.
pub struct Session {
    watcher: Watcher,
    root: AuthorizedPath,
    exclusions: Vec<AuthorizedPath>,
    control: Control,
    options: ScanOptions,
    previous: (u64, Measurement),
    initial: bool,
}

impl Session {
    pub fn start(index: &Index, concurrency: ScanConcurrency, control: Control) -> Result<Self> {
        if control.is_stopped() || index.snapshot_id.is_some() {
            return Err(Error::Invalid(
                "Live requires a current analysis and a new control.".into(),
            ));
        }
        let root = pin_directory(&index.root)?;
        if Some(root.identity()) != index.root_identity {
            return Err(Error::Invalid(
                "Live root identity changed. Scan again.".into(),
            ));
        }
        let exclusions = index
            .exclusions
            .iter()
            .map(|path| {
                path.to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| Error::Invalid("Live exclusion cannot be represented.".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let exclusion_pins = index
            .exclusions
            .iter()
            .map(|path| pin_path(path))
            .collect::<Result<Vec<_>>>()?;
        let watcher = Watcher::start(&root)?;
        Ok(Self {
            root,
            watcher,
            exclusions: exclusion_pins,
            control,
            options: ScanOptions {
                mode: ScanMode::Directory,
                exclusions,
                concurrency,
                max_entries: None,
            },
            previous: measurement(index)?,
            initial: true,
        })
    }

    /// Returns a complete rescan of the same pinned scope, or None after cooperative stop.
    /// Partial measurements are returned as partial, with no known net change.
    pub fn observe(&mut self) -> Result<Option<Update>> {
        if self.control.is_stopped() {
            return Ok(None);
        }
        let reason = if self.initial {
            self.initial = false;
            Reason::Initial
        } else {
            let Some(reason) = self.watcher.batch(&self.control)? else {
                return Ok(None);
            };
            reason
        };
        self.validate_root()?;
        let index = scanner::scan(
            self.root.path(),
            &self.options,
            &self.control.0.cancelled,
            |_| {},
        )?;
        if self.control.is_stopped() {
            return Ok(None);
        }
        self.validate_root()?;
        if index.root_identity != Some(self.root.identity()) {
            return Err(Error::Invalid(
                "Live root identity changed during observation.".into(),
            ));
        }
        let after = measurement(&index)?;
        let net_change = (self.previous.1.complete && after.1.complete)
            .then(|| (i128::from(after.0) - i128::from(self.previous.0)).to_string());
        let before = std::mem::replace(&mut self.previous, after);
        Ok(Some(Update {
            index,
            report: Report {
                reason,
                before: before.1,
                after: self.previous.1.clone(),
                net_change,
            },
        }))
    }

    fn validate_root(&self) -> Result<()> {
        let current = pin_directory(self.root.path())?;
        if current.identity() != self.root.identity()
            || current.ancestor_identities()? != self.root.ancestor_identities()?
        {
            return Err(Error::Invalid("Live scope changed. Scan again.".into()));
        }
        for pinned in &self.exclusions {
            let current = pin_path(pinned.path())?;
            if current.identity() != pinned.identity()
                || current.ancestor_identities()? != pinned.ancestor_identities()?
            {
                return Err(Error::Invalid(
                    "Live exclusions changed. Scan again.".into(),
                ));
            }
        }
        Ok(())
    }
}

fn measurement(index: &Index) -> Result<(u64, Measurement)> {
    let root = index
        .entries
        .first()
        .ok_or_else(|| Error::Invalid("Live analysis is empty.".into()))?;
    let observation = index.observation.as_ref();
    Ok((
        root.logical,
        Measurement {
            logical: root.logical.to_string(),
            files: root.files,
            measured_at: observation.map(|value| value.completed_at),
            complete: !root.incomplete
                && index.error_count == 0
                && observation.is_some_and(|value| {
                    value.complete && value.accounting_version == ACCOUNTING_VERSION
                }),
            accounting_version: observation.map(|value| value.accounting_version),
        },
    ))
}

fn event() -> Result<OwnedHandle> {
    let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    // A successful CreateEventW transfers ownership of this unique event to OwnedHandle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn signal(event: &OwnedHandle) -> Result<()> {
    if unsafe { SetEvent(event.as_raw_handle()) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

enum Wake {
    Stopped,
    Requested,
    Changed(bool),
    Timeout,
}

struct Watcher {
    directory: File,
    event: OwnedHandle,
    // Both allocations stay at fixed addresses through every pending operation and cancellation.
    buffer: Box<[u32; 8192]>,
    overlapped: Box<OVERLAPPED>,
    pending: bool,
    lost: bool,
}

impl Watcher {
    fn start(root: &AuthorizedPath) -> Result<Self> {
        let path = windows::wide(root.path());
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS
                    | FILE_FLAG_OPEN_REPARSE_POINT
                    | FILE_FLAG_OPEN_NO_RECALL
                    | FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        // Root and ancestor pins remain owned by Session while this metadata-only handle is open.
        let directory = unsafe { File::from_raw_handle(handle) };
        let metadata = directory.metadata()?;
        if !metadata.is_dir()
            || windows::is_reparse(&metadata)
            || windows::requires_recall(&metadata)
            || windows::file_identity(&directory)?.key != root.identity()
        {
            return Err(Error::Invalid(
                "Live needs a pinned resident directory.".into(),
            ));
        }
        let event = event()?;
        let overlapped = Box::new(OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..Default::default()
        });
        let mut watcher = Self {
            directory,
            event,
            buffer: Box::new([0; 8192]),
            overlapped,
            pending: false,
            lost: false,
        };
        watcher.arm()?;
        Ok(watcher)
    }

    fn arm(&mut self) -> Result<()> {
        // A notification overflow is consumed by a new request. Repeated failure stops the session.
        for _ in 0..2 {
            if unsafe { ResetEvent(self.event.as_raw_handle()) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            *self.overlapped = OVERLAPPED {
                hEvent: self.event.as_raw_handle(),
                ..Default::default()
            };
            let queued = unsafe {
                ReadDirectoryChangesW(
                    self.directory.as_raw_handle(),
                    self.buffer.as_mut_ptr().cast(),
                    std::mem::size_of_val(&*self.buffer) as u32,
                    1,
                    FILE_NOTIFY_CHANGE_FILE_NAME
                        | FILE_NOTIFY_CHANGE_DIR_NAME
                        | FILE_NOTIFY_CHANGE_ATTRIBUTES
                        | FILE_NOTIFY_CHANGE_SIZE
                        | FILE_NOTIFY_CHANGE_LAST_WRITE
                        | FILE_NOTIFY_CHANGE_CREATION
                        | FILE_NOTIFY_CHANGE_SECURITY,
                    std::ptr::null_mut(),
                    &mut *self.overlapped,
                    None,
                )
            };
            let error = if queued == 0 {
                Some(std::io::Error::last_os_error())
            } else {
                None
            };
            match error {
                None => {
                    self.pending = true;
                    return Ok(());
                }
                Some(error) if error.raw_os_error() == Some(ERROR_IO_PENDING as i32) => {
                    self.pending = true;
                    return Ok(());
                }
                Some(error) if error.raw_os_error() == Some(ERROR_NOTIFY_ENUM_DIR as i32) => {
                    self.lost = true
                }
                Some(error) => return Err(error.into()),
            }
        }
        Err(Error::Invalid(
            "Live notifications repeatedly overflowed. Restart Live.".into(),
        ))
    }

    fn wait(&mut self, control: &Control, timeout: u32) -> Result<Wake> {
        let handles = [
            control.0.stop.as_raw_handle(),
            control.0.resync.as_raw_handle(),
            self.event.as_raw_handle(),
        ];
        let wake =
            unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, timeout) };
        match wake {
            WAIT_OBJECT_0 => Ok(Wake::Stopped),
            value if value == WAIT_OBJECT_0 + 1 => {
                if unsafe { ResetEvent(control.0.resync.as_raw_handle()) } == 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                Ok(Wake::Requested)
            }
            value if value == WAIT_OBJECT_0 + 2 => {
                let mut bytes = 0;
                let completed = unsafe {
                    GetOverlappedResult(
                        self.directory.as_raw_handle(),
                        &*self.overlapped,
                        &mut bytes,
                        0,
                    )
                };
                let error = if completed == 0 {
                    Some(std::io::Error::last_os_error())
                } else {
                    None
                };
                if error
                    .as_ref()
                    .is_some_and(|error| error.raw_os_error() == Some(ERROR_IO_INCOMPLETE as i32))
                {
                    return Err(Error::Invalid(
                        "Live notification signalled before completion.".into(),
                    ));
                }
                self.pending = false;
                let lost = match error {
                    None => bytes == 0,
                    Some(error) if error.raw_os_error() == Some(ERROR_NOTIFY_ENUM_DIR as i32) => {
                        true
                    }
                    Some(error) => return Err(error.into()),
                };
                self.arm()?;
                Ok(Wake::Changed(std::mem::take(&mut self.lost) || lost))
            }
            WAIT_TIMEOUT => Ok(Wake::Timeout),
            _ => Err(std::io::Error::last_os_error().into()),
        }
    }

    fn batch(&mut self, control: &Control) -> Result<Option<Reason>> {
        let mut reason = match self.wait(control, INFINITE)? {
            Wake::Stopped => return Ok(None),
            Wake::Requested => return Ok(Some(Reason::Requested)),
            Wake::Changed(true) => Reason::LostEvents,
            Wake::Changed(false) => Reason::Changes,
            Wake::Timeout => {
                return Err(Error::Invalid("Live wait timed out unexpectedly.".into()));
            }
        };
        let deadline = Instant::now() + GROUP_WINDOW;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match self.wait(control, remaining.as_millis().max(1) as u32)? {
                Wake::Stopped => return Ok(None),
                Wake::Requested if reason != Reason::LostEvents => reason = Reason::Requested,
                Wake::Changed(true) => reason = Reason::LostEvents,
                Wake::Timeout => break,
                Wake::Requested | Wake::Changed(false) => {}
            }
        }
        Ok(Some(reason))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if self.pending {
            // CancelIoEx is only a request. Drain its terminal completion before freeing either allocation.
            // A normal completion racing cancellation is equally safe to drain.
            unsafe {
                CancelIoEx(self.directory.as_raw_handle(), &*self.overlapped);
                let mut bytes = 0;
                GetOverlappedResult(
                    self.directory.as_raw_handle(),
                    &*self.overlapped,
                    &mut bytes,
                    1,
                );
            }
        }
    }
}
