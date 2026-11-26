//! Cloud account metadata is independent of scan paths and volume accounting.
mod browser;
mod credential;
mod loopback;
pub mod recycle;

pub use browser::{BrowserAccount, BrowserProvider};

use crate::{Error, Result};
use reqwest::{StatusCode, blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    io::Read,
    path::Path,
    time::{Duration, Instant},
};
use url::{Url, form_urlencoded};
use zeroize::Zeroizing;

const LOGIN: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/";
const SCOPE: &str = "https://graph.microsoft.com/Files.Read offline_access";
const RECYCLE_SCOPE: &str = "https://graph.microsoft.com/Files.ReadWrite";
const GRAPH: &str = "https://graph.microsoft.com/v1.0/";
const MAX_BODY: u64 = 1024 * 1024;
const PAGE_SIZE: usize = 80;
const MAX_DEPTH: usize = 64;

fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Status,
    Begin {
        client_id: String,
        #[serde(default)]
        client_secret: Option<String>,
    },
    Poll,
    OpenSignIn,
    Cancel,
    Browse,
    EnterFolder {
        id: String,
    },
    Parent,
    FirstPage,
    NextPage,
    Disconnect,
    Revoke,
    OpenPermissions {
        work_account: bool,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum View {
    Disconnected,
    Saved,
    Waiting {
        code: String,
        retry_seconds: u64,
    },
    BrowserWaiting,
    Connected {
        work_account: bool,
        page: Page,
        quota: Quota,
    },
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Quota {
    #[serde(default, deserialize_with = "byte_string")]
    total: Option<String>,
    #[serde(default, deserialize_with = "byte_string")]
    used: Option<String>,
    #[serde(default, deserialize_with = "byte_string")]
    remaining: Option<String>,
}

fn byte_string<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> std::result::Result<Option<String>, D::Error> {
    Ok(Option::<u64>::deserialize(input)?.map(|bytes| bytes.to_string()))
}

#[derive(Clone, Serialize)]
pub struct Page {
    folder: String,
    has_parent: bool,
    number: u32,
    has_next: bool,
    items: Vec<Item>,
}

#[derive(Clone, Serialize)]
struct Item {
    id: String,
    name: String,
    kind: ItemKind,
    bytes: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum ItemKind {
    Folder,
    File,
    Reference,
}

#[derive(Default)]
pub struct Account {
    pending: Option<Pending>,
    active: Option<Active>,
}

struct Pending {
    authorization: DeviceAuthorization,
    client_id: String,
    expires: Instant,
    next: Instant,
    interval: Duration,
}

#[derive(Deserialize)]
struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: u64,
    expires_in: u64,
}

impl Drop for DeviceAuthorization {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.device_code.zeroize();
        self.user_code.zeroize();
    }
}

struct Active {
    sealed: Vec<u8>,
    access: Zeroizing<String>,
    expires: Instant,
    drive: Drive,
    folders: Vec<(String, String)>,
    page: Page,
    next: Option<Url>,
}

#[derive(Deserialize)]
struct Drive {
    id: String,
    #[serde(rename = "driveType")]
    drive_type: String,
    #[serde(default)]
    quota: Quota,
}

#[derive(Deserialize)]
struct Drives {
    value: Vec<Drive>,
    #[serde(rename = "@odata.nextLink")]
    next: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    token_type: String,
    #[serde(default)]
    scope: String,
    expires_in: u64,
}

impl Drop for Tokens {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TokenReply {
    Authorized(Tokens),
    Failed { error: String },
}

impl Account {
    pub fn run(&mut self, directory: &Path, action: Action) -> Result<View> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| Instant::now() >= pending.expires)
        {
            self.pending = None;
        }
        let store = credential::Store::open(directory);
        let mut saved = store.read()?;
        if self
            .active
            .as_ref()
            .is_some_and(|active| Some(&active.sealed) != saved.as_ref())
        {
            self.active = None;
        }
        match action {
            Action::Status => {}
            Action::OpenPermissions { work_account } => {
                let url = if work_account {
                    "https://myapps.microsoft.com/"
                } else {
                    "https://account.live.com/consent/Manage"
                };
                open_in_browser(url)?;
            }
            Action::Begin {
                client_id,
                client_secret,
            } => {
                if client_secret.map(Zeroizing::new).is_some() {
                    return Err(invalid("OneDrive only uses the public application ID."));
                }
                if saved.is_some() || self.pending.is_some() {
                    return Err(invalid("Finish the current OneDrive connection first."));
                }
                self.begin(client_id)?;
            }
            Action::Poll => {
                if saved.is_some() {
                    self.pending = None;
                } else if let Some(active) = self.poll(&store)? {
                    saved = Some(active.sealed.clone());
                    self.active = Some(active);
                }
            }
            Action::OpenSignIn => {
                let pending = self
                    .pending
                    .as_ref()
                    .ok_or_else(|| invalid("Start OneDrive sign-in again."))?;
                pending.open()?;
            }
            Action::Cancel => self.pending = None,
            Action::Revoke => {
                return Err(invalid(
                    "Use Manage Microsoft permissions to revoke OneDrive access.",
                ));
            }
            Action::Disconnect => {
                store.forget()?;
                self.pending = None;
                self.active = None;
                saved = None;
            }
            Action::Browse => {
                let sealed = saved
                    .as_deref()
                    .ok_or_else(|| invalid("Connect OneDrive first."))?;
                let active = resume(&store, sealed)?;
                saved = Some(active.sealed.clone());
                self.active = Some(active);
            }
            Action::EnterFolder { .. } | Action::Parent | Action::FirstPage | Action::NextPage => {
                let active = self
                    .active
                    .as_mut()
                    .ok_or_else(|| invalid("Open OneDrive first."))?;
                active.navigate(&store, action)?;
                saved = Some(active.sealed.clone());
            }
        }
        Ok(self.view(saved.is_some()))
    }

    fn view(&self, saved: bool) -> View {
        if let Some(active) = &self.active {
            View::Connected {
                work_account: active.drive.drive_type != "personal",
                page: active.page.clone(),
                quota: active.drive.quota.clone(),
            }
        } else if let Some(pending) = &self.pending {
            View::Waiting {
                code: pending.code().into(),
                retry_seconds: pending.retry_seconds(),
            }
        } else if saved {
            View::Saved
        } else {
            View::Disconnected
        }
    }

    fn begin(&mut self, client_id: String) -> Result<()> {
        self.pending = Some(Pending::request(client_id, SCOPE)?);
        Ok(())
    }

    fn poll(&mut self, store: &credential::Store) -> Result<Option<Active>> {
        let Some(pending) = &mut self.pending else {
            return Ok(None);
        };
        let client = client()?;
        let Some(tokens) = pending.poll(&client, "Files.Read")? else {
            return Ok(None);
        };
        let pending = self.pending.take().expect("pending sign-in");
        let mut active = Active::open(&client, &tokens, None)?;
        active.sealed = store.save(
            None,
            &pending.client_id,
            &active.drive.id,
            tokens.refresh_token.as_deref(),
        )?;
        Ok(Some(active))
    }

    pub fn recycle_source(&self, directory: &Path, id: &str) -> Result<recycle::Source> {
        let active = self
            .active
            .as_ref()
            .ok_or_else(|| invalid("Open OneDrive first."))?;
        let store = credential::Store::open(directory);
        let selected = recycle::Selection::capture(
            &active.page,
            active.folders.last().map(|(id, _)| id.as_str()),
            id,
            active.expires,
            &store,
            &active.sealed,
        )?;
        let credential: credential::Credential = credential::unseal(&active.sealed)?;
        if credential.drive_id != active.drive.id {
            return Err(invalid("The OneDrive identity changed."));
        }
        recycle::Source::new(
            recycle::Provider::OneDrive,
            store,
            active.sealed.clone(),
            credential.drive_id.clone(),
            recycle::Login::Microsoft(credential.client_id.clone()),
            selected,
            &active.access,
        )
    }
}

