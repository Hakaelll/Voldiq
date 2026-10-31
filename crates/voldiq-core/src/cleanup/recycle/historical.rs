use super::{RestoredItem, file_identity, restore::at_parent};
use crate::{
    Error, Result,
    cleanup::{
        journal::{
            Journal,
            relocation::{Record, RelocationId},
        },
        validate_file,
    },
    windows::{
        self, cleanup_io,
        cleanup_stage::{StageEntry, restore_to_parent},
    },
};
use std::{
    fs::File,
    path::{Component, Path, PathBuf, Prefix},
};

/// Fresh ordinary authority retained between an explicit review and its one-use confirmation.
pub struct RestorationReview {
    record: Record,
    session: RelocationId,
    source: File,
    original_parents: Vec<File>,
    _bin_parents: Vec<File>,
    destination: PathBuf,
    name: StageEntry,
}

impl RestorationReview {
    pub fn prepare(journal: &Journal, id: i64) -> Result<Self> {
        let record = journal
            .relocation(id)?
            .ok_or_else(|| Error::Invalid("Recycling receipt is missing.".into()))?;
        let session = journal.reviewed_relocation(&record)?;
        let destination = PathBuf::from(&record.path);
        let original_parents = pin_parents(&destination)?;
        let parent = original_parents
            .last()
            .ok_or_else(|| Error::Invalid("Missing original parent.".into()))?;
        if file_identity(parent)? != record.parent {
            return Err(Error::Invalid("Original parent was replaced.".into()));
        }
        let name = StageEntry::parse(
            destination
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Error::Invalid("Inexact restoration filename.".into()))?,
        )?;
        vacant(parent, &name)?;
        let locator = record
            .bin_path
            .as_ref()
            .ok_or_else(|| Error::Invalid("Missing recycling locator.".into()))?;
        let bin_parents = pin_parents(locator)?;
        let source = cleanup_io::open_leaf(
            bin_parents
                .last()
                .ok_or_else(|| Error::Invalid("Missing recycling parent.".into()))?,
            locator
                .file_name()
                .ok_or_else(|| Error::Invalid("Missing recycled filename.".into()))?,
            cleanup_io::LeafAccess::CompareAndRemove,
        )?;
        let review = Self {
            record,
            session,
            source,
            original_parents,
            _bin_parents: bin_parents,
            destination,
            name,
        };
        review.check_source(true)?;
        Ok(review)
    }

    pub fn destination(&self) -> &Path {
        &self.destination
    }
    pub fn size(&self) -> u64 {
        self.record.recycled_version.expect("Reviewed version").size
    }

    /// Consuming the review prevents retry after a potentially partial native operation.
    pub fn restore(self, journal: &mut Journal) -> Result<RestoredItem> {
        journal.reviewed_relocation(&self.record)?;
        self.check_source(true)?;
        let parent = self
            .original_parents
            .last()
            .ok_or_else(|| Error::Invalid("Missing retained original parent.".into()))?;
        vacant(parent, &self.name)?;
        journal.start_restore(self.session, self.record.source, &self.destination)?;
        let result = (|| {
            restore_to_parent(&self.source, parent, &self.name)?;
            if file_identity(&at_parent(parent, &self.name)?)? != self.record.source {
                return Err(Error::Invalid("Restored identity is unresolved.".into()));
            }
            self.check_source(false)
        })();
        if let Err(error) = result {
            if let Err(recording) = journal.unresolved_relocation(self.session, &error.to_string())
            {
                return Err(Error::Invalid(format!(
                    "{error}; restoration journal: {recording}"
                )));
            }
            return Err(error);
        }
        let journal_error = journal
            .record_restored(self.session, self.record.source, &self.destination)
            .err()
            .map(|error| error.to_string());
        Ok(RestoredItem {
            path: self.destination.clone(),
            identity: self.record.source,
            journal_error,
        })
    }

    fn check_source(&self, compare_change_time: bool) -> Result<()> {
        let version = self
            .record
            .recycled_version
            .ok_or_else(|| Error::Invalid("Missing observed recycled version.".into()))?;
        let metadata = self.source.metadata()?;
        validate_file(&metadata)?;
        if file_identity(&self.source)? != self.record.source
            || metadata.len() != version.size
            || metadata.modified()? != version.modified
            || (compare_change_time
                && windows::file_identity(&self.source)?.change_time != version.change_time)
        {
            return Err(Error::Invalid(
                "Recycled file changed since its recorded observation.".into(),
            ));
        }
        Ok(())
    }
}

fn vacant(parent: &File, name: &StageEntry) -> Result<()> {
    match at_parent(parent, name) {
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Restoration destination is occupied.",
        )
        .into()),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn pin_parents(path: &Path) -> Result<Vec<File>> {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        || !matches!(components.next(), Some(Component::RootDir))
    {
        return Err(Error::Invalid(
            "Restoration requires a local absolute drive path.".into(),
        ));
    }
    if !components.all(|component| matches!(component, Component::Normal(_))) {
        return Err(Error::Invalid(
            "Restoration path contains traversal.".into(),
        ));
    }
    let mut paths: Vec<_> = path
        .parent()
        .ok_or_else(|| Error::Invalid("Missing restoration parent.".into()))?
        .ancestors()
        .collect();
    paths.reverse();
    let mut parents = Vec::with_capacity(paths.len());
    for path in paths {
        let file = match parents.last() {
            Some(parent) => cleanup_io::open_child(
                parent,
                path.file_name()
                    .ok_or_else(|| Error::Invalid("Missing parent name.".into()))?,
                true,
            )?,
            None => cleanup_io::open_root(path)?,
        };
        let metadata = file.metadata()?;
        if !metadata.is_dir()
            || windows::is_reparse(&metadata)
            || windows::requires_recall(&metadata)
        {
            return Err(Error::Invalid(
                "Redirected or offline restoration parent.".into(),
            ));
        }
        parents.push(file);
    }
    Ok(parents)
}
