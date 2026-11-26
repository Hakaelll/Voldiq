//! Provider contracts for metadata-only review and conditional recycling.
use super::{Provider, invalid};
use crate::Result;
use crate::cloud_accounts::{GRAPH, browser, decode, graph, valid_id};
use reqwest::{
    StatusCode,
    blocking::{Client, RequestBuilder, Response},
    header::IF_MATCH,
};
use serde::Deserialize;
use serde_json::json;
use url::Url;

const DROPBOX: &str = "https://api.dropboxapi.com/2/files/";
const GOOGLE: &str = "https://www.googleapis.com/drive/v2/files/";
const GOOGLE_FIELDS: &str =
    "id,title,fileSize,etag,mimeType,ownedByMe,labels(trashed),parents(id),capabilities(canTrash)";

#[derive(PartialEq, Eq)]
pub(super) struct FileEvidence {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) parent: String,
    pub(super) bytes: Option<u64>,
    revision: String,
}

pub(super) fn evidence(
    client: &Client,
    provider: Provider,
    access: &str,
    owner: &str,
    id: &str,
) -> Result<FileEvidence> {
    match provider {
        Provider::OneDrive => one_drive_evidence(client, access, owner, id),
        Provider::Dropbox => dropbox_evidence(client, access, id),
        Provider::GoogleDrive => google_evidence(client, access, id),
    }
}

pub(super) fn folder_identity(
    client: &Client,
    provider: Provider,
    access: &str,
    owner: &str,
    folder: Option<&str>,
) -> Result<String> {
    #[derive(Deserialize)]
    struct ItemId {
        id: String,
    }
    match (provider, folder) {
        (Provider::Dropbox, Some(id)) => {
            let item: DropboxFile = browser::dropbox(
                client,
                access,
                "files/get_metadata",
                Some(json!({"path": id, "include_deleted": false})),
            )?;
            item.path_lower
                .filter(|path| item.tag == "folder" && item.id == id && valid_dropbox_path(path))
                .ok_or_else(|| invalid("Dropbox did not confirm this folder's location."))
        }
        (Provider::Dropbox, None) => Ok(String::new()),
        (_, Some(id)) => Ok(id.to_owned()),
        (Provider::OneDrive, None) => {
            let mut url = Url::parse(GRAPH).expect("Graph origin");
            url.path_segments_mut()
                .expect("Graph path")
                .pop_if_empty()
                .extend(["drives", owner, "root"]);
            url.query_pairs_mut().append_pair("$select", "id");
            let root: ItemId = graph(client, access, url)?;
            Ok(root.id)
        }
        (Provider::GoogleDrive, None) => {
            let root: ItemId = google_get(client, access, "root", "id")?;
            Ok(root.id)
        }
    }
}

pub(super) fn recycle(
    client: &Client,
    provider: Provider,
    access: &str,
    owner: &str,
    file: &FileEvidence,
) -> Result<()> {
    let response = recycle_request(client, provider, owner, file)?.bearer_auth(access).send()
        .map_err(|_| invalid("The provider did not confirm the outcome. Check its recycle bin before reviewing again."))?;
    match provider {
        Provider::OneDrive => confirm_onedrive(response),
        Provider::Dropbox => confirm_dropbox(response, file),
        Provider::GoogleDrive => confirm_google(response, file),
    }
}

