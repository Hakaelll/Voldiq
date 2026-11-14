use crate::{
    config_file,
    windows_startup::{Status, installed_target},
};
use std::path::Path;
use windows::{
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::{NotificationSetting, ToastNotification, ToastNotificationManager},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::{Result, h},
};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

const PREFERENCE: &str = "scan-completion-notifications";

pub(crate) fn preference(
    directory: &Path,
    change: Option<bool>,
) -> std::result::Result<Status, String> {
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    if let Err(status) = installed_target(&user, &executable).map_err(|error| error.to_string())? {
        return Ok(status);
    }
    let path = directory.join(PREFERENCE);
    let registered = match change {
        Some(enabled) => config_file::set_marker(&path, enabled),
        None => config_file::read_marker(&path),
    }
    .map_err(|()| "Could not access the scan notification preference.")?;
    Ok(Status::Ready { registered })
}

pub(crate) fn scan_finished(directory: &Path) {
    send(directory, false);
}

pub(crate) fn scheduled_growth(directory: &Path) {
    send(directory, true);
}

fn send(directory: &Path, growth: bool) {
    if config_file::read_marker(&directory.join(PREFERENCE)) != Ok(true) {
        return;
    }
    // Notification delivery is independent of the scan result and Windows can suppress it.
    if matches!(
        preference(directory, None),
        Ok(Status::Ready { registered: true })
    ) {
        let _ = show(growth);
    }
}

pub(crate) fn should_notify(progress: &voldiq_core::Progress, cancelled: bool) -> bool {
    matches!(progress.phase, voldiq_core::ScanPhase::Complete)
        && !progress.previous
        && progress.entries > 0
        && !cancelled
}

pub(crate) fn notification() -> Result<ToastNotification> {
    toast(false)
}

fn toast(growth: bool) -> Result<ToastNotification> {
    let xml = XmlDocument::new()?;
    let content = if growth {
        h!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>Voldiq found storage growth</text><text>Open Scheduled scans for the measured change.</text></binding></visual></toast>"
        )
    } else {
        h!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>Voldiq scan finished</text><text>Open Voldiq to review the results.</text></binding></visual></toast>"
        )
    };
    xml.LoadXml(content)?;
    ToastNotification::CreateToastNotification(&xml)
}

fn show(growth: bool) -> Result<()> {
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }

    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let _apartment = Apartment;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(h!("io.voldiq.desktop"))?;
    if notifier.Setting()? == NotificationSetting::Enabled {
        let notification = if growth {
            toast(true)?
        } else {
            notification()?
        };
        notifier.Show(&notification)?;
    }
    Ok(())
}
