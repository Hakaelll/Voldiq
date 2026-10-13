use crate::Engine;
use serde::Deserialize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::State;
use voldiq_core::{
    NodeId,
    index::{Details, FileTypes, Index, Page, Query},
};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QueryFamily {
    FileList,
    OverviewChildren,
    Details,
}

#[derive(Default)]
struct Slot {
    latest: u64,
    active: Option<Arc<AtomicBool>>,
}

#[derive(Default)]
pub(crate) struct Queries {
    state: Mutex<Session>,
}

#[derive(Default)]
struct Session {
    epoch: u64,
    slots: [Slot; 3],
}

impl QueryFamily {
    fn slot(self) -> usize {
        match self {
            Self::FileList => 0,
            Self::OverviewChildren => 1,
            Self::Details => 2,
        }
    }
}

fn validate_id(id: u64) -> Result<(), String> {
    if id == 0 || id > 9_007_199_254_740_991 {
        return Err("Invalid query request ID.".into());
    }
    Ok(())
}

impl Queries {
    fn open_session(&self) -> Result<u64, String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        if state.epoch >= 9_007_199_254_740_991 {
            return Err("Query session limit reached.".into());
        }
        for slot in &mut state.slots {
            if let Some(active) = slot.active.take() {
                active.store(true, Ordering::Relaxed);
            }
            slot.latest = 0;
        }
        state.epoch += 1;
        Ok(state.epoch)
    }

    fn begin(
        &self,
        epoch: u64,
        family: QueryFamily,
        id: u64,
    ) -> Result<Option<Arc<AtomicBool>>, String> {
        validate_id(id)?;
        validate_id(epoch)?;
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        if epoch != state.epoch {
            return Ok(None);
        }
        let slot = &mut state.slots[family.slot()];
        if id <= slot.latest {
            return Ok(None);
        }
        if let Some(previous) = slot.active.take() {
            previous.store(true, Ordering::Relaxed);
        }
        slot.latest = id;
        let cancelled = Arc::new(AtomicBool::new(false));
        slot.active = Some(cancelled.clone());
        Ok(Some(cancelled))
    }

    fn cancel(&self, epoch: u64, family: QueryFamily, id: u64) -> Result<(), String> {
        validate_id(id)?;
        validate_id(epoch)?;
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        if epoch != state.epoch {
            return Ok(());
        }
        let slot = &mut state.slots[family.slot()];
        if id >= slot.latest {
            slot.latest = id;
            if let Some(active) = slot.active.take() {
                active.store(true, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    fn finish(&self, epoch: u64, family: QueryFamily, id: u64) {
        if let Ok(mut state) = self.state.lock() {
            if epoch != state.epoch {
                return;
            }
            let slot = &mut state.slots[family.slot()];
            if slot.latest == id {
                slot.active = None;
            }
        }
    }
}

struct Request {
    engine: Arc<Engine>,
    epoch: u64,
    family: QueryFamily,
    id: u64,
}

impl Drop for Request {
    fn drop(&mut self) {
        self.engine.queries.finish(self.epoch, self.family, self.id);
    }
}

#[derive(serde::Serialize)]
pub(crate) struct Overview {
    directory: Details,
    children: Page,
    types: FileTypes,
}

fn overview(index: &Index, id: NodeId, cancelled: &AtomicBool) -> Result<Option<Overview>, String> {
    let Some(children) = index
        .query_cancellable(
            &Query {
                parent: Some(id),
                limit: 250,
                sort: "size".into(),
                ..Query::default()
            },
            || cancelled.load(Ordering::Relaxed),
        )
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let Some(types) = index
        .file_types_cancellable(id, || cancelled.load(Ordering::Relaxed))
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let Some(directory) = index
        .details_cancellable(id, || cancelled.load(Ordering::Relaxed))
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some(Overview {
        directory,
        children,
        types,
    }))
}

async fn run_query<T: Send + 'static>(
    epoch: u64,
    family: QueryFamily,
    request_id: u64,
    engine: Arc<Engine>,
    execute: impl FnOnce(&Index, &AtomicBool) -> Result<Option<T>, String> + Send + 'static,
) -> Result<Option<T>, String> {
    let Some(cancelled) = engine.queries.begin(epoch, family, request_id)? else {
        return Ok(None);
    };
    let request = Request {
        engine: engine.clone(),
        epoch,
        family,
        id: request_id,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _request = request;
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let index = _request.engine.index()?;
        let index = index.read().map_err(|e| e.to_string())?;
        execute(&index, &cancelled)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn query_overview(
    epoch: u64,
    id: NodeId,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<Overview>, String> {
    run_query(
        epoch,
        QueryFamily::OverviewChildren,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| overview(index, id, cancelled),
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_details_cancellable(
    epoch: u64,
    id: NodeId,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<Details>, String> {
    run_query(
        epoch,
        QueryFamily::Details,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| {
            index
                .details_cancellable(id, || cancelled.load(Ordering::Relaxed))
                .map_err(|error| error.to_string())
        },
    )
    .await
}

#[tauri::command]
pub(crate) fn open_query_session(engine: State<'_, Arc<Engine>>) -> Result<u64, String> {
    engine.queries.open_session()
}

#[tauri::command]
pub(crate) fn cancel_query(
    epoch: u64,
    family: QueryFamily,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    engine.queries.cancel(epoch, family, request_id)
}

#[tauri::command]
pub(crate) async fn query_files_cancellable(
    epoch: u64,
    query: Query,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<Page>, String> {
    run_query(
        epoch,
        QueryFamily::FileList,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| {
            index
                .query_cancellable(&query, || cancelled.load(Ordering::Relaxed))
                .map_err(|error| error.to_string())
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_returns_one_real_index_summary_or_no_cancelled_payload() {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::create_dir(fixture.path().join("folder")).unwrap();
        std::fs::write(fixture.path().join("folder/data.txt"), [1; 7]).unwrap();
        std::fs::write(fixture.path().join("README"), [1; 3]).unwrap();
        let cancelled = AtomicBool::new(false);
        let index = voldiq_core::scanner::scan(
            fixture.path(),
            &voldiq_core::ScanOptions::default(),
            &cancelled,
            |_| {},
        )
        .unwrap();
        let result = overview(&index, NodeId(0), &cancelled).unwrap().unwrap();
        assert_eq!(result.directory.entry.logical, 10);
        assert_eq!(result.children.total, 2);
        assert_eq!(result.children.items[0].name, "folder");
        assert_eq!(result.children.items[0].logical, 7);
        assert_eq!(result.types.items.len(), 2);
        assert_eq!(
            result
                .types
                .items
                .iter()
                .map(|item| item.bytes)
                .sum::<u64>(),
            10
        );
        assert_eq!(result.types.total_bytes, 10);
        assert_eq!(result.types.total_files, 2);
        let payload = serde_json::to_value(result).unwrap();
        assert_eq!(payload["directory"]["entry"]["id"], 0);
        assert_eq!(payload["children"]["items"][0]["name"], "folder");
        assert_eq!(payload["types"]["items"][0]["extension"], "txt");
        assert_eq!(payload["types"]["total_bytes"], 10);
        assert_eq!(payload["types"]["total_files"], 2);
        cancelled.store(true, Ordering::Relaxed);
        assert!(
            overview(&index, NodeId(u32::MAX), &cancelled)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn extensionless_ipc_uses_a_boolean_filter_and_nullable_type_key() {
        assert!(
            !serde_json::from_str::<Query>("{}")
                .unwrap()
                .without_extension
        );
        assert!(
            serde_json::from_str::<Query>(r#"{"without_extension":true}"#)
                .unwrap()
                .without_extension
        );
        assert!(serde_json::from_str::<Query>(r#"{"without_extension":"true"}"#).is_err());
        let missing = serde_json::to_value(voldiq_core::index::FileType {
            extension: None,
            bytes: 65,
            count: 4,
        })
        .unwrap();
        assert!(missing["extension"].is_null());
        let named = serde_json::to_value(voldiq_core::index::FileType {
            extension: Some("no extension".into()),
            bytes: 43,
            count: 1,
        })
        .unwrap();
        assert_eq!(named["extension"], "no extension");
    }

    #[test]
    fn frontend_reload_cancels_old_work_and_rejects_old_session_messages() {
        let queries = Queries::default();
        let previous = queries.open_session().unwrap();
        let old = queries
            .begin(previous, QueryFamily::FileList, 500)
            .unwrap()
            .unwrap();
        let current = queries.open_session().unwrap();
        assert!(old.load(Ordering::Relaxed));
        let new = queries
            .begin(current, QueryFamily::FileList, 1)
            .unwrap()
            .unwrap();
        queries.finish(previous, QueryFamily::FileList, 1);
        queries
            .cancel(previous, QueryFamily::FileList, 1000)
            .unwrap();
        assert!(!new.load(Ordering::Relaxed));
        assert!(
            queries
                .begin(previous, QueryFamily::FileList, 1001)
                .unwrap()
                .is_none()
        );
        queries.cancel(current, QueryFamily::FileList, 1).unwrap();
        assert!(new.load(Ordering::Relaxed));
    }

    #[test]
    fn supersession_completion_and_cancellation_preserve_newer_ownership() {
        let queries = Queries::default();
        assert_eq!(queries.open_session().unwrap(), 1);
        let first = queries.begin(1, QueryFamily::FileList, 1).unwrap().unwrap();
        let second = queries.begin(1, QueryFamily::FileList, 2).unwrap().unwrap();
        assert!(first.load(Ordering::Relaxed));
        queries.finish(1, QueryFamily::FileList, 1);
        queries.cancel(1, QueryFamily::FileList, 1).unwrap();
        assert!(!second.load(Ordering::Relaxed));
        queries.cancel(1, QueryFamily::FileList, 2).unwrap();
        assert!(second.load(Ordering::Relaxed));
        assert!(
            queries
                .begin(1, QueryFamily::FileList, 2)
                .unwrap()
                .is_none()
        );
        queries.begin(1, QueryFamily::FileList, 3).unwrap().unwrap();
        queries.finish(1, QueryFamily::FileList, 3);
        assert!(
            queries
                .begin(1, QueryFamily::FileList, 3)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn cancellation_before_registration_and_independent_families_are_bounded() {
        let queries = Queries::default();
        assert_eq!(queries.open_session().unwrap(), 1);
        queries.cancel(1, QueryFamily::FileList, 10).unwrap();
        assert!(
            queries
                .begin(1, QueryFamily::FileList, 9)
                .unwrap()
                .is_none()
        );
        assert!(
            queries
                .begin(1, QueryFamily::FileList, 10)
                .unwrap()
                .is_none()
        );
        let overview = queries
            .begin(1, QueryFamily::OverviewChildren, 1)
            .unwrap()
            .unwrap();
        let files = queries
            .begin(1, QueryFamily::FileList, 11)
            .unwrap()
            .unwrap();
        queries.cancel(1, QueryFamily::FileList, 11).unwrap();
        assert!(files.load(Ordering::Relaxed));
        assert!(!overview.load(Ordering::Relaxed));
        for id in [0, 9_007_199_254_740_992, u64::MAX] {
            assert!(queries.begin(1, QueryFamily::FileList, id).is_err());
            assert!(queries.cancel(1, QueryFamily::FileList, id).is_err());
        }
        assert!(serde_json::from_str::<QueryFamily>("\"unknown\"").is_err());
    }

    #[test]
    fn detail_requests_supersede_independently_and_session_reload_cancels_every_family() {
        let queries = Queries::default();
        let epoch = queries.open_session().unwrap();
        let files = queries
            .begin(epoch, QueryFamily::FileList, 1)
            .unwrap()
            .unwrap();
        let overview = queries
            .begin(epoch, QueryFamily::OverviewChildren, 1)
            .unwrap()
            .unwrap();
        let old = queries
            .begin(epoch, QueryFamily::Details, 1)
            .unwrap()
            .unwrap();
        let latest = queries
            .begin(epoch, QueryFamily::Details, 2)
            .unwrap()
            .unwrap();
        assert!(old.load(Ordering::Relaxed));
        queries.finish(epoch, QueryFamily::Details, 1);
        queries.cancel(epoch, QueryFamily::Details, 1).unwrap();
        assert!(!latest.load(Ordering::Relaxed));
        assert!(!files.load(Ordering::Relaxed));
        assert!(!overview.load(Ordering::Relaxed));
        let current = queries.open_session().unwrap();
        assert!(files.load(Ordering::Relaxed));
        assert!(overview.load(Ordering::Relaxed));
        assert!(latest.load(Ordering::Relaxed));
        let reopened = queries
            .begin(current, QueryFamily::Details, 1)
            .unwrap()
            .unwrap();
        queries.cancel(epoch, QueryFamily::Details, 500).unwrap();
        queries.finish(epoch, QueryFamily::Details, 1);
        assert!(!reopened.load(Ordering::Relaxed));
        queries.cancel(current, QueryFamily::Details, 1).unwrap();
        assert!(reopened.load(Ordering::Relaxed));
        assert!(
            queries
                .begin(current, QueryFamily::Details, 1)
                .unwrap()
                .is_none()
        );
    }
}
