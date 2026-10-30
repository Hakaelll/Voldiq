//! Local, process-bound transport for the one-use cleanup helper. No filesystem commands.
use std::{
    io,
    os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
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
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

struct LocalMemory(*mut std::ffi::c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

fn descriptor() -> io::Result<LocalMemory> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = owned(token)?;
        let mut bytes = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut bytes,
        );
        if io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
            || !(size_of::<TOKEN_USER>() as u32..=65536).contains(&bytes)
        {
            return Err(io::Error::other("Invalid process token information size."));
        }
        let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut sid = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let sid_memory = LocalMemory(sid.cast());
        let mut length = 0;
        while *sid.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16(std::slice::from_raw_parts(sid, length))
            .map_err(|_| io::Error::other("Invalid process SID."))?;
        drop(sid_memory);
        let policy: Vec<u16> = format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            policy.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(LocalMemory(descriptor))
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
        let descriptor = descriptor()?;
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
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
        })
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
