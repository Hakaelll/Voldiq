//! Local, process-bound transport for the one-use cleanup helper. No filesystem commands.
use super::security::{SecurityDescriptor, current_user_sid, granted_access, source_access};
use std::{
    fs::File,
    io,
    os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::SECURITY_ATTRIBUTES,
    Storage::FileSystem::*,
    System::{Com::CoCreateGuid, IO::*, Pipes::*, Threading::*},
};

pub const MAX_MESSAGE: usize = 4096;
const TIMEOUT: Duration = Duration::from_secs(5);
const PREFIX: &str = r"\\.\pipe\voldiq-cleanup-";

pub struct Server {
    name: String,
    handle: OwnedHandle,
}

pub struct Channel {
    handle: Option<OwnedHandle>,
    peer: OwnedHandle,
}

const SOURCE_HEADER: &[u8; 8] = b"VDQSRC01";
const SOURCE_ACK: &[u8; 8] = b"VDQACK01";
const PARENT_HEADER: &[u8; 8] = b"VDQPAR01";
const PARENT_ACK: &[u8; 8] = b"VDQPAK01";
const HANDLE_FRAME_SIZE: usize = 44;

#[derive(Clone, Copy)]
enum HandleKind {
    Source,
    StageParent,
}

impl HandleKind {
    fn header(self) -> &'static [u8; 8] {
        match self {
            Self::Source => SOURCE_HEADER,
            Self::StageParent => PARENT_HEADER,
        }
    }
    fn acknowledgement(self) -> &'static [u8; 8] {
        match self {
            Self::Source => SOURCE_ACK,
            Self::StageParent => PARENT_ACK,
        }
    }
}

fn invalid_source() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid cleanup handle transfer.",
    )
}

fn handle_frame(file: &File, kind: HandleKind) -> io::Result<Vec<u8>> {
    let handle = file.as_raw_handle();
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
        return Err(invalid_source());
    }
    let granted = match kind {
        HandleKind::Source => source_access(file)?,
        HandleKind::StageParent => {
            let granted = granted_access(file)?;
            let required = FILE_LIST_DIRECTORY
                | FILE_READ_ATTRIBUTES
                | FILE_ADD_SUBDIRECTORY
                | FILE_DELETE_CHILD;
            let allowed = required | SYNCHRONIZE | READ_CONTROL | FILE_TRAVERSE | FILE_READ_EA;
            if granted & required != required || granted & !allowed != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Cleanup parent has unexpected access rights.",
                ));
            }
            granted
        }
    };
    let mut attributes: FILE_ATTRIBUTE_TAG_INFO = unsafe { std::mem::zeroed() };
    let mut identity: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileAttributeTagInfo,
            (&mut attributes as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
        || unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&mut identity as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if (attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0)
        != matches!(kind, HandleKind::StageParent)
        || attributes.FileAttributes
            & (FILE_ATTRIBUTE_REPARSE_POINT
                | FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0
    {
        return Err(invalid_source());
    }
    let mut frame = Vec::with_capacity(HANDLE_FRAME_SIZE);
    frame.extend_from_slice(kind.header());
    frame.extend_from_slice(&(handle as usize as u64).to_le_bytes());
    frame.extend_from_slice(&identity.VolumeSerialNumber.to_le_bytes());
    frame.extend_from_slice(&identity.FileId.Identifier);
    frame.extend_from_slice(&granted.to_le_bytes());
    Ok(frame)
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

fn process_id(process: BorrowedHandle<'_>) -> io::Result<u32> {
    let id = unsafe { GetProcessId(process.as_raw_handle()) };
    if id == 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } != WAIT_TIMEOUT {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Expected peer process is not live.",
        ));
    }
    Ok(id)
}

fn authenticate(pipe: HANDLE, peer: BorrowedHandle<'_>, server: bool) -> io::Result<()> {
    let expected = process_id(peer)?;
    let mut actual = 0;
    let result = unsafe {
        if server {
            GetNamedPipeClientProcessId(pipe, &mut actual)
        } else {
            GetNamedPipeServerProcessId(pipe, &mut actual)
        }
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    if actual != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unexpected cleanup channel peer.",
        ));
    }
    process_id(peer)?;
    Ok(())
}

