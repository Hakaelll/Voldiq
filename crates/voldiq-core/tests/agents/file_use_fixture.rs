use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader},
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{atomic::AtomicBool, mpsc},
    time::Duration,
};
use voldiq_core::{
    NodeId,
    index::Index,
    inspection::file_use::{Report, Target},
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

struct FileHolder(Child);

impl Drop for FileHolder {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub(super) fn verify(index: &Index, id: NodeId, path: &Path) {
    let mut holder = FileHolder(
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                r#"
            $ErrorActionPreference = 'Stop'
            $file = [IO.File]::Open($env:VOLDIQ_FIXTURE_FILE, 'Open', 'ReadWrite', 'Read')
            try {
                [Console]::WriteLine('ready')
                [void][Console]::ReadLine()
            } finally {
                $file.Dispose()
            }
        "#,
            ])
            .env("VOLDIQ_FIXTURE_FILE", path)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = holder.0.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let ready = BufReader::new(stdout).read_line(&mut line).is_ok() && line.trim() == "ready";
        let _ = send.send(ready);
    });
    assert!(receive.recv_timeout(Duration::from_secs(15)).unwrap());

    let pid = holder.0.id();
    let read = |cancelled| {
        Target::from_index(index, id)
            .unwrap()
            .read(&AtomicBool::new(cancelled))
            .unwrap()
    };
    let before = read(false).unwrap();
    assert!(
        matches!(&before, Report::Observed { applications } if applications.iter().any(|application| application.pid == pid)
        && applications.iter().all(|application| application.pid != std::process::id())),
        "{before:?}"
    );
    assert!(read(true).is_none());

    drop(holder.0.stdin.take());
    assert!(holder.0.wait().unwrap().success());
    let after = read(false).unwrap();
    assert!(
        matches!(&after, Report::Observed { applications } if applications.iter().all(|application| application.pid != pid)),
        "{after:?}"
    );

    let exclusive = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap();
    assert!(matches!(read(false), Some(Report::SharingConflict)));
    drop(exclusive);
}
