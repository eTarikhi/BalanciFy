//! Stream transport adapter for the IPC v1 contract.
//!
//! Windows uses an ACL-protected named pipe; the loopback TCP adapter is for
//! non-Windows lab and CI. Both transports carry the same binary framing and
//! message encoding. The server enforces a nonzero client session ID; per-user
//! identity authorization remains a production hardening requirement beyond the ACL.

use crate::{
    decode_frame, encode_frame, frame_event, Event, FrameError, FrameKind, Request, Response, Status,
    FRAME_HEADER_LEN, MAX_PAYLOAD,
};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

/// Stream abstraction shared by the loopback CI transport and Windows named pipe.
pub trait IpcStream: Read + Write {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
}

impl IpcStream for TcpStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, timeout)
    }
}

/// Lab-only loopback TCP server. Windows service builds use `NamedPipeServer`.
pub struct IpcServer {
    listener: TcpListener,
    addr: std::net::SocketAddr,
}

impl IpcServer {
    pub fn bind_loopback() -> io::Result<Self> {
        Self::bind_loopback_at("127.0.0.1:0".parse().expect("valid loopback address"))
    }

    /// Bind a fixed loopback endpoint for the local service/client adapter.
    /// Non-loopback addresses are rejected so this lab transport cannot expose
    /// the control plane to the network; production uses an ACL'd named pipe.
    pub fn bind_loopback_at(addr: std::net::SocketAddr) -> io::Result<Self> {
        if !addr.ip().is_loopback() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "IPC TCP listener must bind to loopback"));
        }
        let listener = TcpListener::bind(addr)?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener, addr })
    }

    pub fn addr(&self) -> std::net::SocketAddr {
        self.addr
    }

    pub fn accept(&self) -> Option<TcpStream> {
        match self.listener.accept() {
            Ok((stream, _)) => Some(stream),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(_) => None,
        }
    }
}

/// ACL-protected, local-only Windows byte-mode named-pipe transport.
#[cfg(windows)]
pub struct NamedPipeServer {
    name: Vec<u16>,
    pending_instance: Option<NamedPipeStream>,
}

#[cfg(windows)]
pub const WINDOWS_SERVICE_PIPE_NAME: &str = r"\\.\pipe\BalanciFy.Service.v1";

#[cfg(windows)]
fn validate_pipe_name(name: &str) -> io::Result<Vec<u16>> {
    if !name.starts_with(r"\\.\pipe\") || name[9..].is_empty() || name.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "named pipe path must use the canonical Windows named-pipe prefix",
        ));
    }
    Ok(name.encode_utf16().chain(std::iter::once(0)).collect())
}

#[cfg(windows)]
impl NamedPipeServer {
    /// Bind `\\.\pipe\<name>` with an explicit DACL for SYSTEM, Administrators,
    /// and interactive users; no network or anonymous principals are granted access.
    pub fn bind(name: &str) -> io::Result<Self> {
        let name = validate_pipe_name(name)?;
        // Create the first instance synchronously so ACL/name errors fail service
        // startup instead of leaving a background accept loop permanently broken.
        let pending_instance = Some(create_named_pipe_instance(&name, true)?);
        Ok(Self { name, pending_instance })
    }

    /// Wait for one client and return a nonblocking byte-stream handle.
    pub fn accept(&mut self) -> io::Result<NamedPipeStream> {
        use std::ptr;

        let pipe = match self.pending_instance.take() {
            Some(pipe) => pipe,
            None => create_named_pipe_instance(&self.name, false)?,
        };
        let handle = pipe.handle;
        if unsafe { ConnectNamedPipe(handle, ptr::null_mut()) } == 0 {
            let error = last_os_error();
            if error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
                return Err(error);
            }
        }

        // Nonblocking reads let the shared stream loop deliver subscribed
        // events and enforce idle-client limits without a second pipe thread.
        let mut mode = PIPE_NOWAIT;
        if unsafe { SetNamedPipeHandleState(handle, &mut mode, ptr::null_mut(), ptr::null_mut()) } == 0 {
            return Err(last_os_error());
        }
        Ok(pipe)
    }
}

