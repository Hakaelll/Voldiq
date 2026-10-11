use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
};
use voldiq_core::{
    NodeId, ScanOptions, ScanPhase,
    index::Query,
    scanner::{SharedIndex, scan_live},
};

#[test]
fn live_queries_see_consistent_totals_and_can_cancel_before_completion() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..8 {
        std::fs::write(dir.path().join(format!("{i}.bin")), [1; 128]).unwrap();
    }
    let published = RefCell::new(None::<SharedIndex>);
    let cancel = AtomicBool::new(false);
    let mut observed_partial = false;
    let index = scan_live(
        dir.path(),
        &ScanOptions::default(),
        &cancel,
        |update| {
            let holder = published.borrow();
            let live = holder.as_ref().unwrap().read().unwrap();
            let page = live
                .query(&Query {
                    parent: Some(NodeId(0)),
                    ..Query::default()
                })
                .unwrap();
            assert_eq!(
                live.entries[0].logical,
                page.items.iter().map(|item| item.logical).sum::<u64>()
            );
            if update.phase == ScanPhase::Scanning && update.entries > 1 {
                assert_eq!(update.logical, live.entries[0].logical);
                assert!(page.total > 0 && page.total < 8);
                observed_partial = true;
                cancel.store(true, Ordering::Relaxed);
            }
        },
        |index| *published.borrow_mut() = Some(index),
    )
    .unwrap();
    assert!(observed_partial);
    let live = index.read().unwrap();
    assert!(live.entries[0].incomplete);
    assert_eq!(live.entries[0].files, 1);
    assert_eq!(live.entries[0].logical, 128);
}