fn recycle_request(
    client: &Client,
    provider: Provider,
    owner: &str,
    file: &FileEvidence,
) -> Result<RequestBuilder> {
    Ok(match provider {
        Provider::OneDrive => client
            .delete(graph_item_url(owner, &file.id)?)
            .header(IF_MATCH, &file.revision),
        Provider::Dropbox => client
            .post(format!("{DROPBOX}delete_v2"))
            .header("content-type", "application/json")
            .body(json!({"path": file.id, "parent_rev": file.revision}).to_string()),
        Provider::GoogleDrive => client
            .patch(google_file_url(&file.id, "id,labels(trashed)")?)
            .header(IF_MATCH, &file.revision)
            .header("content-type", "application/json")
            .body(r#"{"labels":{"trashed":true}}"#),
    })
}

pub(super) fn displayable(name: &str) -> bool {
    (1..=2048).contains(&name.len())
        && !name.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphFile {
    id: String,
    name: String,
    size: Option<u64>,
    #[serde(rename = "eTag")]
    etag: Option<String>,
    file: Option<serde::de::IgnoredAny>,
    remote_item: Option<serde::de::IgnoredAny>,
    parent_reference: GraphParent,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphParent {
    id: String,
    drive_id: String,
}

fn graph_item_url(drive: &str, item: &str) -> Result<Url> {
    if !valid_id(drive) || !valid_id(item) {
        return Err(invalid("Invalid OneDrive item identity."));
    }
    let mut url = Url::parse(GRAPH).expect("Graph origin");
    url.path_segments_mut()
        .expect("Graph path")
        .pop_if_empty()
        .extend(["drives", drive, "items", item]);
    Ok(url)
}

fn is_strong_etag(value: &str) -> bool {
    value.len() <= 1024
        && value
            .strip_prefix('"')
            .and_then(|tag| tag.strip_suffix('"'))
            .is_some_and(|tag| {
                !tag.is_empty()
                    && tag
                        .bytes()
                        .all(|byte| byte.is_ascii_graphic() && byte != b'"')
            })
}

fn one_drive_evidence(
    client: &Client,
    access: &str,
    drive: &str,
    id: &str,
) -> Result<FileEvidence> {
    let mut url = graph_item_url(drive, id)?;
    url.query_pairs_mut().append_pair(
        "$select",
        "id,name,size,eTag,file,remoteItem,parentReference",
    );
    let file: GraphFile = graph(client, access, url)?;
    if file.id != id
        || file.parent_reference.drive_id != drive
        || !valid_id(&file.parent_reference.id)
        || file.file.is_none()
        || file.remote_item.is_some()
        || !displayable(&file.name)
    {
        return Err(invalid(
            "OneDrive did not return an eligible file in this drive.",
        ));
    }
    Ok(FileEvidence {
        id: file.id,
        name: file.name,
        parent: file.parent_reference.id,
        bytes: file.size,
        revision: file
            .etag
            .filter(|tag| is_strong_etag(tag))
            .ok_or_else(|| invalid("OneDrive did not return a usable file revision."))?,
    })
}

fn confirm_onedrive(response: Response) -> Result<()> {
    match response.status() {
        StatusCode::NO_CONTENT => Ok(()),
        StatusCode::PRECONDITION_FAILED => Err(invalid("The reviewed OneDrive file changed.")),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            Err(invalid("OneDrive refused the recycle request."))
        }
        _ => Err(invalid(
            "OneDrive did not confirm recycling. Check the provider before making a new plan.",
        )),
    }
}

#[derive(Deserialize)]
struct DropboxFile {
    #[serde(rename = ".tag")]
    tag: String,
    id: String,
    name: String,
    path_lower: Option<String>,
    size: Option<u64>,
    rev: Option<String>,
}

fn valid_dropbox_path(path: &str) -> bool {
    path.starts_with('/') && !path.ends_with('/') && displayable(path)
}

fn dropbox_file_evidence(file: DropboxFile, id: &str) -> Result<FileEvidence> {
    if file.tag != "file" || file.id != id || !displayable(&file.name) || !id.starts_with("id:") {
        return Err(invalid("Dropbox did not return the selected file."));
    }
    Ok(FileEvidence {
        id: file.id,
        name: file.name,
        parent: file
            .path_lower
            .as_deref()
            .filter(|path| valid_dropbox_path(path))
            .and_then(|path| path.rsplit_once('/'))
            .map(|(parent, _)| parent.to_owned())
            .ok_or_else(|| invalid("Dropbox did not report the file location."))?,
        bytes: file.size,
        revision: file
            .rev
            .filter(|rev| {
                (9..=128).contains(&rev.len()) && rev.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .ok_or_else(|| invalid("Dropbox did not return a usable file revision."))?,
    })
}

fn dropbox_evidence(client: &Client, access: &str, id: &str) -> Result<FileEvidence> {
    dropbox_file_evidence(
        browser::dropbox(
            client,
            access,
            "files/get_metadata",
            Some(json!({
                "path": id, "include_deleted": false
            })),
        )?,
        id,
    )
}

fn confirm_dropbox(response: Response, expected: &FileEvidence) -> Result<()> {
    if !response.status().is_success() {
        return Err(invalid(
            "Dropbox did not confirm recycling. Check the provider before making a new plan.",
        ));
    }
    #[derive(Deserialize)]
    struct ResultItem {
        metadata: DropboxFile,
    }
    let result: ResultItem = decode(response)?;
    let confirmed = dropbox_file_evidence(result.metadata, &expected.id)?;
    if confirmed != *expected {
        return Err(invalid(
            "Dropbox returned different metadata for the completed request. Check the provider.",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleFile {
    id: String,
    title: String,
    file_size: Option<String>,
    etag: Option<String>,
    mime_type: String,
    owned_by_me: bool,
    labels: GoogleLabels,
    parents: Vec<GoogleParent>,
    capabilities: GoogleCapabilities,
}

#[derive(Deserialize)]
struct GoogleParent {
    id: String,
}

#[derive(Deserialize)]
struct GoogleLabels {
    trashed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleCapabilities {
    can_trash: bool,
}

fn google_file_url(id: &str, fields: &str) -> Result<Url> {
    if !(1..=256).contains(&id.len())
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid("Invalid Google Drive item identity."));
    }
    let mut url = Url::parse(GOOGLE).expect("Google Drive files URL");
    url.path_segments_mut()
        .expect("Google Drive path")
        .pop_if_empty()
        .push(id);
    url.query_pairs_mut().append_pair("fields", fields);
    Ok(url)
}

fn google_file_evidence(file: GoogleFile, id: &str) -> Result<FileEvidence> {
    let [parent] = file.parents.as_slice() else {
        return Err(invalid(
            "Google Drive must report one parent for the selected file.",
        ));
    };
    if file.id != id
        || !displayable(&file.title)
        || file.labels.trashed
        || !file.owned_by_me
        || !file.capabilities.can_trash
        || parent.id.is_empty()
        || matches!(
            file.mime_type.as_str(),
            "application/vnd.google-apps.folder" | "application/vnd.google-apps.shortcut"
        )
    {
        return Err(invalid(
            "Google Drive can only recycle owned files that it reports as eligible.",
        ));
    }
    Ok(FileEvidence {
        id: file.id,
        name: file.title,
        parent: parent.id.clone(),
        bytes: file.file_size.and_then(|size| size.parse().ok()),
        revision: file
            .etag
            .filter(|tag| is_strong_etag(tag))
            .ok_or_else(|| invalid("Google Drive did not return a usable file revision."))?,
    })
}

fn google_evidence(client: &Client, access: &str, id: &str) -> Result<FileEvidence> {
    google_file_evidence(google_get(client, access, id, GOOGLE_FIELDS)?, id)
}

fn google_get<T: serde::de::DeserializeOwned>(
    client: &Client,
    access: &str,
    id: &str,
    fields: &str,
) -> Result<T> {
    let response = client
        .get(google_file_url(id, fields)?)
        .bearer_auth(access)
        .send()
        .map_err(|_| invalid("Google Drive file metadata could not be reached."))?;
    if !response.status().is_success() {
        return Err(invalid(
            "Google Drive did not confirm this file's metadata.",
        ));
    }
    decode(response)
}

fn confirm_google(response: Response, expected: &FileEvidence) -> Result<()> {
    match response.status() {
        StatusCode::PRECONDITION_FAILED => {
            return Err(invalid("The reviewed Google Drive file changed."));
        }
        status if !status.is_success() => {
            return Err(invalid(
                "Google Drive did not confirm recycling. Check Drive before making a new plan.",
            ));
        }
        _ => {}
    }
    #[derive(Deserialize)]
    struct ResultItem {
        id: String,
        labels: GoogleLabels,
    }
    let result: ResultItem = decode(response)?;
    if result.id != expected.id || !result.labels.trashed {
        return Err(invalid(
            "Google Drive did not confirm that the exact file is in the trash.",
        ));
    }
    Ok(())
}
