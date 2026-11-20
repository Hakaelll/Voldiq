use super::{
    Action, Item, ItemKind, MAX_DEPTH, PAGE_SIZE, Page, Quota, Tokens, View, client, credential,
    decode, invalid, loopback, open_in_browser, send_form,
};
use crate::Result;
use reqwest::{StatusCode, blocking::Client};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

const GOOGLE_SCOPE: &str = "https://www.googleapis.com/auth/drive.metadata.readonly";
const DROPBOX_SCOPE: &str = "account_info.read files.metadata.read";
const DROPBOX_PORT: u16 = 27716;
const GOOGLE_FILES: &str = "https://www.googleapis.com/drive/v3/files";
const DROPBOX_API: &str = "https://api.dropboxapi.com/2/";

#[derive(Clone, Copy)]
pub enum BrowserProvider {
    GoogleDrive,
    Dropbox,
}

impl BrowserProvider {
    fn name(self) -> &'static str {
        match self {
            Self::GoogleDrive => "Google Drive",
            Self::Dropbox => "Dropbox",
        }
    }

    fn scope(self) -> &'static str {
        match self {
            Self::GoogleDrive => GOOGLE_SCOPE,
            Self::Dropbox => DROPBOX_SCOPE,
        }
    }

    fn token_url(self) -> &'static str {
        match self {
            Self::GoogleDrive => "https://oauth2.googleapis.com/token",
            Self::Dropbox => "https://api.dropboxapi.com/oauth2/token",
        }
    }
}

pub struct BrowserAccount {
    provider: BrowserProvider,
    pending: Option<Pending>,
    active: Option<Active>,
}

struct Pending {
    callback: loopback::Pending,
    registration: Registration,
}

#[derive(Deserialize, Serialize)]
struct Registration {
    client_id: String,
    client_secret: Option<String>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.client_secret.zeroize();
    }
}

#[derive(Deserialize, Serialize)]
struct Credential {
    registration: Registration,
    account_id: String,
    refresh_token: String,
}

impl Drop for Credential {
    fn drop(&mut self) {
        self.refresh_token.zeroize();
    }
}

struct Active {
    sealed: Vec<u8>,
    access: Zeroizing<String>,
    expires: Instant,
    quota: Quota,
    folders: Vec<(String, String)>,
    page: Page,
    next: Option<String>,
}

impl BrowserAccount {
    pub fn new(provider: BrowserProvider) -> Self {
        Self {
            provider,
            pending: None,
            active: None,
        }
    }

    pub fn run(&mut self, directory: &Path, action: Action) -> Result<View> {
        let provider = self.provider;
        let store = credential::Store::browser(directory, provider);
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
            Action::Begin {
                client_id,
                client_secret,
            } => {
                let registration = Registration {
                    client_id,
                    client_secret,
                };
                if saved.is_some() || self.pending.is_some() {
                    return Err(invalid("Finish the current connection first."));
                }
                self.pending = Some(begin(provider, registration)?);
            }
            Action::Cancel => self.pending = None,
            Action::Poll => {
                if saved.is_some() {
                    self.pending = None;
                } else if let Some(pending) = &self.pending {
                    match pending.callback.result() {
                        Ok(None) => {}
                        result => {
                            let pending = self.pending.take().expect("polled sign-in");
                            let code = result?.expect("callback code");
                            let active = activate(&store, provider, pending, &code)?;
                            saved = Some(active.sealed.clone());
                            self.active = Some(active);
                        }
                    }
                }
            }
            Action::Browse => {
                let sealed = saved
                    .as_deref()
                    .ok_or_else(|| invalid("Connect this provider first."))?;
                let active = resume(&store, provider, sealed)?;
                saved = Some(active.sealed.clone());
                self.active = Some(active);
            }
            Action::EnterFolder { .. } | Action::Parent | Action::FirstPage | Action::NextPage => {
                let active = self
                    .active
                    .as_mut()
                    .ok_or_else(|| invalid("Open this account first."))?;
                active.navigate(&store, provider, action)?;
                saved = Some(active.sealed.clone());
            }
            Action::Disconnect => {
                if let Some(sealed) = &saved {
                    store.forget_exact(sealed)?;
                }
                self.active = None;
                self.pending = None;
                saved = None;
            }
            Action::Revoke => {
                let sealed = saved
                    .as_deref()
                    .ok_or_else(|| invalid("Connect this provider first."))?;
                revoke(provider, sealed)?;
                store.forget_exact(sealed)?;
                self.active = None;
                saved = None;
            }
            Action::OpenPermissions { .. } => open_in_browser(match provider {
                BrowserProvider::GoogleDrive => "https://myaccount.google.com/connections",
                BrowserProvider::Dropbox => "https://www.dropbox.com/account/connected_apps",
            })?,
            Action::OpenSignIn => return Err(invalid("Start a new browser sign-in.")),
        }
        Ok(if let Some(active) = &self.active {
            View::Connected {
                work_account: false,
                page: active.page.clone(),
                quota: active.quota.clone(),
            }
        } else if self.pending.is_some() {
            View::BrowserWaiting
        } else if saved.is_some() {
            View::Saved
        } else {
            View::Disconnected
        })
    }
}

