use std::{
    sync::{Arc, RwLock, atomic::AtomicBool},
    time::Instant,
};
use voldiq_core::{
    NodeId, ScanOptions,
    duplicates::{HashCache, find},
    scanner::scan,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    for i in 0..200 {
        let mut bytes = vec![7; 64 * 1024];
        bytes[8192] = (i % 4) as u8;
        std::fs::write(fixture.path().join(format!("{i}.bin")), bytes)?;
    }
    let index = Arc::new(RwLock::new(scan(
        fixture.path(),
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |_| {},
    )?));
    let mut cache = HashCache::default();
    let policy_state = tempfile::tempdir()?;
    let protections = voldiq_core::metadata::Store::open(policy_state.path())?.protections()?;
    println!("run,groups,files,errors,elapsed_ms,bytes_read,cache_hits");
    for run in 1..=2 {
        let start = Instant::now();
        let result = find(
            &index,
            NodeId(0),
            0,
            &AtomicBool::new(false),
            &mut cache,
            |_| {},
            &protections,
        )?;
        let files: usize = result.groups.iter().map(|group| group.members.len()).sum();
        assert_eq!((result.groups.len(), files, result.errors), (4, 200, 0));
        if run == 2 {
            assert_eq!((result.bytes_read, result.cache_hits), (0, 200));
        }
        println!(
            "{run},{},{files},{},{:.3},{},{}",
            result.groups.len(),
            result.errors,
            start.elapsed().as_secs_f64() * 1000.,
            result.bytes_read,
            result.cache_hits
        );
    }
    Ok(())
}
