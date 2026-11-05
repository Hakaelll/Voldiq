//! Confirmed-plan execution. Admissions finish before Shell work begins.
use super::{ShellApartment, check_content, file_identity};
use crate::{
    Error, NodeId, Result,
    cleanup::{
        Plan, RetainedCopies, VerifiedFile,
        helper::Session,
        journal::{Journal, relocation::RelocationId},
    },
    windows::{
        cleanup_io,
        cleanup_stage::{StageEntry, StageName, StagePin},
    },
};
use serde::Serialize;
use std::{
    fs::File,
    os::windows::ffi::OsStrExt,
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    NotStarted,
    Unmoved,
    Returned,
    Recycled,
    Unknown,
}

#[derive(Serialize)]
pub struct ItemResult {
    pub id: NodeId,
    pub path: String,
    pub outcome: Outcome,
    pub recorded: bool,
    pub operation: Option<i64>,
    pub bin_path: Option<Vec<u16>>,
    pub error: Option<String>,
    pub cleanup_error: Option<String>,
}

#[derive(Serialize)]
pub struct Report {
    pub items: Vec<ItemResult>,
    pub cancelled: bool,
    pub journal_error: Option<String>,
    pub helper_error: Option<String>,
    pub preflight_error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Ready,
    Unmoved,
    Staged,
    Unknown,
}

struct Prepared {
    row: usize,
    relocation: RelocationId,
    source: Option<VerifiedFile>,
    parent: File,
    stage: Option<StagePin>,
    name: StageName,
    entry: StageEntry,
    phase: Phase,
}

pub struct Batch {
    _plan: Plan,
    keepers: Option<RetainedCopies>,
    prepared: Vec<Prepared>,
    report: Report,
}

/// The caller must consume a confirmed current-index plan once. This launches UAC only after ordinary preparation.
pub fn run(
    plan: Plan,
    journal: &mut Journal,
    cancel: &AtomicBool,
    progress: impl FnMut(usize) + Send,
) -> Result<Report> {
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                let apartment = ShellApartment::new()?;
                let batch = Batch::prepare(plan, journal, cancel, &apartment)?;
                if !batch.ready_for_admission() || cancel.load(Ordering::Relaxed) {
                    return Ok(batch.abort(journal, cancel, progress));
                }
                match Session::launch(&apartment) {
                    Ok(session) => {
                        Ok(batch.execute(journal, cancel, &apartment, session, progress))
                    }
                    Err(error) => {
                        let mut batch = batch;
                        batch.report.cancelled = error.raw_os_error() == Some(1223);
                        batch.report.helper_error = Some(error.to_string());
                        Ok(batch.abort(journal, cancel, progress))
                    }
                }
            })
            .join()
            .map_err(|_| {
                Error::Invalid(
                    "Cleanup worker failed; review the durable journal before retrying.".into(),
                )
            })?
    })
}

impl Batch {
    pub fn ready_for_admission(&self) -> bool {
        !self.prepared.is_empty()
            && self.prepared.len() == self._plan.files.len()
            && self.report.items.len() == self._plan.files.len()
            && self.report.journal_error.is_none()
            && self.report.items.iter().all(|item| item.error.is_none())
    }

    pub fn prepare(
        plan: Plan,
        journal: &mut Journal,
        cancel: &AtomicBool,
        apartment: &ShellApartment,
    ) -> Result<Self> {
        Self::prepare_with_origin(plan, journal, cancel, apartment, None, None)
    }

    pub(crate) fn prepare_agent(
        plan: Plan,
        journal: &mut Journal,
        cancel: &AtomicBool,
        apartment: &ShellApartment,
        origin: &crate::cleanup::journal::AgentExecutionRef<'_>,
    ) -> Result<Self> {
        Self::prepare_with_origin(plan, journal, cancel, apartment, Some(origin), None)
    }

