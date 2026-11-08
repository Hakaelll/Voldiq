use super::Provider;
use crate::{Error, Kind, NodeId, Result, index::Index};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_ROOTS: usize = 2560;
const VERSION: u8 = 1;

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct GameEvidence {
    pub source: NodeId,
    pub folder: NodeId,
    pub steamapps: Option<NodeId>,
    pub provider: Provider,
    pub title: String,
    pub product: String,
    pub main: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Capture {
    version: u8,
    pub(super) games: Vec<GameEvidence>,
    pub(super) media: Vec<NodeId>,
}

impl Capture {
    pub(crate) fn roots(&self) -> HashSet<NodeId> {
        self.games
            .iter()
            .flat_map(|game| {
                [Some(game.source), Some(game.folder), game.steamapps]
                    .into_iter()
                    .flatten()
            })
            .chain(self.media.iter().copied())
            .collect()
    }

    pub(crate) fn remap(&mut self, mut map: impl FnMut(NodeId) -> Result<NodeId>) -> Result<()> {
        for game in &mut self.games {
            game.source = map(game.source)?;
            game.folder = map(game.folder)?;
            game.steamapps = game.steamapps.map(&mut map).transpose()?;
        }
        for id in &mut self.media {
            *id = map(*id)?;
        }
        Ok(())
    }

    pub(crate) fn validate(&self, index: &Index) -> Result<()> {
        let mut sources = HashSet::new();
        let regular = |id| {
            index
                .entry(id)
                .is_ok_and(|entry| entry.kind == Kind::File && !entry.incomplete)
        };
        let directory = |id| {
            index
                .entry(id)
                .is_ok_and(|entry| entry.kind == Kind::Directory)
        };
        if self.version != VERSION
            || self.games.len() > 512
            || self.media.len() > 1024
            || self.games.iter().any(|game| {
                !regular(game.source)
                    || !sources.insert(game.source)
                    || !directory(game.folder)
                    || (game.provider == Provider::Steam) != game.steamapps.is_some()
                    || game.steamapps.is_some_and(|id| !directory(id))
                    || game.title.is_empty()
                    || game.title.len() > 1024
                    || game.title.chars().any(char::is_control)
                    || !product(&game.product)
                    || game.main.as_ref().is_some_and(|id| !product(id))
            })
            || self
                .media
                .iter()
                .any(|&node| !regular(node) || !sources.insert(node))
        {
            return Err(Error::Invalid("Invalid captured library evidence.".into()));
        }
        Ok(())
    }
}

fn product(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

/// The caller authorizes each indexed metadata file before its bounded read.
pub fn capture(
    index: &Index,
    cancel: &AtomicBool,
    mut authorize: impl FnMut(NodeId) -> bool,
) -> Result<Option<Capture>> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    if index.snapshot_id.is_some() {
        return Ok(index.captured_libraries.clone());
    }
    #[cfg(windows)]
    {
        let Some(games) = super::games::capture(index, cancel, &mut authorize)? else {
            return Ok(None);
        };
        let Some(media) =
            super::media::indexed::signatures(index, NodeId(0), cancel, &mut authorize)
        else {
            return Ok(None);
        };
        let mut media: Vec<_> = media.into_iter().collect();
        media.sort_unstable_by_key(|id| id.0);
        let capture = Capture {
            version: VERSION,
            games,
            media,
        };
        capture.validate(index)?;
        Ok(Some(capture))
    }
    #[cfg(not(windows))]
    {
        let _ = &mut authorize;
        Ok(None)
    }
}
