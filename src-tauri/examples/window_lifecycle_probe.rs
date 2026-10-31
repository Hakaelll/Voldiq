//! Isolates native window teardown without opening Voldiq's databases or user profile.
use std::{path::PathBuf, time::Duration};
use tauri::Manager;

fn main() {
    assert!(!tauri::is_dev(), "Run with --features production");
    let mut arguments = std::env::args_os().skip(1);
    let profile_path = PathBuf::from(arguments.next().expect("Supply owned profile directory"));
    let mode = arguments.next().expect("Supply run or run_return");
    assert!(arguments.next().is_none(), "Unexpected argument");
    assert!(profile_path.is_absolute() && profile_path.is_dir());
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "io.voldiq.lifecycle-probe".into();
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "probe",
                tauri::WebviewUrl::External("about:blank".parse()?),
            )
            .visible(false)
            .data_directory(profile_path.clone())
            .build()?;
            println!("PROBE window-created");
            Ok(())
        })
        .build(context)
        .expect("Build isolated runtime");
    match mode.to_str() {
        Some("run") => app.run(lifecycle_event),
        Some("run_return") => {
            let exit_code = app.run_return(lifecycle_event);
            println!("PROBE run-returned {exit_code}");
            assert_eq!(exit_code, 0, "Runtime exit code");
        }
        _ => panic!("Expected run or run_return"),
    }
}

fn lifecycle_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    match event {
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
    }
}
