use crate::{
    Engine, analysis_status::ScanProgress, launch_scan, snapshot_commands::SnapshotDatabase,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io,
    net::Ipv4Addr,
    sync::{Arc, Mutex, MutexGuard},
};
use tauri::{State, ipc::Channel};
use voldiq_core::{
    ScanOptions,
    lan::{Invitation, LocalVolume, Peer, ScanReview, ScanStatus, Share, Summary, local_addresses},
};

const REMOTE_LIMIT: usize = 8;

#[derive(Default)]
pub(crate) struct Lan(Arc<Session>);

#[derive(Default)]
struct Session {
    state: Mutex<Sharing>,
    remote_operation: Mutex<()>,
}

#[derive(Default)]
struct Sharing {
    host: Option<Host>,
    remotes: BTreeMap<u32, Remote>,
    next_remote: u32,
}

struct Host {
    share: Share,
    volume: LocalVolume,
    sample: Summary,
}

struct Remote {
    peer: Arc<Peer>,
    sample: Summary,
    scan: Option<ScanStatus>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Status,
    Start { address: Ipv4Addr, drive: String },
    Stop,
    CreateInvitation,
    UpdateSharedSummary,
    Revoke { device: u32 },
    Join { code: String },
    Refresh { device: u32 },
    RequestScan { device: u32 },
    CheckScan { device: u32 },
    DeclineScan { request: String },
    Forget { device: u32 },
}

#[derive(Serialize)]
pub(crate) struct View {
    host: Option<HostView>,
    invitation: Option<Invitation>,
    remotes: Vec<RemoteView>,
}

#[derive(Serialize)]
struct HostView {
    endpoint: String,
    running: bool,
    drive: String,
    sample: SampleView,
    devices: Vec<u32>,
    invitation_seconds: u64,
    scans: Vec<ScanReview>,
}

#[derive(Serialize)]
struct RemoteView {
    device: u32,
    endpoint: String,
    sample: SampleView,
    scan: Option<ScanStatus>,
}

#[derive(Serialize)]
struct SampleView {
    total: String,
    free: String,
    used: String,
    observed_at: Option<u64>,
}

impl From<Summary> for SampleView {
    fn from(sample: Summary) -> Self {
        Self {
            total: sample.total.to_string(),
            free: sample.free.to_string(),
            used: sample.used.to_string(),
            observed_at: sample.observed_at,
        }
    }
}

impl Sharing {
    fn view(&self, invitation: Option<Invitation>) -> io::Result<View> {
        let host = self
            .host
            .as_ref()
            .map(|host| {
                let status = host.share.status()?;
                Ok::<_, io::Error>(HostView {
                    endpoint: status.endpoint,
                    running: status.running,
                    drive: host.volume.path().display().to_string(),
                    sample: host.sample.into(),
                    devices: status.devices,
                    invitation_seconds: status.invitation_seconds,
                    scans: status.scans,
                })
            })
            .transpose()?;
        let remotes = self
            .remotes
            .iter()
            .map(|(&device, remote)| RemoteView {
                device,
                endpoint: remote.peer.endpoint().to_string(),
                sample: remote.sample.into(),
                scan: remote.scan,
            })
            .collect();
        Ok(View {
            host,
            invitation,
            remotes,
        })
    }

    fn host(&mut self) -> io::Result<&mut Host> {
        self.host
            .as_mut()
            .ok_or_else(|| io::Error::other("Storage sharing is off."))
    }
}

