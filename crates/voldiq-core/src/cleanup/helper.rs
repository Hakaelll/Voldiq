//! One-use admission helper. Only caller-owned capabilities cross this boundary.
use super::{VerifiedFile, recycle::ShellApartment};
use crate::windows::{
    cleanup_channel::{Channel, PeerElevation, Server, current_elevation},
    cleanup_stage::{StageEntry, StageIdentity, StageName, StagePin},
    wide,
};
use std::{
    fs::File,
    io,
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::{FILE_SHARE_READ, SYNCHRONIZE},
    System::Threading::*,
    UI::Shell::*,
};

const HELLO: &[u8] = b"VDQADMIT1";
const NEXT: &[u8] = b"next";
const FINISH: &[u8] = b"finish";
const BYE: &[u8] = b"released";
const MAX_ITEMS: u16 = 500;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    sequence: u16,
    stage: String,
    identity: StageIdentity,
    entry: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    sequence: u16,
    pub admitted: bool,
    pub native_error: Option<i32>,
    pub message: Option<String>,
}

pub struct Session {
    channel: Option<Channel>,
    peer: OwnedHandle,
    _image: File,
    sequence: u16,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn json_error(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn open_parent(id: u32) -> io::Result<OwnedHandle> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_DUP_HANDLE | SYNCHRONIZE,
            0,
            id,
        )
    };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// The desktop entry point always requires an elevated token. Ordinary mode is for the isolated native fixture.
pub fn serve(pipe: &str, parent: u32, expected: PeerElevation) -> io::Result<()> {
    if current_elevation()? != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unexpected helper elevation.",
        ));
    }
    let parent = open_parent(parent)?;
    let mut channel = Channel::connect(pipe, parent.as_handle())?;
    channel.require_peer(PeerElevation::Ordinary)?;
    channel.send(HELLO)?;
    let mut sequence = 0;
    loop {
        let command = channel.receive()?;
        if command == FINISH {
            channel.send(BYE)?;
            return Ok(());
        }
        if command != NEXT || sequence >= MAX_ITEMS {
            return Err(invalid("Unexpected or excessive admission request."));
        }
        let directory = channel.receive_stage_parent()?;
        let source = channel.receive_source()?;
        let request: Request = serde_json::from_slice(&channel.receive()?).map_err(json_error)?;
        if request.sequence != sequence {
            return Err(invalid("Admission sequence mismatch."));
        }
        let result = (|| {
            let name = StageName::parse(&request.stage)?;
            let entry = StageEntry::parse(&request.entry)?;
            let mut stage = StagePin::acquire(&directory, &name, request.identity)?;
            stage.admit(&source, &entry)
        })();
        let response = Admission {
            sequence,
            admitted: result.is_ok(),
            native_error: result.as_ref().err().and_then(io::Error::raw_os_error),
            message: result.err().map(|error| error.to_string()),
        };
        // Reply only after all duplicated source and directory authority has been released.
        drop(source);
        drop(directory);
        channel.send(&serde_json::to_vec(&response).map_err(json_error)?)?;
        sequence += 1;
    }
}

impl Session {
    /// Caller must pin the selected image before starting the actual peer process.
    pub fn connect(
        server: Server,
        peer: BorrowedHandle<'_>,
        expected: PeerElevation,
        image: File,
    ) -> io::Result<Self> {
        let mut channel = server.accept(peer)?;
        channel.require_peer(expected)?;
        if channel.receive()? != HELLO {
            return Err(invalid("Unsupported admission helper protocol."));
        }
        Ok(Self {
            channel: Some(channel),
            peer: peer.try_clone_to_owned()?,
            _image: image,
            sequence: 0,
        })
    }

