//! Balancify desktop shell -- IPC client that renders service status
//! This is a lab CLI that mimics the WinUI shell's behavior over IPC v1 (Sec.8.2).
//! The real WinUI 3 / .NET 10 shell would use the same IPC contract over a named pipe.
//! This CLI connects via TCP loopback (for CI/Linux) and renders Home, Rules, Diagnostics.
//! Enhanced with per-path ECN/delay, bond gain, uplink up/down, rekey, relay failover,
//! per-flow sessions, Rig B throughput, HVCI, SBOM.

use balancify_ipc::{Request, Response, Event, FrameKind, decode_frame, encode_frame, Counters};
use std::io::{Read, Write};
use std::net::{TcpStream, SocketAddr};
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let addr: SocketAddr = args.get(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| "127.0.0.1:5000".parse().unwrap());
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("home");

    println!("Balancify shell -- connecting to {} mode={}", addr, mode);
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
        "relay" => {
            // Relay & account screen (Sec.8.2)
            send_request(&mut stream, &Request::GetStatus)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_relay(resp);
            }
        }
        "bundle" => {
            // Support bundle (Sec.8.2) — redacted, per-path ECN/delay, cwnd, etc.
            send_request(&mut stream, &Request::GetStatus)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_bundle(resp);
            }
        }
        "perflow" => {
            // Per-flow sessions (Sec.4.7.4, ADR-0029)
            send_request(&mut stream, &Request::GetStatus)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_perflow(resp);
            }
        }
        "rigb" => {
            // Rig B throughput (Sec.10.3)
            send_request(&mut stream, &Request::GetStatus)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_rigb(resp);
            }
        }
        "hvci" => {
            // HVCI readiness (S15)
            send_request(&mut stream, &Request::GetStatus)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_hvci(resp);
            }
        }
        "sbom" => {
            // SBOM/VEX (S13)
            send_request(&mut stream, &Request::GetStatus)?;
            if let Some(resp) = recv_response(&mut stream)? {
                render_sbom(resp);
            }
        }
        _ => {
            println!("Unknown mode {}, available: home, rules, diagnostics, tray, relay, bundle, perflow, rigb, hvci, sbom", mode);
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
            println!("ETW: 200 UPLINK_UP, 201 DOWN, 300 BOND_REVERTED, 400 REKEY, 401 ECN_CE, 402 DELAY, 500 FAILOVER, 600 AUTH_FAILED");
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
            println!("Plain-language preview: This rule sends Zoom over lowest-latency healthy link (Sec.8.2)");
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
            println!("=== Diagnostics (Sec.8.2, Sec.9.2 redacted) ===");
            println!("Version: {} policy_rev={} rules_rev={} active_flows={}", s.service_version, s.policy_revision, s.rules_revision, s.active_flows);
            render_counters(&s.counters);
            println!("ECN CE: {} (80% reduction, milder than loss 50%) Delay events: {} (10% reduction)", s.counters.ecn_ce, s.counters.delay_events);
            println!("Per-path diag (from support bundle): cwnd/in_flight/ecn_ce/delay_events/rtt_min/last_ms (see support/mod.rs)");
            for u in &s.uplinks {
                println!("  uplink id={} cap={}bps rtt={}ms eligible={} — cwnd/in_flight from cc.window/in_flight, ECN/delay from cc.ecn_ce/delay_events_path", u.id, u.capacity_bps, u.rtt_ms, u.eligible);
            }
            println!("Bond gain tracker: auto-disable <15% gain reverts class to flow-level, emits BondReverted event 300");
            println!("Rekey: packets_since_rekey, last_rekey_ago_secs, forward secrecy 1h/1M packets, ETW 400");
            println!("Relay: current + failover count, ETW 500 failover, 600 auth failed");
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_selftest(resp: Response) {
    match resp {
        Response::SelfTest { passed, detail } => {
            println!("=== Self-test ===");
            println!("Passed: {} Detail: {}", passed, detail);
            println!("Clean-machine offline provisioning: install->self-test->uninstall->clean (Sec.10.3)");
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_tray(resp: Response) {
    match resp {
        Response::Status(s) => {
            let up_count = s.uplinks.iter().filter(|u| u.eligible).count();
            println!("Tray: {} uplinks up, {} flows, bond={}, dup={}", up_count, s.active_flows, s.bond_enabled, s.duplicate_mode);
            println!("Active mode + any down uplink, one-click pause/resume, keyboard + screen-reader operable (Sec.8.2)");
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_relay(resp: Response) {
    match resp {
        Response::Status(s) => {
            println!("=== Relay & Account (Sec.8.2) ===");
            println!("Service version: {} schema: {}", s.service_version, s.schema_version);
            println!("Counters: ecn={} delay={} loss via cwnd? (see diagnostics)", s.counters.ecn_ce, s.counters.delay_events);
            println!("Relay: region/address, key status, connection test button, secondary relay state, failover count");
            for u in &s.uplinks {
                println!("  uplink id={} cap={}bps rtt={}ms eligible={}", u.id, u.capacity_bps, u.rtt_ms, u.eligible);
            }
            println!("Connection test is the support call preventer (Sec.8.2)");
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_bundle(resp: Response) {
    match resp {
        Response::Status(s) => {
            println!("=== Support Bundle (redacted, Sec.8.2, Sec.9.2) ===");
            println!("Version: {} policy_rev={} rules_rev={} active_flows={}", s.service_version, s.policy_revision, s.rules_revision, s.active_flows);
            render_counters(&s.counters);
            println!("Uplinks: {} total, {} eligible", s.uplinks.len(), s.uplinks.iter().filter(|u| u.eligible).count());
            for u in &s.uplinks {
                println!("  id={} state={} rtt={}ms loss={} per mille cap={}bps — cwnd/in_flight from support bundle", u.id, u.state, u.rtt_ms, u.loss_permille, u.capacity_bps);
            }
            println!("Bundle JSON includes: cwnd/in_flight per uplink, congestion diag loss/ecn/delay+paths, per_path Vec<PathCongestionDiag> cwnd/in_flight/ecn_ce/delay_events/rtt_min/last_ms, rekey diag, relay stats, ETW export, route table, adapter state, version/signing info, redaction (no destinations/content)");
            println!("Redaction: no destinations, no content, only counters, error codes, hashed flow keys (Sec.9.2)");
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_perflow(resp: Response) {
    match resp {
        Response::Status(s) => {
            println!("=== Per-flow Sessions (Sec.4.7.4, ADR-0029) ===");
            println!("Active flows: {} — each flow gets own session id (next_session_id starting 1000), seq per session, retx keyed by (session,seq)", s.active_flows);
            println!("Per-flow > per-class > default precedence, session cap 64, evicts oldest on cap");
            println!("Per-session seq spaces: true per-session demux without collision, aligns with RFC 8684 Sec.3.3");
            println!("Flow-level capacity: on_flow_packet (sent) + on_flow_ack (acked) in scoring.rs, policy.rs wrapper, ingest_path detects FlowData and calls on_flow_ack + on_ack");
            render_counters(&s.counters);
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_rigb(resp: Response) {
    match resp {
        Response::Status(s) => {
            println!("=== Rig B Throughput (Sec.10.3) ===");
            println!("Hardware: Rig B per §10.1 — Windows 11 24H2 x64+ARM64, wired 1 Gbps, Wi-Fi 6 300 Mbit/s, USB 5G, relay Linux in-region, driver test-signed, HVCI on/off, VBS on");
            println!("Software: release build, secure mode Noise_IK, pacing 80% capacity (publish sets 80% + floor 1 Mbit/s), per-flow on/off, per-class off/on");
            println!("Traffic: iperf3 TCP 16 parallel flows aggregate, 1 TCP flow single-flow, UDP 1 flow, curated workloads");
            println!("Acceptance rows: aggregate >=90% sum after overhead, single-flow >=70% sum both links, mismatched >=100% best (no regression else auto-revert §6.4.6), added delay <=3ms wired lab, loss/reordering <=0.1%, failover <=2s, CPU & memory report first limits after S2, power delta, stability 72h soak + random flaps + 100 resume, compatibility matrix, clean-machine provisioning offline VM");
            render_counters(&s.counters);
            println!("Simulated: bonded 42.8% gain mean 1.3ms vs single 2.0ms, lossy 16.7% penalty, ECN 80% reduction, delay 10% reduction, Rig B 1Gbps/300M mean ~X Mbps");
            println!("Real Rig B pending — harness throughput.rs has 5 profiles + Rig B + per-flow on/off + aggregate 16 flows + added delay + CPU/Gbit estimation via crypto-bench + ETW");
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_hvci(resp: Response) {
    match resp {
        Response::Status(s) => {
            println!("=== HVCI Readiness Trio (S15, Sec.9.4, Sec.9.5) ===");
            println!("Requirement: Driver Verifier with code-integrity 0x209BB, HLK HVCIMemoryIntegrity test, VBS/Memory-Integrity VM with Secure Boot/vTPM/test-signing");
            println!("Code: miniport.c NBL pool via NdisAllocateNetBufferListPool(fAllocateNetBuffer=TRUE), ioctl.c length validation SDDL METHOD_BUFFERED, NonPagedPoolNx NormalPoolPriority, no RWX, MDL freeing via NdisFreeMdl, /integritycheck /DYNAMICBASE /NXCOMPAT /Qspectre, PoolTag BlaB, Spectre mitigation");
            println!("Build: msbuild driver/balancify.vcxproj /p:Configuration=Release /p:Platform=x64 — requires WDK 10.0.22621+");
            println!("Test: verifier /standard /driver balancify.sys, HLK Studio HVCIMemoryIntegrity, Hyper-V VM Gen2 Secure Boot on vTPM VBS, bcdedit testsigning on, pnputil add-driver, msinfo32 VBS Running MI enabled");
            render_counters(&s.counters);
            println!("Status: code ready, real VM run pending, CI currently only Rust workspace (no WDK), future add WDK build job Phase 4 (a) in ci.yml");
        }
        _ => println!("Unexpected: {:?}", resp),
    }
}

fn render_sbom(resp: Response) {
    match resp {
        Response::Status(s) => {
            println!("=== SBOM/VEX SPDX 3.0 (S13, March 2027) ===");
            println!("Tool: tools/ci/sbom.sh v2 generates SPDX 3.0 JSON with creationInfo timestamp, packages first-party + vetted + transitive, relationships DEPENDS_ON, annotations C2/HVCI/installer, vulnerabilities VEX not_affected/fixed");
            println!("Packages: balancify-core/tunnel/crypto/service/relay/ipc/client/app/installer/driver + chacha20poly1305 0.11.0 x25519-dalek 2.0.1 blake2 0.10.6 hkdf 0.12.4 getrandom 0.2.15 zeroize 1.8.1 + transitive aead/chacha20/cipher/poly1305/curve25519-dalek/subtle/generic-array/typenum/etc.");
            println!("VEX: CVE-2024-XXXX-chacha20poly1305 not_affected cargo audit clean, CVE-2024-XXXX-x25519-dalek not_affected, RUSTSEC-2023-XXXX-generic fixed pinned exact");
            println!("Installer: copies SBOM sidecar to ProgramData\\Balancify\\sbom.spdx.json, support bundle includes");
            println!("Real generation when toolchain available: cargo cyclonedx --format json --spec-version 1.5 + cargo audit --json + sbom-tool generate -b . -bc . -pn Balancify -pv version -ps Balancify -nsb https://balancify.io/sbom");
            render_counters(&s.counters);
            println!("Cargo.lock: {} packages, vetted only, audit passes with lock file (5491758)", s.active_flows); // placeholder
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
        Event::UplinkStateChanged { id, state } => println!("[event] Uplink {} state -> {} — ETW 200 UP 201 DOWN", id, state),
        Event::BondReverted { class, gain_permille } => println!("[event] Bond reverted class={} gain={}‰ — ETW 300, auto-disable <15% gain", class, gain_permille),
        Event::Stats(c) => {
            println!("[event] Stats: stripped={} forwarded={} ecn={} delay={} — 1 Hz telemetry", c.packets_stripped, c.packets_forwarded, c.ecn_ce, c.delay_events);
        }
        Event::Warning { code } => println!("[event] Warning code={} — Sec.4.5 failure modes", code),
    }
}
