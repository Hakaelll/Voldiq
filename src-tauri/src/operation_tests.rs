use super::*;

#[test]
fn the_native_guard_holds_shared_exclusion_until_worker_exit_even_after_local_completion() {
    let state = tempfile::tempdir().unwrap();
    let engine = Arc::new(Engine::new(state.path()).unwrap());
    let other = voldiq_core::operations::Coordinator::open(state.path()).unwrap();
    let operation = engine.begin().unwrap();
    assert!(other.acquire().is_err());
    assert!(engine.begin().is_err());
    operation.cancel.store(true, Ordering::Relaxed);
    assert!(other.acquire().is_err());
    *engine.active.lock().unwrap() = None;
    assert!(engine.begin().is_err());
    assert!(other.acquire().is_err());
    drop(operation);
    let next = engine.begin().unwrap();
    assert!(!next.cancel.load(Ordering::Relaxed));
    assert!(other.acquire().is_err());
    drop(next);
    assert!(other.acquire().is_ok());
}
