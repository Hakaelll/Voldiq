use super::{
    Plan,
    journal::{Journal, Outcome},
};
use crate::{Error, NodeId, Result};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Serialize)]
pub struct ItemResult {
    pub id: NodeId,
    pub path: String,
    pub removed: bool,
    pub error: Option<String>,
    pub recorded: bool,
}

#[derive(Serialize)]
pub struct Report {
    pub items: Vec<ItemResult>,
    pub cancelled: bool,
    pub journal_error: Option<String>,
}

/// The caller must consume an explicitly confirmed, current-index permanent-delete plan once.
pub fn permanent(
    plan: Plan,
    journal: &mut Journal,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize),
) -> Result<Report> {
    if !plan.preview.retained.is_empty() {
        return Err(Error::Invalid(
            "Duplicate execution requires retained-copy authority.".into(),
        ));
    }
    let operation = journal.prepare_permanent(&plan)?;
    let mut report = Report {
        items: Vec::new(),
        cancelled: false,
        journal_error: None,
    };
    for file in &plan.files {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        if let Err(error) = journal.start_item(operation, file.id) {
            report.journal_error = Some(error.to_string());
            return Ok(report);
        }
        let result = plan
            .verify(file.id)
            .and_then(|verified| verified.remove_permanently());
        let error = result.err().map(|error| error.to_string());
        let recorded = journal.complete_item(
            operation,
            file.id,
            match &error {
                None => Outcome::Succeeded,
                Some(message) => Outcome::Failed(message.clone()),
            },
        );
        report.items.push(ItemResult {
            id: file.id,
            path: file.path.to_string_lossy().into_owned(),
            removed: error.is_none(),
            error,
            recorded: recorded.is_ok(),
        });
        progress(report.items.len());
        if let Err(error) = recorded {
            report.journal_error = Some(error.to_string());
            return Ok(report);
        }
    }
    if let Err(error) = journal
        .cancel_pending(operation)
        .and_then(|()| journal.finish(operation))
    {
        report.journal_error = Some(error.to_string());
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