impl Pending {
    fn request(client_id: String, scope: &str) -> Result<Self> {
        let client_id = client_id.trim().to_ascii_lowercase();
        let parts = client_id.split('-');
        if !parts.clone().map(str::len).eq([8, 4, 4, 4, 12])
            || !client_id
                .bytes()
                .all(|byte| byte == b'-' || byte.is_ascii_hexdigit())
        {
            return Err(invalid(
                "Enter the application (client) ID from Microsoft Entra.",
            ));
        }
        let (status, authorization) = oauth::<DeviceAuthorization>(
            &client()?,
            "devicecode",
            &[("client_id", &client_id), ("scope", scope)],
        )?;
        if !status.is_success()
            || !matches!(
                authorization.verification_uri.as_str(),
                "https://microsoft.com/devicelogin" | "https://www.microsoft.com/devicelogin"
            )
            || authorization.device_code.is_empty()
            || !(1..=40).contains(&authorization.user_code.len())
            || !authorization
                .user_code
                .bytes()
                .all(|byte| byte == b'-' || byte.is_ascii_alphanumeric())
            || !(1..=30).contains(&authorization.interval)
            || !(1..=1800).contains(&authorization.expires_in)
        {
            return Err(invalid(
                "Microsoft returned invalid OneDrive sign-in details.",
            ));
        }
        let now = Instant::now();
        let interval = Duration::from_secs(authorization.interval);
        Ok(Self {
            client_id,
            next: now + interval,
            interval,
            expires: now + Duration::from_secs(authorization.expires_in),
            authorization,
        })
    }