fn operation(
    handle: HANDLE,
    connect: bool,
    start: impl FnOnce(*mut OVERLAPPED) -> i32,
) -> io::Result<u32> {
    let event = owned(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) })?;
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    overlapped.hEvent = event.as_raw_handle();
    if start(&mut overlapped) == 0 {
        let error = io::Error::last_os_error();
        if connect && error.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32) {
            return Ok(0);
        }
        if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            return Err(error);
        }
        let wait =
            unsafe { WaitForSingleObject(event.as_raw_handle(), TIMEOUT.as_millis() as u32) };
        if wait != WAIT_OBJECT_0 {
            let error = if wait == WAIT_TIMEOUT {
                io::Error::new(io::ErrorKind::TimedOut, "Cleanup channel I/O timed out.")
            } else {
                io::Error::last_os_error()
            };
            // Cancellation must finish before the kernel can stop referencing this stack and buffer.
            unsafe {
                CancelIoEx(handle, &overlapped);
                let mut transferred = 0;
                GetOverlappedResult(handle, &overlapped, &mut transferred, 1);
            }
            return Err(error);
        }
    }
    let mut transferred = 0;
    if unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(transferred)
}

impl Server {
    pub fn create() -> io::Result<Self> {
        let mut guid = windows_sys::core::GUID::from_u128(0);
        let hr = unsafe { CoCreateGuid(&mut guid) };
        if hr < 0 {
            return Err(io::Error::other(format!("Pipe GUID failed: {hr:#x}")));
        }
        let name = format!(
            "{PREFIX}{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            guid.data1,
            guid.data2,
            guid.data3,
            u16::from_be_bytes([guid.data4[0], guid.data4[1]]),
            u64::from_be_bytes([
                0,
                0,
                guid.data4[2],
                guid.data4[3],
                guid.data4[4],
                guid.data4[5],
                guid.data4[6],
                guid.data4[7]
            ])
        );
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let sid = current_user_sid()?;
        let descriptor = SecurityDescriptor::from_sddl(&format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})"))?;
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.as_ptr(),
            bInheritHandle: 0,
        };
        let handle = owned(unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                MAX_MESSAGE as u32,
                MAX_MESSAGE as u32,
                0,
                &security,
            )
        })?;
        Ok(Self { name, handle })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn accept(self, expected_peer: BorrowedHandle<'_>) -> io::Result<Channel> {
        process_id(expected_peer)?;
        operation(self.handle.as_raw_handle(), true, |overlapped| unsafe {
            ConnectNamedPipe(self.handle.as_raw_handle(), overlapped)
        })?;
        authenticate(self.handle.as_raw_handle(), expected_peer, true)?;
        Ok(Channel {
            handle: Some(self.handle),
            peer: expected_peer.try_clone_to_owned()?,
        })
    }
}