    /// Reuses exact retained source authority after a verified transfer, without a close/reopen gap.
    pub(crate) fn prepare_transferred(
        plan: Plan,
        sources: Vec<(NodeId, VerifiedFile)>,
        journal: &mut Journal,
        cancel: &AtomicBool,
        apartment: &ShellApartment,
        origin: Option<&crate::cleanup::journal::AgentExecutionRef<'_>>,
    ) -> Result<Self> {
        let mut retained = std::collections::HashMap::new();
        for (node, source) in sources {
            let planned = plan
                .files
                .iter()
                .find(|file| file.id == node)
                .ok_or_else(|| {
                    Error::Invalid(
                        "Retained source is outside the exact transfer selection.".into(),
                    )
                })?;
            source.check_stamp()?;
            let identity = crate::windows::file_identity(&source.file)?;
            let access = crate::windows::security::granted_access(&source.file)?;
            let required = windows_sys::Win32::Storage::FileSystem::FILE_READ_DATA
                | windows_sys::Win32::Storage::FileSystem::DELETE;
            if source.path != planned.path
                || source.stamp != planned.stamp
                || identity.allocated != planned.allocated
                || identity.links != planned.links
                || access & required != required
                || retained.insert(node, source).is_some()
            {
                return Err(Error::Invalid("Retained source does not match the approved transfer or lacks recycling access.".into()));
            }
        }
        if retained.len() != plan.files.len() {
            return Err(Error::Invalid(
                "Incomplete retained transfer selection.".into(),
            ));
        }
        Self::prepare_with_origin(plan, journal, cancel, apartment, origin, Some(retained))
    }