#[cfg(windows)]
fn validate_client_session(handle: *mut std::ffi::c_void) -> io::Result<()> {
    let mut session_id = 0u32;
    if unsafe { GetNamedPipeClientSessionId(handle, &mut session_id) } == 0 {
        return Err(last_os_error());
    }
    if session_id == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "named-pipe IPC is restricted to interactive user sessions",
        ));
    }
    if unsafe { ImpersonateNamedPipeClient(handle) } == 0 {
        return Err(last_os_error());
    }

    // Verify the connected token itself is an interactive-user token in
    // addition to relying on the pipe DACL and nonzero Terminal Services ID.
    let membership = (|| {
        const WIN_INTERACTIVE_SID: u32 = 11;
        let mut sid_buffer = [0usize; 9];
        let mut sid_size = std::mem::size_of_val(&sid_buffer) as u32;
        if unsafe {
            CreateWellKnownSid(
                WIN_INTERACTIVE_SID,
                std::ptr::null_mut(),
                sid_buffer.as_mut_ptr().cast(),
                &mut sid_size,
            )
        } == 0
        {
            return Err(last_os_error());
        }
        let mut is_member = 0i32;
        if unsafe {
            CheckTokenMembership(
                std::ptr::null_mut(),
                sid_buffer.as_mut_ptr().cast(),
                &mut is_member,
            )
        } == 0
        {
            return Err(last_os_error());
        }
        if is_member == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "named-pipe IPC requires an Interactive Users token",
            ));
        }
        Ok(())
    })();

    if unsafe { RevertToSelf() } == 0 {
        return Err(last_os_error());
    }
    membership
}

#[cfg(windows)]
fn create_named_pipe_instance(name: &[u16], first_instance: bool) -> io::Result<NamedPipeStream> {
    use std::ffi::c_void;
    const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
    const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
    const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    const PIPE_READMODE_BYTE: u32 = 0x0000_0000;
    const PIPE_WAIT: u32 = 0x0000_0000;
    const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;
    const PIPE_UNLIMITED_INSTANCES: u32 = 255;
    const MAX_PIPE_BUFFER: u32 = 65_536;
    const SDDL_REVISION_1: u32 = 1;

    let sddl: Vec<u16> = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)"
        .encode_utf16().chain(std::iter::once(0)).collect();
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    let mut descriptor_size = 0u32;
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(), SDDL_REVISION_1, &mut descriptor, &mut descriptor_size,
        )
    } == 0 {
        return Err(last_os_error());
    }
    let _descriptor = LocalSecurityDescriptor(descriptor);
    let mut attributes = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        security_descriptor: descriptor,
        inherit_handle: 0,
    };
    let open_mode = PIPE_ACCESS_DUPLEX
        | if first_instance { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            MAX_PIPE_BUFFER,
            MAX_PIPE_BUFFER,
            0,
            &mut attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(last_os_error());
    }
    Ok(NamedPipeStream {
        handle,
        peer_auth_pending: true,
    })
}

#[cfg(windows)]
pub struct NamedPipeStream {
    handle: *mut std::ffi::c_void,
    peer_auth_pending: bool,
}

#[cfg(windows)]
impl NamedPipeStream {
    /// Connect to a local named pipe using nonblocking byte-mode reads.
    pub fn connect(name: &str) -> io::Result<Self> {
        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const OPEN_EXISTING: u32 = 3;
        const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;

        let name = validate_pipe_name(name)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let handle = loop {
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    std::ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                break handle;
            }
            let error = last_os_error();
            if !matches!(
                error.raw_os_error(),
                Some(ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY | ERROR_SEM_TIMEOUT)
            ) || Instant::now() >= deadline
            {
                return Err(error);
            }
            unsafe { WaitNamedPipeW(name.as_ptr(), 100); }
            std::thread::sleep(Duration::from_millis(10));
        };
        let stream = Self {
            handle,
            peer_auth_pending: false,
        };
        let mut mode = PIPE_NOWAIT;
        if unsafe { SetNamedPipeHandleState(handle, &mut mode, std::ptr::null_mut(), std::ptr::null_mut()) } == 0 {
            return Err(last_os_error());
        }
        Ok(stream)
    }
}