fn begin(provider: BrowserProvider, mut registration: Registration) -> Result<Pending> {
    registration.client_id = registration.client_id.trim().into();
    let id = &registration.client_id;
    let valid_id = (8..=256).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    let valid_secret = registration.client_secret.as_ref().is_none_or(|secret| {
        (1..=256).contains(&secret.len()) && secret.bytes().all(|byte| byte.is_ascii_graphic())
    });
    let (endpoint, extra, port, host) = match provider {
        BrowserProvider::GoogleDrive
            if valid_id && valid_secret && id.ends_with(".apps.googleusercontent.com") =>
        {
            (
                "https://accounts.google.com/o/oauth2/v2/auth",
                [("access_type", "offline"), ("prompt", "consent")].as_slice(),
                0,
                "127.0.0.1",
            )
        }
        BrowserProvider::Dropbox
            if valid_id && registration.client_secret.is_none() && !id.contains('.') =>
        {
            (
                "https://www.dropbox.com/oauth2/authorize",
                [("token_access_type", "offline")].as_slice(),
                DROPBOX_PORT,
                "localhost",
            )
        }
        _ => {
            return Err(invalid(
                "Enter the credentials from this provider's desktop app registration.",
            ));
        }
    };
    let mut url = Url::parse(endpoint).expect("provider authorization URL");
    url.query_pairs_mut()
        .extend_pairs([
            ("client_id", id.as_str()),
            ("response_type", "code"),
            ("scope", provider.scope()),
        ])
        .extend_pairs(extra.iter().copied());
    Ok(Pending {
        callback: loopback::Pending::open(url, port, host)?,
        registration,
    })
}

fn exchange<'a>(
    client: &Client,
    provider: BrowserProvider,
    registration: &'a Registration,
    mut fields: Vec<(&'a str, &'a str)>,
    renewal: bool,
) -> Result<Tokens> {
    fields.push(("client_id", &registration.client_id));
    if let Some(secret) = &registration.client_secret {
        fields.push(("client_secret", secret));
    }
    let response = send_form(client, provider.token_url(), &fields)?;
    if !response.status().is_success() {
        return Err(invalid(
            "The provider refused the sign-in or refresh. Check the registration and reconnect.",
        ));
    }
    let tokens: Tokens = decode(response)?;
    let required = provider.scope();
    if !tokens.token_type.eq_ignore_ascii_case("bearer")
        || !(1..=86_400).contains(&tokens.expires_in)
        || tokens.access_token.is_empty()
        || (!(renewal && tokens.scope.is_empty()) && !valid_scope(required, &tokens.scope))
    {
        return Err(invalid(
            "The provider did not grant the expected read-only access.",
        ));
    }
    Ok(tokens)
}

fn valid_scope(required: &str, observed: &str) -> bool {
    required
        .split_whitespace()
        .all(|scope| observed.split_whitespace().any(|present| present == scope))
        && observed
            .split_whitespace()
            .all(|scope| required.split_whitespace().any(|present| present == scope))
}

fn fresh_tokens(
    client: &Client,
    provider: BrowserProvider,
    credential: &Credential,
) -> Result<Tokens> {
    let mut fields = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", credential.refresh_token.as_str()),
    ];
    if matches!(provider, BrowserProvider::Dropbox) {
        fields.push(("scope", provider.scope()));
    }
    exchange(client, provider, &credential.registration, fields, true)
}

fn activate(
    store: &credential::Store,
    provider: BrowserProvider,
    pending: Pending,
    code: &str,
) -> Result<Active> {
    let client = client()?;
    let tokens = exchange(
        &client,
        provider,
        &pending.registration,
        vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", pending.callback.redirect_uri()),
            ("code_verifier", pending.callback.verifier()),
        ],
        false,
    )?;
    let refresh_token = tokens
        .refresh_token
        .as_ref()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| invalid("The provider did not grant offline access."))?
        .clone();
    let (account_id, quota) = account_metadata(&client, provider, &tokens.access_token)?;
    let credential = Credential {
        registration: pending.registration,
        account_id,
        refresh_token,
    };
    let mut active = Active::open(&client, provider, &tokens, quota)?;
    active.sealed = store.put(None, &credential)?;
    Ok(active)
}

