use crate::{NodeId, Result, index::Index};
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    Preview,
    DownloadSource,
    DownloadUrl,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Inspection {
    Metadata {
        reason: &'static str,
    },
    Text {
        text: String,
        truncated: bool,
    },
    Image {
        width: u32,
        height: u32,
        png: String,
    },
    #[cfg(windows)]
    Archive(archive::Report),
    DownloadUnavailable {
        reason: &'static str,
    },
    DownloadSource {
        domain: String,
    },
    DownloadUrl {
        url: String,
    },
}

#[cfg(windows)]
mod archive;
#[cfg(windows)]
mod native;

pub fn inspect(
    index: &Index,
    id: NodeId,
    request: Request,
    cancel: &AtomicBool,
) -> Result<Option<Inspection>> {
    #[cfg(windows)]
    {
        native::inspect(index, id, request, cancel)
    }
    #[cfg(not(windows))]
    {
        let _ = (index, id, request, cancel);
        Err(crate::Error::Invalid(
            "Native file inspection requires Windows.".into(),
        ))
    }
}
