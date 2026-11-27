//! User-defined, native-folder-bound cleanup suggestions for the live desktop.
use super::{Query, RuleSource, rank_candidates, scope::DirectoryScope};
use crate::{
    Entry, Error, Kind, NodeId, Result,
    agents::{AuthorizedPath, pin_directory},
    cleanup::{Plan, score::Assessment},
    cloud::CloudState,
    duplicates::Analysis,
    index::{Index, PreparedQuery, Query as SearchQuery},
    metadata::{Protections, Store as Metadata},
    operations::Coordinator,
    search::CompiledQuery,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_RULES: i64 = 64;
const MAX_AGE_DAYS: u16 = 36_500;
pub(crate) const RECIPE: super::Rule = super::Rule {
    id: "custom-cleanup-rule",
    name: "Custom cleanup rule",
    description: "Age, extension, logical size and an optional search filter in a user-selected indexed folder.",
    discovery: "A complete regular file under the saved native folder identity in the live analysis.",
    selection: "Explicit current selection that still matches the exact saved revision.",
    exclusions: "Protected locations, Voldiq state, incomplete files, unavailable identity or modified date, reparse points and recalled content.",
    risk: "review",
    permissions: "Metadata access for review and verified native file access for the chosen action.",
    policy: "recycle",
    verification: "Saved rule revision, pinned folder identity and live file state are rechecked for review and execution.",
};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub id: i64,
    pub revision: i64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub name: String,
    pub extension: String,
    pub minimum_age_days: u16,
    pub minimum_logical: String,
    #[serde(default)]
    pub expression: String,
}

#[derive(Serialize)]
pub struct Rule {
    pub reference: Revision,
    #[serde(flatten)]
    pub criteria: Draft,
    pub root: String,
}

pub struct Environment<'a> {
    pub now: u64,
    pub protections: &'a Protections,
    pub duplicates: Option<&'a Analysis>,
}

#[derive(Serialize)]
pub struct Page {
    pub rule: Rule,
    pub complete: bool,
    pub unverified_branches: usize,
    pub unverified_files: usize,
    pub assessed_at: u64,
    pub total: usize,
    pub logical: String,
    pub items: Vec<Item>,
}

#[derive(Serialize)]
pub struct Item {
    pub id: NodeId,
    pub path: String,
    pub logical: String,
    pub allocated: Option<String>,
    pub hard_link: bool,
    pub modified: Option<u64>,
    pub priority: Assessment,
}

pub struct Store {
    connection: Connection,
    directory: Coordinator,
}

struct Saved {
    rule: Rule,
    identity: Vec<u8>,
}

pub(super) struct Criteria<'a> {
    extension: &'a str,
    minimum_age_days: u16,
    minimum_logical: u64,
}

impl Draft {
    fn normalize(mut self) -> Result<Self> {
        self.name = self.name.trim().to_owned();
        let extension = self.extension.trim();
        self.extension = extension
            .strip_prefix('.')
            .unwrap_or(extension)
            .to_ascii_lowercase();
        self.expression = self.expression.trim().to_owned();
        self.validate()?;
        Ok(self)
    }

    fn validate(&self) -> Result<()> {
        self.criteria()?;
        self.search_query(None).compile()?;
        Ok(())
    }

    fn search_query(&self, scope: Option<NodeId>) -> SearchQuery {
        SearchQuery {
            expression: self.expression.clone(),
            files_only: true,
            scope,
            ..Default::default()
        }
    }

