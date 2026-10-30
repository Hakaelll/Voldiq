#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        io::{self, Read},
        os::windows::io::{AsHandle, FromRawHandle, OwnedHandle},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    use voldiq_core::windows::cleanup_channel::{Channel, MAX_MESSAGE, Server};
    use windows_sys::Win32::{
        Storage::FileSystem::SYNCHRONIZE,
        System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };

    fn process(id: u32) -> io::Result<OwnedHandle> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, id) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
    fn spawn(role: &str, name: &str, server: u32) -> io::Result<Child> {
        Command::new(std::env::current_exe()?)
            .args([role, name, &server.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
    }
    fn finish(mut child: Child) -> Result<String, Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(8);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                return Err("Owned probe child timed out.".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut output = String::new();
        child
            .stdout
            .take()
            .ok_or("Missing child stdout.")?
            .read_to_string(&mut output)?;
        if !status.success() {
            return Err(format!("Owned child failed: {status}").into());
        }
        Ok(output)
    }
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() == 4 {
        let role = arguments[1].as_str();
        if role == "idle" {
            io::stdin().read_to_end(&mut Vec::new())?;
            return Ok(());
        }
        let parent = process(arguments[3].parse()?)?;
        let result = Channel::connect(&arguments[2], parent.as_handle());
        if role == "wrong-server" {
            assert_eq!(
                result.err().ok_or("Unexpected server accepted.")?.kind(),
                io::ErrorKind::PermissionDenied
            );
            println!("wrong_server_rejected");
            return Ok(());
        }
        let mut channel = result?;
        match role {
            "echo" => {
                assert_eq!(channel.receive()?, vec![42; MAX_MESSAGE]);
                channel.send(b"ready")?;
            }
            "rejected" => {
                assert!(channel.receive().is_err());
                println!("disconnected_without_message");
            }
            "silent" => {
                io::stdin().read_to_end(&mut Vec::new())?;
            }
            _ => return Err("Unknown probe role.".into()),
        }
        return Ok(());
    }
    if arguments.len() != 1 {
        return Err("Unexpected arguments.".into());
    }

    let server = Server::create()?;
    let child = spawn("echo", server.name(), std::process::id())?;
    let mut channel = server.accept(child.as_handle())?;
    assert_eq!(
        channel.send(&[]).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        channel.send(&vec![0; MAX_MESSAGE + 1]).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    channel.send(&vec![42; MAX_MESSAGE])?;
    assert_eq!(channel.receive()?, b"ready");
    finish(child)?;
    drop(channel);

    let server = Server::create()?;
    let mut expected = spawn("idle", "unused", std::process::id())?;
    let intruder = spawn("rejected", server.name(), std::process::id())?;
    assert_eq!(
        server
            .accept(expected.as_handle())
            .err()
            .ok_or("Wrong client accepted.")?
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    expected.stdin.take();
    finish(expected)?;
    assert_eq!(finish(intruder)?.trim(), "disconnected_without_message");

    let server = Server::create()?;
    let mut unrelated = spawn("idle", "unused", std::process::id())?;
    let peer = spawn("wrong-server", server.name(), unrelated.id())?;
    let _connection = server.accept(peer.as_handle());
    assert_eq!(finish(peer)?.trim(), "wrong_server_rejected");
    unrelated.stdin.take();
    finish(unrelated)?;
    drop(_connection);

    let server = Server::create()?;
    let mut silent = spawn("silent", server.name(), std::process::id())?;
    let mut channel = server.accept(silent.as_handle())?;
    assert_eq!(
        channel.receive().unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(
        channel.receive().unwrap_err().kind(),
        io::ErrorKind::NotConnected
    );
    silent.stdin.take();
    finish(silent)?;
    println!(
        "{{\"maximumMessageExchanged\":true,\"invalidOutboundSizesRejected\":true,\"wrongClientRejected\":true,\"wrongServerRejected\":true,\"idleReadCancelledAndClosed\":true}}"
    );
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The cleanup channel probe requires Windows.");
    std::process::exit(1);
}