fn resume(store: &credential::Store, provider: BrowserProvider, sealed: &[u8]) -> Result<Active> {
    let mut credential: Credential = credential::unseal(sealed)?;
    let client = client()?;
    let tokens = fresh_tokens(&client, provider, &credential)?;
    let (account_id, quota) = account_metadata(&client, provider, &tokens.access_token)?;
    if credential.account_id != account_id {
        return Err(invalid(
            "The provider returned a different account. Disconnect and sign in again.",
        ));
    }
    let mut active = Active::open(&client, provider, &tokens, quota)?;
    active.sealed = rotate(store, sealed, &mut credential, &tokens)?;
    Ok(active)
}

fn rotate(
    store: &credential::Store,
    sealed: &[u8],
    credential: &mut Credential,
    tokens: &Tokens,
) -> Result<Vec<u8>> {
    if let Some(refresh) = tokens
        .refresh_token
        .as_ref()
        .filter(|refresh| !refresh.is_empty())
    {
        credential.refresh_token.zeroize();
        credential.refresh_token.clone_from(refresh);
        return store.put(Some(sealed), credential);
    }
    if store.read()?.as_deref() != Some(sealed) {
        return Err(invalid("The cloud connection changed in another window."));
    }
    Ok(sealed.to_vec())
}

fn revoke(provider: BrowserProvider, sealed: &[u8]) -> Result<()> {
    let credential: Credential = credential::unseal(sealed)?;
    let client = client()?;
    let response = match provider {
        BrowserProvider::GoogleDrive => send_form(
            &client,
            "https://oauth2.googleapis.com/revoke",
            &[("token", &credential.refresh_token)],
        )?,
        BrowserProvider::Dropbox => {
            let tokens = fresh_tokens(&client, provider, &credential)?;
            client
                .post(format!("{DROPBOX_API}auth/token/revoke"))
                .bearer_auth(&tokens.access_token)
                .send()
                .map_err(|_| invalid("Dropbox revocation could not be reached."))?
        }
    };
    if !response.status().is_success() {
        return Err(invalid(
            "The provider did not confirm revocation. Manage access on the provider's website.",
        ));
    }
    Ok(())
}

impl Active {
    fn open(
        client: &Client,
        provider: BrowserProvider,
        tokens: &Tokens,
        quota: Quota,
    ) -> Result<Self> {
        let (page, next) = folder_page(client, provider, &tokens.access_token, &[], None, 1)?;
        Ok(Self {
            sealed: Vec::new(),
            access: Zeroizing::new(tokens.access_token.clone()),
            expires: Instant::now() + Duration::from_secs(tokens.expires_in),
            quota,
            folders: Vec::new(),
            page,
            next,
        })
    }

    fn navigate(
        &mut self,
        store: &credential::Store,
        provider: BrowserProvider,
        action: Action,
    ) -> Result<()> {
        let mut folders = self.folders.clone();
        let (cursor, number) = match action {
            Action::EnterFolder { id } if folders.len() < MAX_DEPTH => {
                let item = self
                    .page
                    .items
                    .iter()
                    .find(|item| item.id == id && matches!(item.kind, ItemKind::Folder))
                    .ok_or_else(|| invalid("That folder is outside the current page."))?;
                folders.push((item.id.clone(), item.name.clone()));
                (None, 1)
            }
            Action::Parent if folders.pop().is_some() => (None, 1),
            Action::FirstPage => (None, 1),
            Action::NextPage if self.next.is_some() => {
                (self.next.as_deref(), self.page.number.saturating_add(1))
            }
            _ => return Err(invalid("That page is unavailable.")),
        };
        let client = client()?;
        if self.expires <= Instant::now() + Duration::from_secs(15) {
            let mut credential: Credential = credential::unseal(&self.sealed)?;
            let tokens = fresh_tokens(&client, provider, &credential)?;
            let (account_id, _) = account_metadata(&client, provider, &tokens.access_token)?;
            if credential.account_id != account_id {
                return Err(invalid(
                    "The provider returned a different account. Disconnect and sign in again.",
                ));
            }
            self.sealed = rotate(store, &self.sealed, &mut credential, &tokens)?;
            self.access = Zeroizing::new(tokens.access_token.clone());
            self.expires = Instant::now() + Duration::from_secs(tokens.expires_in);
        }
        let (page, next) = folder_page(&client, provider, &self.access, &folders, cursor, number)?;
        if store.read()?.as_ref() != Some(&self.sealed) {
            return Err(invalid("The cloud connection changed in another window."));
        }
        self.folders = folders;
        self.page = page;
        self.next = next;
        Ok(())
    }
}