    fn criteria(&self) -> Result<Criteria<'_>> {
        if self.name.is_empty()
            || self.name.chars().count() > 80
            || self.name.chars().any(char::is_control)
        {
            return Err(Error::Invalid(
                "Enter a rule name of 1 to 80 characters.".into(),
            ));
        }
        if self.extension.is_empty()
            || self.extension.len() > 16
            || !self
                .extension
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(Error::Invalid(
                "Enter one literal file extension of 1 to 16 ASCII letters or digits.".into(),
            ));
        }
        if !(1..=MAX_AGE_DAYS).contains(&self.minimum_age_days) {
            return Err(Error::Invalid(
                "Enter an age of 1 to 36,500 whole days.".into(),
            ));
        }
        let logical = self.minimum_logical.as_str();
        let minimum_logical = logical
            .parse::<u64>()
            .ok()
            .filter(|&value| value > 0 && value.to_string() == logical)
            .ok_or_else(|| {
                Error::Invalid("Enter a positive whole number of logical bytes.".into())
            })?;
        Ok(Criteria {
            extension: &self.extension,
            minimum_age_days: self.minimum_age_days,
            minimum_logical,
        })
    }
}

impl Criteria<'_> {
    pub(super) fn matches_file(&self, entry: &Entry) -> bool {
        entry.kind == Kind::File
            && !entry.incomplete
            && entry.logical >= self.minimum_logical
            && entry
                .name
                .rsplit_once('.')
                .is_some_and(|(stem, extension)| {
                    !stem.is_empty() && extension.eq_ignore_ascii_case(self.extension)
                })
    }

    pub(super) fn has_evidence(&self, entry: &Entry) -> bool {
        entry.file_state.is_some()
            && entry.modified.is_some()
            && entry.cloud == CloudState::NotPlaceholder
    }

    pub(super) fn old_enough(&self, entry: &Entry, now: u64) -> bool {
        entry.modified.is_some_and(|modified| {
            now.checked_sub(modified)
                .is_some_and(|age| age >= u64::from(self.minimum_age_days) * 86_400)
        })
    }
}