    fn code(&self) -> &str {
        &self.authorization.user_code
    }

    fn retry_seconds(&self) -> u64 {
        self.next
            .saturating_duration_since(Instant::now())
            .as_secs()
            + 1
    }

    fn open(&self) -> Result<()> {
        open_in_browser(&self.authorization.verification_uri)
    }

    fn poll(&mut self, client: &Client, required: &str) -> Result<Option<Tokens>> {
        let now = Instant::now();
        if now >= self.expires {
            return Err(invalid("Microsoft sign-in expired. Start sign-in again."));
        }
        if now < self.next {
            return Ok(None);
        }
        self.next = now + self.interval;
        let (status, reply) = oauth(
            client,
            "token",
            &[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", &self.client_id),
                ("device_code", &self.authorization.device_code),
            ],
        )?;
        match reply {
            TokenReply::Failed { error } if error == "authorization_pending" => Ok(None),
            TokenReply::Failed { error } if error == "slow_down" => {
                self.interval += Duration::from_secs(5);
                self.next = Instant::now() + self.interval;
                Ok(None)
            }
            reply => {
                self.expires = now;
                checked_tokens(status, reply, required).map(Some)
            }
        }
    }
}

fn resume(store: &credential::Store, sealed: &[u8]) -> Result<Active> {
    let credential: credential::Credential = credential::unseal(sealed)?;
    let client = client()?;
    let tokens = refresh(&client, &credential)?;
    let mut active = Active::open(&client, &tokens, Some(&credential.drive_id))?;
    active.sealed = store.save(
        Some(sealed),
        &credential.client_id,
        &active.drive.id,
        tokens
            .refresh_token
            .as_deref()
            .or(Some(&credential.refresh_token)),
    )?;
    Ok(active)
}

impl Active {
    fn open(client: &Client, tokens: &Tokens, expected_drive: Option<&str>) -> Result<Self> {
        let access = Zeroizing::new(tokens.access_token.clone());
        let expires = Instant::now() + Duration::from_secs(tokens.expires_in);
        let drive = current_drive(client, &access, expected_drive)?;
        let (page, next) = folder_page(client, &access, &drive.id, &[], None, 1)?;
        Ok(Self {
            sealed: Vec::new(),
            access,
            expires,
            drive,
            folders: Vec::new(),
            page,
            next,
        })
    }

    fn navigate(&mut self, store: &credential::Store, action: Action) -> Result<()> {
        let mut folders = self.folders.clone();
        let (next, number) = match action {
            Action::EnterFolder { id } if folders.len() < MAX_DEPTH => {
                let item = self
                    .page
                    .items
                    .iter()
                    .find(|item| item.id == id && matches!(item.kind, ItemKind::Folder))
                    .ok_or_else(|| invalid("That folder is outside the current OneDrive page."))?;
                folders.push((item.id.clone(), item.name.clone()));
                (None, 1)
            }
            Action::Parent if folders.pop().is_some() => (None, 1),
            Action::FirstPage => (None, 1),
            Action::NextPage if self.next.is_some() => {
                (self.next.clone(), self.page.number.saturating_add(1))
            }
            _ => return Err(invalid("That OneDrive page is unavailable.")),
        };
        let client = client()?;
        if self.expires <= Instant::now() + Duration::from_secs(15) {
            let credential: credential::Credential = credential::unseal(&self.sealed)?;
            let tokens = refresh(&client, &credential)?;
            self.sealed = store.save(
                Some(&self.sealed),
                &credential.client_id,
                &self.drive.id,
                tokens
                    .refresh_token
                    .as_deref()
                    .or(Some(&credential.refresh_token)),
            )?;
            self.access = Zeroizing::new(tokens.access_token.clone());
            self.expires = Instant::now() + Duration::from_secs(tokens.expires_in);
        }
        let (page, next) = folder_page(
            &client,
            &self.access,
            &self.drive.id,
            &folders,
            next,
            number,
        )?;
        if store.read()?.as_ref() != Some(&self.sealed) {
            return Err(invalid(
                "The OneDrive connection changed. Refresh its status.",
            ));
        }
        self.folders = folders;
        self.page = page;
        self.next = next;
        Ok(())
    }
}

