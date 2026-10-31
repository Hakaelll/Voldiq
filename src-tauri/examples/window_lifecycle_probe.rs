//! Isolates native window teardown without opening Voldiq's databases or user profile.
use std::time::{Duration, Instant};
use tauri::Manager;

fn main() {
    assert!(!tauri::is_dev(), "Run with --features production");
    let profile = tempfile::tempdir().expect("Create isolated WebView profile");
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "io.voldiq.lifecycle-probe".into();
    context.config_mut().app.windows.clear();
    let profile_path = profile.path().to_owned();
    let webview_profile = profile_path.clone();
    let exit_code = tauri::Builder::default()
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "probe",
                tauri::WebviewUrl::External("about:blank".parse()?),
            )
            .visible(false)
            .data_directory(webview_profile.clone())
            .build()?;
            println!("PROBE window-created");
            Ok(())
        })
        .build(context)
        .expect("Build isolated runtime")
        .run_return(|app, event| match event {
            tauri::RunEvent::Ready => {
                println!("PROBE ready");
                let handle = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(1));
                    println!("PROBE requesting-close");
                    let result = handle
                        .get_webview_window("probe")
                        .expect("Probe window exists")
                        .close();
                    println!("PROBE close-result {result:?}");
                });
            }
            tauri::RunEvent::WindowEvent { label, event, .. } => match event {
                tauri::WindowEvent::CloseRequested { .. } => {
                    println!("PROBE close-requested {label}");
                }
                tauri::WindowEvent::Destroyed => println!("PROBE destroyed {label}"),
                _ => {}
            },
            tauri::RunEvent::ExitRequested { .. } => println!("PROBE exit-requested"),
            tauri::RunEvent::Exit => println!("PROBE exit"),
            _ => {}
        });
    println!("PROBE run-returned {exit_code}");
    assert_eq!(exit_code, 0, "Runtime exit code");
    // WebView's browser process can release profile files after the event loop exits.
    let temporary_root = std::env::temp_dir()
        .canonicalize()
        .expect("Resolve temp root");
    let resolved_profile = profile_path.canonicalize().expect("Resolve owned profile");
    assert_eq!(resolved_profile.parent(), Some(temporary_root.as_path()));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::remove_dir_all(&resolved_profile) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("Remove owned profile {}: {error}", profile_path.display()),
        }
    }
    drop(profile);
    println!("PROBE profile-removed");
}
