use std::{sync::atomic::AtomicBool, time::Instant};
use voldiq_core::{NodeId, ScanOptions, index::Query, scanner::scan};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "10000".into())
        .parse()?;
    if !(100..=1_000_000).contains(&count) {
        return Err("Fixture requires 100 to 1,000,000 files".into());
    }
    let root = tempfile::tempdir()?;
    check_space(root.path(), count)?;
    eprintln!("Creating {count} real files in {}", root.path().display());
    for i in 0..100 {
        std::fs::create_dir(root.path().join(format!("folder-{i:03}")))?;
    }
    for i in 0..count {
        std::fs::write(
            root.path()
                .join(format!("folder-{:03}/file-{i:08}.bin", i % 100)),
            [i as u8; 128],
        )?;
        if (i + 1) % 100_000 == 0 {
            eprintln!("Created {} files", i + 1);
        }
    }
    eprintln!(
        "Newly written fixture; warm metadata, default directory concurrency of one. Memory describes this scanner process only."
    );
    println!(
        "run,files,entries,errors,logical,allocated,scan_ms,root_page_ms,global_search_ms,node_struct_bytes,private_bytes,working_set_bytes,process_peak_working_set_bytes"
    );
    for run in 1..=5 {
        let start = Instant::now();
        let index = scan(
            root.path(),
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if index.entries[0].files != count as u64
            || index.entries[0].logical != (count * 128) as u64
            || index.entries.len() != count + 101
            || index.entries[0].incomplete
            || !index.entries[0].allocation_known
            || index.error_count != 0
        {
            return Err("Fixture totals did not match the expected values".into());
        }
        let start = Instant::now();
        let page = index.query(&Query {
            parent: Some(NodeId(0)),
            limit: 100,
            ..Query::default()
        })?;
        let query = start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(page.total, 100);
        let start = Instant::now();
        let results = index.query(&Query {
            text: "file-0000".into(),
            files_only: true,
            limit: 100,
            ..Query::default()
        })?;
        let search = start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(results.total, count.min(10_000));
        assert_eq!(results.items.len(), count.min(100));
        let memory = process_memory()?;
        println!(
            "{run},{count},{},{},{},{},{elapsed:.3},{query:.3},{search:.3},{},{},{},{}",
            index.entries.len(),
            index.error_count,
            index.entries[0].logical,
            index.entries[0].allocated,
            std::mem::size_of::<voldiq_core::Entry>(),
            memory.0,
            memory.1,
            memory.2
        );
    }
    eprintln!("Removing the owned fixture directory");
    root.close()?;
    Ok(())
}

#[cfg(windows)]
fn check_space(path: &std::path::Path, count: usize) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0;
    if unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    // Reserve is a safety allowance, not a prediction of NTFS fixture allocation.
    let reserve = count as u64 * 8192 + 512 * 1024 * 1024;
    if available < reserve {
        return Err(std::io::Error::other(
            "Insufficient space for the fixture safety reserve",
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn check_space(_: &std::path::Path, _: usize) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "Run this fixture on Windows for allocation and memory measurements",
    ))
}

#[cfg(windows)]
fn process_memory() -> std::io::Result<(usize, usize, usize)> {
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess,
    };
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    if unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            memory.cb,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok((
        memory.PrivateUsage,
        memory.WorkingSetSize,
        memory.PeakWorkingSetSize,
    ))
}

#[cfg(not(windows))]
fn process_memory() -> std::io::Result<(usize, usize, usize)> {
    Err(std::io::Error::other(
        "Windows memory counters are unavailable",
    ))
}