fn account_metadata(
    client: &Client,
    provider: BrowserProvider,
    access: &str,
) -> Result<(String, Quota)> {
    let (account_id, quota) = match provider {
        BrowserProvider::GoogleDrive => {
            let mut url =
                Url::parse("https://www.googleapis.com/drive/v3/about").expect("Drive about URL");
            url.query_pairs_mut()
                .append_pair("fields", "user(permissionId),storageQuota(limit,usage)");
            let response = client.get(url).bearer_auth(access).send();
            let about: GoogleAbout = metadata(response)?;
            let total = about.storage_quota.limit.as_deref().and_then(parse_bytes);
            let used = about.storage_quota.usage.as_deref().and_then(parse_bytes);
            (about.user.permission_id, quota(total, used))
        }
        BrowserProvider::Dropbox => {
            let account: DropboxAccount =
                dropbox(client, access, "users/get_current_account", None)?;
            let space: DropboxSpace = dropbox(client, access, "users/get_space_usage", None)?;
            let total = match space.allocation {
                DropboxAllocation::Individual { allocated } => Some(allocated),
                DropboxAllocation::Team | DropboxAllocation::Other => None,
            };
            (account.account_id, quota(total, space.used))
        }
    };
    if !(1..=256).contains(&account_id.len()) || account_id.chars().any(char::is_control) {
        return Err(invalid(
            "The provider did not return a valid account identity.",
        ));
    }
    Ok((account_id, quota))
}

fn quota(total: Option<u64>, used: Option<u64>) -> Quota {
    Quota {
        total: total.map(|value| value.to_string()),
        used: used.map(|value| value.to_string()),
        remaining: total
            .zip(used)
            .and_then(|(total, used)| total.checked_sub(used))
            .map(|remaining| remaining.to_string()),
    }
}

fn parse_bytes(value: &str) -> Option<u64> {
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().ok())
        .flatten()
}

fn folder_page(
    client: &Client,
    provider: BrowserProvider,
    access: &str,
    folders: &[(String, String)],
    cursor: Option<&str>,
    number: u32,
) -> Result<(Page, Option<String>)> {
    let folder = folders.last();
    let (mut items, next) = match provider {
        BrowserProvider::GoogleDrive => {
            let id = folder.map_or("root", |(id, _)| id.as_str());
            google_page(client, access, id, cursor)?
        }
        BrowserProvider::Dropbox => {
            let id = folder.map_or("", |(id, _)| id.as_str());
            dropbox_page(client, access, id, cursor)?
        }
    };
    // Dropbox treats its requested page limit as approximate.
    if items.len() > 400
        || next
            .as_ref()
            .is_some_and(|cursor| !(1..=16384).contains(&cursor.len()))
    {
        return Err(invalid("The provider returned an invalid metadata page."));
    }
    if folders.len() >= MAX_DEPTH {
        for item in &mut items {
            if matches!(item.kind, ItemKind::Folder) {
                item.kind = ItemKind::Reference;
            }
        }
    }
    Ok((
        Page {
            folder: folder.map_or_else(|| provider.name().into(), |(_, name)| name.clone()),
            has_parent: folder.is_some(),
            number,
            has_next: next.is_some(),
            items,
        },
        next,
    ))
}