    fn prepare_with_origin(
        plan: Plan,
        journal: &mut Journal,
        cancel: &AtomicBool,
        apartment: &ShellApartment,
        origin: Option<&crate::cleanup::journal::AgentExecutionRef<'_>>,
        mut sources: Option<std::collections::HashMap<NodeId, VerifiedFile>>,
    ) -> Result<Self> {
        plan.validate_retained()?;
        plan.validate_protections()?;
        if plan.files.is_empty() || plan.files.len() > 500 {
            return Err(Error::Invalid("Invalid recycling batch size.".into()));
        }
        let keepers = if plan.retained.is_empty() || cancel.load(Ordering::Relaxed) {
            None
        } else {
            Some(plan.verify_retained(cancel)?)
        };
        let items = plan
            .preview
            .items
            .iter()
            .map(|item| ItemResult {
                id: item.id,
                path: item.path.clone(),
                outcome: Outcome::NotStarted,
                recorded: false,
                operation: None,
                bin_path: None,
                error: None,
                cleanup_error: None,
            })
            .collect();
        let mut batch = Self {
            _plan: plan,
            keepers,
            prepared: Vec::new(),
            report: Report {
                items,
                cancelled: false,
                journal_error: None,
                helper_error: None,
                preflight_error: None,
            },
        };
        for (row, file) in batch._plan.files.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                batch.report.cancelled = true;
                break;
            }
            let mut pending = None;
            let prepared = (|| {
                check_keepers(&batch.keepers)?;
                let mut source = match &mut sources {
                    Some(sources) => sources.remove(&file.id).ok_or_else(|| {
                        Error::Invalid("Missing retained transfer source.".into())
                    })?,
                    None => batch._plan.verify(file.id)?,
                };
                if let Some(digest) = &file.digest {
                    source.check_digest(digest, cancel)?;
                }
                source.check_recycling_access()?;
                let parent = source.stage_parent()?;
                let entry = StageEntry::parse(
                    file.path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(|| Error::Invalid("Inexact recycling filename.".into()))?,
                )?;
                let name = StageName::generate()?;
                let relocation = journal.prepare_relocation_with_origin(
                    &batch._plan,
                    file.id,
                    &source,
                    &name,
                    origin,
                )?;
                batch.report.items[row].operation = Some(relocation.value());
                pending = Some(Prepared {
                    row,
                    relocation,
                    source: Some(source),
                    parent,
                    stage: None,
                    name,
                    entry,
                    phase: Phase::Ready,
                });
                let item = pending
                    .as_mut()
                    .ok_or_else(|| Error::Invalid("Missing prepared source.".into()))?;
                item.stage = Some(StagePin::create(&item.parent, &item.name)?);
                let stage = item
                    .stage
                    .as_ref()
                    .ok_or_else(|| Error::Invalid("Missing protected stage.".into()))?;
                journal.relocation_stage_ready(relocation, stage.identity())?;
                apartment.check_stage(stage)?;
                Ok::<_, Error>(())
            })();
            match prepared {
                Ok(()) => batch
                    .prepared
                    .push(pending.ok_or_else(|| Error::Invalid("Missing prepared item.".into()))?),
                Err(error) => {
                    batch.report.items[row].error = Some(error.to_string());
                    if let Some(item) = pending {
                        cancel_item(item, journal, &mut batch.report);
                    }
                    if batch.report.journal_error.is_some() {
                        break;
                    }
                }
            }
        }
        Ok(batch)
    }

    /// Only valid before admission. Source files remain untouched; owned stages are removed by identity.
    pub fn abort(
        mut self,
        journal: &mut Journal,
        cancel: &AtomicBool,
        mut progress: impl FnMut(usize),
    ) -> Report {
        self.report.cancelled |= cancel.load(Ordering::Relaxed);
        if !self.report.cancelled && !self.ready_for_admission() {
            self.report.preflight_error = Some(
                "The complete selection did not pass preflight. No source was admitted.".into(),
            );
        }
        for item in self.prepared {
            cancel_item(item, journal, &mut self.report);
        }
        progress(self.report.items.len());
        self.report
    }

    /// Consumes one authenticated helper session and closes it before any recycling or return work.
    pub fn execute(
        self,
        journal: &mut Journal,
        cancel: &AtomicBool,
        apartment: &ShellApartment,
        session: Session,
        progress: impl FnMut(usize),
    ) -> Report {
        self.execute_guarded(journal, cancel, apartment, session, progress, || Ok(()))
    }

    /// Additional live authority is checked before admission and before recycling each source.
    pub fn execute_guarded(
        mut self,
        journal: &mut Journal,
        cancel: &AtomicBool,
        apartment: &ShellApartment,
        mut session: Session,
        mut progress: impl FnMut(usize),
        mut check_authority: impl FnMut() -> Result<()>,
    ) -> Report {
        if let Err(error) = self
            ._plan
            .validate_protections()
            .and_then(|()| check_authority())
        {
            self.report.preflight_error = Some(error.to_string());
        }
        if !self.ready_for_admission()
            || self.report.preflight_error.is_some()
            || cancel.load(Ordering::Relaxed)
        {
            if let Err(error) = session.finish() {
                self.report.helper_error = Some(error.to_string());
            }
            return self.abort(journal, cancel, progress);
        }
        for item in &mut self.prepared {
            if cancel.load(Ordering::Relaxed) || self.report.journal_error.is_some() {
                break;
            }
            let source = item
                .source
                .as_ref()
                .expect("Prepared source is retained until finalization");
            let stage = item
                .stage
                .as_ref()
                .expect("Prepared stage exists before admission");
            if let Err(error) = self
                ._plan
                .validate_protections()
                .and_then(|()| check_keepers(&self.keepers))
                .and_then(|()| source.check_stamp())
                .and_then(|()| check_authority())
            {
                self.report.items[item.row].error = Some(error.to_string());
                break;
            }
            if let Err(error) = journal.start_relocation(item.relocation) {
                self.report.journal_error = Some(error.to_string());
                break;
            }
            let result = session.admit(
                source,
                &item.parent,
                &item.name,
                stage.identity(),
                &item.entry,
            );
            match result {
                Ok(reply) if reply.admitted => {
                    if let Err(error) = stage
                        .verify_source(&source.file, &item.entry)
                        .map_err(Error::from)
                        .and_then(|_| check_content(&source.file, &source.stamp))
                    {
                        unresolved(item, journal, &mut self.report, error.to_string());
                        break;
                    }
                    item.phase = Phase::Staged;
                    if let Err(error) = journal.record_staged(item.relocation) {
                        self.report.items[item.row].error = Some(error.to_string());
                        self.report.journal_error = Some(error.to_string());
                        item.phase = Phase::Unknown;
                        self.report.items[item.row].outcome = Outcome::Unknown;
                        break;
                    }
                }
                Ok(reply) => {
                    if let Err(error) = verify_original(source) {
                        unresolved(item, journal, &mut self.report, error.to_string());
                        break;
                    }
                    item.phase = Phase::Unmoved;
                    let row = &mut self.report.items[item.row];
                    row.outcome = Outcome::Unmoved;
                    row.error = reply.message;
                    match journal.record_unmoved(item.relocation) {
                        Ok(()) => row.recorded = true,
                        Err(error) => {
                            self.report.journal_error = Some(error.to_string());
                            break;
                        }
                    }
                }
                Err(error) => {
                    unresolved(item, journal, &mut self.report, error.to_string());
                    break;
                }
            }
        }
        if let Err(error) = session.finish() {
            self.report.helper_error = Some(error.to_string());
        }
        let mut stop = self.report.journal_error.is_some()
            || self.report.helper_error.is_some()
            || self
                .prepared
                .iter()
                .any(|item| item.phase == Phase::Unknown);
        let mut completed = self.report.items.len() - self.prepared.len();
        for mut item in self.prepared {
            self.report.cancelled |= cancel.load(Ordering::Relaxed);
            match item.phase {
                Phase::Ready => cancel_item(item, journal, &mut self.report),
                Phase::Unmoved => cleanup(item, &mut self.report),
                Phase::Unknown => {}
                Phase::Staged => {
                    if let Err(error) = check_authority() {
                        stop = true;
                        self.report.cancelled |= cancel.load(Ordering::Relaxed);
                        self.report.items[item.row].error = Some(error.to_string());
                    }
                    if let Err(error) = self._plan.validate_protections() {
                        stop = true;
                        self.report.items[item.row].error = Some(error.to_string());
                    }
                    if let Err(error) = check_keepers(&self.keepers) {
                        stop = true;
                        self.report.items[item.row].error = Some(error.to_string());
                    }
                    if stop || self.report.cancelled {
                        return_source(item, journal, &mut self.report);
                    } else {
                        let stage = item.stage.as_ref().expect("Admitted stage remains pinned");
                        let source = item
                            .source
                            .take()
                            .expect("Admitted source remains retained");
                        let entry =
                            StageEntry::parse(item.entry.as_str()).expect("Validated stage entry");
                        match source.prepare_recycling(stage, entry) {
                            Err(failure) => {
                                self.report.items[item.row].error = Some(failure.error.to_string());
                                item.source = Some(failure.source);
                                stop = true;
                                return_source(item, journal, &mut self.report);
                            }
                            Ok(mut staged) => {
                                let result = staged.recycle_guarded(
                                    apartment,
                                    journal,
                                    item.relocation,
                                    &mut check_authority,
                                );
                                self.report.cancelled |= cancel.load(Ordering::Relaxed);
                                let mut failure = None;
                                match result {
                                    Ok(recycled) => {
                                        let row = &mut self.report.items[item.row];
                                        row.outcome = Outcome::Recycled;
                                        row.recorded = recycled.recorded();
                                        row.bin_path = Some(
                                            recycled.locator().as_os_str().encode_wide().collect(),
                                        );
                                        if let Some(error) = recycled.journal_error() {
                                            row.error = Some(error.into());
                                            self.report.journal_error = Some(error.into());
                                            stop = true;
                                        }
                                    }
                                    Err(error) => {
                                        failure = Some(error.to_string());
                                        stop = true;
                                    }
                                }
                                if let Some(message) = failure {
                                    if staged.attempted {
                                        drop(staged);
                                        unresolved(&mut item, journal, &mut self.report, message);
                                    } else {
                                        match staged.into_verified() {
                                            Ok(source) => {
                                                item.source = Some(source);
                                                self.report.items[item.row].error = Some(message);
                                                return_source(item, journal, &mut self.report);
                                            }
                                            Err(error) => unresolved(
                                                &mut item,
                                                journal,
                                                &mut self.report,
                                                format!("{message}; rollback guard: {error}"),
                                            ),
                                        }
                                    }
                                } else {
                                    drop(staged);
                                    cleanup(item, &mut self.report);
                                }
                            }
                        }
                    }
                }
            }
            completed += 1;
            progress(completed);
            stop |= self.report.journal_error.is_some();
        }
        self.report
    }
}

