use super::{invalid, open_in_browser};
use crate::Result;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
};
use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};
use url::Url;
use zeroize::Zeroizing;

const LIFETIME: Duration = Duration::from_secs(180);
const IO_WINDOW: Duration = Duration::from_secs(1);
const CALLBACK_PATH: &str = "/oauth/callback";

pub(super) struct Pending {
    redirect: Url,
    verifier: Zeroizing<String>,
    expires: Instant,
    cancel: Arc<AtomicBool>,
    reply: Receiver<Result<Zeroizing<String>>>,
}

impl Pending {
    pub(super) fn open(mut authorize: Url, port: u16, host: &'static str) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let redirect = Url::parse(&format!("http://{host}:{port}{CALLBACK_PATH}"))
            .expect("loopback redirect URL");
        let verifier = Zeroizing::new(random_value()?);
        let state = Zeroizing::new(random_value()?);
        let challenge = URL_SAFE_NO_PAD.encode(digest(&SHA256, verifier.as_bytes()).as_ref());
        authorize.query_pairs_mut().extend_pairs([
            ("redirect_uri", redirect.as_str()),
            ("state", state.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ]);
        let cancel = Arc::new(AtomicBool::new(false));
        let (send, reply) = mpsc::sync_channel(1);
        let worker_cancel = cancel.clone();
        thread::Builder::new()
            .name("cloud-sign-in".into())
            .spawn(move || {
                let _ = send.send(receive(listener, port, host, &state, &worker_cancel));
            })?;
        let pending = Self {
            redirect,
            verifier,
            expires: Instant::now() + LIFETIME,
            cancel,
            reply,
        };
        open_in_browser(authorize.as_str())?;
        Ok(pending)
    }

    pub(super) fn result(&self) -> Result<Option<Zeroizing<String>>> {
        if Instant::now() >= self.expires {
            return Err(invalid("Browser sign-in expired. Connect again to retry."));
        }
        match self.reply.try_recv() {
            Ok(result) => result.map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(invalid("The browser sign-in session ended.")),
        }
    }

    pub(super) fn redirect_uri(&self) -> &str {
        self.redirect.as_str()
    }

    pub(super) fn verifier(&self) -> &str {
        self.verifier.as_str()
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

fn random_value() -> Result<String> {
    let mut bytes = Zeroizing::new([0; 32]);
    SystemRandom::new()
        .fill(&mut *bytes)
        .map_err(|_| invalid("Windows could not create a sign-in challenge."))?;
    Ok(URL_SAFE_NO_PAD.encode(*bytes))
}

fn receive(
    listener: TcpListener,
    port: u16,
    host: &str,
    state: &str,
    cancel: &AtomicBool,
) -> Result<Zeroizing<String>> {
    let expires = Instant::now() + LIFETIME;
    let expected_host = format!("{host}:{port}");
    while Instant::now() < expires && !cancel.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, peer)) if peer.ip().is_loopback() => {
                if let Some(callback) = handle(&mut stream, &expected_host, state) {
                    return callback;
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(35));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(invalid("Browser sign-in expired. Connect again to retry."))
}

fn handle(
    stream: &mut TcpStream,
    expected_host: &str,
    state: &str,
) -> Option<Result<Zeroizing<String>>> {
    let deadline = Instant::now() + IO_WINDOW;
    let mut bytes = Zeroizing::new(Vec::with_capacity(8192));
    let mut chunk = Zeroizing::new([0; 1024]);
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        stream.set_read_timeout(Some(remaining)).ok()?;
        let size = stream.read(&mut *chunk).ok()?;
        if size == 0 || bytes.len() + size > 8192 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..size]);
        if bytes.windows(4).any(|end| end == b"\r\n\r\n") {
            break;
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut request = httparse::Request::new(&mut headers);
    if !matches!(request.parse(&bytes), Ok(httparse::Status::Complete(_)))
        || request.method != Some("GET")
        || request
            .headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case("host"))
            .count()
            != 1
        || !request.headers.iter().any(|header| {
            header.name.eq_ignore_ascii_case("host") && header.value == expected_host.as_bytes()
        })
    {
        respond(stream, false);
        return None;
    }
    let callback = request.path.and_then(|path| parse_callback(path, state));
    respond(stream, callback.is_some());
    callback
}

fn parse_callback(target: &str, expected_state: &str) -> Option<Result<Zeroizing<String>>> {
    let (path, query) = target.split_once('?')?;
    if path != CALLBACK_PATH || query.contains('#') {
        return None;
    }
    let mut state = None;
    let mut code = None;
    let mut denied = false;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "state" if state.is_none() => state = Some(Zeroizing::new(value.into_owned())),
            "code" if code.is_none() && (1..=4096).contains(&value.len()) => {
                code = Some(Zeroizing::new(value.into_owned()))
            }
            "error" if !denied => denied = true,
            "state" | "code" | "error" => return None,
            _ => {}
        }
    }
    if state.as_deref().map(String::as_str) != Some(expected_state) {
        return None;
    }
    match (code, denied) {
        (Some(code), false) => Some(Ok(code)),
        (None, true) => Some(Err(invalid("The provider declined browser sign-in."))),
        _ => None,
    }
}

fn respond(stream: &mut TcpStream, accepted: bool) {
    let (status, message) = if accepted {
        ("200 OK", "Return to Voldiq to continue.")
    } else {
        (
            "400 Bad Request",
            "Voldiq could not match this sign-in request.",
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
        message.len()
    );
    let _ = stream.set_write_timeout(Some(IO_WINDOW));
    let _ = stream.write_all(response.as_bytes());
}
