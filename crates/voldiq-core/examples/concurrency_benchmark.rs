use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use voldiq_core::{ScanConcurrency, ScanMode, ScanOptions, ScanPhase, scanner::scan};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "10000".into())
        .parse()?;
    if !(100..=100_000).contains(&count) {
        return Err("Fixture requires 100 to 100,000 files.".into());
    }
    let cancel_only = match std::env::args().nth(2).as_deref() {
        None => false,
        Some("--cancel-only") => true,
        Some(_) => return Err("Expected optional --cancel-only.".into()),
    };
    let fixture = tempfile::tempdir()?;
    check_space(fixture.path(), count)?;
    let canonical_root = std::fs::canonicalize(fixture.path())?;
    eprintln!("Owned fixture: {}", fixture.path().display());
    for directory in 0..100 {
        std::fs::create_dir(fixture.path().join(format!("folder-{directory:03}")))?;
    }
    for file in 0..count {
        std::fs::write(
            fixture
                .path()
                .join(format!("folder-{:03}/file-{file:08}.bin", file % 100)),
            [0_u8; 128],
        )?;
    }
    eprintln!(
        "Newly created fixture; warm metadata, no content reads. Logical CPUs: {}. Directory mode only.",
        std::thread::available_parallelism()?.get()
    );
    eprintln!(
        "Seek penalty: {:?}; automatic preflight workers: {}.",
        voldiq_core::windows::storage_seek_penalty(fixture.path()),
        ScanConcurrency::automatic().for_path(&canonical_root)
    );
    println!(
        "round,phase,setting,preflight_workers,fixture_files,files,entries,logical,allocated,errors,wall_ms,cpu_ms,cancel_ms"
    );
    let cancel = AtomicBool::new(false);
    if !cancel_only {
        for round in 0..5 {
            let mut order = [Some(1), Some(2), Some(4), Some(8), None];
            order.rotate_left(round % 5);
            for requested in order {
                let concurrency = requested
                    .map(ScanConcurrency::try_from)
                    .transpose()?
                    .unwrap_or(ScanConcurrency::automatic());
                let setting =
                    requested.map_or_else(|| "auto".to_owned(), |workers| workers.to_string());
                let workers = concurrency.for_path(&canonical_root);
                let options = ScanOptions {
                    mode: ScanMode::Directory,
                    concurrency,
                    profile: if requested.is_some() {
                        voldiq_core::ScanProfile::Fast
                    } else {
                        voldiq_core::ScanProfile::Auto
                    },
                    ..ScanOptions::default()
                };
                let cpu = cpu_ms()?;
                let start = Instant::now();
                let index = scan(fixture.path(), &options, &cancel, |_| {})?;
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                let cpu = cpu_ms()? - cpu;
                let root = &index.entries[0];
                if index.entries.len() != count + 101
                    || root.files != count as u64
                    || root.logical != (count * 128) as u64
                    || !root.allocation_known
                    || root.incomplete
                    || index.error_count != 0
                {
                    return Err("Measured scan does not match fixture contents.".into());
                }
                println!(
                    "{},full,{setting},{workers},{count},{},{},{},{},{},{elapsed:.3},{cpu:.3},",
                    round + 1,
                    root.files,
                    index.entries.len(),
                    root.logical,
                    root.allocated,
                    index.error_count
                );
            }
        }
    }
    // Cancellation is a separate workload, so it does not contaminate full-scan timings.
    if count >= 10_000 {
        for round in 0..5 {
            let mut order = [Some(1), Some(2), Some(4), Some(8), None];
            order.rotate_left(round % 5);
            for requested in order {
                let concurrency = requested
                    .map(ScanConcurrency::try_from)
                    .transpose()?
                    .unwrap_or(ScanConcurrency::automatic());
                let setting =
                    requested.map_or_else(|| "auto".to_owned(), |workers| workers.to_string());
                let workers = concurrency.for_path(&canonical_root);
                let options = ScanOptions {
                    mode: ScanMode::Directory,
                    concurrency,
                    profile: if requested.is_some() {
                        voldiq_core::ScanProfile::Fast
                    } else {
                        voldiq_core::ScanProfile::Auto
                    },
                    ..ScanOptions::default()
                };
                cancel.store(false, Ordering::Relaxed);
                let mut phase = ScanPhase::Scanning;
                let cpu = cpu_ms()?;
                let start = Instant::now();
                let (index, stopped, requested_at) = std::thread::scope(|scope| {
                    let (signal, notification) = std::sync::mpsc::channel();
                    let flag = &cancel;
                    let trigger = scope.spawn(move || {
                        notification.recv().ok()?;
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        let requested = Instant::now();
                        flag.store(true, Ordering::Relaxed);
                        Some(requested)
                    });
                    let mut notified = false;
                    let index = scan(fixture.path(), &options, &cancel, |update| {
                        phase = update.phase;
                        if update.phase == ScanPhase::Scanning
                            && update.files >= (count / 4).min(10_000) as u64
                            && !notified
                        {
                            notified = signal.send(()).is_ok();
                        }
                    });
                    let stopped = Instant::now();
                    drop(signal);
                    let requested_at = trigger
                        .join()
                        .map_err(|_| "Cancellation trigger panicked.")?;
                    Ok::<_, Box<dyn std::error::Error>>((index?, stopped, requested_at))
                })?;
                let cpu = cpu_ms()? - cpu;
                let cancel_ms = stopped.duration_since(requested_at.ok_or("Fixture completed before a live cancellation request; use a larger fixture.")?).as_secs_f64() * 1000.0;
                let elapsed = stopped.duration_since(start).as_secs_f64() * 1000.0;
                let root = &index.entries[0];
                if phase != ScanPhase::Cancelled
                    || !root.incomplete
                    || root.files == 0
                    || root.files >= count as u64
                    || root.logical != root.files * 128
                    || index.error_count != 0
                {
                    return Err("Cancellation did not preserve a consistent partial scan.".into());
                }
                println!(
                    "{},cancel_external,{setting},{workers},{count},{},{},{},{},{},{elapsed:.3},{cpu:.3},{cancel_ms:.3}",
                    round + 1,
                    root.files,
                    index.entries.len(),
                    root.logical,
                    root.allocated,
                    index.error_count
                );
            }
        }
    }
    let path = fixture.path().to_owned();
    fixture.close()?;
    if path.exists() {
        return Err("Owned fixture remains after cleanup.".into());
    }
    eprintln!("Owned fixture removed: {}", path.display());
    Ok(())
}

#[cfg(windows)]
fn cpu_ms() -> std::io::Result<f64> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };
    let mut times = [FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    }; 4];
    let [created, exited, kernel, user] = &mut times;
    if unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let ticks =
        |time: &FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Ok((ticks(kernel) + ticks(user)) as f64 / 10_000.0)
}

#[cfg(not(windows))]
fn cpu_ms() -> std::io::Result<f64> {
    Ok(f64::NAN)
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
