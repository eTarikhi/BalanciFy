//! Balancify desktop shell — IPC client that renders service status
//! This is a lab CLI that mimics the WinUI shell's behavior over IPC v1 (§8.2).
//! The real WinUI 3 / .NET 10 shell would use the same IPC contract over a named pipe.
//! This CLI connects via TCP loopback (for CI/Linux) and renders Home, Rules, Diagnostics.

use balancify_ipc::{Request, Response, Event, FrameKind, decode_frame, encode_frame, Counters};
use std::io::{Read, Write};
use std::net::{TcpStream, SocketAddr};
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let addr: SocketAddr = args.get(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| "127.0.0.1:5000".parse().unwrap());
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("home");

    println!("Balancify shell — connecting to {} mode={}", addr, mode);
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;

    // Send requests based on mode
    match mode {
        "home" => {
            send_request(&mut stream, &Request::GetStatus)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_home(resp);
            }
            // Subscribe to events at 1 Hz
            send_request(&mut stream, &Request::SubscribeEvents { interval_hz: 1 })?;
            println!("Subscribed to events at 1 Hz, listening for 5 events...");
            for _ in 0..5 {
                if let Some(ev) = recv_event(&mut stream)? {
                    render_event(ev);
                }
            }
        }
        "rules" => {
            send_request(&mut stream, &Request::ListFlowRules)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_rules(resp);
            }
            send_request(&mut stream, &Request::ListUplinks)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_uplinks(resp);
            }
        }
        "diagnostics" => {
            send_request(&mut stream, &Request::GetStatus)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_diagnostics(resp);
            }
            send_request(&mut stream, &Request::RunSelfTest)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_selftest(resp);
            }
        }
        "tray" => {
            // Tray flyout: minimal status
            send_request(&mut stream, &Request::GetStatus)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_tray(resp);
            }
        }
        _ => {
            println!("Unknown mode {}, available: home, rules, diagnostics, tray", mode);
        }
    }

    Ok(())
}

fn send_request(stream: &mut TcpStream, req: &Request) -> std::io::Result<()> {
    let payload = req.encode();
    let mut frame = Vec::new();
    encode_frame(FrameKind::Request, &payload, &mut frame).unwrap();
    stream.write_all(&frame)?;
    Ok(())
}

fn recv_response(stream: &mut TcpStream) -> std::io::Result<Option<Response>> {
    let mut buf = [0u8; 65536];
    let n = stream.read(&mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if let Ok(Some((kind, payload, _))) = decode_frame(&buf[..n]) {
        if kind == FrameKind::Response {
            if let Some(resp) = Response::decode(payload) {
                return Ok(Some(resp));
            }
        }
    }
    Ok(None)
}

fn recv_event(stream: &mut TcpStream) -> std::io::Result<Option<Event>> {
    let mut buf = [0u8; 65536];
    match stream.read(&mut buf) {
        Ok(0) => Ok(None),
        Ok(n) => {
            if let Ok(Some((kind, payload, _))) = decode_frame(&buf[..n]) {
                if kind == FrameKind::Event {
                    if let Some(ev) = Event::decode(payload) {
                        return Ok(Some(ev));
                    }
                }
            }
            Ok(None)
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => Ok(None),
        Err(e) => Err(e),
    }
}

fn render_home(resp: Response) {
    match resp {
        Response::Status(s) => {
            println!("=== Home ===");
            println!("Service version: {} schema: {}", s.service_version, s.schema_version);
            println!("Bond: {} Duplicate: {} Allow metered: {} MTU: {}", s.bond_enabled, s.duplicate_mode, s.allow_metered, s.adapter_mtu);
            println!("Policy rev: {} Rules rev: {} Active flows: {}", s.policy_revision, s.rules_revision, s.active_flows);
            println!("Uplinks:");
            for u in &s.uplinks {
                println!("  id={} state={} rtt={}ms loss={}‰ cap={}bps weight={}‰ metered={} eligible={}",
                    u.id, u.state, u.rtt_ms, u.loss_permille, u.capacity_bps, u.weight_permille, u.metered, u.eligible);
            }
            render_counters(&s.counters);
        }
        _ => println!("Unexpected response: {:?}", resp),
    }
}

fn render_rules(resp: Response) {
    match resp {
        Response::FlowRules(rules) => {
            println!("=== Flow Rules ({} rules) ===", rules.len());
            for r in rules {
                println!("  id={} name={} class={} pinned={} prio={} proto={} ports={}-{} dup={}",
                    r.id, r.name, r.class, r.pinned_uplink, r.priority, r.proto, r.port_start, r.port_end, r.duplicate);
            }
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_uplinks(resp: Response) {
    match resp {
        Response::Uplinks(list) => {
            println!("=== Uplinks ({} uplinks) ===", list.len());
            for u in list {
                println!("  id={} state={} rtt={}ms loss={}‰ cap={}bps", u.id, u.state, u.rtt_ms, u.loss_permille, u.capacity_bps);
            }
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_diagnostics(resp: Response) {
    match resp {
        Response::Status(s) => {
            println!("=== Diagnostics ===");
            println!("Version: {} policy_rev={} rules_rev={}", s.service_version, s.policy_revision, s.rules_revision);
            render_counters(&s.counters);
            // Show ECN/delay if present
            println!("ECN CE: {} Delay events: {}", s.counters.ecn_ce, s.counters.delay_events);
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_selftest(resp: Response) {
    match resp {
        Response::SelfTest { passed, detail } => {
            println!("=== Self-test ===");
            println!("Passed: {} Detail: {}", passed, detail);
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_tray(resp: Response) {
    match resp {
        Response::Status(s) => {
            let up_count = s.uplinks.iter().filter(|u| u.eligible).count();
            println!("Tray: {} uplinks up, {} flows, bond={}, dup={}",
                up_count, s.active_flows, s.bond_enabled, s.duplicate_mode);
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_counters(c: &Counters) {
    println!("Counters: stripped={} forwarded={} dups={} reinj={} delivered={} ooo={} refused={} auth_fail={} ecn={} delay={}",
        c.packets_stripped, c.packets_forwarded, c.duplicates_sent, c.reinjections, c.delivered, c.out_of_order, c.refused, c.auth_failed, c.ecn_ce, c.delay_events);
}

fn render_event(ev: Event) {
    match ev {
        Event::PolicyChanged { revision } => println!("[event] Policy changed rev={}", revision),
        Event::UplinkStateChanged { id, state } => println!("[event] Uplink {} state -> {}", id, state),
        Event::BondReverted { class, gain_permille } => println!("[event] Bond reverted class={} gain={}‰", class, gain_permille),
        Event::Stats(c) => {
            println!("[event] Stats: stripped={} forwarded={} ecn={} delay={}", c.packets_stripped, c.packets_forwarded, c.ecn_ce, c.delay_events);
        }
        Event::Warning { code } => println!("[event] Warning code={}", code),
    }
}