impl Store {
    pub fn open(state: &Path) -> Result<Self> {
        let directory = Coordinator::open(state)?;
        let mut connection = open_connection(&directory, OpenFlags::default())?;
        crate::database::configure(&connection)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version = tx.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))?;
        match version {
            0 => tx.execute_batch(
                "CREATE TABLE rules(
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    revision INTEGER NOT NULL CHECK(revision > 0),
                    name TEXT NOT NULL, extension TEXT NOT NULL,
                    minimum_age_days INTEGER NOT NULL CHECK(minimum_age_days BETWEEN 1 AND 36500),
                    minimum_logical TEXT NOT NULL, root TEXT NOT NULL, identity BLOB NOT NULL,
                    expression TEXT NOT NULL DEFAULT ''
                );
                PRAGMA user_version=2;",
            )?,
            1 => tx.execute_batch(
                "ALTER TABLE rules ADD COLUMN expression TEXT NOT NULL DEFAULT '';
                 PRAGMA user_version=2;",
            )?,
            2 => {}
            _ => {
                return Err(Error::Invalid(
                    "Unsupported custom cleanup rule schema.".into(),
                ));
            }
        }
        tx.commit()?;
        Ok(Self {
            connection,
            directory,
        })
    }

    pub fn list(&self) -> Result<Vec<Rule>> {
        let mut statement = self.connection.prepare(
            "SELECT id, revision, name, extension, minimum_age_days, minimum_logical, root, identity, expression
             FROM rules ORDER BY id LIMIT ?1",
        )?;
        let saved = statement
            .query_map([MAX_RULES + 1], read)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if saved.len() > MAX_RULES as usize {
            return Err(Error::Invalid(
                "Custom cleanup rule store exceeds its limit.".into(),
            ));
        }
        saved
            .into_iter()
            .map(|saved| {
                saved.rule.criteria.validate()?;
                Ok(saved.rule)
            })
            .collect()
    }

    pub fn save(
        &mut self,
        index: &Index,
        scope: Option<NodeId>,
        expected: Option<Revision>,
        draft: Draft,
        cancel: &AtomicBool,
    ) -> Result<Rule> {
        let draft = draft.normalize()?;
        live(index)?;
        crate::cleanup::check_cancelled(cancel)?;
        let (scope, folder) = match (scope, expected) {
            (Some(scope), None) => (scope, index.pin_folder(scope)?),
            (None, Some(reference)) => {
                let saved = self.get(reference)?;
                let (binding, indexed) = Self::bind(index, &saved)?;
                (binding.id, indexed)
            }
            _ => {
                return Err(Error::Invalid(
                    "Specify a folder for a new rule or a revision for editing.".into(),
                ));
            }
        };
        self.check_state_path(&folder)?;
        let protections = Metadata::open(self.directory.directory())?.protections()?;
        if !protections.for_index(index, cancel)?.allows(scope) {
            return Err(Error::Invalid(
                "This folder is protected or could not be verified.".into(),
            ));
        }
        let root = folder
            .path()
            .to_str()
            .ok_or_else(|| Error::Invalid("Rule folder cannot be represented exactly.".into()))?;
        let identity = folder.identity_bytes()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        crate::cleanup::check_cancelled(cancel)?;
        let reference = if let Some(expected) = expected {
            let revision = expected.revision.checked_add(1).ok_or_else(stale)?;
            if tx.execute(
                "UPDATE rules SET revision=?1, name=?2, extension=?3,
                 minimum_age_days=?4, minimum_logical=?5, expression=?6 WHERE id=?7 AND revision=?8",
                params![
                    revision,
                    draft.name,
                    draft.extension,
                    draft.minimum_age_days,
                    draft.minimum_logical,
                    draft.expression,
                    expected.id,
                    expected.revision
                ],
            )? != 1
            {
                return Err(stale());
            }
            Revision {
                id: expected.id,
                revision,
            }
        } else {
            let count: i64 = tx.query_row("SELECT COUNT(*) FROM rules", [], |row| row.get(0))?;
            if count >= MAX_RULES {
                return Err(Error::Invalid(
                    "Keep at most 64 custom cleanup rules.".into(),
                ));
            }
            tx.execute(
                "INSERT INTO rules(revision, name, extension, minimum_age_days, minimum_logical, root, identity, expression)
                 VALUES(1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![draft.name, draft.extension, draft.minimum_age_days, draft.minimum_logical,
                    root, identity, draft.expression],
            )?;
            Revision {
                id: tx.last_insert_rowid(),
                revision: 1,
            }
        };
        tx.commit()?;
        Ok(Rule {
            reference,
            criteria: draft,
            root: root.to_owned(),
        })
    }

    pub fn remove(&mut self, reference: Revision) -> Result<()> {
        if self.connection.execute(
            "DELETE FROM rules WHERE id=?1 AND revision=?2",
            params![reference.id, reference.revision],
        )? != 1
        {
            return Err(stale());
        }
        Ok(())
    }

    fn get(&self, reference: Revision) -> Result<Saved> {
        self.connection.query_row(
            "SELECT id, revision, name, extension, minimum_age_days, minimum_logical, root, identity, expression
             FROM rules WHERE id=?1 AND revision=?2",
            params![reference.id, reference.revision],
            read,
        ).optional()?.ok_or_else(stale)
    }

    pub(crate) fn verify_revisions(&self, revisions: &[Revision]) -> Result<()> {
        verify_revisions(&self.connection, revisions)
    }

    fn bind(index: &Index, saved: &Saved) -> Result<(DirectoryScope, AuthorizedPath)> {
        live(index)?;
        let changed = || {
            Error::Invalid("The saved rule folder changed or is outside the live analysis.".into())
        };
        let pinned = pin_directory(Path::new(&saved.rule.root)).map_err(|_| changed())?;
        if pinned.identity_bytes()? != saved.identity {
            return Err(changed());
        }
        let binding = DirectoryScope::bind_pinned(index, pinned)?
            .filter(|binding| binding.whole)
            .ok_or_else(changed)?;
        let indexed = index.pin_folder(binding.id)?;
        if indexed.identity_bytes()? != saved.identity {
            return Err(changed());
        }
        Ok((binding, indexed))
    }

    fn check_state_path(&self, folder: &AuthorizedPath) -> Result<()> {
        if folder
            .ancestor_identities()?
            .contains(&self.directory.identity())
        {
            return Err(Error::Invalid(
                "Voldiq state is outside custom cleanup scope.".into(),
            ));
        }
        Ok(())
    }

    fn state_scope(&self, index: &Index) -> Result<Option<NodeId>> {
        Ok(DirectoryScope::bind(index, self.directory.directory())?.map(|scope| scope.id))
    }

    pub fn query(
        &self,
        index: &Index,
        reference: Revision,
        request: Query,
        env: &Environment<'_>,
        cancel: &AtomicBool,
    ) -> Result<Page> {
        if !(1..=100).contains(&request.limit) || request.offset > index.entries.len() {
            return Err(Error::Invalid("Invalid custom cleanup page bounds.".into()));
        }
        crate::cleanup::check_cancelled(cancel)?;
        let saved = self.get(reference)?;
        let (binding, indexed) = Self::bind(index, &saved)?;
        self.check_state_path(&indexed)?;
        let criteria = saved.rule.criteria.criteria()?;
        let search = saved.rule.criteria.search_query(Some(binding.id));
        let compiled = search.compile()?;
        let filter = prepare_search(index, &compiled, cancel)?;
        let ranked = rank_candidates(
            index,
            binding.id,
            request,
            super::Ranking {
                source: RuleSource::Custom {
                    criteria: &criteria,
                    state_scope: self.state_scope(index)?,
                    filter: filter.as_ref(),
                },
                now: env.now,
                protections: env.protections,
                duplicates: env.duplicates,
            },
            cancel,
        )?;
        let mut items = Vec::with_capacity(ranked.items.len());
        for (entry, priority) in ranked.items {
            crate::cleanup::check_cancelled(cancel)?;
            let path = binding.path(index, entry.id)?;
            let path = path.to_str().ok_or_else(|| {
                Error::Invalid("A matching file path cannot be represented exactly.".into())
            })?;
            items.push(Item {
                id: entry.id,
                path: path.into(),
                logical: entry.logical.to_string(),
                allocated: (entry.allocation_known && !entry.hard_link)
                    .then(|| entry.allocated.to_string()),
                hard_link: entry.hard_link,
                modified: entry.modified,
                priority,
            });
        }
        self.verify_revisions(&[reference])?;
        if Metadata::open(self.directory.directory())?.protection_fingerprint()?
            != env.protections.fingerprint()
        {
            return Err(Error::Invalid(
                "Protections changed while reviewing the custom rule.".into(),
            ));
        }
        crate::cleanup::check_cancelled(cancel)?;
        Ok(Page {
            rule: saved.rule,
            complete: !index.entry(binding.id)?.incomplete
                && ranked.unverified_branches == 0
                && ranked.unverified_files == 0,
            unverified_branches: ranked.unverified_branches,
            unverified_files: ranked.unverified_files,
            assessed_at: env.now,
            total: ranked.total,
            logical: ranked.logical.to_string(),
            items,
        })
    }

    pub(crate) fn prepare(
        &self,
        index: &Index,
        reference: Revision,
        now: u64,
        protections: &Protections,
        selection: &[NodeId],
        cancel: &AtomicBool,
    ) -> Result<Plan> {
        crate::cleanup::check_cancelled(cancel)?;
        let saved = self.get(reference)?;
        let (binding, indexed) = Self::bind(index, &saved)?;
        self.check_state_path(&indexed)?;
        let state_scope = self.state_scope(index)?;
        let criteria = saved.rule.criteria.criteria()?;
        let search = saved.rule.criteria.search_query(Some(binding.id));
        let compiled = search.compile()?;
        let filter = prepare_search(index, &compiled, cancel)?;
        let mask = protections.for_index(index, cancel)?;
        let mut budget = crate::search::GLOB_WORK_LIMIT;
        for &id in selection {
            crate::cleanup::check_cancelled(cancel)?;
            let entry = index.entry(id)?;
            if !mask.allows(id)
                || !index.in_scope(id, binding.id)
                || state_scope.is_some_and(|state| index.in_scope(id, state))
                || !criteria.matches_file(entry)
                || !criteria.has_evidence(entry)
                || !criteria.old_enough(entry, now)
                || !matches_search(filter.as_ref(), id, &mut budget, cancel)?
            {
                return Err(Error::Invalid(
                    "Selected file is outside the custom rule or is protected.".into(),
                ));
            }
        }
        let mut plan = Plan::prepare(index, selection, cancel)?;
        self.verify_revisions(&[reference])?;
        plan.preview.rule = RECIPE.id;
        Ok(plan)
    }
}

