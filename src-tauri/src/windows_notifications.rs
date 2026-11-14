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
    send(directory, Notice::Scan);
}

pub(crate) fn scheduled_growth(directory: &Path) {
    send(directory, Notice::Growth);
}

pub(crate) fn scheduled_cleanup(directory: &Path) {
    send(directory, Notice::Cleanup);
}

enum Notice {
    Scan,
    Growth,
    Cleanup,
}

fn send(directory: &Path, notice: Notice) {
    if config_file::read_marker(&directory.join(PREFERENCE)) != Ok(true) {
        return;
    }
    // Notification delivery is independent of the scan result and Windows can suppress it.
    if matches!(
        preference(directory, None),
        Ok(Status::Ready { registered: true })
    ) {
        let _ = show(notice);
    }
}

pub(crate) fn should_notify(progress: &voldiq_core::Progress, cancelled: bool) -> bool {
    matches!(progress.phase, voldiq_core::ScanPhase::Complete)
        && !progress.previous
        && progress.entries > 0
        && !cancelled
}

pub(crate) fn notification() -> Result<ToastNotification> {
    toast(Notice::Scan)
}

fn toast(notice: Notice) -> Result<ToastNotification> {
    let xml = XmlDocument::new()?;
    let content = match notice {
        Notice::Growth => h!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>Voldiq found storage growth</text><text>Open Scheduled scans for the measured change.</text></binding></visual></toast>"
        ),
        Notice::Cleanup => h!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>Voldiq cleanup review ready</text><text>Open Scheduled scans to review the proposed files.</text></binding></visual></toast>"
        ),
        Notice::Scan => h!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>Voldiq scan finished</text><text>Open Voldiq to review the results.</text></binding></visual></toast>"
        ),
    };
    xml.LoadXml(content)?;
    ToastNotification::CreateToastNotification(&xml)
}

fn show(notice: Notice) -> Result<()> {
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
        let notification = match notice {
            Notice::Scan => notification()?,
            _ => toast(notice)?,
        };
        notifier.Show(&notification)?;
    }
    Ok(())
}