#[cfg(windows)]
unsafe impl Send for NamedPipeStream {}

#[cfg(windows)]
impl IpcStream for NamedPipeStream {
    fn set_read_timeout(&mut self, _timeout: Option<Duration>) -> io::Result<()> {
        // PIPE_NOWAIT supplies the polling behavior required by read_header.
        Ok(())
    }
}

#[cfg(windows)]
impl Read for NamedPipeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() { return Ok(0); }
        let length = buffer.len().min(u32::MAX as usize) as u32;
        let mut read = 0u32;
        if unsafe {
            ReadFile(
                self.handle,
                buffer.as_mut_ptr().cast(),
                length,
                &mut read,
                std::ptr::null_mut(),
            )
        } != 0
        {
            if read > 0 && self.peer_auth_pending {
                validate_client_session(self.handle)?;
                self.peer_auth_pending = false;
            }
            return Ok(read as usize);
        }
        let error = last_os_error();
        match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_NO_DATA) | Some(ERROR_PIPE_LISTENING) => Err(io::ErrorKind::WouldBlock.into()),
            Some(ERROR_BROKEN_PIPE) | Some(ERROR_PIPE_NOT_CONNECTED) => Ok(0),
            _ => Err(error),
        }
    }
}

#[cfg(windows)]
impl Write for NamedPipeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() { return Ok(0); }
        let length = buffer.len().min(u32::MAX as usize) as u32;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut written = 0u32;
            if unsafe {
                WriteFile(
                    self.handle,
                    buffer.as_ptr().cast(),
                    length,
                    &mut written,
                    std::ptr::null_mut(),
                )
            } != 0
            {
                return Ok(written as usize);
            }
            let error = last_os_error();
            match error.raw_os_error().map(|code| code as u32) {
                Some(ERROR_NO_DATA) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Some(ERROR_NO_DATA) => {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "named-pipe write timed out"));
                }
                Some(ERROR_BROKEN_PIPE) | Some(ERROR_PIPE_NOT_CONNECTED) => return Err(error),
                _ => return Err(error),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

#[cfg(windows)]
impl Drop for NamedPipeStream {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle); }
    }
}

#[cfg(windows)]
const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;
#[cfg(windows)]
const PIPE_NOWAIT: u32 = 0x0000_0001;
#[cfg(windows)]
const ERROR_PIPE_CONNECTED: u32 = 535;
#[cfg(windows)]
const ERROR_PIPE_LISTENING: u32 = 536;
#[cfg(windows)]
const ERROR_FILE_NOT_FOUND: i32 = 2;
#[cfg(windows)]
const ERROR_PIPE_BUSY: i32 = 231;
#[cfg(windows)]
const ERROR_SEM_TIMEOUT: i32 = 121;
#[cfg(windows)]
const ERROR_NO_DATA: u32 = 232;
#[cfg(windows)]
const ERROR_BROKEN_PIPE: u32 = 109;
#[cfg(windows)]
const ERROR_PIPE_NOT_CONNECTED: u32 = 233;

#[cfg(windows)]
#[repr(C)]
struct SecurityAttributes {
    length: u32,
    security_descriptor: *mut std::ffi::c_void,
    inherit_handle: i32,
}

#[cfg(windows)]
struct LocalSecurityDescriptor(*mut std::ffi::c_void);

#[cfg(windows)]
impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) { unsafe { LocalFree(self.0); } }
}