fn check_keepers(keepers: &Option<RetainedCopies>) -> Result<()> {
    if let Some(keepers) = keepers {
        for file in &keepers._files {
            file.check_stamp()?;
        }
    }
    Ok(())
}

fn verify_original(source: &VerifiedFile) -> Result<()> {
    source.check_stamp()?;
    let parent = source
        .parents
        .last()
        .ok_or_else(|| Error::Invalid("Missing original parent.".into()))?;
    let name = source
        .path
        .file_name()
        .ok_or_else(|| Error::Invalid("Missing original filename.".into()))?;
    let original = cleanup_io::open_component(
        parent,
        name,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )?;
    if file_identity(&original)? != file_identity(&source.file)? {
        return Err(Error::Invalid(
            "Original location no longer contains the retained source.".into(),
        ));
    }
    Ok(())
}

fn cleanup(mut item: Prepared, report: &mut Report) {
    drop(item.source.take());
    if let Some(stage) = item.stage.take() {
        if let Err(error) = stage.remove_empty(&item.name) {
            report.items[item.row].cleanup_error = Some(error.to_string());
        }
    } else {
        report.items[item.row].cleanup_error =
            Some("Stage creation was not verified; its recorded name requires review.".into());
    }
}

fn cancel_item(item: Prepared, journal: &mut Journal, report: &mut Report) {
    match journal.cancel_relocation(item.relocation) {
        Ok(()) => report.items[item.row].recorded = true,
        Err(error) => report.journal_error = Some(error.to_string()),
    }
    cleanup(item, report);
}

