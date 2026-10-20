//! Stream transport adapter for the IPC v1 contract.
//!
//! Production uses an ACL-protected named pipe with caller identity checks
//! (Sec.9.2). This loopback TCP adapter is for the lab and CI; it carries the
//! exact same binary framing and message encoding as the named-pipe adapter.

use crate::{
    decode_frame, encode_frame, FrameError, FrameKind, Request, Response, Status,
    FRAME_HEADER_LEN, MAX_PAYLOAD,
};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};

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

/// Serve requests until the peer closes the stream. Frames are read exactly,
/// so TCP fragmentation and multiple requests in a single write are both
/// handled. Malformed framing or a non-request frame terminates the connection;
/// malformed request payloads receive `BadRequest` without invoking `handler`.
pub fn handle_stream<F>(mut stream: TcpStream, mut handler: F) -> io::Result<()>
where
    F: FnMut(Request) -> Response,
{
    loop {
        let mut frame = [0u8; FRAME_HEADER_LEN].to_vec();
        match stream.read_exact(&mut frame) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
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

        let response = match Request::decode(payload) {
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
    }
}

fn frame_error(error: FrameError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Shutdown, TcpListener};
    use std::thread;

    fn read_response(stream: &mut TcpStream) -> Response {
        let mut header = [0u8; FRAME_HEADER_LEN];
        stream.read_exact(&mut header).unwrap();
        let len = u32::from_be_bytes([header[2], header[3], header[4], header[5]]) as usize;
        let mut frame = header.to_vec();
        frame.resize(FRAME_HEADER_LEN + len, 0);
        stream.read_exact(&mut frame[FRAME_HEADER_LEN..]).unwrap();
        let (kind, payload, consumed) = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(kind, FrameKind::Response);
        assert_eq!(consumed, frame.len());
        Response::decode(payload).unwrap()
    }

    #[test]
    fn fixed_listener_rejects_non_loopback_addresses() {
        let addr = "0.0.0.0:5000".parse().unwrap();
        let result = IpcServer::bind_loopback_at(addr);
        assert!(matches!(result, Err(ref error) if error.kind() == io::ErrorKind::InvalidInput));
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
