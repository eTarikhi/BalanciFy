//! Balancify in-binary cooperating endpoint ("server mode"), blueprint §4.7.
//!
//! Lab/Phase-2 form: binds one UDP socket per path, decodes tunnel headers,
//! re-sequences through the first-party reorder buffer and emits the ordered
//! stream. Production form (Phase 4) adds: Noise-style handshake + AEAD,
//! session auth/PSK, forwarding/NAT egress from one stable address, dual
//! relays (§6.6), abuse controls and the no-destination-logging rule (§9.2).

use balancify_tunnel::{Header, ReorderBuffer};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

pub struct Reassembler {
    buf: ReorderBuffer,
}

impl Reassembler {
    pub fn new(hold_cap: Duration) -> Self { Self { buf: ReorderBuffer::new(hold_cap) } }
    /// Feed one tunnel datagram; returns payloads released in order.
    pub fn ingest(&mut self, dg: &[u8], now: Instant) -> Vec<Vec<u8>> {
        let Some((hdr, payload)) = Header::decode(dg) else { return Vec::new(); };
        let _ = hdr.path; // path identity is carried by the receiving socket in prod
        self.buf.push(hdr.seq, payload.to_vec());
        self.buf.pop_ready(now)
    }
    /// Release everything deliverable at `now` without ingesting (final drain).
    pub fn drain(&mut self, now: Instant) -> Vec<Vec<u8>> { self.buf.pop_ready(now) }
    pub fn out_of_order_releases(&self) -> u64 { self.buf.released_out_of_order }
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ports: Vec<u16> = args
        .first()
        .map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![4430, 4431]);
    let sockets: Vec<UdpSocket> = ports
        .iter()
        .map(|p| UdpSocket::bind(("0.0.0.0", *p)))
        .collect::<Result<_, _>>()?;
    for s in &sockets { s.set_read_timeout(Some(Duration::from_millis(100)))?; }
    eprintln!("balancify-relay (phase-2 lab form): listening on UDP {ports:?}");
    let mut re = Reassembler::new(Duration::from_millis(80));
    let mut dg = [0u8; 65535];
    loop {
        for s in &sockets {
            match s.recv_from(&mut dg) {
                Ok((n, peer)) => {
                    let now = Instant::now();
                    for payload in re.ingest(&dg[..n], now) {
                        // Phase 2: echo the ordered stream back on the path it arrived on.
                        let _ = s.send_to(&payload, peer);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use balancify_tunnel::{PassThrough, Sender};

    #[test]
    fn two_paths_with_delay_gap_reassemble_in_order() {
        let mut send = Sender::new(1, PassThrough, vec![(0, 1000), (1, 1000)]);
        let mut wire: Vec<(u8, Vec<u8>)> = (0..40).map(|i| send.send(format!("p{i:02}").as_bytes())).collect();
        // simulate: path 1 datagrams delayed (arrive after later path-0 ones)
        wire.sort_by_key(|(p, b)| {
            let (h, _) = Header::decode(b).unwrap();
            h.seq + if *p == 1 { 5 } else { 0 } // artificial skew
        });
        let mut re = Reassembler::new(Duration::from_millis(80));
        let mut got: Vec<String> = Vec::new();
        let t0 = Instant::now();
        for (_, bytes) in &wire {
            for payload in re.ingest(bytes, t0) {
                got.push(String::from_utf8(payload).unwrap());
            }
        }
        got.extend(re.drain(t0).into_iter().map(|p| String::from_utf8(p).unwrap()));
        // Once every datagram has arrived, the full stream must be released in order.
        let expect: Vec<String> = (0..40).map(|i| format!("p{i:02}")).collect();
        assert_eq!(got, expect);
        assert_eq!(re.out_of_order_releases(), 0, "no cap expiry when all packets arrive");
    }

    #[test]
    fn reassembler_ignores_garbage() {
        let mut re = Reassembler::new(Duration::from_millis(10));
        assert!(re.ingest(b"short", Instant::now()).is_empty());
        assert!(re.ingest(&[0u8; 4], Instant::now()).is_empty());
    }
}