impl Channel {
    pub fn connect(name: &str, expected_server: BorrowedHandle<'_>) -> io::Result<Self> {
        let suffix = name.strip_prefix(PREFIX).unwrap_or("");
        if suffix.len() != 36
            || !suffix.bytes().enumerate().all(|(position, byte)| {
                if [8, 13, 18, 23].contains(&position) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid cleanup pipe name.",
            ));
        }
        process_id(expected_server)?;
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let deadline = Instant::now() + TIMEOUT;
        let handle = loop {
            let result = owned(unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    ptr::null_mut(),
                )
            });
            match result {
                Ok(handle) => break handle,
                Err(error)
                    if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => return Err(error),
            }
        };
        authenticate(handle.as_raw_handle(), expected_server, false)?;
        let mode = PIPE_READMODE_MESSAGE;
        if unsafe {
            SetNamedPipeHandleState(handle.as_raw_handle(), &mode, ptr::null(), ptr::null())
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle: Some(handle),
            peer: expected_server.try_clone_to_owned()?,
        })
    }

    /// Keeps the sender's handle borrowed until the peer confirms an identical, independently owned duplicate.
    /// Acquisition must exclude delete-on-close; metadata cannot establish that flag's absence.
    /// Desktop callers should use VerifiedFile::offer_source from a reviewed plan.
    pub fn offer_source(&mut self, file: &File) -> io::Result<()> {
        self.offer_handle(file, HandleKind::Source)
    }

    /// Requires directory creation/removal authority already granted to the ordinary caller.
    pub fn offer_stage_parent(&mut self, directory: &File) -> io::Result<()> {
        self.offer_handle(directory, HandleKind::StageParent)
    }

    fn offer_handle(&mut self, file: &File, kind: HandleKind) -> io::Result<()> {
        let frame = handle_frame(file, kind)?;
        let result = (|| {
            self.send(&frame)?;
            let acknowledgement = self.receive()?;
            if acknowledgement.len() != 36
                || &acknowledgement[..8] != kind.acknowledgement()
                || acknowledgement[8..] != frame[16..]
            {
                return Err(invalid_source());
            }
            Ok(())
        })();
        self.close_on_error(result)
    }

    /// Pulls only from the authenticated peer; never adopts a handle number as local authority.
    /// The peer process handle supplied at connection must include PROCESS_DUP_HANDLE.
    pub fn receive_source(&mut self) -> io::Result<File> {
        self.receive_handle(HandleKind::Source)
    }

    /// Receives a parent directory with the same, independently checked access rights as its sender.
    pub fn receive_stage_parent(&mut self) -> io::Result<File> {
        self.receive_handle(HandleKind::StageParent)
    }

    fn receive_handle(&mut self, kind: HandleKind) -> io::Result<File> {
        let result = (|| {
            let frame = self.receive()?;
            if frame.len() != HANDLE_FRAME_SIZE || &frame[..8] != kind.header() {
                return Err(invalid_source());
            }
            let number = u64::from_le_bytes(frame[8..16].try_into().map_err(|_| invalid_source())?);
            let number = usize::try_from(number).map_err(|_| invalid_source())?;
            if number == 0 || number >= isize::MAX as usize {
                return Err(invalid_source());
            }
            let mut duplicate = ptr::null_mut();
            if unsafe {
                DuplicateHandle(
                    self.peer.as_raw_handle(),
                    number as HANDLE,
                    GetCurrentProcess(),
                    &mut duplicate,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let file = File::from(owned(duplicate)?);
            let actual = handle_frame(&file, kind)?;
            if actual[16..] != frame[16..] {
                return Err(invalid_source());
            }
            let mut acknowledgement = Vec::with_capacity(36);
            acknowledgement.extend_from_slice(kind.acknowledgement());
            acknowledgement.extend_from_slice(&actual[16..]);
            self.send(&acknowledgement)?;
            Ok(file)
        })();
        self.close_on_error(result)
    }

    pub fn send(&mut self, message: &[u8]) -> io::Result<()> {
        if message.is_empty() || message.len() > MAX_MESSAGE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cleanup message size is invalid.",
            ));
        }
        let handle = self.raw()?;
        let result = operation(handle, false, |overlapped| unsafe {
            WriteFile(
                handle,
                message.as_ptr(),
                message.len() as u32,
                ptr::null_mut(),
                overlapped,
            )
        })
        .and_then(|written| {
            if written as usize == message.len() {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Incomplete cleanup message.",
                ))
            }
        });
        self.close_on_error(result)
    }

    pub fn receive(&mut self) -> io::Result<Vec<u8>> {
        let handle = self.raw()?;
        let mut buffer = vec![0; MAX_MESSAGE];
        let result = operation(handle, false, |overlapped| unsafe {
            ReadFile(
                handle,
                buffer.as_mut_ptr(),
                MAX_MESSAGE as u32,
                ptr::null_mut(),
                overlapped,
            )
        })
        .and_then(|read| {
            if read == 0 {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Empty cleanup message.",
                ))
            } else {
                buffer.truncate(read as usize);
                Ok(buffer)
            }
        });
        self.close_on_error(result)
    }

    fn raw(&self) -> io::Result<HANDLE> {
        self.handle
            .as_ref()
            .map(AsRawHandle::as_raw_handle)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotConnected, "Cleanup channel is closed.")
            })
    }

    fn close_on_error<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        if result.is_err() {
            self.handle.take();
        }
        result
    }
}
