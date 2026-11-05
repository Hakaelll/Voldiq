use crate::{Error, Result, ScanMode};
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanControlState {
    Idle,
    Running,
    PauseRequested,
    Paused,
    Finished,
}

/// One scan owns this controller; a pause is acknowledged only at a drained I/O boundary.
#[derive(Debug)]
pub struct ScanControl {
    state: Mutex<ScanControlState>,
    changed: Condvar,
}

impl Default for ScanControl {
    fn default() -> Self {
        Self {
            state: Mutex::new(ScanControlState::Idle),
            changed: Condvar::new(),
        }
    }
}

impl ScanControl {
    pub fn state(&self) -> Result<ScanControlState> {
        Ok(*self.state.lock().map_err(|_| poisoned())?)
    }

    pub fn request_pause(&self) -> Result<bool> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        match *state {
            ScanControlState::Running => {
                *state = ScanControlState::PauseRequested;
                Ok(true)
            }
            ScanControlState::PauseRequested | ScanControlState::Paused => Ok(true),
            ScanControlState::Idle | ScanControlState::Finished => Ok(false),
        }
    }

    pub fn resume(&self) -> Result<bool> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        match *state {
            ScanControlState::PauseRequested | ScanControlState::Paused => {
                *state = ScanControlState::Running;
                self.changed.notify_all();
                Ok(true)
            }
            ScanControlState::Running => Ok(true),
            ScanControlState::Idle | ScanControlState::Finished => Ok(false),
        }
    }

    pub(super) fn begin(&self, mode: ScanMode) -> Result<Run<'_>> {
        if mode != ScanMode::Directory {
            return Err(Error::Invalid(
                "Pause control requires directory enumeration.".into(),
            ));
        }
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        if *state != ScanControlState::Idle {
            return Err(Error::Invalid(
                "Scan controller has already been used.".into(),
            ));
        }
        *state = ScanControlState::Running;
        Ok(Run(self))
    }

    pub(super) fn checkpoint(&self, cancel: &AtomicBool) -> Result<bool> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Ok(false);
            }
            match *state {
                ScanControlState::Running => return Ok(true),
                ScanControlState::PauseRequested => {
                    *state = ScanControlState::Paused;
                    self.changed.notify_all();
                }
                ScanControlState::Paused => {}
                ScanControlState::Idle | ScanControlState::Finished => {
                    return Err(Error::Invalid("Scan controller is not active.".into()));
                }
            }
            // Existing cancellation tokens have no wake primitive. Poll only while paused.
            state = self
                .changed
                .wait_timeout(state, Duration::from_millis(50))
                .map_err(|_| poisoned())?
                .0;
        }
    }
}

pub(super) struct Run<'a>(&'a ScanControl);
impl Drop for Run<'_> {
    fn drop(&mut self) {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *state = ScanControlState::Finished;
        self.0.changed.notify_all();
    }
}
fn poisoned() -> Error {
    Error::Invalid("Scan control is unavailable.".into())
}