#[cfg(windows)]
fn last_os_error() -> io::Error {
    io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut std::ffi::c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn WaitNamedPipeW(name: *const u16, timeout: u32) -> i32;
    fn CreateNamedPipeW(name: *const u16, open_mode: u32, pipe_mode: u32, max_instances: u32,
        out_buffer_size: u32, in_buffer_size: u32, default_timeout: u32,
        security_attributes: *mut SecurityAttributes) -> *mut std::ffi::c_void;
    fn ConnectNamedPipe(pipe: *mut std::ffi::c_void, overlapped: *mut std::ffi::c_void) -> i32;
    fn GetNamedPipeClientSessionId(pipe: *mut std::ffi::c_void, session_id: *mut u32) -> i32;
    fn SetNamedPipeHandleState(pipe: *mut std::ffi::c_void, mode: *mut u32,
        max_collection_count: *mut u32, collect_data_timeout: *mut u32) -> i32;
    fn ReadFile(handle: *mut std::ffi::c_void, buffer: *mut std::ffi::c_void, bytes_to_read: u32,
        bytes_read: *mut u32, overlapped: *mut std::ffi::c_void) -> i32;
    fn WriteFile(handle: *mut std::ffi::c_void, buffer: *const std::ffi::c_void, bytes_to_write: u32,
        bytes_written: *mut u32, overlapped: *mut std::ffi::c_void) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    fn GetLastError() -> u32;
    fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
}

#[cfg(windows)]
#[link(name = "advapi32")]
extern "system" {
    fn ImpersonateNamedPipeClient(pipe: *mut std::ffi::c_void) -> i32;
    fn RevertToSelf() -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(string: *const u16, revision: u32,
        descriptor: *mut *mut std::ffi::c_void, size: *mut u32) -> i32;
    fn CreateWellKnownSid(sid_type: u32, domain_sid: *mut std::ffi::c_void,
        sid: *mut std::ffi::c_void, sid_size: *mut u32) -> i32;
    fn CheckTokenMembership(token: *mut std::ffi::c_void, sid: *mut std::ffi::c_void,
        is_member: *mut i32) -> i32;
}

/// Serve request/response frames until the peer closes the stream. Framing is
/// exact, so TCP fragmentation and pipelined requests are supported.
pub fn handle_stream<S, F>(stream: S, handler: F) -> io::Result<()>
where
    S: IpcStream,
    F: FnMut(Request) -> Response,
{
    handle_stream_inner(stream, handler, || Vec::new(), false)
}

/// Serve IPC requests and push subscribed events while the client is idle.
/// `poll_events` is called at the negotiated subscription rate; event frames
/// are written on the same connection after the subscription Ack.
pub fn handle_stream_with_events<S, F, E>(stream: S, handler: F, poll_events: E) -> io::Result<()>
where
    S: IpcStream,
    F: FnMut(Request) -> Response,
    E: FnMut() -> Vec<Event>,
{
    handle_stream_inner(stream, handler, poll_events, true)
}

