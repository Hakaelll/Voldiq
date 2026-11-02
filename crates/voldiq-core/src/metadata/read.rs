use super::Store;
use crate::{NodeId, Result, cleanup, index::Index};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct FolderRead {
    pub source_snapshot: Option<i64>,
    pub node_id: NodeId,
    pub path: PathBuf,
    pub protected_from_cleanup: bool,
    pub note: NoteContent,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoteContent {
    NotRequested,
    PlainText { text: String, updated: Option<i64> },
}

impl Store {
    /// Trusted human adapters may request text; agent adapters must first authorize ReadContent.
    pub fn read_indexed(
        &mut self,
        index: &Index,
        node: NodeId,
        include_note: bool,
    ) -> Result<FolderRead> {
        let folder = index.pin_folder(node)?;
        let ancestors = folder.ancestor_identities()?;
        let system = cleanup::protected_identities()?;
        let protected_from_cleanup = self
            .protections()?
            .check_evidence(folder.path(), folder.identity(), &ancestors)
            .is_err()
            || cleanup::record::sensitive(folder.path())
            || ancestors.iter().any(|identity| system.contains(identity));
        let note = if include_note {
            let note = self.get(folder.path())?;
            NoteContent::PlainText {
                text: note.note,
                updated: note.updated,
            }
        } else {
            NoteContent::NotRequested
        };
        Ok(FolderRead {
            source_snapshot: index.snapshot_id,
            node_id: node,
            path: folder.path().to_owned(),
            protected_from_cleanup,
            note,
        })
    }
}
