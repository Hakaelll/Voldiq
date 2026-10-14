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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    #[ignore = "Synthetic million-entry query measurement; run explicitly with --ignored --nocapture"]
    fn benchmark_million_entry_overview() {
        fn thread_cpu_ticks() -> u64 {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn GetCurrentThread() -> isize;
                fn GetThreadTimes(
                    thread: isize,
                    created: *mut u32,
                    exited: *mut u32,
                    kernel: *mut u32,
                    user: *mut u32,
                ) -> i32;
            }
            let mut created = [0_u32; 2];
            let mut exited = [0_u32; 2];
            let mut kernel = [0_u32; 2];
            let mut user = [0_u32; 2];
            assert_ne!(
                unsafe {
                    GetThreadTimes(
                        GetCurrentThread(),
                        created.as_mut_ptr(),
                        exited.as_mut_ptr(),
                        kernel.as_mut_ptr(),
                        user.as_mut_ptr(),
                    )
                },
                0
            );
            ((u64::from(kernel[1]) << 32) | u64::from(kernel[0]))
                + ((u64::from(user[1]) << 32) | u64::from(user[0]))
        }
        let fixture = tempfile::tempdir().unwrap();
        let cancelled = AtomicBool::new(false);
        let mut index = voldiq_core::scanner::scan(
            fixture.path(),
            &voldiq_core::ScanOptions::default(),
            &cancelled,
            |_| {},
        )
        .unwrap();
        let template = index.entries[0].clone();
        for id in 1..=16_u32 {
            let mut entry = template.clone();
            entry.id = NodeId(id);
            entry.parent = Some(NodeId(0));
            entry.name = format!("folder-{id}");
            entry.logical = 0;
            entry.allocated = 0;
            entry.files = 0;
            index.entries.push(entry);
        }
        for id in 17..1_000_017_u32 {
            let parent = 1 + (id - 17) % 16;
            let mut entry = template.clone();
            entry.id = NodeId(id);
            entry.parent = Some(NodeId(parent));
            entry.kind = voldiq_core::Kind::File;
            entry.file_state = None;
            entry.name = format!("file-{id:08}.dat");
            entry.logical = 1;
            entry.allocated = 1;
            entry.files = 1;
            index.entries[parent as usize].logical += 1;
            index.entries[parent as usize].allocated += 1;
            index.entries[parent as usize].files += 1;
            index.entries.push(entry);
        }
        index.entries[0].logical = 1_000_000;
        index.entries[0].allocated = 1_000_000;
        index.entries[0].files = 1_000_000;
        let children = index
            .query_cancellable(
                &Query {
                    parent: Some(NodeId(0)),
                    limit: 250,
                    ..Query::default()
                },
                || false,
            )
            .unwrap()
            .unwrap();
        let verify_branches = |branches: &[MapBranch]| {
            assert_eq!(branches.len(), 12);
            assert_eq!(
                branches
                    .iter()
                    .map(|branch| branch.items.len())
                    .sum::<usize>(),
                96
            );
            for branch in branches {
                assert_eq!(
                    branch
                        .items
                        .iter()
                        .map(|item| item.id.0)
                        .collect::<Vec<_>>(),
                    (0..8)
                        .map(|position| 16 + branch.parent.0 + position * 16)
                        .collect::<Vec<_>>()
                );
                assert!(
                    branch
                        .items
                        .iter()
                        .all(|item| item.parent == Some(branch.parent) && item.logical == 1)
                );
            }
        };
        println!(
            "SYNTHETIC_OVERVIEW_SCOPE,debug_profile,memory_index,no_disk_scan,no_ipc,no_canvas,no_whole_app_measurement"
        );
        println!(
            "run,query,milliseconds,thread_cpu_milliseconds,index_entries,branches,children,errors"
        );
        for run in 1..=5 {
            let cases = if run % 2 == 0 {
                ["branches", "overview"]
            } else {
                ["overview", "branches"]
            };
            for case in cases {
                let cpu_before = thread_cpu_ticks();
                let started = std::time::Instant::now();
                if case == "overview" {
                    let result = overview(&index, NodeId(0), &cancelled).unwrap().unwrap();
                    let elapsed = started.elapsed();
                    let cpu_ms = (thread_cpu_ticks() - cpu_before) as f64 / 10_000.0;
                    assert_eq!(result.directory.entry.logical, 1_000_000);
                    assert_eq!(result.types.total_bytes, 1_000_000);
                    assert_eq!(result.types.total_files, 1_000_000);
                    verify_branches(&result.branches);
                    println!(
                        "{run},{case},{:.3},{cpu_ms:.3},1000017,12,96,0",
                        elapsed.as_secs_f64() * 1000.0
                    );
                } else {
                    let branches = map_branches(&index, &children, &cancelled).unwrap();
                    let elapsed = started.elapsed();
                    let cpu_ms = (thread_cpu_ticks() - cpu_before) as f64 / 10_000.0;
                    verify_branches(&branches);
                    println!(
                        "{run},{case},{:.3},{cpu_ms:.3},1000017,12,96,0",
                        elapsed.as_secs_f64() * 1000.0
                    );
                }
            }
        }
    }

    #[test]
    fn overview_map_branches_are_bounded_ranked_and_direct() {
        let fixture = tempfile::tempdir().unwrap();
        for directory in 0..14 {
            let folder = fixture.path().join(format!("folder{directory}"));
            std::fs::create_dir(&folder).unwrap();
            for size in 1..=10 {
                std::fs::write(folder.join(format!("file{size}")), vec![1; size]).unwrap();
            }
        }
        let cancelled = AtomicBool::new(false);
        let index = voldiq_core::scanner::scan(
            fixture.path(),
            &voldiq_core::ScanOptions::default(),
            &cancelled,
            |_| {},
        )
        .unwrap();
        let result = overview(&index, NodeId(0), &cancelled).unwrap().unwrap();
        assert_eq!(result.branches.len(), 12);
        for branch in &result.branches {
            assert_eq!(branch.items.len(), 8);
            assert_eq!(
                branch.items.iter().map(|e| e.logical).collect::<Vec<_>>(),
                [10, 9, 8, 7, 6, 5, 4, 3]
            );
            assert!(branch.items.iter().all(|e| e.parent == Some(branch.parent)));
            assert_eq!(index.entry(branch.parent).unwrap().logical, 55);
        }
        cancelled.store(true, Ordering::Relaxed);
        assert!(map_branches(&index, &result.children, &cancelled).is_none());
    }

    #[test]
    fn selected_folder_details_rank_only_five_direct_children() {
        let fixture = tempfile::tempdir().unwrap();
        for size in 1..=7 {
            std::fs::write(fixture.path().join(format!("file{size}")), vec![1; size]).unwrap();
        }
        std::fs::create_dir(fixture.path().join("nested")).unwrap();
        std::fs::write(fixture.path().join("nested/data"), [1; 20]).unwrap();
        let cancelled = AtomicBool::new(false);
        let index = voldiq_core::scanner::scan(
            fixture.path(),
            &voldiq_core::ScanOptions::default(),
            &cancelled,
            |_| {},
        )
        .unwrap();
        let result = selected_details(&index, NodeId(0), &cancelled)
            .unwrap()
            .unwrap();
        let children = result.children.as_ref().unwrap();
        assert_eq!(children.total, 8);
        assert_eq!(
            children
                .items
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["nested", "file7", "file6", "file5", "file4"]
        );
        assert_eq!(
            children.items.iter().map(|e| e.logical).collect::<Vec<_>>(),
            [20, 7, 6, 5, 4]
        );
        let payload = serde_json::to_value(&result).unwrap();
        assert_eq!(payload["entry"]["id"], 0);
        assert_eq!(payload["children"]["total"], 8);
        let file = children.items[1].id;
        let query = Query {
            parent: Some(NodeId(0)),
            files_only: true,
            sort: "name".into(),
            offset: 2,
            limit: 2,
            ..Query::default()
        };
        let files = file_page(&index, &query, &cancelled).unwrap().unwrap();
        assert_eq!(files.page.total, 7);
        assert_eq!(files.page.largest, 7);
        assert_eq!(
            files
                .page
                .items
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["file3", "file4"]
        );
        assert_eq!(
            files.paths,
            [
                std::fs::canonicalize(fixture.path())
                    .unwrap()
                    .join("file3")
                    .to_string_lossy(),
                std::fs::canonicalize(fixture.path())
                    .unwrap()
                    .join("file4")
                    .to_string_lossy()
            ]
        );
        let beyond = file_page(
            &index,
            &Query {
                offset: 99,
                ..query
            },
            &cancelled,
        )
        .unwrap()
        .unwrap();
        assert!(beyond.paths.is_empty());
        assert_eq!(beyond.page.largest, 7);
        assert!(
            selected_details(&index, file, &cancelled)
                .unwrap()
                .unwrap()
                .children
                .is_none()
        );
        let nested = children.items[0].id;
        let nested_children = selected_details(&index, nested, &cancelled)
            .unwrap()
            .unwrap()
            .children
            .unwrap();
        assert_eq!(nested_children.total, 1);
        assert_eq!(nested_children.items[0].name, "data");
        cancelled.store(true, Ordering::Relaxed);
        assert!(
            file_page(&index, &Query::default(), &cancelled)
                .unwrap()
                .is_none()
        );
        assert!(
            selected_details(&index, NodeId(u32::MAX), &cancelled)
                .unwrap()
                .is_none()
        );
    }

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
