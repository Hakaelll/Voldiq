//! The desktop owns one exact, short-lived review and keeps write access in memory.
mod provider;

use super::{
    ItemKind, Page, Pending, RECYCLE_SCOPE, browser, client, credential, current_drive, invalid,
};
use crate::Result;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

const SIGN_IN_WINDOW: Duration = Duration::from_secs(300);
const APPROVAL_WINDOW: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    OneDrive,
    GoogleDrive,
    Dropbox,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::OneDrive => "OneDrive",
            Self::GoogleDrive => "Google Drive",
            Self::Dropbox => "Dropbox",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::OneDrive => "one_drive",
            Self::GoogleDrive => "google_drive",
            Self::Dropbox => "dropbox",
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Review { id: String },
    Authorize { review: String },
    Poll { review: String },
    OpenSignIn { review: String },
    Status,
    Cancel { review: Option<String> },
    Confirm { review: String },
}

#[derive(Serialize)]
pub struct View {
    pub review: Option<ReviewView>,
    pub status: Status,
}

#[derive(Serialize)]
pub struct ReviewView {
    id: String,
    name: String,
    folder: String,
    bytes: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Status {
    Idle,
    Review,
    Device { code: String, retry_seconds: u64 },
    Browser,
    Ready,
    Recycled,
    Declined,
}

pub(super) enum Login {
    Microsoft(String),
    Browser(browser::BrowserProvider, browser::Registration),
}

pub(super) struct Selection {
    id: String,
    name: String,
    folder: String,
    folder_id: Option<String>,
    bytes: Option<u64>,
}

impl Selection {
    pub(super) fn capture(
        page: &Page,
        folder_id: Option<&str>,
        id: &str,
        expires: Instant,
        store: &credential::Store,
        sealed: &[u8],
    ) -> Result<Self> {
        if expires <= Instant::now() + Duration::from_secs(15) {
            return Err(invalid("Reload the account before reviewing a file."));
        }
        require_current(store, sealed)?;
        let item = page
            .items
            .iter()
            .find(|item| item.id == id && matches!(item.kind, ItemKind::File))
            .ok_or_else(|| invalid("Select a file from the current provider page."))?;
        if !provider::displayable(&page.folder) || !provider::displayable(&item.name) {
            return Err(invalid("This name cannot be shown safely in the review."));
        }
        Ok(Self {
            id: item.id.clone(),
            name: item.name.clone(),
            folder: page.folder.clone(),
            folder_id: folder_id.map(str::to_owned),
            bytes: item.bytes.as_ref().and_then(|bytes| bytes.parse().ok()),
        })
    }
}

pub struct Source {
    provider: Provider,
    store: credential::Store,
    sealed: Vec<u8>,
    owner: String,
    login: Login,
    file: provider::FileEvidence,
    folder: String,
}

impl Source {
    pub(super) fn new(
        provider: Provider,
        store: credential::Store,
        sealed: Vec<u8>,
        owner: String,
        login: Login,
        selection: Selection,
        access: &str,
    ) -> Result<Self> {
        let client = client()?;
        let file = provider::evidence(&client, provider, access, &owner, &selection.id)?;
        let parent = provider::folder_identity(
            &client,
            provider,
            access,
            &owner,
            selection.folder_id.as_deref(),
        )?;
        if selection.name != file.name
            || file.parent != parent
            || selection
                .bytes
                .is_some_and(|bytes| file.bytes != Some(bytes))
        {
            return Err(invalid(
                "The file changed. Reload the folder before reviewing it.",
            ));
        }
        require_current(&store, &sealed)?;
        Ok(Self {
            provider,
            store,
            sealed,
            owner,
            login,
            file,
            folder: selection.folder,
        })
    }

