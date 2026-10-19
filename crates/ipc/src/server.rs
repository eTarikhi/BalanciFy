//! IPC server stub — named pipe on Windows, TCP loopback for CI (§7.1, §8.1)
//!
//! The real product uses an ACL-protected named pipe with caller identity check.
//! For CI on Linux, we use a TCP loopback socket on 127.0.0.1:0 with same framing.

use crate::{Request, Response, SCHEMA_VERSION};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// Length-prefixed framing: 4-byte BE length + JSON payload (for simplicity in stub)
/// Real product uses binary messages (§7.1).
pub struct IpcServer {
    listener: TcpListener,
    addr: std::net::SocketAddr,
}

impl IpcServer {
    pub fn bind_loopback() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener, addr })
    }
    pub fn addr(&self) -> std::net::SocketAddr { self.addr }

    pub fn accept(&self) -> Option<TcpStream> {
        match self.listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                Some(stream)
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
            Err(_) => None,
        }
    }
}

pub fn handle_stream<F>(mut stream: TcpStream, mut handler: F) -> std::io::Result<()>
where
    F: FnMut(Request) -> Response,
{
    let mut len_buf = [0u8; 4];
    loop {
        match stream.read_exact(&mut len_buf) {
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 1024*1024 {
            break; // too large
        }
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf)?;
        // For stub, we parse very simple JSON or just return Pong for any request
        // Real implementation would use binary codec.
        let req = Request::Ping; // stub: treat any as Ping
        let resp = handler(req);
        let resp_json = format!("{{\"schema\":{},\"resp\":\"{:?}\"}}", SCHEMA_VERSION, resp);
        let resp_bytes = resp_json.as_bytes();
        let resp_len = (resp_bytes.len() as u32).to_be_bytes();
        stream.write_all(&resp_len)?;
        stream.write_all(resp_bytes)?;
    }
    Ok(())
}
