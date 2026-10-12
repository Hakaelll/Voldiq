use std::{sync::atomic::AtomicBool, time::Instant};
use voldiq_core::{ScanConcurrency, ScanMode, ScanOptions, scanner::scan};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "10000".into())
        .parse()?;
    if !(100..=100_000).contains(&count) {
        return Err("Fixture requires 100 to 100,000 files.".into());
    }
    let fixture = tempfile::tempdir()?;
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
    println!("round,workers,files,entries,logical,allocated,errors,wall_ms,cpu_ms");
    let cancel = AtomicBool::new(false);
    for round in 0..5 {
        let mut order = [1, 2, 4, 8];
        order.rotate_left(round % 4);
        for workers in order {
            let options = ScanOptions {
                mode: ScanMode::Directory,
                concurrency: ScanConcurrency::try_from(workers)?,
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
                "{},{workers},{count},{},{},{},{},{elapsed:.3},{cpu:.3}",
                round + 1,
                index.entries.len(),
                root.logical,
                root.allocated,
                index.error_count
            );
        }
    }
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