    fn ensure_current(&self) -> Result<()> {
        require_current(&self.store, &self.sealed)
    }
}

fn require_current(store: &credential::Store, sealed: &[u8]) -> Result<()> {
    if store.read()?.as_deref() != Some(sealed) {
        return Err(invalid(
            "The cloud connection changed. Review the file again.",
        ));
    }
    Ok(())
}

#[derive(Default)]
pub struct Session(Option<Review>);

struct Review {
    id: String,
    source: Source,
    deadline: Instant,
    phase: Phase,
}

enum Phase {
    Review,
    Device(Pending),
    Browser(browser::Pending, browser::BrowserProvider),
    Ready(Zeroizing<String>),
}

impl Session {
    pub fn prepare(&mut self, directory: &Path, source: Source) -> Result<View> {
        self.expire(directory)?;
        if self.0.is_some() {
            return Err(invalid("Finish or cancel the current cloud review."));
        }
        let mut id = [0; 24];
        getrandom::fill(&mut id)
            .map_err(|_| invalid("Windows could not create a review identifier."))?;
        let review = Review {
            id: URL_SAFE_NO_PAD.encode(id),
            source,
            deadline: Instant::now() + SIGN_IN_WINDOW,
            phase: Phase::Review,
        };
        record(directory, &review, "reviewed")?;
        let view = review.view();
        self.0 = Some(review);
        Ok(view)
    }

    pub fn run(&mut self, directory: &Path, provider: Provider, action: Action) -> Result<View> {
        self.expire(directory)?;
        let active = self
            .0
            .as_mut()
            .filter(|review| review.source.provider == provider);
        if matches!(action, Action::Status) {
            return Ok(active.map_or_else(|| empty(Status::Idle), |review| review.view()));
        }
        if let Action::Cancel { review } = action {
            if let Some(active) = active.filter(|active| review.is_none_or(|id| id == active.id)) {
                record(directory, active, "cancelled")?;
                self.0 = None;
            }
            return Ok(empty(Status::Idle));
        }
        let review =
            active.ok_or_else(|| invalid("Start a review from this provider's file list."))?;
        review.source.ensure_current()?;
        match action {
            Action::Authorize { review: id }
                if id == review.id && matches!(review.phase, Phase::Review) =>
            {
                record(directory, review, "write_requested")?;
                review.phase = match &review.source.login {
                    Login::Microsoft(id) => {
                        Phase::Device(Pending::request(id.clone(), RECYCLE_SCOPE)?)
                    }
                    Login::Browser(provider, registration) => Phase::Browser(
                        browser::begin(
                            *provider,
                            registration.clone(),
                            browser::Permission::Recycle,
                        )?,
                        *provider,
                    ),
                };
            }
            Action::OpenSignIn { review: id } if id == review.id => match &review.phase {
                Phase::Device(pending) => pending.open()?,
                _ => return Err(invalid("There is no device sign-in to open.")),
            },
            Action::Poll { review: id }
                if id == review.id
                    && matches!(review.phase, Phase::Device(_) | Phase::Browser(..)) =>
            {
                if let Err(error) = review.poll(directory) {
                    review.phase = Phase::Review;
                    record(directory, review, "authorization_failed")?;
                    return Err(error);
                }
            }
            _ => {
                return Err(invalid(
                    "That recycle action is unavailable in the current review.",
                ));
            }
        }
        Ok(review.view())
    }

    pub fn confirm(
        &mut self,
        directory: &Path,
        provider: Provider,
        id: &str,
        human_confirm: impl FnOnce(&str) -> bool,
    ) -> Result<View> {
        self.expire(directory)?;
        let review = self
            .0
            .as_ref()
            .filter(|review| {
                review.id == id
                    && review.source.provider == provider
                    && matches!(review.phase, Phase::Ready(_))
            })
            .ok_or_else(|| invalid("This exact cloud review is no longer active."))?;
        review.source.ensure_current()?;
        let review = self.0.take().expect("validated review");
        let file = &review.source.file;
        let bytes = file
            .bytes
            .map_or_else(|| "Unknown size".into(), |bytes| format!("{bytes} bytes"));
        let target = if provider == Provider::Dropbox {
            "Dropbox Deleted files"
        } else {
            "the provider's recycle bin"
        };
        let description = format!(
            "Send this file to {target}?\n\nProvider: {}\nFolder: {}\nFile: {}\nSize: {bytes}\n\nThe provider will move the cloud file, which can affect synced copies. Recovery follows provider policy.",
            provider.name(),
            review.source.folder,
            file.name,
        );
        if !human_confirm(&description) {
            record(directory, &review, "declined")?;
            return Ok(empty(Status::Declined));
        }
        if Instant::now() >= review.deadline {
            record(directory, &review, "expired")?;
            return Err(invalid("The file review expired. Review the file again."));
        }
        record(directory, &review, "approved")?;
        let Phase::Ready(access) = &review.phase else {
            unreachable!()
        };
        let client = client().and_then(|client| {
            review.source.ensure_current()?;
            let observed =
                provider::evidence(&client, provider, access, &review.source.owner, &file.id)?;
            if observed != *file {
                return Err(invalid(
                    "The cloud file changed after review. Review it again.",
                ));
            }
            Ok(client)
        });
        let client = match client {
            Ok(client) => client,
            Err(error) => {
                record(directory, &review, "revalidation_failed")?;
                return Err(error);
            }
        };
        record(directory, &review, "submission_intent")?;
        check_before_send(directory, &review)?;
        match provider::recycle(&client, provider, access, &review.source.owner, file) {
            Ok(()) => {
                record(directory, &review, "recycled").map_err(|_| invalid(
                    "The provider confirmed recycling but Voldiq could not record it. Check the provider before reviewing again."
                ))?;
                Ok(empty(Status::Recycled))
            }
            Err(error) => {
                record(directory, &review, "unconfirmed")?;
                Err(error)
            }
        }
    }