impl Session {
    fn apply(&self, action: Action) -> io::Result<View> {
        if let Action::Join { code } = action {
            let _operation = lock(&self.remote_operation)?;
            if lock(&self.state)?.remotes.len() >= REMOTE_LIMIT {
                return Err(io::Error::other(
                    "Disconnect a device before adding another.",
                ));
            }
            let (peer, sample) = Peer::pair(&code)?;
            let mut state = lock(&self.state)?;
            state.next_remote = state
                .next_remote
                .checked_add(1)
                .ok_or_else(|| io::Error::other("Restart Voldiq to add another device."))?;
            let device = state.next_remote;
            state.remotes.insert(
                device,
                Remote {
                    peer: Arc::new(peer),
                    sample,
                    scan: None,
                },
            );
            return state.view(None);
        }
        if let Action::Refresh { device }
        | Action::RequestScan { device }
        | Action::CheckScan { device } = action
        {
            let _operation = lock(&self.remote_operation)?;
            let (peer, scan) = {
                let state = lock(&self.state)?;
                let remote = state
                    .remotes
                    .get(&device)
                    .ok_or_else(|| io::Error::other("This device is disconnected."))?;
                (remote.peer.clone(), remote.scan)
            };
            let mut sample = None;
            let scan = match action {
                Action::RequestScan { .. } => peer.request_scan()?,
                Action::CheckScan { .. } => peer.scan_status()?,
                _ => {
                    sample = Some(peer.summary()?);
                    if scan.is_some() {
                        peer.scan_status()?
                    } else {
                        None
                    }
                }
            };
            let mut state = lock(&self.state)?;
            if let Some(remote) = state.remotes.get_mut(&device) {
                remote.scan = scan;
                if let Some(sample) = sample {
                    remote.sample = sample;
                }
            }
            return state.view(None);
        }
        let mut state = lock(&self.state)?;
        let invitation = match action {
            Action::Status => None,
            Action::Start { address, drive } => {
                if state.host.is_some() {
                    return Err(io::Error::other(
                        "Stop the current share before starting another.",
                    ));
                }
                let (volume, sample) = LocalVolume::open(&drive)?;
                let share = Share::start(address, sample)?;
                state.host = Some(Host {
                    share,
                    volume,
                    sample,
                });
                None
            }
            Action::Stop => {
                drop(state.host.take());
                None
            }
            Action::CreateInvitation => Some(state.host()?.share.invitation()?),
            Action::UpdateSharedSummary => {
                let host = state.host()?;
                let sample = host.volume.refresh()?;
                host.share.publish(sample)?;
                host.sample = sample;
                None
            }
            Action::Revoke { device } => {
                state.host()?.share.revoke(device)?;
                None
            }
            Action::DeclineScan { request } => {
                state
                    .host()?
                    .share
                    .decline_scan(&request)
                    .map_err(io::Error::other)?;
                None
            }
            Action::Forget { device } => {
                state.remotes.remove(&device);
                None
            }
            Action::Join { .. }
            | Action::Refresh { .. }
            | Action::RequestScan { .. }
            | Action::CheckScan { .. } => {
                unreachable!()
            }
        };
        state.view(invitation)
    }
}

fn lock<T>(value: &Mutex<T>) -> io::Result<MutexGuard<'_, T>> {
    value
        .lock()
        .map_err(|_| io::Error::other("Storage sharing is unavailable."))
}

#[tauri::command]
pub(crate) async fn local_network_addresses() -> Result<Vec<Ipv4Addr>, String> {
    tauri::async_runtime::spawn_blocking(|| local_addresses().map_err(|error| error.to_string()))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn lan_sharing(action: Action, lan: State<'_, Lan>) -> Result<View, String> {
    let lan = lan.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        lan.apply(action).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

// Invoked by the host's explicit approval control. The LAN listener can only queue requests.
#[tauri::command]
pub(crate) async fn start_lan_scan(
    request: String,
    mut options: ScanOptions,
    progress: Channel<ScanProgress>,
    lan: State<'_, Lan>,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, SnapshotDatabase>,
) -> Result<u64, String> {
    let lan = lan.0.clone();
    let engine = engine.inner().clone();
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut state = lock(&lan.state).map_err(|error| error.to_string())?;
        let host = state.host().map_err(|error| error.to_string())?;
        let current = host.share.status().map_err(|error| error.to_string())?;
        if !current.scans.iter().any(|pending| pending.id == request) {
            return Err("This scan request is no longer pending.".into());
        }
        let root = host.volume.pin_scan().map_err(|error| error.to_string())?;
        let path = root
            .path()
            .to_str()
            .ok_or("Cannot display the approved drive path.")?
            .to_owned();
        options.expected_root = Some(root.identity());
        host.share.start_requested_scan(&request, || {
            launch_scan(path, options, progress, engine, &database, Some(root))
        })
    })
    .await
    .map_err(|error| error.to_string())?
}