fn handle_stream_inner<S, F, E>(
    mut stream: S,
    mut handler: F,
    mut poll_events: E,
    events_enabled: bool,
) -> io::Result<()>
where
    S: IpcStream,
    F: FnMut(Request) -> Response,
    E: FnMut() -> Vec<Event>,
{
    const EVENT_READ_POLL: Duration = Duration::from_millis(100);
    const UNSUBSCRIBED_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
    // TCP uses a short socket timeout for event polling; Windows pipes use
    // PIPE_NOWAIT and are polled by read_header directly.
    if events_enabled {
        stream.set_read_timeout(Some(EVENT_READ_POLL))?;
    }
    let mut event_interval = None;
    let mut next_event = Instant::now();
    let mut last_activity = Instant::now();

    loop {
        let mut frame = vec![0u8; FRAME_HEADER_LEN];
        if !read_header(
            &mut stream,
            &mut frame,
            events_enabled,
            event_interval,
            UNSUBSCRIBED_IDLE_TIMEOUT,
            last_activity,
            &mut next_event,
            &mut poll_events,
        )? {
            return Ok(());
        }

        let len = u32::from_be_bytes([frame[2], frame[3], frame[4], frame[5]]) as usize;
        if len > MAX_PAYLOAD {
            return Err(frame_error(FrameError::TooLarge(len)));
        }
        frame.resize(FRAME_HEADER_LEN + len, 0);
        read_exact_stream(&mut stream, &mut frame[FRAME_HEADER_LEN..])?;

        let (kind, payload, consumed) = decode_frame(&frame)
            .map_err(frame_error)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "incomplete IPC frame"))?;
        if consumed != frame.len() || kind != FrameKind::Request {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "expected one IPC request frame"));
        }

        let request = Request::decode(payload);
        let requested_interval = match request.as_ref() {
            Some(Request::SubscribeEvents { interval_hz }) => Some(*interval_hz),
            _ => None,
        };
        let response = match request {
            Some(request) => handler(request),
            None => Response::Error {
                status: Status::BadRequest,
                message: "invalid request payload".to_string(),
            },
        };
        let mut out = Vec::with_capacity(128);
        encode_frame(FrameKind::Response, &response.encode(), &mut out)
            .map_err(frame_error)?;
        stream.write_all(&out)?;
        last_activity = Instant::now();

        if let Some(interval_hz) = requested_interval {
            event_interval = if response == Response::Ack && (1..=10).contains(&interval_hz) {
                Some(Duration::from_millis(1000 / interval_hz as u64))
            } else {
                None
            };
            next_event = Instant::now();
        }
    }
}

