//! Explicit desktop sharing of one volume's counters over a bounded, pinned TLS channel.
mod tls;
mod windows;
pub use windows::{LocalVolume, local_addresses};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rustls::ServerConfig;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const INVITE_LIFETIME: Duration = Duration::from_secs(120);
const INVITE_ATTEMPTS: u8 = 5;
const SCAN_REVIEW_LIFETIME: Duration = Duration::from_secs(120);
const SCAN_RETRY_DELAY: Duration = Duration::from_secs(60);
const PEER_LIMIT: usize = 8;
const FRAME_LIMIT: usize = 1024;
const INVITE_BYTES: usize = 62;
const INVITE_PREFIX: &str = "vld1-";

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub total: u64,
    pub free: u64,
    pub used: u64,
    pub observed_at: Option<u64>,
}

impl Summary {
    fn validate(self) -> io::Result<Self> {
        if self.total == 0
            || self.free > self.total
            || self.used > self.total
            || self.observed_at.is_some_and(|time| time > 253_402_300_799)
        {
            return Err(invalid("Inconsistent volume summary."));
        }
        Ok(self)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Pair {
        code: [u8; 24],
    },
    Summary {
        device: u32,
        token: [u8; 32],
    },
    #[serde(rename = "request_scan")]
    QueueScan {
        device: u32,
        token: [u8; 32],
    },
    ScanStatus {
        device: u32,
        token: [u8; 32],
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Reply {
    Paired {
        device: u32,
        token: [u8; 32],
        summary: Summary,
    },
    Summary {
        summary: Summary,
    },
    Scan {
        status: Option<ScanStatus>,
    },
    Denied,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Pending,
    Started,
    Declined,
    Expired,
}

#[derive(Serialize)]
pub struct ScanReview {
    pub id: String,
    pub device: u32,
}

struct ScanIntent {
    id: String,
    status: ScanStatus,
    until: Instant,
}

impl ScanIntent {
    fn status(&self, now: Instant) -> ScanStatus {
        if self.status == ScanStatus::Pending && now >= self.until {
            ScanStatus::Expired
        } else {
            self.status
        }
    }
}

struct PairedPeer {
    token_hash: blake3::Hash,
    scan: Option<ScanIntent>,
}

struct Invite {
    code_hash: blake3::Hash,
    until: Instant,
    attempts: u8,
}

#[derive(Serialize)]
pub struct Invitation {
    pub code: String,
    pub lifetime_seconds: u64,
}

#[derive(Serialize)]
pub struct ShareStatus {
    pub endpoint: String,
    pub running: bool,
    pub devices: Vec<u32>,
    pub invitation_seconds: u64,
    pub scans: Vec<ScanReview>,
}

struct State {
    summary: Summary,
    invite: Option<Invite>,
    peers: BTreeMap<u32, PairedPeer>,
    next_device: u32,
}

impl State {
    fn authorize(&mut self, device: u32, token: &[u8; 32]) -> Option<&mut PairedPeer> {
        self.peers
            .get_mut(&device)
            .filter(|peer| peer.token_hash == blake3::hash(token))
    }

    fn reply(&mut self, request: Request) -> io::Result<Reply> {
        match request {
            Request::Pair { code } => {
                let Some(invite) = self
                    .invite
                    .as_mut()
                    .filter(|invite| invite.until > Instant::now() && invite.attempts > 0)
                else {
                    return Ok(Reply::Denied);
                };
                invite.attempts -= 1;
                if blake3::hash(&code) != invite.code_hash || self.peers.len() == PEER_LIMIT {
                    return Ok(Reply::Denied);
                }
                self.invite = None;
                let token = random()?;
                let device = self.next_device;
                self.next_device = device
                    .checked_add(1)
                    .ok_or_else(|| invalid("Restart sharing to add another device."))?;
                self.peers.insert(
                    device,
                    PairedPeer {
                        token_hash: blake3::hash(&token),
                        scan: None,
                    },
                );
                Ok(Reply::Paired {
                    token,
                    device,
                    summary: self.summary,
                })
            }
            Request::Summary { device, token } => {
                if self.authorize(device, &token).is_some() {
                    Ok(Reply::Summary {
                        summary: self.summary,
                    })
                } else {
                    Ok(Reply::Denied)
                }
            }
            Request::QueueScan { device, token } => {
                let Some(peer) = self.authorize(device, &token) else {
                    return Ok(Reply::Denied);
                };
                let now = Instant::now();
                if peer
                    .scan
                    .as_ref()
                    .is_none_or(|request| now >= request.until)
                {
                    peer.scan = Some(ScanIntent {
                        id: URL_SAFE_NO_PAD.encode(random::<16>()?),
                        status: ScanStatus::Pending,
                        until: now + SCAN_REVIEW_LIFETIME,
                    });
                }
                Ok(Reply::Scan {
                    status: peer.scan.as_ref().map(|scan| scan.status(now)),
                })
            }
            Request::ScanStatus { device, token } => {
                let Some(peer) = self.authorize(device, &token) else {
                    return Ok(Reply::Denied);
                };
                Ok(Reply::Scan {
                    status: peer.scan.as_ref().map(|scan| scan.status(Instant::now())),
                })
            }
        }
    }
}

pub struct Share {
    endpoint: SocketAddrV4,
    fingerprint: [u8; 32],
    state: Arc<Mutex<State>>,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Share {
    pub fn start(address: Ipv4Addr, summary: Summary) -> io::Result<Self> {
        if !address.is_private() {
            return Err(invalid("Choose a private IPv4 address on this computer."));
        }
        Self::bind(address, summary)
    }

    fn bind(address: Ipv4Addr, summary: Summary) -> io::Result<Self> {
        let summary = summary.validate()?;
        let (config, fingerprint) = tls::server()?;
        let listener = TcpListener::bind(SocketAddrV4::new(address, 0))?;
        listener.set_nonblocking(true)?;
        let endpoint = SocketAddrV4::new(address, listener.local_addr()?.port());
        let state = Arc::new(Mutex::new(State {
            summary,
            invite: None,
            peers: BTreeMap::new(),
            next_device: 1,
        }));
        let stopped = Arc::new(AtomicBool::new(false));
        let worker = {
            let state = state.clone();
            let stopped = stopped.clone();
            thread::Builder::new()
                .name("voldiq-lan".into())
                .spawn(move || listen(listener, config, &state, &stopped))?
        };
        Ok(Self {
            endpoint,
            fingerprint,
            state,
            stopped,
            worker: Some(worker),
        })
    }

    pub fn status(&self) -> io::Result<ShareStatus> {
        let state = self.state.lock().map_err(|_| unavailable())?;
        let running = self.running();
        let now = Instant::now();
        let remaining = state
            .invite
            .as_ref()
            .filter(|invite| running && invite.attempts > 0)
            .map(|invite| invite.until.saturating_duration_since(now))
            .unwrap_or_default();
        Ok(ShareStatus {
            endpoint: self.endpoint.to_string(),
            running,
            devices: state.peers.keys().copied().collect(),
            invitation_seconds: remaining.as_secs() + u64::from(remaining.subsec_nanos() != 0),
            scans: state
                .peers
                .iter()
                .filter_map(|(&device, peer)| {
                    let scan = peer.scan.as_ref()?;
                    (running && scan.status(now) == ScanStatus::Pending).then(|| ScanReview {
                        id: scan.id.clone(),
                        device,
                    })
                })
                .collect(),
        })
    }

    pub fn invitation(&self) -> io::Result<Invitation> {
        if !self.running() {
            return Err(unavailable());
        }
        let code = random::<24>()?;
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        if state.peers.len() >= PEER_LIMIT {
            return Err(invalid("Revoke a paired device before adding another."));
        }
        state.invite = Some(Invite {
            code_hash: blake3::hash(&code),
            until: Instant::now() + INVITE_LIFETIME,
            attempts: INVITE_ATTEMPTS,
        });
        let mut bytes = [0; INVITE_BYTES];
        bytes[..4].copy_from_slice(&self.endpoint.ip().octets());
        bytes[4..6].copy_from_slice(&self.endpoint.port().to_be_bytes());
        bytes[6..38].copy_from_slice(&self.fingerprint);
        bytes[38..].copy_from_slice(&code);
        Ok(Invitation {
            code: format!("{INVITE_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes)),
            lifetime_seconds: INVITE_LIFETIME.as_secs(),
        })
    }

    pub fn revoke(&self, device: u32) -> io::Result<()> {
        self.state
            .lock()
            .map_err(|_| unavailable())?
            .peers
            .remove(&device);
        Ok(())
    }

    pub fn publish(&self, summary: Summary) -> io::Result<()> {
        if !self.running() {
            return Err(unavailable());
        }
        self.state.lock().map_err(|_| unavailable())?.summary = summary.validate()?;
        Ok(())
    }

    /// Only called by a host-local action, after the exact volume has been checked.
    pub fn start_requested_scan<T>(
        &self,
        id: &str,
        launch: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        self.finish_review(id, ScanStatus::Started, launch)
    }

    pub fn decline_scan(&self, id: &str) -> Result<(), String> {
        self.finish_review(id, ScanStatus::Declined, || Ok(()))
    }

    fn finish_review<T>(
        &self,
        id: &str,
        status: ScanStatus,
        apply: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        if !self.running() {
            return Err(unavailable().to_string());
        }
        let mut state = self.state.lock().map_err(|_| unavailable().to_string())?;
        let now = Instant::now();
        let scan = state
            .peers
            .values_mut()
            .filter_map(|peer| peer.scan.as_mut())
            .find(|scan| scan.id == id && scan.status(now) == ScanStatus::Pending)
            .ok_or("This scan request has expired or is no longer pending.")?;
        let result = apply()?;
        scan.status = status;
        scan.until = Instant::now() + SCAN_RETRY_DELAY;
        Ok(result)
    }

    fn running(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn listen(
    listener: TcpListener,
    config: Arc<ServerConfig>,
    state: &Mutex<State>,
    stopped: &AtomicBool,
) {
    while !stopped.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((socket, peer))
                if peer.ip().is_loopback()
                    || matches!(peer.ip(), std::net::IpAddr::V4(ip) if ip.is_private()) =>
            {
                let _ = serve(socket, config.clone(), state, stopped);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(60));
            }
            Err(_) => break,
        }
    }
}

fn serve(
    socket: TcpStream,
    config: Arc<ServerConfig>,
    state: &Mutex<State>,
    stopped: &AtomicBool,
) -> io::Result<()> {
    socket.set_nonblocking(false)?;
    let mut channel = tls::accept(socket, config)?;
    let request = read_frame(&mut channel)?;
    let mut state = state.lock().map_err(|_| unavailable())?;
    if stopped.load(Ordering::Acquire) {
        return Err(unavailable());
    }
    let reply = state.reply(request)?;
    let delivered = write_frame(&mut channel, &reply);
    if delivered.is_err()
        && let Reply::Paired { device, .. } = reply
    {
        state.peers.remove(&device);
    }
    delivered
}

struct Address {
    endpoint: SocketAddrV4,
    fingerprint: [u8; 32],
}

impl Address {
    fn request(&self, request: &Request) -> io::Result<Reply> {
        let mut stream = tls::connect(self.endpoint.into(), self.fingerprint)?;
        write_frame(&mut stream, request)?;
        read_frame(&mut stream)
    }
}

pub struct Peer {
    address: Address,
    device: u32,
    token: [u8; 32],
}

impl Peer {
    pub fn pair(invitation: &str) -> io::Result<(Self, Summary)> {
        let (address, code) = decode(invitation, Ipv4Addr::is_private)?;
        Self::connect(address, code)
    }

    fn connect(address: Address, code: [u8; 24]) -> io::Result<(Self, Summary)> {
        match address.request(&Request::Pair { code })? {
            Reply::Paired {
                device,
                token,
                summary,
            } => Ok((
                Self {
                    address,
                    device,
                    token,
                },
                summary.validate()?,
            )),
            _ => Err(denied(
                "Invitation expired, was already used, or was refused.",
            )),
        }
    }

    pub fn endpoint(&self) -> SocketAddrV4 {
        self.address.endpoint
    }

    pub fn summary(&self) -> io::Result<Summary> {
        match self.address.request(&Request::Summary {
            device: self.device,
            token: self.token,
        })? {
            Reply::Summary { summary } => summary.validate(),
            _ => Err(denied("Access to this device was revoked.")),
        }
    }

    pub fn request_scan(&self) -> io::Result<Option<ScanStatus>> {
        self.scan_reply(Request::QueueScan {
            device: self.device,
            token: self.token,
        })
    }

    pub fn scan_status(&self) -> io::Result<Option<ScanStatus>> {
        self.scan_reply(Request::ScanStatus {
            device: self.device,
            token: self.token,
        })
    }

    fn scan_reply(&self, request: Request) -> io::Result<Option<ScanStatus>> {
        match self.address.request(&request)? {
            Reply::Scan { status } => Ok(status),
            _ => Err(denied("The host refused this scan request.")),
        }
    }
}

fn decode(
    invitation: &str,
    allowed: impl FnOnce(&Ipv4Addr) -> bool,
) -> io::Result<(Address, [u8; 24])> {
    let invalid_invite = || invalid("Paste a Voldiq invitation for a private network.");
    let encoded = invitation
        .trim()
        .strip_prefix(INVITE_PREFIX)
        .ok_or_else(invalid_invite)?;
    if encoded.len() != 83 {
        return Err(invalid_invite());
    }
    let mut bytes = [0; INVITE_BYTES];
    if URL_SAFE_NO_PAD.decode_slice(encoded, &mut bytes).ok() != Some(INVITE_BYTES) {
        return Err(invalid_invite());
    }
    let [a, b, c, d, port_hi, port_lo, ..] = bytes;
    let ip = Ipv4Addr::new(a, b, c, d);
    let port = u16::from_be_bytes([port_hi, port_lo]);
    if port == 0 || !allowed(&ip) {
        return Err(invalid_invite());
    }
    let mut fingerprint = [0; 32];
    fingerprint.copy_from_slice(&bytes[6..38]);
    let mut code = [0; 24];
    code.copy_from_slice(&bytes[38..]);
    Ok((
        Address {
            endpoint: SocketAddrV4::new(ip, port),
            fingerprint,
        },
        code,
    ))
}

fn read_frame<T: DeserializeOwned>(stream: &mut impl Read) -> io::Result<T> {
    let mut length = [0; 2];
    stream.read_exact(&mut length)?;
    let size = u16::from_be_bytes(length) as usize;
    let mut buffer = [0; FRAME_LIMIT];
    let payload = buffer
        .get_mut(..size)
        .ok_or_else(|| invalid("Local message is too large."))?;
    stream.read_exact(payload)?;
    serde_json::from_slice(payload).map_err(io::Error::other)
}

fn write_frame(stream: &mut impl Write, reply: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(reply).map_err(io::Error::other)?;
    if bytes.len() > FRAME_LIMIT {
        return Err(invalid("Local message is too large."));
    }
    stream.write_all(&(bytes.len() as u16).to_be_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()
}

fn random<const N: usize>() -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| unavailable())?;
    Ok(bytes)
}

fn unavailable() -> io::Error {
    io::Error::other("Local storage sharing is unavailable.")
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
