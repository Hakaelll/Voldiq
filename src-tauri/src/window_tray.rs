use crate::{Engine, window_close_preference as preference};
use std::sync::Arc;
use tauri::{
    Manager, State, Window,
    menu::{Menu, MenuItem},
    tray::{TrayIcon, TrayIconBuilder},
};

const TRAY_ID: &str = "voldiq-window";
const OPEN_ID: &str = "voldiq-window-open";
const QUIT_ID: &str = "voldiq-window-quit";

#[tauri::command]
pub(crate) fn get_window_close_behavior(
    engine: State<'_, Arc<Engine>>,
) -> Result<preference::CloseBehavior, String> {
    preference::read(engine.operations.directory())
}

#[tauri::command]
pub(crate) fn set_window_close_behavior(
    behavior: preference::CloseBehavior,
    engine: State<'_, Arc<Engine>>,
) -> Result<preference::CloseBehavior, String> {
    preference::save(engine.operations.directory(), behavior)
}

pub(crate) fn hide_on_close(window: &Window, engine: &Engine) -> bool {
    if window.label() != "main"
        || !matches!(
            preference::read(engine.operations.directory()),
            Ok(preference::CloseBehavior::Tray)
        )
    {
        return false;
    }

    let Ok(tray) = show_tray(window.app_handle()) else {
        return false;
    };
    if window.hide().is_err() {
        let _ = tray.set_visible(false);
        return false;
    }
    let _ = engine.live.stop(None);
    true
}

fn show_tray(app: &tauri::AppHandle) -> tauri::Result<TrayIcon> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_visible(true)?;
        return Ok(tray);
    }
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("Voldiq icon".into()))?;
    let open = MenuItem::with_id(app, OPEN_ID, "Open Voldiq", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_ID, "Exit Voldiq", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Voldiq")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            OPEN_ID => {
                if let Some(window) = app.get_webview_window("main")
                    && window.show().is_ok()
                {
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                    if let Some(tray) = app.tray_by_id(TRAY_ID) {
                        let _ = tray.set_visible(false);
                    }
                }
            }
            QUIT_ID => app.exit(0),
            _ => {}
        })
        .build(app)
}