fn prepare_search<'i, 'q>(
    index: &'i Index,
    compiled: &'q CompiledQuery<'q>,
    cancel: &AtomicBool,
) -> Result<Option<PreparedQuery<'i, 'q>>> {
    if compiled.query.expression.is_empty() {
        return Ok(None);
    }
    index
        .prepare_query(compiled, None, &mut || cancel.load(Ordering::Relaxed))?
        .ok_or_else(search_cancelled)
        .map(Some)
}

pub(super) fn matches_search(
    filter: Option<&PreparedQuery<'_, '_>>,
    id: NodeId,
    budget: &mut u64,
    cancel: &AtomicBool,
) -> Result<bool> {
    match filter {
        Some(filter) => filter
            .matches(id, budget, &mut || cancel.load(Ordering::Relaxed))?
            .ok_or_else(search_cancelled),
        None => Ok(true),
    }
}

fn search_cancelled() -> Error {
    Error::Invalid("Custom rule search was cancelled.".into())
}

pub(crate) fn verify_persisted_revisions(
    directory: &Coordinator,
    revisions: &[Revision],
) -> Result<()> {
    let connection = open_connection(directory, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    let version = connection.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))?;
    if !matches!(version, 1 | 2) {
        return Err(Error::Invalid(
            "Unsupported custom cleanup rule schema.".into(),
        ));
    }
    verify_revisions(&connection, revisions)
}

