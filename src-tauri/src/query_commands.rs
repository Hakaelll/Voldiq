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
    branches: Vec<MapBranch>,
}

#[derive(serde::Serialize)]
pub(crate) struct MapBranch {
    parent: NodeId,
    items: Vec<voldiq_core::Entry>,
}

fn map_branches(index: &Index, children: &Page, cancelled: &AtomicBool) -> Option<Vec<MapBranch>> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let mut groups: std::collections::BTreeMap<u32, Vec<&voldiq_core::Entry>> = children
        .items
        .iter()
        .filter(|entry| entry.kind == voldiq_core::Kind::Directory)
        .take(12)
        .map(|entry| (entry.id.0, Vec::new()))
        .collect();
    if groups.is_empty() {
        return Some(Vec::new());
    }
    for (position, entry) in index.entries.iter().enumerate() {
        if position % 256 == 0 && cancelled.load(Ordering::Relaxed) {
            return None;
        }
        if let Some(items) = entry.parent.and_then(|parent| groups.get_mut(&parent.0)) {
            if items.len() == 8
                && items.last().is_some_and(|last| {
                    entry.logical < last.logical
                        || (entry.logical == last.logical && entry.id.0 >= last.id.0)
                })
            {
                continue;
            }
            items.push(entry);
            items.sort_unstable_by(|a, b| b.logical.cmp(&a.logical).then(a.id.0.cmp(&b.id.0)));
            items.truncate(8);
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    Some(
        groups
            .into_iter()
            .map(|(parent, items)| MapBranch {
                parent: NodeId(parent),
                items: items.into_iter().cloned().collect(),
            })
            .collect(),
    )
}

#[derive(serde::Serialize)]
pub(crate) struct SelectedDetails {
    #[serde(flatten)]
    details: Details,
    children: Option<Page>,
}

#[derive(serde::Serialize)]
pub(crate) struct FilePage {
    #[serde(flatten)]
    page: Page,
    paths: Vec<String>,
}

fn file_page(
    index: &Index,
    query: &Query,
    cancelled: &AtomicBool,
) -> Result<Option<FilePage>, String> {
    let Some(page) = index
        .query_cancellable(query, || cancelled.load(Ordering::Relaxed))
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let mut paths = Vec::with_capacity(page.items.len());
    for item in &page.items {
        let Some(path) = index
            .path_cancellable(item.id, || cancelled.load(Ordering::Relaxed))
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        paths.push(path.to_string_lossy().into_owned());
    }
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some(FilePage { page, paths }))
}

fn selected_details(
    index: &Index,
    id: NodeId,
    cancelled: &AtomicBool,
) -> Result<Option<SelectedDetails>, String> {
    let Some(details) = index
        .details_cancellable(id, || cancelled.load(Ordering::Relaxed))
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let children = if details.entry.kind == voldiq_core::Kind::Directory {
        let Some(page) = index
            .query_cancellable(
                &Query {
                    parent: Some(id),
                    limit: 5,
                    ..Query::default()
                },
                || cancelled.load(Ordering::Relaxed),
            )
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        Some(page)
    } else {
        None
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some(SelectedDetails { details, children }))
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
    let Some(branches) = map_branches(index, &children, cancelled) else {
        return Ok(None);
    };
    Ok(Some(Overview {
        directory,
        children,
        types,
        branches,
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
) -> Result<Option<SelectedDetails>, String> {
    run_query(
        epoch,
        QueryFamily::Details,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| selected_details(index, id, cancelled),
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
) -> Result<Option<FilePage>, String> {
    run_query(
        epoch,
        QueryFamily::FileList,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| file_page(index, &query, cancelled),
    )
    .await
}
