use crate::analysis_status::IndexId;
use crate::{
    Engine,
    overview::{Overview, overview},
};
use serde::Deserialize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::State;
use voldiq_core::{
    NodeId,
    index::{Details, Index, Page, Query},
};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QueryFamily {
    FileList,
    OverviewChildren,
    Details,
    FolderBudget,
    SnapshotVisual,
    SnapshotChanges,
    SnapshotCapture,
    SnapshotRetention,
    SnapshotCalendar,
    SnapshotCalendarEntries,
    SnapshotDiary,
    SpaceAccount,
    RecoveryCopies,
    HistoryClip,
    Applications,
    CleanupTray,
    FileInspection,
    GitStorage,
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
    slots: [Slot; 18],
}

impl QueryFamily {
    fn slot(self) -> usize {
        match self {
            Self::FileList => 0,
            Self::OverviewChildren => 1,
            Self::Details => 2,
            Self::FolderBudget => 3,
            Self::SnapshotVisual => 4,
            Self::SnapshotChanges => 5,
            Self::SnapshotCapture => 6,
            Self::SnapshotRetention => 7,
            Self::SnapshotCalendar => 8,
            Self::SnapshotCalendarEntries => 9,
            Self::SnapshotDiary => 10,
            Self::SpaceAccount => 11,
            Self::RecoveryCopies => 12,
            Self::HistoryClip => 13,
            Self::Applications => 14,
            Self::CleanupTray => 15,
            Self::FileInspection => 16,
            Self::GitStorage => 17,
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

#[tauri::command]
pub(crate) async fn query_snapshot_calendar(
    snapshot: i64,
    month: String,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::snapshot_commands::SnapshotDatabase>,
) -> Result<Option<voldiq_core::snapshots::CalendarReport>, String> {
    run_stored_query(
        epoch,
        QueryFamily::SnapshotCalendar,
        request_id,
        engine.inner().clone(),
        database.0.clone(),
        move |store, cancel| store.calendar(snapshot, &month, cancel),
    )
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CalendarEntriesQuery {
    snapshot: i64,
    month: String,
    day: u8,
    offset: usize,
    limit: usize,
}

#[tauri::command]
pub(crate) async fn query_snapshot_calendar_entries(
    query: CalendarEntriesQuery,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::snapshot_commands::SnapshotDatabase>,
) -> Result<Option<voldiq_core::snapshots::CalendarPage>, String> {
    run_stored_query(
        epoch,
        QueryFamily::SnapshotCalendarEntries,
        request_id,
        engine.inner().clone(),
        database.0.clone(),
        move |store, cancel| {
            store.calendar_entries(
                query.snapshot,
                &query.month,
                query.day,
                query.offset,
                query.limit,
                cancel,
            )
        },
    )
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiaryQuery {
    snapshot: i64,
    offset: usize,
    limit: usize,
}

#[tauri::command]
pub(crate) async fn query_snapshot_diary(
    query: DiaryQuery,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::snapshot_commands::SnapshotDatabase>,
) -> Result<Option<voldiq_core::snapshots::DiaryPage>, String> {
    run_stored_query(
        epoch,
        QueryFamily::SnapshotDiary,
        request_id,
        engine.inner().clone(),
        database.0.clone(),
        move |store, cancel| store.diary(query.snapshot, query.offset, query.limit, 5, cancel),
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_snapshot_retention(
    snapshot: i64,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::snapshot_commands::SnapshotDatabase>,
) -> Result<Option<voldiq_core::snapshots::RetentionPreview>, String> {
    run_stored_query(
        epoch,
        QueryFamily::SnapshotRetention,
        request_id,
        engine.inner().clone(),
        database.0.clone(),
        move |store, cancel| store.preview_retention(snapshot, cancel),
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_snapshot_capture(
    snapshot: i64,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::snapshot_commands::SnapshotDatabase>,
) -> Result<Option<voldiq_core::snapshots::VisualFrame>, String> {
    run_stored_query(
        epoch,
        QueryFamily::SnapshotCapture,
        request_id,
        engine.inner().clone(),
        database.0.clone(),
        move |store, cancel| store.visual_capture(snapshot, cancel),
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_history_clip(
    request: voldiq_core::snapshots::ExportClipRequest,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::snapshot_commands::SnapshotDatabase>,
) -> Result<Option<voldiq_core::snapshots::ExportClip>, String> {
    run_stored_query(
        epoch,
        QueryFamily::HistoryClip,
        request_id,
        engine.inner().clone(),
        database.0.clone(),
        move |store, cancel| store.export_clip(&request, cancel, |_| {}),
    )
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SnapshotVisualQuery {
    before: i64,
    after: i64,
    scope: Option<String>,
    folder_cursor: Option<String>,
    reverse: bool,
}

#[tauri::command]
pub(crate) async fn query_snapshot_visual(
    query: SnapshotVisualQuery,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::snapshot_commands::SnapshotDatabase>,
) -> Result<Option<voldiq_core::snapshots::VisualPair>, String> {
    run_stored_query(
        epoch,
        QueryFamily::SnapshotVisual,
        request_id,
        engine.inner().clone(),
        database.0.clone(),
        move |store, cancel| {
            store.visual_pair(
                query.before,
                query.after,
                query.scope.as_deref(),
                query.folder_cursor.as_deref(),
                query.reverse,
                cancel,
            )
        },
    )
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SnapshotChangesQuery {
    before: i64,
    after: i64,
    offset: usize,
    limit: usize,
}

#[tauri::command]
pub(crate) async fn query_snapshot_changes(
    query: SnapshotChangesQuery,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::snapshot_commands::SnapshotDatabase>,
) -> Result<Option<voldiq_core::snapshots::Comparison>, String> {
    run_stored_query(
        epoch,
        QueryFamily::SnapshotChanges,
        request_id,
        engine.inner().clone(),
        database.0.clone(),
        move |store, cancel| {
            store.compare_cancellable(query.before, query.after, query.offset, query.limit, cancel)
        },
    )
    .await
}

async fn run_stored_query<T: Send + 'static>(
    epoch: u64,
    family: QueryFamily,
    request_id: u64,
    engine: Arc<Engine>,
    store: Arc<Mutex<voldiq_core::snapshots::Store>>,
    execute: impl FnOnce(&voldiq_core::snapshots::Store, &AtomicBool) -> voldiq_core::Result<Option<T>>
    + Send
    + 'static,
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
        let result = {
            let store = store.lock().map_err(|e| e.to_string())?;
            execute(&store, &cancelled).map_err(|e| e.to_string())?
        };
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

impl Drop for Request {
    fn drop(&mut self) {
        self.engine.queries.finish(self.epoch, self.family, self.id);
    }
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

async fn run_query<T: Send + 'static>(
    index_id: IndexId,
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
        let shared = _request.engine.index_for(index_id)?;
        let result = {
            let index = shared.read().map_err(|error| error.to_string())?;
            execute(&index, &cancelled)?
        };
        if cancelled.load(Ordering::Relaxed)
            || !_request.engine.index_is_current(index_id, &shared)?
        {
            return Ok(None);
        }
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn query_file_inspection(
    index_id: IndexId,
    id: NodeId,
    inspection: voldiq_core::inspection::Request,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<voldiq_core::inspection::Inspection>, String> {
    run_query(
        index_id,
        epoch,
        QueryFamily::FileInspection,
        request_id,
        engine.inner().clone(),
        move |index, cancel| {
            voldiq_core::inspection::inspect(index, id, inspection, cancel)
                .map_err(|error| error.to_string())
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_git_storage(
    index_id: IndexId,
    query: voldiq_core::developer::git::Query,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<voldiq_core::developer::git::Report>, String> {
    run_query(
        index_id,
        epoch,
        QueryFamily::GitStorage,
        request_id,
        engine.inner().clone(),
        move |index, cancel| {
            voldiq_core::developer::git::query(index, &query, cancel)
                .map_err(|error| error.to_string())
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_overview(
    index_id: IndexId,
    epoch: u64,
    id: NodeId,
    cloud: Option<voldiq_core::cloud::CloudFilter>,
    search: Option<Query>,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<Overview>, String> {
    run_query(
        index_id,
        epoch,
        QueryFamily::OverviewChildren,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| overview(index, id, cloud, search.as_ref(), cancelled),
    )
    .await
}

#[tauri::command]
pub(crate) fn inspect_search(query: Query) -> voldiq_core::search::SearchInspection {
    query.inspect()
}

#[tauri::command]
pub(crate) async fn query_folder_budget(
    index_id: IndexId,
    epoch: u64,
    node_id: NodeId,
    detailed: bool,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
    database: State<'_, crate::SnapshotDatabase>,
) -> Result<Option<voldiq_core::budgets::Report>, String> {
    let directory = engine.operations.clone();
    let history = database.0.clone();
    run_query(
        index_id,
        epoch,
        QueryFamily::FolderBudget,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| {
            let history = history.lock().map_err(|e| e.to_string())?;
            voldiq_core::budgets::Store::open(directory.directory())
                .and_then(|store| store.read(index, node_id, &history, detailed, cancelled))
                .map_err(|e| e.to_string())
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_recovery_copies(
    index_id: IndexId,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<voldiq_core::recovery::Report>, String> {
    run_query(
        index_id,
        epoch,
        QueryFamily::RecoveryCopies,
        request_id,
        engine.inner().clone(),
        |index, cancelled| Ok(voldiq_core::recovery::read_desktop(index, cancelled)),
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_space_account(
    index_id: IndexId,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<voldiq_core::space::Report>, String> {
    run_query(
        index_id,
        epoch,
        QueryFamily::SpaceAccount,
        request_id,
        engine.inner().clone(),
        |index, cancelled| {
            voldiq_core::space::read_desktop(index, cancelled).map_err(|error| error.to_string())
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_applications(
    index_id: IndexId,
    query: voldiq_core::applications::Query,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<voldiq_core::applications::Page>, String> {
    run_query(
        index_id,
        epoch,
        QueryFamily::Applications,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| {
            let result = voldiq_core::applications::read_desktop(index, &query, cancelled);
            if cancelled.load(Ordering::Relaxed) {
                Ok(None)
            } else {
                result.map(Some).map_err(|error| error.to_string())
            }
        },
    )
    .await
}

#[tauri::command]
pub(crate) fn translate_search(source: String) -> Result<voldiq_core::search::Translation, String> {
    voldiq_core::search::translate(&source).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn query_cleanup_tray(
    index_id: IndexId,
    groups: Vec<voldiq_core::cleanup::tray::Group>,
    target: Option<voldiq_core::cleanup::tray::Target>,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<voldiq_core::cleanup::tray::Summary>, String> {
    run_query(
        index_id,
        epoch,
        QueryFamily::CleanupTray,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| {
            let result =
                voldiq_core::cleanup::tray::summarize(index, &groups, target.as_ref(), cancelled);
            if cancelled.load(Ordering::Relaxed) {
                Ok(None)
            } else {
                result.map(Some).map_err(|error| error.to_string())
            }
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_application_cache(
    index_id: IndexId,
    request: voldiq_core::applications::review::Request,
    epoch: u64,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<voldiq_core::applications::review::Page>, String> {
    let directory = engine.operations.clone();
    let current = engine.index_for(index_id)?;
    let duplicates = engine
        .duplicates
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .filter(|run| run.index_id == index_id && Arc::ptr_eq(&run.index, &current));
    run_query(
        index_id,
        epoch,
        QueryFamily::Applications,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| {
            let result = (|| {
                let local_data = voldiq_core::windows::local_app_data()?;
                let protections =
                    voldiq_core::metadata::Store::open(directory.directory())?.protections()?;
                let catalog = voldiq_core::applications::installed_programs(cancelled)?;
                voldiq_core::applications::review::query(
                    index,
                    catalog,
                    &request,
                    &voldiq_core::applications::review::Environment {
                        local_data: &local_data,
                        now: voldiq_core::cleanup::rules::now()?,
                        protections: &protections,
                        duplicates: duplicates.as_ref().map(|run| &run.analysis),
                    },
                    cancelled,
                )
            })();
            if cancelled.load(Ordering::Relaxed) {
                Ok(None)
            } else {
                result
                    .map(Some)
                    .map_err(|error: voldiq_core::Error| error.to_string())
            }
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn query_details_cancellable(
    index_id: IndexId,
    epoch: u64,
    id: NodeId,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<SelectedDetails>, String> {
    run_query(
        index_id,
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
    index_id: IndexId,
    epoch: u64,
    query: Query,
    request_id: u64,
    engine: State<'_, Arc<Engine>>,
) -> Result<Option<FilePage>, String> {
    run_query(
        index_id,
        epoch,
        QueryFamily::FileList,
        request_id,
        engine.inner().clone(),
        move |index, cancelled| file_page(index, &query, cancelled),
    )
    .await
}
