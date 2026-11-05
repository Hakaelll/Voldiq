use crate::Engine;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;
use voldiq_core::{
    ScanMode,
    scanner::{ScanControl, ScanControlState},
};

#[derive(Default)]
pub(crate) struct Sessions(Mutex<Option<(u64, Arc<ScanControl>)>>);
impl Sessions {
    pub(crate) fn attach(
        &self,
        id: u64,
        mode: ScanMode,
    ) -> Result<Option<Arc<ScanControl>>, String> {
        let controller = (mode == ScanMode::Directory).then(|| Arc::new(ScanControl::default()));
        *self.0.lock().map_err(|error| error.to_string())? =
            controller.as_ref().map(|control| (id, control.clone()));
        Ok(controller)
    }
    pub(crate) fn finish(&self, id: u64) {
        if let Ok(mut slot) = self.0.lock()
            && slot.as_ref().is_some_and(|(owner, _)| *owner == id)
        {
            *slot = None;
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum View {
    Idle,
    Running,
    PauseRequested,
    Paused,
    Finished,
}

enum Action {
    Read,
    Pause,
    Resume,
}
pub(crate) fn cancel_on_close(engine: &Engine) {
    if let Ok(active) = engine.active.lock()
        && let Some((id, cancel)) = active.as_ref()
        && let Ok(sessions) = engine.scan_controls.0.lock()
        && let Some((_, control)) = sessions.as_ref().filter(|(owner, _)| owner == id)
    {
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = control.resume();
    }
}
fn act(id: u64, engine: &Engine, action: Action) -> Result<View, String> {
    let active = engine.active.lock().map_err(|error| error.to_string())?;
    if !active.as_ref().is_some_and(|(owner, cancel)| {
        *owner == id && !cancel.load(std::sync::atomic::Ordering::Relaxed)
    }) {
        return Err("The scan ended or was cancelled.".into());
    }
    let sessions = engine
        .scan_controls
        .0
        .lock()
        .map_err(|error| error.to_string())?;
    let control = sessions
        .as_ref()
        .filter(|(owner, _)| *owner == id)
        .map(|(_, control)| control)
        .ok_or("Pause requires an active Directory enumeration scan.")?;
    match action {
        Action::Read => {}
        Action::Pause => {
            control.request_pause().map_err(|error| error.to_string())?;
        }
        Action::Resume => {
            control.resume().map_err(|error| error.to_string())?;
        }
    }
    Ok(match control.state().map_err(|error| error.to_string())? {
        ScanControlState::Idle => View::Idle,
        ScanControlState::Running => View::Running,
        ScanControlState::PauseRequested => View::PauseRequested,
        ScanControlState::Paused => View::Paused,
        ScanControlState::Finished => View::Finished,
    })
}

#[tauri::command]
pub(crate) fn get_scan_control(id: u64, engine: State<'_, Arc<Engine>>) -> Result<View, String> {
    act(id, &engine, Action::Read)
}
#[tauri::command]
pub(crate) fn pause_scan(id: u64, engine: State<'_, Arc<Engine>>) -> Result<View, String> {
    act(id, &engine, Action::Pause)
}
#[tauri::command]
pub(crate) fn resume_scan(id: u64, engine: State<'_, Arc<Engine>>) -> Result<View, String> {
    act(id, &engine, Action::Resume)
}
