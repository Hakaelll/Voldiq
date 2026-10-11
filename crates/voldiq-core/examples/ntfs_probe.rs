#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::atomic::AtomicBool;
    use voldiq_core::{ScanOptions, ntfs::VolumeReader, scanner::scan, windows};
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("native-file.bin");
    std::fs::write(&path, [1; 128])?;
    std::fs::hard_link(&path, fixture.path().join("hard-link.bin"))?;
    match VolumeReader::open(fixture.path()).and_then(|reader| {
        reader.journal()?;
        Ok(reader)
    }) {
        Ok(reader) => {
            let reference = windows::identity(&path)?.key.1;
            let file = reader.file(reference)?;
            assert_eq!(file.metadata()?.len(), 128);
            assert_eq!(windows::file_identity(&file)?.links, 2);
            let start = std::time::Instant::now();
            let records = reader.enumerate(1000, &AtomicBool::new(false))?;
            println!(
                "ntfs_available=true,records={},elapsed_ms={:.3},state={:?}",
                records.items.len(),
                start.elapsed().as_secs_f64() * 1000.0,
                records.state
            );
        }
        Err(error) => {
            println!("ntfs_available=false,reason={error}");
            let index = scan(
                fixture.path(),
                &ScanOptions::default(),
                &AtomicBool::new(false),
                |_| {},
            )?;
            assert_eq!(index.entries[0].logical, 256);
            assert_eq!(index.entries[0].files, 2);
            assert_eq!(index.error_count, 0);
            println!("conventional_fixture=passed,files=2,logical=256");
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    println!("NTFS probing is available on Windows only.");
}
