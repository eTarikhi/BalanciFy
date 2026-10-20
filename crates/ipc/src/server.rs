//! Stream transport adapter for the IPC v1 contract.
//!
//! Production uses an ACL-protected named pipe with caller identity checks
//! (Sec.9.2). This loopback TCP adapter is for the lab and CI; it carries the
//! exact same binary framing and message encoding as the named-pipe adapter.

use crate::{
    decode_frame, encode_frame, frame_event, Event, FrameError, FrameKind, Request, Response, Status,
    FRAME_HEADER_LEN, MAX_PAYLOAD,
};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

/// Lab-only loopback server. The production Windows adapter must enforce the
/// service's named-pipe ACL and authenticate the connecting Windows identity.
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

/// Serve request/response frames until the peer closes the stream. Framing is
/// exact, so TCP fragmentation and pipelined requests are supported.
pub fn handle_stream<F>(stream: TcpStream, handler: F) -> io::Result<()>
where
    F: FnMut(Request) -> Response,
{
    handle_stream_inner(stream, handler, || Vec::new(), false)
}

/// Serve IPC requests and push subscribed events while the client is idle.
/// `poll_events` is called at the negotiated subscription rate; event frames
/// are written on the same connection after the subscription Ack.
pub fn handle_stream_with_events<F, E>(stream: TcpStream, handler: F, poll_events: E) -> io::Result<()>
where
    F: FnMut(Request) -> Response,
    E: FnMut() -> Vec<Event>,
{
    handle_stream_inner(stream, handler, poll_events, true)
}

fn handle_stream_inner<F, E>(
    mut stream: TcpStream,
    mut handler: F,
    mut poll_events: E,
    events_enabled: bool,
) -> io::Result<()>
where
    F: FnMut(Request) -> Response,
    E: FnMut() -> Vec<Event>,
{
    const EVENT_READ_POLL: Duration = Duration::from_millis(100);
    const UNSUBSCRIBED_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
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
        stream.read_exact(&mut frame[FRAME_HEADER_LEN..])?;

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

fn read_header<E>(
    stream: &mut TcpStream,
    header: &mut [u8],
    events_enabled: bool,
    event_interval: Option<Duration>,
    idle_timeout: Duration,
    last_activity: Instant,
    next_event: &mut Instant,
    poll_events: &mut E,
) -> io::Result<bool>
where
    E: FnMut() -> Vec<Event>,
{
    let mut filled = 0;
    while filled < header.len() {
        match stream.read(&mut header[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated IPC header")),
            Ok(count) => filled += count,
            Err(error)
                if events_enabled
                    && filled == 0
                    && (error.kind() == io::ErrorKind::WouldBlock
                        || error.kind() == io::ErrorKind::TimedOut) =>
            {
                let now = Instant::now();
                if idle_subscription_expired(last_activity, now, event_interval, idle_timeout) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "IPC client idle without an event subscription",
                    ));
                }
                if let Some(interval) = event_interval {
                    if now >= *next_event {
                        for event in poll_events() {
                            stream.write_all(&frame_event(&event))?;
                        }
                        *next_event = now + interval;
                    }
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(true)
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
