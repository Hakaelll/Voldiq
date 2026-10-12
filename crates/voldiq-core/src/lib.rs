pub mod cleanup;
pub mod developer;
pub mod duplicates;
pub mod index;
pub mod ntfs;
pub mod scanner;
pub mod snapshots;
pub mod windows;

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Directory,
    File,
    Link,
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    #[serde(skip)]
    pub file_state: Option<FileState>,
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub name: String,
    pub kind: Kind,
    pub logical: u64,
    pub allocated: u64,
    pub allocation_known: bool,
    pub modified: Option<u64>,
    pub files: u64,
    pub incomplete: bool,
    pub hard_link: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileState {
    pub identity: (u64, u64),
    pub change_time: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanPhase {
    Scanning,
    Complete,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub previous: bool,
    pub phase: ScanPhase,
    pub entries: usize,
    pub files: u64,
    pub logical: u64,
    pub elapsed_ms: u64,
    pub errors: u64,
    pub method: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct ScanOptions {
    #[serde(default)]
    pub mode: ScanMode,
    #[serde(default)]
    pub exclusions: Vec<String>,
    pub max_entries: Option<usize>,
    #[serde(default)]
    pub concurrency: ScanConcurrency,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(try_from = "u8")]
pub struct ScanConcurrency(u8);

impl Default for ScanConcurrency {
    fn default() -> Self {
        Self(1)
    }
}

impl TryFrom<u8> for ScanConcurrency {
    type Error = &'static str;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        if (1..=8).contains(&value) {
            Ok(Self(value))
        } else {
            Err("Scan concurrency must be between 1 and 8.")
        }
    }
}

impl ScanConcurrency {
    pub fn get(self) -> usize {
        usize::from(self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanMode {
    #[default]
    Auto,
    Directory,
}