fn current_drive(client: &Client, access: &str, expected_drive: Option<&str>) -> Result<Drive> {
    let mut url = Url::parse(GRAPH)
        .expect("Graph base URL")
        .join("me/drives")
        .expect("drive URL");
    url.query_pairs_mut()
        .append_pair("$select", "id,driveType,quota");
    let drives: Drives = graph(client, access, url)?;
    let mut matches = drives.value.into_iter().filter(|drive| {
        valid_id(&drive.id)
            && matches!(drive.drive_type.as_str(), "personal" | "business")
            && expected_drive.is_none_or(|id| id == drive.id)
    });
    let drive = matches.next().ok_or_else(|| {
        invalid("Microsoft did not return an existing OneDrive for this connection.")
    })?;
    if matches.next().is_some() || (expected_drive.is_none() && drives.next.is_some()) {
        return Err(invalid(
            "Microsoft returned multiple OneDrive candidates. This connection requires one existing drive.",
        ));
    }
    Ok(drive)
}

#[derive(Deserialize)]
struct GraphPage {
    value: Vec<GraphItem>,
    #[serde(rename = "@odata.nextLink")]
    next: Option<String>,
}

#[derive(Deserialize)]
struct GraphItem {
    id: String,
    name: String,
    size: Option<u64>,
    folder: Option<serde::de::IgnoredAny>,
    file: Option<serde::de::IgnoredAny>,
    #[serde(rename = "remoteItem")]
    remote: Option<serde::de::IgnoredAny>,
}

fn folder_page(
    client: &Client,
    access: &str,
    drive: &str,
    folders: &[(String, String)],
    next: Option<Url>,
    number: u32,
) -> Result<(Page, Option<Url>)> {
    let mut url = Url::parse(GRAPH).expect("Graph base URL");
    let mut path = url.path_segments_mut().expect("Graph path");
    path.pop_if_empty().extend(["drives", drive]);
    if let Some((id, _)) = folders.last() {
        path.extend(["items", id]);
    } else {
        path.push("root");
    }
    path.push("children");
    drop(path);
    let collection = url.clone();
    url.query_pairs_mut()
        .append_pair("$select", "id,name,size,folder,file,remoteItem")
        .append_pair("$top", &PAGE_SIZE.to_string());
    let reply: GraphPage = graph(client, access, next.unwrap_or(url))?;
    if reply.value.len() > PAGE_SIZE {
        return Err(invalid("OneDrive returned more items than the page limit."));
    }
    let next = reply
        .next
        .as_deref()
        .map(|next| continuation(&collection, next))
        .transpose()?;
    let items = reply
        .value
        .into_iter()
        .map(|item| {
            if !valid_id(&item.id) || item.name.is_empty() || item.name.len() > 2048 {
                return Err(invalid("OneDrive returned invalid item metadata."));
            }
            let kind = if item.remote.is_some() {
                ItemKind::Reference
            } else if item.folder.is_some() && folders.len() < MAX_DEPTH {
                ItemKind::Folder
            } else if item.file.is_some() {
                ItemKind::File
            } else {
                ItemKind::Reference
            };
            let bytes = (!matches!(kind, ItemKind::Reference))
                .then_some(item.size)
                .flatten()
                .map(|size| size.to_string());
            Ok(Item {
                id: item.id,
                name: item.name,
                kind,
                bytes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((
        Page {
            folder: folders
                .last()
                .map_or_else(|| "OneDrive".into(), |(_, name)| name.clone()),
            has_parent: !folders.is_empty(),
            number,
            has_next: next.is_some(),
            items,
        },
        next,
    ))
}

fn continuation(collection: &Url, next: &str) -> Result<Url> {
    if next.len() > 16_384 {
        return Err(invalid("OneDrive returned an oversized continuation."));
    }
    let url =
        Url::parse(next).map_err(|_| invalid("OneDrive returned an invalid continuation."))?;
    if url.origin() != collection.origin()
        || url.path() != collection.path()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !url
            .query_pairs()
            .any(|(key, value)| key.eq_ignore_ascii_case("$skiptoken") && !value.is_empty())
        || url.query_pairs().any(|(key, _)| {
            !["$skiptoken", "$top", "$select"]
                .iter()
                .any(|allowed| key.eq_ignore_ascii_case(allowed))
        })
    {
        return Err(invalid(
            "OneDrive returned a continuation outside the current folder.",
        ));
    }
    Ok(url)
}

fn valid_id(id: &str) -> bool {
    (1..=256).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'!' | b'.'))
        && !matches!(id, "." | "..")
}

fn client() -> Result<Client> {
    Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| invalid("The cloud connection could not be initialized."))
}

fn decode<T: DeserializeOwned>(response: reqwest::blocking::Response) -> Result<T> {
    let mut body = Zeroizing::new(Vec::new());
    response
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .map_err(|_| invalid("The cloud response could not be read."))?;
    if body.len() as u64 > MAX_BODY {
        return Err(invalid("The cloud response exceeded the size limit."));
    }
    serde_json::from_slice(&body).map_err(|_| invalid("The provider returned an unexpected response. Check the app registration or try again."))
}

fn oauth<T: DeserializeOwned>(
    client: &Client,
    route: &str,
    fields: &[(&str, &str)],
) -> Result<(StatusCode, T)> {
    let response = send_form(client, &format!("{LOGIN}{route}"), fields)?;
    Ok((response.status(), decode(response)?))
}

fn send_form(
    client: &Client,
    endpoint: &str,
    fields: &[(&str, &str)],
) -> Result<reqwest::blocking::Response> {
    let body = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().copied())
        .finish();
    client
        .post(endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|_| invalid("The cloud sign-in service could not be reached."))
}