fn google_page(
    client: &Client,
    access: &str,
    id: &str,
    cursor: Option<&str>,
) -> Result<(Vec<Item>, Option<String>)> {
    let query = format!("'{id}' in parents and trashed = false");
    let mut url = Url::parse(GOOGLE_FILES).expect("Drive files URL");
    url.query_pairs_mut().extend_pairs([
        ("q", query.as_str()),
        ("spaces", "drive"),
        ("corpora", "user"),
        ("pageSize", PAGE_SIZE.to_string().as_str()),
        (
            "fields",
            "nextPageToken,incompleteSearch,files(id,name,mimeType,size)",
        ),
    ]);
    if let Some(cursor) = cursor {
        url.query_pairs_mut().append_pair("pageToken", cursor);
    }
    let page: GooglePage = metadata(client.get(url).bearer_auth(access).send())?;
    if page.incomplete_search {
        return Err(invalid(
            "Google returned incomplete folder metadata. Reload to try again.",
        ));
    }
    let items = page
        .files
        .into_iter()
        .map(|item| {
            if !valid_item(&item.id, &item.name)
                || !item
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(invalid("Google returned invalid file metadata."));
            }
            let kind = match item.mime_type.as_str() {
                "application/vnd.google-apps.folder" => ItemKind::Folder,
                "application/vnd.google-apps.shortcut" => ItemKind::Reference,
                _ => ItemKind::File,
            };
            let bytes = item
                .size
                .as_deref()
                .and_then(parse_bytes)
                .map(|size| size.to_string());
            Ok(Item {
                id: item.id,
                name: item.name,
                kind,
                bytes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((items, page.next_page_token))
}

fn dropbox_page(
    client: &Client,
    access: &str,
    id: &str,
    cursor: Option<&str>,
) -> Result<(Vec<Item>, Option<String>)> {
    let (route, payload) = if let Some(cursor) = cursor {
        ("files/list_folder/continue", json!({ "cursor": cursor }))
    } else {
        (
            "files/list_folder",
            json!({ "path": id, "limit": PAGE_SIZE, "recursive": false, "include_deleted": false }),
        )
    };
    let page: DropboxPage = dropbox(client, access, route, Some(payload))?;
    let items = page
        .entries
        .into_iter()
        .map(|item| {
            if !valid_item(&item.id, &item.name) || !item.id.starts_with("id:") {
                return Err(invalid("Dropbox returned invalid file metadata."));
            }
            let kind = match item.tag.as_str() {
                "file" => ItemKind::File,
                "folder" => ItemKind::Folder,
                _ => ItemKind::Reference,
            };
            Ok(Item {
                id: item.id,
                name: item.name,
                kind,
                bytes: item.size.map(|size| size.to_string()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((items, page.has_more.then_some(page.cursor)))
}

fn valid_item(id: &str, name: &str) -> bool {
    (1..=1024).contains(&id.len())
        && !id.chars().any(char::is_control)
        && (1..=2048).contains(&name.len())
}

fn dropbox<T: DeserializeOwned>(
    client: &Client,
    access: &str,
    route: &str,
    body: Option<Value>,
) -> Result<T> {
    let mut request = client
        .post(format!("{DROPBOX_API}{route}"))
        .bearer_auth(access);
    if let Some(body) = body {
        request = request
            .header("content-type", "application/json")
            .body(body.to_string());
    }
    metadata(request.send())
}

fn metadata<T: DeserializeOwned>(
    response: reqwest::Result<reqwest::blocking::Response>,
) -> Result<T> {
    let response = response.map_err(|_| invalid("The provider could not be reached."))?;
    match response.status() {
        status if status.is_success() => decode(response),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(invalid(
            "The provider refused access. Check permissions or reconnect.",
        )),
        StatusCode::TOO_MANY_REQUESTS => Err(invalid(
            "The provider limited requests. Wait before trying again.",
        )),
        StatusCode::NOT_FOUND | StatusCode::CONFLICT => Err(invalid(
            "This folder or page is no longer available. Reload the account.",
        )),
        _ => Err(invalid(
            "The provider could not fetch this metadata. Try again.",
        )),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleAbout {
    user: GoogleUser,
    #[serde(default)]
    storage_quota: GoogleQuota,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleUser {
    permission_id: String,
}

#[derive(Default, Deserialize)]
struct GoogleQuota {
    limit: Option<String>,
    usage: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GooglePage {
    #[serde(default)]
    files: Vec<GoogleItem>,
    #[serde(default)]
    incomplete_search: bool,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleItem {
    id: String,
    name: String,
    mime_type: String,
    size: Option<String>,
}

#[derive(Deserialize)]
struct DropboxAccount {
    account_id: String,
}

#[derive(Deserialize)]
struct DropboxSpace {
    used: Option<u64>,
    allocation: DropboxAllocation,
}

#[derive(Deserialize)]
#[serde(tag = ".tag", rename_all = "snake_case")]
enum DropboxAllocation {
    Individual {
        allocated: u64,
    },
    Team,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct DropboxPage {
    entries: Vec<DropboxItem>,
    cursor: String,
    has_more: bool,
}

#[derive(Deserialize)]
struct DropboxItem {
    #[serde(rename = ".tag")]
    tag: String,
    id: String,
    name: String,
    size: Option<u64>,
}
