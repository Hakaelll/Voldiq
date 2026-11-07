use crate::{Error, Result};
use serde::Serialize;
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

mod iso;
mod zip;

const MAX_RECORDS: usize = 512;
const MAX_DEPTH: usize = 16;
const MAX_PATH: usize = 768;
const METADATA_BUDGET: u64 = 4 * 1024 * 1024;
const GZIP_BUDGET: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Format {
    Zip,
    Jar,
    Tar,
    Tgz,
    Iso,
}

impl Format {
    pub(super) fn of(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?.to_ascii_lowercase();
        match name.rsplit_once('.')?.1 {
            "zip" => Some(Self::Zip),
            "jar" => Some(Self::Jar),
            "tar" => Some(Self::Tar),
            "tgz" => Some(Self::Tgz),
            "gz" if name.ends_with(".tar.gz") => Some(Self::Tgz),
            "iso" => Some(Self::Iso),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    File,
    Folder,
    Link,
}

#[derive(Serialize)]
struct Entry {
    path: String,
    kind: Kind,
    logical_bytes: Option<String>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Limit {
    Records,
    Bytes,
}

#[derive(Serialize)]
pub struct Report {
    format: Format,
    items: Vec<Entry>,
    omitted: usize,
    limit: Option<Limit>,
    #[serde(skip)]
    records: usize,
}

impl Report {
    fn next(&mut self) -> bool {
        if self.records >= MAX_RECORDS {
            self.limit = Some(Limit::Records);
            return false;
        }
        self.records += 1;
        true
    }

    fn push(&mut self, path: Option<String>, kind: Kind, logical_bytes: u64) {
        if let Some(path) = path {
            self.items.push(Entry {
                path,
                kind,
                logical_bytes: (kind == Kind::File).then(|| logical_bytes.to_string()),
            });
        } else {
            self.omitted += 1;
        }
    }
}

pub(super) fn read(
    file: &File,
    length: u64,
    format: Format,
    cancel: &AtomicBool,
) -> Result<Report> {
    let mut report = Report {
        format,
        items: Vec::new(),
        omitted: 0,
        limit: None,
        records: 0,
    };
    let exhausted = AtomicBool::new(false);
    let budget = if matches!(format, Format::Tgz) {
        GZIP_BUDGET
    } else {
        METADATA_BUDGET
    };
    let mut source = Bounded::new(file, budget, length, cancel, &exhausted);
    let outcome = match format {
        Format::Zip | Format::Jar => zip::list(&mut source, length, &mut report),
        Format::Iso => iso::list(&mut source, length, &mut report),
        Format::Tar => {
            let mut archive = tar::Archive::new(source);
            archive
                .entries_with_seek()
                .and_then(|entries| list_tar(entries, &mut report))
        }
        Format::Tgz => {
            let decoded = flate2::read::MultiGzDecoder::new(source);
            let mut archive = tar::Archive::new(Bounded::new(
                decoded,
                GZIP_BUDGET,
                u64::MAX,
                cancel,
                &exhausted,
            ));
            archive
                .entries()
                .and_then(|entries| list_tar(entries, &mut report))
        }
    };
    if cancel.load(Ordering::Relaxed) {
        // The native request owner discards this result after releasing its file guards.
        return Ok(report);
    }
    if exhausted.load(Ordering::Relaxed) {
        report.limit = Some(Limit::Bytes);
    } else {
        outcome.map_err(|_| {
            Error::Invalid("Archive metadata is invalid or uses an unsupported layout.".into())
        })?;
    }
    Ok(report)
}

fn list_tar<R: Read>(entries: tar::Entries<'_, R>, report: &mut Report) -> io::Result<()> {
    for entry in entries {
        if !report.next() {
            break;
        }
        let entry = entry?;
        let kind = match entry.header().entry_type() {
            item if item.is_file() || item.is_gnu_sparse() => Kind::File,
            item if item.is_dir() => Kind::Folder,
            item if item.is_symlink() || item.is_hard_link() => Kind::Link,
            _ => {
                report.omitted += 1;
                continue;
            }
        };
        let path = entry.path_bytes();
        let path = std::str::from_utf8(&path).ok().and_then(safe_path);
        // A root marker in a TAR archive has no virtual name.
        if kind == Kind::Folder && entry.path_bytes().as_ref() == b"./" {
            continue;
        }
        report.push(path, kind, entry.size());
    }
    Ok(())
}

fn safe_path(path: &str) -> Option<String> {
    let path = path
        .strip_prefix("./")
        .unwrap_or(path)
        .trim_end_matches('/');
    if path.is_empty()
        || path.len() > MAX_PATH
        || path.starts_with('/')
        || path.chars().any(|ch| {
            ch.is_control()
                || matches!(ch, '\\' | ':' | '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return None;
    }
    let mut parts = path.split('/');
    if parts.clone().count() > MAX_DEPTH
        || parts.any(|part| part.is_empty() || part.trim_end() == "." || part.trim_end() == "..")
    {
        return None;
    }
    Some(path.to_owned())
}

fn invalid() -> io::Error {
    io::Error::other("Invalid archive metadata")
}

struct Bounded<'a, R> {
    reader: R,
    remaining: u64,
    length: u64,
    cancel: &'a AtomicBool,
    exhausted: &'a AtomicBool,
}

impl<'a, R> Bounded<'a, R> {
    fn new(
        reader: R,
        remaining: u64,
        length: u64,
        cancel: &'a AtomicBool,
        exhausted: &'a AtomicBool,
    ) -> Self {
        Self {
            reader,
            remaining,
            length,
            cancel,
            exhausted,
        }
    }

    fn check(&self) -> io::Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::other("Archive inspection cancelled"));
        }
        Ok(())
    }
}

impl<R: Read> Read for Bounded<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            self.exhausted.store(true, Ordering::Relaxed);
            return Err(io::Error::other("Archive inspection byte limit reached"));
        }
        let length = buffer.len().min(self.remaining as usize);
        let read = self.reader.read(&mut buffer[..length])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

impl<R: Seek> Seek for Bounded<'_, R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.check()?;
        let offset = self.reader.seek(position)?;
        if offset > self.length {
            return Err(invalid());
        }
        Ok(offset)
    }
}