fn verify_revisions(connection: &Connection, revisions: &[Revision]) -> Result<()> {
    let mut statement = connection.prepare("SELECT 1 FROM rules WHERE id=?1 AND revision=?2")?;
    for reference in revisions {
        if !statement.exists(params![reference.id, reference.revision])? {
            return Err(stale());
        }
    }
    Ok(())
}

fn open_connection(directory: &Coordinator, flags: OpenFlags) -> Result<Connection> {
    for name in [
        "cleanup-rules.db",
        "cleanup-rules.db-wal",
        "cleanup-rules.db-shm",
        "cleanup-rules.db-journal",
    ] {
        match fs::symlink_metadata(directory.directory().join(name)) {
            Ok(metadata)
                if !metadata.is_file()
                    || crate::windows::is_reparse(&metadata)
                    || crate::windows::requires_recall(&metadata) =>
            {
                return Err(Error::Invalid(
                    "Custom rule storage requires regular local files.".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(Connection::open_with_flags(
        directory.directory().join("cleanup-rules.db"),
        flags | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?)
}

fn read(row: &Row<'_>) -> rusqlite::Result<Saved> {
    Ok(Saved {
        rule: Rule {
            reference: Revision {
                id: row.get(0)?,
                revision: row.get(1)?,
            },
            criteria: Draft {
                name: row.get(2)?,
                extension: row.get(3)?,
                minimum_age_days: row.get(4)?,
                minimum_logical: row.get(5)?,
                expression: row.get(8)?,
            },
            root: row.get(6)?,
        },
        identity: row.get(7)?,
    })
}

fn live(index: &Index) -> Result<()> {
    if index.snapshot_id.is_some() {
        return Err(Error::Invalid(
            "Use a current analysis with custom cleanup rules.".into(),
        ));
    }
    Ok(())
}

fn stale() -> Error {
    Error::Invalid(
        "The custom cleanup rule changed or was removed. Review the latest revision.".into(),
    )
}