    /// Requires explicit user confirmation for this batch. No GUI, service or permanent setting is elevated.
    pub fn launch(_apartment: &ShellApartment) -> io::Result<Self> {
        if current_elevation()? != PeerElevation::Ordinary {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Admission controller must not be elevated.",
            ));
        }
        let executable = std::env::current_exe()?;
        let image = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&executable)?;
        let server = Server::create()?;
        let file = wide(&executable);
        let verb: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
        let parameters: Vec<u16> =
            format!("--cleanup-helper {} {}", server.name(), std::process::id())
                .encode_utf16()
                .chain(Some(0))
                .collect();
        let mut execution = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: parameters.as_ptr(),
            nShow: 0,
            ..Default::default()
        };
        if unsafe { ShellExecuteExW(&mut execution) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if execution.hProcess.is_null() {
            return Err(invalid(
                "Windows did not return the launched helper process.",
            ));
        }
        let peer = unsafe { OwnedHandle::from_raw_handle(execution.hProcess) };
        Self::connect(server, peer.as_handle(), PeerElevation::Elevated, image)
    }

    /// Commit relocation intent before calling. Any transport failure leaves an unresolved operation.
    pub fn admit(
        &mut self,
        source: &VerifiedFile,
        parent: &File,
        name: &StageName,
        identity: StageIdentity,
        entry: &StageEntry,
    ) -> crate::Result<Admission> {
        source.check_stamp()?;
        if self.sequence >= MAX_ITEMS {
            return Err(invalid("Admission batch limit reached.").into());
        }
        let request = Request {
            sequence: self.sequence,
            stage: name.as_str().to_owned(),
            identity,
            entry: entry.as_str().to_owned(),
        };
        let result = (|| {
            let channel = self.channel.as_mut().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotConnected, "Admission helper is closed.")
            })?;
            channel.send(NEXT)?;
            channel.offer_stage_parent(parent)?;
            source.offer_source(channel).map_err(io::Error::other)?;
            channel.send(&serde_json::to_vec(&request).map_err(json_error)?)?;
            let response: Admission =
                serde_json::from_slice(&channel.receive()?).map_err(json_error)?;
            if response.sequence != self.sequence {
                return Err(invalid("Admission result sequence mismatch."));
            }
            Ok(response)
        })();
        match result {
            Ok(response) => {
                self.sequence += 1;
                Ok(response)
            }
            Err(error) => {
                self.channel = None;
                Err(error.into())
            }
        }
    }

    /// Confirms protocol completion and the exit of the retained, authenticated process. Never kills or restarts it.
    pub fn finish(mut self) -> io::Result<()> {
        let channel = self.channel.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "Admission helper is closed.")
        })?;
        channel.send(FINISH)?;
        if channel.receive()? != BYE {
            return Err(invalid("Admission helper did not confirm release."));
        }
        match unsafe { WaitForSingleObject(self.peer.as_raw_handle(), 10000) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Admission helper remains live; do not restart.",
                ));
            }
            _ => return Err(io::Error::last_os_error()),
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.peer.as_raw_handle(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if code != 0 {
            return Err(invalid("Admission helper failed on exit."));
        }
        Ok(())
    }
}

/// Run before Tauri initialization so an elevated helper never creates a desktop window or opens application databases.
pub fn run_cli() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--cleanup-helper")) {
        return None;
    }
    let result = (|| {
        let pipe = args
            .next()
            .ok_or_else(|| invalid("Missing admission pipe."))?;
        let pipe = pipe
            .to_str()
            .ok_or_else(|| invalid("Invalid admission pipe."))?;
        let parent = args
            .next()
            .and_then(|arg| arg.to_str().and_then(|value| value.parse::<u32>().ok()))
            .ok_or_else(|| invalid("Missing admission controller."))?;
        if args.next().is_some() {
            return Err(invalid("Unexpected helper arguments."));
        }
        serve(pipe, parent, PeerElevation::Elevated)
    })();
    if let Err(error) = &result {
        eprintln!("Cleanup helper: {error}");
    }
    Some(if result.is_ok() { 0 } else { 1 })
}