fn graph<T: DeserializeOwned>(client: &Client, access: &str, url: Url) -> Result<T> {
    let response = client
        .get(url)
        .bearer_auth(access)
        .send()
        .map_err(|_| invalid("OneDrive could not be reached."))?;
    match response.status() {
        status if status.is_success() => decode(response),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(invalid(
            "Microsoft refused access. Check account permissions or reconnect OneDrive.",
        )),
        StatusCode::TOO_MANY_REQUESTS => Err(invalid(
            "OneDrive limited requests. Wait before trying again.",
        )),
        StatusCode::NOT_FOUND => Err(invalid("OneDrive could not find this drive or folder.")),
        _ => Err(invalid(
            "OneDrive could not fetch this metadata. Try again.",
        )),
    }
}

fn refresh(client: &Client, credential: &credential::Credential) -> Result<Tokens> {
    let (status, reply) = oauth(
        client,
        "token",
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &credential.client_id),
            ("refresh_token", &credential.refresh_token),
            ("scope", SCOPE),
        ],
    )?;
    checked_tokens(status, reply, "Files.Read")
}

fn checked_tokens(status: StatusCode, reply: TokenReply, required: &str) -> Result<Tokens> {
    match reply {
        TokenReply::Authorized(tokens)
            if status.is_success()
                && tokens.token_type.eq_ignore_ascii_case("bearer")
                && (1..=86_400).contains(&tokens.expires_in)
                && !tokens.access_token.is_empty()
                && microsoft_scope(&tokens.scope, required) =>
        {
            Ok(tokens)
        }
        TokenReply::Failed { error } if error == "authorization_declined" => {
            Err(invalid("OneDrive sign-in was declined."))
        }
        TokenReply::Failed { error } if error == "expired_token" || error == "invalid_grant" => {
            Err(invalid(
                "Microsoft sign-in expired or was revoked. Disconnect any saved account and sign in again.",
            ))
        }
        _ => Err(invalid(
            "Microsoft did not grant the expected OneDrive access. Check the app registration.",
        )),
    }
}

fn microsoft_scope(observed: &str, required: &str) -> bool {
    let scopes = || {
        observed.split_whitespace().map(|scope| {
            scope
                .strip_prefix("https://graph.microsoft.com/")
                .unwrap_or(scope)
        })
    };
    scopes().any(|scope| scope == required)
        && scopes().all(|scope| {
            scope == required
                || matches!(scope, "openid" | "profile" | "email" | "offline_access")
                || (required == "Files.ReadWrite" && scope == "Files.Read")
        })
}

fn open_in_browser(url: &str) -> Result<()> {
    use std::{iter::once, ptr::null};
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let wide = url.encode_utf16().chain(once(0)).collect::<Vec<_>>();
    let opened = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            null(),
            wide.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    if opened as isize <= 32 {
        return Err(invalid("Windows could not open the browser."));
    }
    Ok(())
}
