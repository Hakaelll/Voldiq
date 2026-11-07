use super::{Inspection, Request};
use crate::{Entry, Error, Kind, NodeId, Result, index::Index, windows};
use std::{
    fs::File,
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
};
use windows::cleanup_io::{LeafAccess, open_download_stream, open_leaf};

mod preview;

const ZONE_LIMIT: usize = 16 * 1024;

fn changed() -> Error {
    Error::Invalid("File or folder changed since analysis. Rescan and try again.".into())
}

pub(super) fn inspect(
    index: &Index,
    id: NodeId,
    request: Request,
    cancel: &AtomicBool,
) -> Result<Option<Inspection>> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let entry = index.entry(id)?;
    let path = index.path(id)?;
    if index.snapshot_id.is_some()
        || entry.kind != Kind::File
        || entry.incomplete
        || entry.file_state.is_none()
        || entry.cloud.is_placeholder()
        || crate::security::warning(&path).is_some()
    {
        return Err(Error::Invalid(
            "Inspection requires a live, complete local file outside sensitive locations.".into(),
        ));
    }
    let parent = index.pin_folder(entry.parent.ok_or_else(changed)?)?;
    let root = index.root_identity.ok_or_else(changed)?;
    if !parent.ancestor_identities()?.contains(&root) {
        return Err(changed());
    }
    let directory = parent.directory_handle()?;
    let name = path.file_name().ok_or_else(changed)?;
    let file = open_leaf(directory, name, LeafAccess::Retain)?;
    verify(&file, entry, true)?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let result = match request {
        Request::Preview => match super::archive::Format::of(&path) {
            Some(format) => {
                Inspection::Archive(super::archive::read(&file, entry.logical, format, cancel)?)
            }
            None => preview::read(&file, &path, entry.logical)?,
        },
        Request::DownloadSource | Request::DownloadUrl => {
            let stream = match open_download_stream(directory, name) {
                Ok(stream) => Some(stream),
                Err(Error::Io(error))
                    if error.raw_os_error()
                        == Some(windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND as i32) =>
                {
                    None
                }
                Err(error) => return Err(error),
            };
            if let Some(stream) = stream {
                verify(&stream, entry, false)?;
                let data = read_prefix(&stream, ZONE_LIMIT)?;
                verify(&stream, entry, false)?;
                let record = if data.len() <= ZONE_LIMIT {
                    decode_text(&data, false).and_then(|text| recorded_url(&text))
                } else {
                    None
                };
                match record {
                    Some((domain, _)) if matches!(request, Request::DownloadSource) => {
                        Inspection::DownloadSource { domain }
                    }
                    Some((_, url)) => Inspection::DownloadUrl { url },
                    None => Inspection::DownloadUnavailable {
                        reason: "No supported web download source is recorded in this stream.",
                    },
                }
            } else {
                Inspection::DownloadUnavailable {
                    reason: "No download source stream is recorded for this file.",
                }
            }
        }
    };
    verify(&file, entry, true)?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some(result))
}

fn verify(file: &File, entry: &Entry, default_stream: bool) -> Result<()> {
    let expected = entry.file_state.ok_or_else(changed)?;
    let metadata = file.metadata()?;
    let identity = windows::file_identity(file)?;
    if !metadata.is_file()
        || windows::is_reparse(&metadata)
        || windows::requires_recall(&metadata)
        || identity.cloud != crate::cloud::CloudState::NotPlaceholder
        || identity.key != expected.identity
        || identity.change_time != expected.change_time
        || (default_stream && metadata.len() != entry.logical)
    {
        return Err(changed());
    }
    Ok(())
}

fn read_prefix(file: &File, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn decode_text(bytes: &[u8], truncated: bool) -> Option<String> {
    let text = if let Some((little, bytes)) = bytes
        .strip_prefix(&[0xff, 0xfe])
        .map(|bytes| (true, bytes))
        .or_else(|| {
            bytes
                .strip_prefix(&[0xfe, 0xff])
                .map(|bytes| (false, bytes))
        }) {
        if !truncated && !bytes.len().is_multiple_of(2) {
            return None;
        }
        let mut units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|pair| {
                if little {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        if truncated
            && units
                .last()
                .is_some_and(|unit| (0xd800..=0xdbff).contains(unit))
        {
            units.pop();
        }
        String::from_utf16(&units).ok()?
    } else {
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        match std::str::from_utf8(bytes) {
            Ok(text) => text.to_owned(),
            Err(error) if truncated && error.error_len().is_none() => {
                std::str::from_utf8(&bytes[..error.valid_up_to()])
                    .ok()?
                    .to_owned()
            }
            Err(_) => return None,
        }
    };
    text.chars()
        .all(|ch| !ch.is_control() || matches!(ch, '\t' | '\r' | '\n'))
        .then_some(text)
}

fn recorded_url(text: &str) -> Option<(String, String)> {
    let mut in_zone = false;
    let mut host = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_zone = line.eq_ignore_ascii_case("[ZoneTransfer]");
        } else if in_zone
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("HostUrl")
            && host.replace(value.trim()).is_some()
        {
            return None;
        }
    }
    let original = host?;
    let (scheme, _) = original.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return None;
    }
    let url = url::Url::parse(original).ok()?;
    Some((url.host_str()?.to_owned(), original.to_owned()))
}