fn read_header<S, E>(
    stream: &mut S,
    header: &mut [u8],
    events_enabled: bool,
    event_interval: Option<Duration>,
    idle_timeout: Duration,
    last_activity: Instant,
    next_event: &mut Instant,
    poll_events: &mut E,
) -> io::Result<bool>
where
    S: IpcStream,
    E: FnMut() -> Vec<Event>,
{
    let mut filled = 0;
    while filled < header.len() {
        match stream.read(&mut header[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated IPC header")),
            Ok(count) => filled += count,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || (filled == 0 && error.kind() == io::ErrorKind::TimedOut) =>
            {
                let now = Instant::now();
                if idle_subscription_expired(last_activity, now, event_interval, idle_timeout) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "IPC client idle without an event subscription",
                    ));
                }
                if events_enabled {
                    if let Some(interval) = event_interval {
                        if now >= *next_event {
                            for event in poll_events() {
                                stream.write_all(&frame_event(&event))?;
                            }
                            *next_event = now + interval;
                        }
                    }
                }
                if error.kind() == io::ErrorKind::WouldBlock {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn read_exact_stream<S: IpcStream>(stream: &mut S, buffer: &mut [u8]) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut filled = 0;
    while filled < buffer.len() {
        match stream.read(&mut buffer[filled..]) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated IPC frame")),
            Ok(count) => filled += count,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock =>
            {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "IPC frame read timed out"));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn idle_subscription_expired(
    last_activity: Instant,
    now: Instant,
    event_interval: Option<Duration>,
    idle_timeout: Duration,
) -> bool {
    event_interval.is_none() && now.duration_since(last_activity) >= idle_timeout
}

fn frame_error(error: FrameError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Shutdown, TcpListener};
    use std::thread;

    fn read_frame_payload(stream: &mut TcpStream, expected_kind: FrameKind) -> Vec<u8> {
        let mut header = [0u8; FRAME_HEADER_LEN];
        stream.read_exact(&mut header).unwrap();
        let len = u32::from_be_bytes([header[2], header[3], header[4], header[5]]) as usize;
        let mut frame = header.to_vec();
        frame.resize(FRAME_HEADER_LEN + len, 0);
        stream.read_exact(&mut frame[FRAME_HEADER_LEN..]).unwrap();
        let (kind, payload, consumed) = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(kind, expected_kind);
        assert_eq!(consumed, frame.len());
        payload.to_vec()
    }

    fn read_response(stream: &mut TcpStream) -> Response {
        Response::decode(&read_frame_payload(stream, FrameKind::Response)).unwrap()
    }

    #[test]
    fn fixed_listener_rejects_non_loopback_addresses() {
        let addr = "0.0.0.0:5000".parse().unwrap();
        let result = IpcServer::bind_loopback_at(addr);
        assert!(matches!(result, Err(ref error) if error.kind() == io::ErrorKind::InvalidInput));
    }

    #[test]
    fn idle_timeout_only_closes_unsubscribed_clients() {
        let last_activity = Instant::now();
        let now = last_activity + Duration::from_secs(61);
        let timeout = Duration::from_secs(60);
        assert!(idle_subscription_expired(last_activity, now, None, timeout));
        assert!(!idle_subscription_expired(
            last_activity,
            now,
            Some(Duration::from_secs(1)),
            timeout,
        ));
    }

    #[test]
    fn serves_fragmented_and_pipelined_binary_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut calls = 0;
            handle_stream(stream, |request| {
                calls += 1;
                match request {
                    Request::Ping => Response::Pong { service_version: "test".into() },
                    Request::GetStatus => Response::Ack,
                    _ => unreachable!(),
                }
            }).unwrap();
            calls
        });

        let mut client = TcpStream::connect(addr).unwrap();
        let ping = crate::frame_request(&Request::Ping);
        let status = crate::frame_request(&Request::GetStatus);
        // Deliberately split the first frame, then send two requests together.
        client.write_all(&ping[..2]).unwrap();
        client.write_all(&ping[2..]).unwrap();
        let mut pipelined = status;
        pipelined.extend_from_slice(&crate::frame_request(&Request::Ping));
        client.write_all(&pipelined).unwrap();
        assert_eq!(read_response(&mut client), Response::Pong { service_version: "test".into() });
        assert_eq!(read_response(&mut client), Response::Ack);
        assert_eq!(read_response(&mut client), Response::Pong { service_version: "test".into() });
        client.shutdown(Shutdown::Write).unwrap();
        assert_eq!(server.join().unwrap(), 3);
    }

    #[test]
    fn pushes_event_frames_after_subscription_ack() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_stream_with_events(
                stream,
                |_| Response::Ack,
                || vec![Event::Warning { code: crate::WARN_ALL_PATHS_DOWN }],
            )
        });

        let mut client = TcpStream::connect(addr).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        client.write_all(&crate::frame_request(&Request::SubscribeEvents { interval_hz: 1 })).unwrap();
        assert_eq!(read_response(&mut client), Response::Ack);
        let event = Event::decode(&read_frame_payload(&mut client, FrameKind::Event)).unwrap();
        assert_eq!(event, Event::Warning { code: crate::WARN_ALL_PATHS_DOWN });
        client.shutdown(Shutdown::Write).unwrap();
        server.join().unwrap().unwrap();
    }

    #[test]
    fn invalid_payload_gets_bad_request_and_does_not_reach_handler() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_stream(stream, |_| panic!("invalid request reached handler")).unwrap();
        });
        let mut client = TcpStream::connect(addr).unwrap();
        let mut frame = Vec::new();
        crate::encode_frame(FrameKind::Request, &[255], &mut frame).unwrap();
        client.write_all(&frame).unwrap();
        assert_eq!(read_response(&mut client), Response::Error {
            status: Status::BadRequest,
            message: "invalid request payload".into(),
        });
        client.shutdown(Shutdown::Write).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn rejects_response_frame_on_request_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_stream(stream, |_| Response::Ack)
        });
        let mut client = TcpStream::connect(addr).unwrap();
        let mut frame = Vec::new();
        encode_frame(FrameKind::Response, &Response::Ack.encode(), &mut frame).unwrap();
        client.write_all(&frame).unwrap();
        assert_eq!(server.join().unwrap().unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
