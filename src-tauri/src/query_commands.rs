use crate::Engine;
use serde::Deserialize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::State;
use voldiq_core::index::{Page, Query};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QueryFamily {
    FileList,
    OverviewChildren,
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
    slots: [Slot; 2],
}

impl QueryFamily {
    fn slot(self) -> usize {
        match self {
            Self::FileList => 0,
            Self::OverviewChildren => 1,
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
    family: QueryFamily,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<Page>, String> {
    let Some(cancelled) = engine.queries.begin(epoch, family, request_id)? else {
        return Ok(None);
    };
    let request = Request {
        engine: engine.inner().clone(),
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
        index
            .query_cancellable(&query, || cancelled.load(Ordering::Relaxed))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
