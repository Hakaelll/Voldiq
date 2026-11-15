use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};
use voldiq_core::{
    ScanOptions,
    inspection::{VideoPhase, VideoRequest, VideoResult, compress_video},
    operations::Lease,
};

pub(super) fn verify() {
    let fixture = tempfile::tempdir().unwrap();
    let profile = fixture.path().join("profile");
    let files = fixture.path().join("files");
    let outputs = fixture.path().join("outputs");
    for directory in [&profile, &files, &outputs] {
        fs::create_dir(directory).unwrap();
    }
    let input = files.join("source.mp4");
    let ffmpeg = std::env::var_os("VOLDIQ_FFMPEG_FIXTURE").map(PathBuf::from);
    if let Some(encoder) = ffmpeg.as_ref() {
        assert!(encoder.is_absolute());
        let result = Command::new(encoder)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-n",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=24",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
                "-t",
                "5",
                "-c:v",
                "mpeg4",
                "-q:v",
                "1",
                "-g",
                "1",
                "-c:a",
                "aac",
            ])
            .arg(&input)
            .status()
            .unwrap();
        assert!(result.success());
    } else {
        fs::write(&input, vec![0; 1024 * 1024]).unwrap();
    }
    let original = fs::read(&input).unwrap();
    assert!(original.len() >= 1024 * 1024);
    let cancel = AtomicBool::new(false);
    let index =
        voldiq_core::scanner::scan(&files, &ScanOptions::default(), &cancel, |_| {}).unwrap();
    let id = index
        .entries
        .iter()
        .find(|entry| entry.name == "source.mp4")
        .unwrap()
        .id;
    let lease = Lease::acquire(&profile).unwrap();
    let request = |encoder_path, destination| VideoRequest {
        index: &index,
        id,
        encoder_path,
        destination,
    };
    let output = outputs.join("copy.mp4");
    let missing = outputs.join("ffmpeg.exe");
    assert!(
        compress_video(request(&missing, &output), &lease, &cancel, |_| {})
            .err()
            .unwrap()
            .to_string()
            .contains("same local folder")
    );
    assert_eq!(fs::read_dir(&outputs).unwrap().count(), 0);
    assert_eq!(fs::read(&input).unwrap(), original);

    let Some(encoder) = ffmpeg.as_deref() else {
        return;
    };
    let compress = || compress_video(request(encoder, &output), &lease, &cancel, |_| {});
    let cancelled = compress_video(request(encoder, &output), &lease, &cancel, |phase| {
        if matches!(phase, VideoPhase::Encoding) {
            cancel.store(true, Ordering::Release);
        }
    })
    .unwrap();
    assert!(matches!(cancelled, VideoResult::Cancelled));
    assert_eq!(fs::read_dir(&outputs).unwrap().count(), 0);
    cancel.store(false, Ordering::Release);
    let receipt = compress().unwrap();
    let VideoResult::Published {
        original: initial,
        encoded,
        hardware,
        width: 640,
        height: 360,
        duration_ms: 5000,
    } = receipt
    else {
        panic!(
            "Unexpected compression receipt: {}",
            serde_json::to_string(&receipt).unwrap()
        );
    };
    assert_eq!(initial, original.len() as u64);
    assert!(encoded < initial && encoded > 0);
    let bytes = fs::read(&output).unwrap();
    assert_eq!(encoded, bytes.len() as u64);
    assert_eq!(bytes.get(4..8), Some(b"ftyp".as_slice()));
    assert!(
        compress()
            .err()
            .unwrap()
            .to_string()
            .contains("Existing files")
    );
    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert_eq!(fs::read(&input).unwrap(), original);
    assert_eq!(fs::read_dir(&outputs).unwrap().count(), 1);
    println!(
        "Voldiq video fixture: {}",
        serde_json::json!({"original":initial,"encoded":encoded,"hardware":hardware,
            "width":640,"height":360,"duration_ms":5000,"cancelled":true,
            "collision_preserved":true,"original_preserved":true})
    );
}