    fn expire(&mut self, directory: &Path) -> Result<()> {
        if self
            .0
            .as_ref()
            .is_some_and(|review| Instant::now() >= review.deadline)
        {
            let review = self.0.take().expect("expired review");
            record(directory, &review, "expired")?;
        }
        Ok(())
    }
}

fn check_before_send(directory: &Path, review: &Review) -> Result<()> {
    if let Err(error) = review.source.ensure_current() {
        record(directory, review, "not_sent")?;
        return Err(error);
    }
    if Instant::now() >= review.deadline {
        record(directory, review, "expired")?;
        return Err(invalid("The file review expired. Review the file again."));
    }
    Ok(())
}

impl Review {
    fn poll(&mut self, directory: &Path) -> Result<()> {
        let client = client()?;
        let tokens = match &mut self.phase {
            Phase::Device(pending) => pending.poll(&client, "Files.ReadWrite")?,
            Phase::Browser(pending, provider) => pending.poll_recycle(&client, *provider)?,
            _ => return Err(invalid("Start provider sign-in first.")),
        };
        let Some(tokens) = tokens else { return Ok(()) };
        let now = Instant::now();
        let access = Zeroizing::new(tokens.access_token.clone());
        let owner = match &self.source.login {
            Login::Microsoft(_) => current_drive(&client, &access, Some(&self.source.owner))?.id,
            Login::Browser(provider, _) => {
                browser::account_metadata(&client, *provider, &access)?.0
            }
        };
        if owner != self.source.owner {
            return Err(invalid(
                "Sign in to the same cloud account you used to review the file.",
            ));
        }
        self.source.ensure_current()?;
        if Instant::now() >= self.deadline {
            return Err(invalid("The file review expired. Review the file again."));
        }
        record(directory, self, "write_granted")?;
        self.phase = Phase::Ready(access);
        self.deadline = now + APPROVAL_WINDOW.min(Duration::from_secs(tokens.expires_in));
        Ok(())
    }

    fn view(&self) -> View {
        let status = match &self.phase {
            Phase::Review => Status::Review,
            Phase::Device(pending) => Status::Device {
                code: pending.code().into(),
                retry_seconds: pending.retry_seconds(),
            },
            Phase::Browser(..) => Status::Browser,
            Phase::Ready(_) => Status::Ready,
        };
        View {
            status,
            review: Some(ReviewView {
                id: self.id.clone(),
                name: self.source.file.name.clone(),
                folder: self.source.folder.clone(),
                bytes: self.source.file.bytes.map(|bytes| bytes.to_string()),
            }),
        }
    }
}

fn empty(status: Status) -> View {
    View {
        review: None,
        status,
    }
}

fn record(directory: &Path, review: &Review, event: &str) -> Result<()> {
    let database = rusqlite::Connection::open(directory.join("cloud-recycle.db"))?;
    crate::database::configure(&database)?;
    database.execute_batch(
        "CREATE TABLE IF NOT EXISTS cloud_recycle_events (
            sequence INTEGER PRIMARY KEY,
            review_id TEXT NOT NULL,
            provider TEXT NOT NULL,
            event TEXT NOT NULL,
            observed_at INTEGER NOT NULL
        );",
    )?;
    let observed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let observed_at = i64::try_from(observed_at)
        .map_err(|_| invalid("Windows reported an invalid clock value."))?;
    database.execute(
        "INSERT INTO cloud_recycle_events (review_id, provider, event, observed_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![review.id, review.source.provider.key(), event, observed_at],
    )?;
    Ok(())
}