fn unresolved(item: &mut Prepared, journal: &mut Journal, report: &mut Report, message: String) {
    item.phase = Phase::Unknown;
    let row = &mut report.items[item.row];
    row.outcome = Outcome::Unknown;
    row.error = Some(message.clone());
    match journal.unresolved_relocation(item.relocation, &message) {
        Ok(()) => row.recorded = true,
        Err(error) => report.journal_error = Some(error.to_string()),
    }
}

fn return_source(mut item: Prepared, journal: &mut Journal, report: &mut Report) {
    let result = (|| {
        journal.start_return(item.relocation)?;
        item.source
            .as_ref()
            .ok_or_else(|| Error::Invalid("Missing rollback source.".into()))?
            .return_from_stage(
                item.stage
                    .as_mut()
                    .ok_or_else(|| Error::Invalid("Missing rollback stage.".into()))?,
                &item.entry,
                &item.entry,
            )?;
        verify_returned(
            item.source
                .as_ref()
                .ok_or_else(|| Error::Invalid("Missing returned source.".into()))?,
        )?;
        journal.record_returned(item.relocation)
    })();
    match result {
        Ok(()) => {
            let row = &mut report.items[item.row];
            row.outcome = Outcome::Returned;
            row.recorded = true;
            cleanup(item, report);
        }
        Err(error) => unresolved(&mut item, journal, report, error.to_string()),
    }
}

fn verify_returned(source: &VerifiedFile) -> Result<()> {
    check_content(&source.file, &source.stamp)?;
    let parent = source
        .parents
        .last()
        .ok_or_else(|| Error::Invalid("Missing return parent.".into()))?;
    let file = cleanup_io::open_component(
        parent,
        source
            .path
            .file_name()
            .ok_or_else(|| Error::Invalid("Missing return filename.".into()))?,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )?;
    if file_identity(&file)? != file_identity(&source.file)? {
        return Err(Error::Invalid("Return identity was not preserved.".into()));
    }
    Ok(())
}
