//! Balancify IPC contract **v1** -- service ⇄ desktop shell (blueprint Sec.9.2,
//! roadmap Phase 3 milestone M6).
//!
//! Rules this contract exists to satisfy:
//! - *Authenticated IPC ... message schema versioned and length-checked* (Sec.9.2):
//!   every frame carries a schema version and an explicit payload length, and a
//!   truncated or over-long frame is rejected, never parsed partially.
//! - *The shell never opens sockets or touches the driver* (Sec.8.1): the shell can
//!   only express itself through [`Request`]; everything it renders comes back
//!   as [`Response`] or [`Event`].
//! - *Minimal dependencies* (ADR-0002): the encoding is first-party. No
//!   serialisation crate, no schema generator -- see ADR-0004 for why that beats
//!   JSON here.
//!
//! # Transport
//! Named pipe with an explicit SDDL on Windows (Sec.9.2); the framing below is
//! transport-independent, so the same contract runs over a socket pair in the
//! lab and in CI on Linux.
//!
//! # Framing
//! ```text
//! 0       1       2                       6                       6+len
//! +-------+-------+-----------------------+-----------------------+
//! | ver=1 | kind  | payload length (u32 BE)| payload (len bytes)   |
//! +-------+-------+-----------------------+-----------------------+
//! ```
//! `kind`: 1 = request, 2 = response, 3 = event. Payloads longer than
//! [`MAX_PAYLOAD`] are refused outright.

pub mod server;

pub const SCHEMA_VERSION: u8 = 1;
pub const FRAME_HEADER_LEN: usize = 6;
/// Hard ceiling on one message. Status snapshots are a few hundred bytes; a
/// multi-megabyte frame on a control pipe is an attack, not a use case (Sec.9.1a).
pub const MAX_PAYLOAD: usize = 1 << 20;

// ------------------------------------------------------------------- framing

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Request,
    Response,
    Event,
}

impl FrameKind {
    pub const fn as_u8(self) -> u8 {
        match self {
            FrameKind::Request => 1,
            FrameKind::Response => 2,
            FrameKind::Event => 3,
        }
    }
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => FrameKind::Request,
            2 => FrameKind::Response,
            3 => FrameKind::Event,
            _ => return None,
        })
    }
}

/// Encode into a byte buffer.
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self { Self { buf: Vec::new() } }
    pub fn with_capacity(n: usize) -> Self { Self { buf: Vec::with_capacity(n) } }
    pub fn u8(&mut self, v: u8) -> &mut Self { self.buf.push(v); self }
    pub fn u16(&mut self, v: u16) -> &mut Self { self.buf.extend_from_slice(&v.to_be_bytes()); self }
    pub fn u32(&mut self, v: u32) -> &mut Self { self.buf.extend_from_slice(&v.to_be_bytes()); self }
    pub fn u64(&mut self, v: u64) -> &mut Self { self.buf.extend_from_slice(&v.to_be_bytes()); self }
    pub fn i16(&mut self, v: i16) -> &mut Self { self.buf.extend_from_slice(&v.to_be_bytes()); self }
    pub fn bool(&mut self, v: bool) -> &mut Self { self.u8(v as u8) }
    /// Length-prefixed UTF-8 string (u8 byte length; longer names are
    /// truncated on a char boundary so the prefix is always valid UTF-8).
    pub fn str(&mut self, s: &str) -> &mut Self {
        let mut end = s.len().min(255);
        while end > 0 && !s.is_char_boundary(end) { end -= 1; }
        let t = &s[..end];
        self.u8(t.len() as u8);
        self.buf.extend_from_slice(t.as_bytes());
        self
    }
    /// Encode a UTF-8 string with a u32 byte length for large payload fields.
    pub fn str32(&mut self, s: &str) -> &mut Self {
        assert!(
            self.buf.len().saturating_add(4).saturating_add(s.len()) <= MAX_PAYLOAD,
            "IPC string exceeds frame payload limit",
        );
        self.u32(s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
        self
    }
    pub fn into_bytes(self) -> Vec<u8> { self.buf }
    pub fn len(&self) -> usize { self.buf.len() }
    pub fn is_empty(&self) -> bool { self.buf.is_empty() }
}

impl Default for Writer {
    fn default() -> Self { Self::new() }
}

/// Decode with explicit length checks. Every accessor returns `None` past the
/// end of the buffer -- there is no partial-parse state to get wrong.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self { Self { buf, pos: 0 } }
    pub fn remaining(&self) -> usize { self.buf.len().saturating_sub(self.pos) }
    pub fn is_empty(&self) -> bool { self.remaining() == 0 }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.remaining() < n { return None; }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> { self.take(1).map(|b| b[0]) }
    pub fn u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }
    pub fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn u64(&mut self) -> Option<u64> {
        let b = self.take(8)?;
        Some(u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }
    pub fn i16(&mut self) -> Option<i16> {
        let b = self.take(2)?;
        Some(i16::from_be_bytes([b[0], b[1]]))
    }
    /// Decode a canonical boolean. Values other than zero or one are malformed.
    pub fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    pub fn string(&mut self) -> Option<String> {
        let n = self.u8()? as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).ok()
    }
    pub fn string32(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        if n > MAX_PAYLOAD { return None; }
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

/// Split one frame off the front of a stream buffer.
///
/// Returns `(kind, payload, consumed)`. `Ok(None)` means "need more bytes",
/// which is what a pipe reader does with a partial write. `Err` means the
/// stream is corrupt and the connection must be dropped (Sec.9.2 length-checked).
pub fn decode_frame(buf: &[u8]) -> Result<Option<(FrameKind, &[u8], usize)>, FrameError> {
    if buf.len() < FRAME_HEADER_LEN { return Ok(None); }
    if buf[0] != SCHEMA_VERSION { return Err(FrameError::UnsupportedVersion(buf[0])); }
    let kind = FrameKind::from_u8(buf[1]).ok_or(FrameError::UnknownKind(buf[1]))?;
    let len = u32::from_be_bytes([buf[2], buf[3], buf[4], buf[5]]) as usize;
    if len > MAX_PAYLOAD { return Err(FrameError::TooLarge(len)); }
    if buf.len() < FRAME_HEADER_LEN + len { return Ok(None); }
    let payload = &buf[FRAME_HEADER_LEN..FRAME_HEADER_LEN + len];
    Ok(Some((kind, payload, FRAME_HEADER_LEN + len)))
}

/// Encode one frame (header + payload).
pub fn encode_frame(kind: FrameKind, payload: &[u8], out: &mut Vec<u8>) -> Result<(), FrameError> {
    if payload.len() > MAX_PAYLOAD { return Err(FrameError::TooLarge(payload.len())); }
    out.push(SCHEMA_VERSION);
    out.push(kind.as_u8());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    UnsupportedVersion(u8),
    UnknownKind(u8),
    TooLarge(usize),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::UnsupportedVersion(v) => write!(f, "unsupported IPC schema version {v}"),
            FrameError::UnknownKind(v) => write!(f, "unknown IPC frame kind {v}"),
            FrameError::TooLarge(n) => write!(f, "IPC frame too large: {n} bytes"),
        }
    }
}

impl std::error::Error for FrameError {}

// ------------------------------------------------------------------ messages

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Ping,
    GetStatus,
    ListUplinks,
    ListFlowRules,
    SetFlowRule,
    RemoveFlowRule,
    SetBondMode,
    SetDuplicateMode,
    SetAllowMetered,
    RunSelfTest,
    SubscribeEvents,
    GetSupportBundle,
}

impl Opcode {
    pub const fn as_u8(self) -> u8 {
        match self {
            Opcode::Ping => 0,
            Opcode::GetStatus => 1,
            Opcode::ListUplinks => 2,
            Opcode::ListFlowRules => 3,
            Opcode::SetFlowRule => 4,
            Opcode::RemoveFlowRule => 5,
            Opcode::SetBondMode => 6,
            Opcode::SetDuplicateMode => 7,
            Opcode::SetAllowMetered => 8,
            Opcode::RunSelfTest => 9,
            Opcode::SubscribeEvents => 10,
            Opcode::GetSupportBundle => 11,
        }
    }
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Opcode::Ping,
            1 => Opcode::GetStatus,
            2 => Opcode::ListUplinks,
            3 => Opcode::ListFlowRules,
            4 => Opcode::SetFlowRule,
            5 => Opcode::RemoveFlowRule,
            6 => Opcode::SetBondMode,
            7 => Opcode::SetDuplicateMode,
            8 => Opcode::SetAllowMetered,
            9 => Opcode::RunSelfTest,
            10 => Opcode::SubscribeEvents,
            11 => Opcode::GetSupportBundle,
            _ => return None,
        })
    }
    pub fn all() -> [Opcode; 12] {
        [
            Opcode::Ping,
            Opcode::GetStatus,
            Opcode::ListUplinks,
            Opcode::ListFlowRules,
            Opcode::SetFlowRule,
            Opcode::RemoveFlowRule,
            Opcode::SetBondMode,
            Opcode::SetDuplicateMode,
            Opcode::SetAllowMetered,
            Opcode::RunSelfTest,
            Opcode::SubscribeEvents,
            Opcode::GetSupportBundle,
        ]
    }
}

/// Flow-class tag on the wire (mirrors `balancify_core::FlowClass`).
pub const CLASS_BULK: u8 = 0;
pub const CLASS_LATENCY_SENSITIVE: u8 = 1;
pub const CLASS_PINNED: u8 = 2;

/// Link-state tag on the wire (mirrors `balancify_core::LinkState`).
pub const STATE_PROBING: u8 = 0;
pub const STATE_UP: u8 = 1;
pub const STATE_DEGRADED: u8 = 2;
pub const STATE_DOWN: u8 = 3;
pub const STATE_RECOVERING: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    UnknownOpcode,
    BadRequest,
    Unavailable,
    Internal,
}

impl Status {
    pub const fn as_u8(self) -> u8 {
        match self {
            Status::Ok => 0,
            Status::UnknownOpcode => 1,
            Status::BadRequest => 2,
            Status::Unavailable => 3,
            Status::Internal => 4,
        }
    }
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Status::Ok,
            1 => Status::UnknownOpcode,
            2 => Status::BadRequest,
            3 => Status::Unavailable,
            4 => Status::Internal,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Ping,
    GetStatus,
    ListUplinks,
    ListFlowRules,
    SetFlowRule(FlowRule),
    RemoveFlowRule { id: u32 },
    SetBondMode { enabled: bool },
    SetDuplicateMode { enabled: bool },
    SetAllowMetered { allowed: bool },
    RunSelfTest,
    SubscribeEvents { interval_hz: u8 },
    GetSupportBundle,
}

/// One row of the Rules screen (Sec.8.2), and the wire form of a core rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowRule {
    pub id: u32,
    pub name: String,
    /// One of `CLASS_*`.
    pub class: u8,
    /// Uplink id when `class == CLASS_PINNED`, else 0.
    pub pinned_uplink: u8,
    pub priority: u32,
    /// IP protocol, 0 = any.
    pub proto: u8,
    /// Inclusive destination port range; `0..=0` means any port.
    pub port_start: u16,
    pub port_end: u16,
    pub duplicate: bool,
}

impl FlowRule {
    pub fn any_port(&self) -> bool { self.port_start == 0 && self.port_end == 0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UplinkInfo {
    pub id: u8,
    /// One of `STATE_*`.
    pub state: u8,
    pub rtt_ms: u32,
    pub loss_permille: u16,
    pub capacity_bps: u64,
    /// Weight relative to the best uplink, in per-mille (1000 = best).
    pub weight_permille: u16,
    pub metered: bool,
    pub eligible: bool,
}

/// Everything the Home screen and tray flyout render (Sec.8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub service_version: String,
    pub schema_version: u8,
    pub policy_revision: u64,
    pub rules_revision: u64,
    pub bond_enabled: bool,
    pub duplicate_mode: bool,
    pub allow_metered: bool,
    pub adapter_mtu: u16,
    pub uplinks: Vec<UplinkInfo>,
    pub active_flows: u32,
    pub counters: Counters,
}

/// Counters only -- never destinations or content (Sec.9.2 privacy telemetry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counters {
    pub packets_stripped: u64,
    pub packets_forwarded: u64,
    pub duplicates_sent: u64,
    pub reinjections: u64,
    pub delivered: u64,
    pub out_of_order: u64,
    pub refused: u64,
    pub auth_failed: u64,
    pub ecn_ce: u64,
    pub delay_events: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Pong { service_version: String },
    Status(StatusSnapshot),
    Uplinks(Vec<UplinkInfo>),
    FlowRules(Vec<FlowRule>),
    Ack,
    SelfTest { passed: bool, detail: String },
    SupportBundle(String),
    Error { status: Status, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The policy snapshot changed; the shell should re-fetch or use `uplinks`.
    PolicyChanged { revision: u64 },
    UplinkStateChanged { id: u8, state: u8 },
    /// Sec.6.4 rule 6: a flow class was reverted to flow-level placement.
    BondReverted { class: u8, gain_permille: i16 },
    /// Counters for the Diagnostics view (Sec.8.2), pushed at the subscribed rate.
    Stats(Counters),
    /// A condition the user should see (Sec.4.5 failure modes).
    Warning { code: u8 },
}

/// Warning codes surfaced to the user (Sec.4.5). Kept numeric on the wire so new
/// codes never break an older shell.
pub const WARN_ALL_PATHS_DOWN: u8 = 1;
pub const WARN_RELAY_UNREACHABLE: u8 = 2;
pub const WARN_METERED_BONDING: u8 = 3;
pub const WARN_VPN_COEXISTENCE: u8 = 4;
pub const WARN_ADAPTER_MTU_BELOW_IPV4_MIN: u8 = 5;

// ------------------------------------------------------------------ encoding

impl Request {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(64);
        match self {
            Request::Ping => { w.u8(Opcode::Ping.as_u8()); }
            Request::GetStatus => { w.u8(Opcode::GetStatus.as_u8()); }
            Request::ListUplinks => { w.u8(Opcode::ListUplinks.as_u8()); }
            Request::ListFlowRules => { w.u8(Opcode::ListFlowRules.as_u8()); }
            Request::RunSelfTest => { w.u8(Opcode::RunSelfTest.as_u8()); }
            Request::SetBondMode { enabled } => {
                w.u8(Opcode::SetBondMode.as_u8()).bool(*enabled);
            }
            Request::SetDuplicateMode { enabled } => {
                w.u8(Opcode::SetDuplicateMode.as_u8()).bool(*enabled);
            }
            Request::SetAllowMetered { allowed } => {
                w.u8(Opcode::SetAllowMetered.as_u8()).bool(*allowed);
            }
            Request::SubscribeEvents { interval_hz } => {
                w.u8(Opcode::SubscribeEvents.as_u8()).u8(*interval_hz);
            }
            Request::GetSupportBundle => {
                w.u8(Opcode::GetSupportBundle.as_u8());
            }
            Request::RemoveFlowRule { id } => {
                w.u8(Opcode::RemoveFlowRule.as_u8()).u32(*id);
            }
            Request::SetFlowRule(r) => {
                w.u8(Opcode::SetFlowRule.as_u8());
                encode_rule(&mut w, r);
            }
        }
        w.into_bytes()
    }

    pub fn decode(payload: &[u8]) -> Option<Self> {
        let mut r = Reader::new(payload);
        let op = Opcode::from_u8(r.u8()?)?;
        let request = match op {
            Opcode::Ping => Request::Ping,
            Opcode::GetStatus => Request::GetStatus,
            Opcode::ListUplinks => Request::ListUplinks,
            Opcode::ListFlowRules => Request::ListFlowRules,
            Opcode::RunSelfTest => Request::RunSelfTest,
            Opcode::SetBondMode => Request::SetBondMode { enabled: r.bool()? },
            Opcode::SetDuplicateMode => Request::SetDuplicateMode { enabled: r.bool()? },
            Opcode::SetAllowMetered => Request::SetAllowMetered { allowed: r.bool()? },
            Opcode::SubscribeEvents => Request::SubscribeEvents { interval_hz: r.u8()? },
            Opcode::GetSupportBundle => Request::GetSupportBundle,
            Opcode::RemoveFlowRule => Request::RemoveFlowRule { id: r.u32()? },
            Opcode::SetFlowRule => Request::SetFlowRule(decode_rule(&mut r)?),
        };
        // Reject extensions or garbage rather than interpreting only a prefix.
        if !r.is_empty() { return None; }
        Some(request)
    }
}

fn encode_rule(w: &mut Writer, r: &FlowRule) {
    w.u32(r.id)
        .str(&r.name)
        .u8(r.class)
        .u8(r.pinned_uplink)
        .u32(r.priority)
        .u8(r.proto)
        .u16(r.port_start)
        .u16(r.port_end)
        .bool(r.duplicate);
}

fn decode_rule(r: &mut Reader<'_>) -> Option<FlowRule> {
    Some(FlowRule {
        id: r.u32()?,
        name: r.string()?,
        class: r.u8()?,
        pinned_uplink: r.u8()?,
        priority: r.u32()?,
        proto: r.u8()?,
        port_start: r.u16()?,
        port_end: r.u16()?,
        duplicate: r.bool()?,
    })
}

fn encode_uplink(w: &mut Writer, u: &UplinkInfo) {
    w.u8(u.id)
        .u8(u.state)
        .u32(u.rtt_ms)
        .u16(u.loss_permille)
        .u64(u.capacity_bps)
        .u16(u.weight_permille)
        .bool(u.metered)
        .bool(u.eligible);
}

fn decode_uplink(r: &mut Reader<'_>) -> Option<UplinkInfo> {
    Some(UplinkInfo {
        id: r.u8()?,
        state: r.u8()?,
        rtt_ms: r.u32()?,
        loss_permille: r.u16()?,
        capacity_bps: r.u64()?,
        weight_permille: r.u16()?,
        metered: r.bool()?,
        eligible: r.bool()?,
    })
}

fn encode_counters(w: &mut Writer, c: &Counters) {
    w.u64(c.packets_stripped)
        .u64(c.packets_forwarded)
        .u64(c.duplicates_sent)
        .u64(c.reinjections)
        .u64(c.delivered)
        .u64(c.out_of_order)
        .u64(c.refused)
        .u64(c.auth_failed)
        .u64(c.ecn_ce)
        .u64(c.delay_events);
}

fn decode_counters(r: &mut Reader<'_>) -> Option<Counters> {
    let packets_stripped = r.u64()?;
    let packets_forwarded = r.u64()?;
    let duplicates_sent = r.u64()?;
    let reinjections = r.u64()?;
    let delivered = r.u64()?;
    let out_of_order = r.u64()?;
    let refused = r.u64()?;
    let auth_failed = r.u64()?;
    // v1 extension: ECN + delay counters, backward compat -- missing = 0
    let ecn_ce = r.u64().unwrap_or(0);
    let delay_events = r.u64().unwrap_or(0);
    Some(Counters {
        packets_stripped,
        packets_forwarded,
        duplicates_sent,
        reinjections,
        delivered,
        out_of_order,
        refused,
        auth_failed,
        ecn_ce,
        delay_events,
    })
}

fn encode_status(w: &mut Writer, s: &StatusSnapshot) {
    w.str(&s.service_version)
        .u8(s.schema_version)
        .u64(s.policy_revision)
        .u64(s.rules_revision)
        .bool(s.bond_enabled)
        .bool(s.duplicate_mode)
        .bool(s.allow_metered)
        .u16(s.adapter_mtu)
        .u32(s.uplinks.len() as u32);
    for u in &s.uplinks { encode_uplink(w, u); }
    w.u32(s.active_flows);
    encode_counters(w, &s.counters);
}

fn decode_status(r: &mut Reader<'_>) -> Option<StatusSnapshot> {
    let service_version = r.string()?;
    let schema_version = r.u8()?;
    let policy_revision = r.u64()?;
    let rules_revision = r.u64()?;
    let bond_enabled = r.bool()?;
    let duplicate_mode = r.bool()?;
    let allow_metered = r.bool()?;
    let adapter_mtu = r.u16()?;
    let n = r.u32()? as usize;
    if n > 256 { return None; } // 255 uplink ids max; anything more is corrupt
    let mut uplinks = Vec::with_capacity(n);
    for _ in 0..n { uplinks.push(decode_uplink(r)?); }
    let active_flows = r.u32()?;
    let counters = decode_counters(r)?;
    Some(StatusSnapshot {
        service_version,
        schema_version,
        policy_revision,
        rules_revision,
        bond_enabled,
        duplicate_mode,
        allow_metered,
        adapter_mtu,
        uplinks,
        active_flows,
        counters,
    })
}

/// Response tags (separate namespace from opcodes on purpose: a response is
/// never mistaken for a request even if the framing is misaligned).
const RESP_PONG: u8 = 1;
const RESP_STATUS: u8 = 2;
const RESP_UPLINKS: u8 = 3;
const RESP_FLOW_RULES: u8 = 4;
const RESP_ACK: u8 = 5;
const RESP_SELF_TEST: u8 = 6;
const RESP_ERROR: u8 = 7;
const RESP_SUPPORT_BUNDLE: u8 = 8;

impl Response {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(128);
        match self {
            Response::Pong { service_version } => {
                w.u8(RESP_PONG).str(service_version);
            }
            Response::Status(s) => {
                w.u8(RESP_STATUS);
                encode_status(&mut w, s);
            }
            Response::Uplinks(list) => {
                w.u8(RESP_UPLINKS).u32(list.len() as u32);
                for u in list { encode_uplink(&mut w, u); }
            }
            Response::FlowRules(list) => {
                w.u8(RESP_FLOW_RULES).u32(list.len() as u32);
                for r in list { encode_rule(&mut w, r); }
            }
            Response::Ack => {
                w.u8(RESP_ACK);
            }
            Response::SelfTest { passed, detail } => {
                w.u8(RESP_SELF_TEST).bool(*passed).str(detail);
            }
            Response::SupportBundle(json) => {
                w.u8(RESP_SUPPORT_BUNDLE).str32(json);
            }
            Response::Error { status, message } => {
                w.u8(RESP_ERROR).u8(status.as_u8()).str(message);
            }
        }
        w.into_bytes()
    }

    pub fn decode(payload: &[u8]) -> Option<Self> {
        let mut r = Reader::new(payload);
        let response = match r.u8()? {
            RESP_PONG => Response::Pong { service_version: r.string()? },
            RESP_STATUS => Response::Status(decode_status(&mut r)?),
            RESP_UPLINKS => {
                let n = r.u32()? as usize;
                if n > 256 { return None; }
                let mut list = Vec::with_capacity(n);
                for _ in 0..n { list.push(decode_uplink(&mut r)?); }
                Response::Uplinks(list)
            }
            RESP_FLOW_RULES => {
                let n = r.u32()? as usize;
                if n > 4096 { return None; }
                let mut list = Vec::with_capacity(n);
                for _ in 0..n { list.push(decode_rule(&mut r)?); }
                Response::FlowRules(list)
            }
            RESP_ACK => Response::Ack,
            RESP_SELF_TEST => Response::SelfTest { passed: r.bool()?, detail: r.string()? },
            RESP_SUPPORT_BUNDLE => Response::SupportBundle(r.string32()?),
            RESP_ERROR => Response::Error {
                status: Status::from_u8(r.u8()?)?,
                message: r.string()?,
            },
            _ => return None,
        };
        if !r.is_empty() { return None; }
        Some(response)
    }
}

const EV_POLICY_CHANGED: u8 = 1;
const EV_UPLINK_STATE: u8 = 2;
const EV_BOND_REVERTED: u8 = 3;
const EV_STATS: u8 = 4;
const EV_WARNING: u8 = 5;

impl Event {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(64);
        match self {
            Event::PolicyChanged { revision } => {
                w.u8(EV_POLICY_CHANGED).u64(*revision);
            }
            Event::UplinkStateChanged { id, state } => {
                w.u8(EV_UPLINK_STATE).u8(*id).u8(*state);
            }
            Event::BondReverted { class, gain_permille } => {
                w.u8(EV_BOND_REVERTED).u8(*class).i16(*gain_permille);
            }
            Event::Stats(c) => {
                w.u8(EV_STATS);
                encode_counters(&mut w, c);
            }
            Event::Warning { code } => {
                w.u8(EV_WARNING).u8(*code);
            }
        }
        w.into_bytes()
    }

    pub fn decode(payload: &[u8]) -> Option<Self> {
        let mut r = Reader::new(payload);
        let event = match r.u8()? {
            EV_POLICY_CHANGED => Event::PolicyChanged { revision: r.u64()? },
            EV_UPLINK_STATE => Event::UplinkStateChanged { id: r.u8()?, state: r.u8()? },
            EV_BOND_REVERTED => Event::BondReverted { class: r.u8()?, gain_permille: r.i16()? },
            EV_STATS => Event::Stats(decode_counters(&mut r)?),
            EV_WARNING => Event::Warning { code: r.u8()? },
            _ => return None,
        };
        if !r.is_empty() { return None; }
        Some(event)
    }
}

/// Convenience: build the frame bytes for a message.
pub fn frame_request(req: &Request) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = encode_frame(FrameKind::Request, &req.encode(), &mut out);
    out
}
pub fn frame_response(resp: &Response) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = encode_frame(FrameKind::Response, &resp.encode(), &mut out);
    out
}
pub fn frame_event(ev: &Event) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = encode_frame(FrameKind::Event, &ev.encode(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_status() -> StatusSnapshot {
        StatusSnapshot {
            service_version: "0.1.0".to_string(),
            schema_version: SCHEMA_VERSION,
            policy_revision: 7,
            rules_revision: 3,
            bond_enabled: true,
            duplicate_mode: false,
            allow_metered: false,
            adapter_mtu: 1440,
            uplinks: vec![
                UplinkInfo {
                    id: 0,
                    state: STATE_UP,
                    rtt_ms: 12,
                    loss_permille: 1,
                    capacity_bps: 200_000_000,
                    weight_permille: 1000,
                    metered: false,
                    eligible: true,
                },
                UplinkInfo {
                    id: 1,
                    state: STATE_RECOVERING,
                    rtt_ms: 48,
                    loss_permille: 22,
                    capacity_bps: 50_000_000,
                    weight_permille: 310,
                    metered: true,
                    eligible: false,
                },
            ],
            active_flows: 42,
            counters: Counters {
                packets_stripped: 1000,
                packets_forwarded: 250,
                duplicates_sent: 12,
                reinjections: 3,
                delivered: 1240,
                out_of_order: 4,
                refused: 1,
                auth_failed: 0,
                ecn_ce: 2,
                delay_events: 1,
            },
        }
    }

    fn sample_rule() -> FlowRule {
        FlowRule {
            id: 1,
            name: "dns".to_string(),
            class: CLASS_LATENCY_SENSITIVE,
            pinned_uplink: 0,
            priority: 10,
            proto: 0,
            port_start: 53,
            port_end: 53,
            duplicate: true,
        }
    }

    #[test]
    fn ipc_string_decoder_rejects_invalid_utf8() {
        let short_string = [1, 0xff];
        let mut reader = Reader::new(&short_string);
        assert_eq!(reader.string(), None);

        let long_string = [0, 0, 0, 1, 0xff];
        let mut reader = Reader::new(&long_string);
        assert_eq!(reader.string32(), None);
    }

    #[test]
    fn framing_roundtrips_and_reports_consumed_bytes() {
        let mut stream = Vec::new();
        encode_frame(FrameKind::Request, &Request::GetStatus.encode(), &mut stream).unwrap();
        encode_frame(FrameKind::Event, &Event::PolicyChanged { revision: 9 }.encode(), &mut stream).unwrap();

        let (kind, payload, consumed) = decode_frame(&stream).unwrap().unwrap();
        assert_eq!(kind, FrameKind::Request);
        assert_eq!(Request::decode(payload), Some(Request::GetStatus));
        let rest = &stream[consumed..];
        let (kind2, payload2, consumed2) = decode_frame(rest).unwrap().unwrap();
        assert_eq!(kind2, FrameKind::Event);
        assert_eq!(Event::decode(payload2), Some(Event::PolicyChanged { revision: 9 }));
        assert_eq!(consumed2, rest.len(), "the stream is fully consumed");
    }

    #[test]
    fn partial_frame_means_need_more_bytes_not_error() {
        let frame = frame_request(&Request::ListUplinks);
        for cut in 1..frame.len() {
            assert_eq!(decode_frame(&frame[..cut]).unwrap(), None, "cut at {cut} must wait, not fail");
        }
        assert!(decode_frame(&frame).unwrap().is_some());
    }

    #[test]
    fn corrupt_streams_are_rejected() {
        let mut bad = frame_request(&Request::Ping);
        bad[0] = 99;
        assert_eq!(decode_frame(&bad), Err(FrameError::UnsupportedVersion(99)));

        let mut bad = frame_request(&Request::Ping);
        bad[1] = 77;
        assert_eq!(decode_frame(&bad), Err(FrameError::UnknownKind(77)));

        // declared length beyond the ceiling: refuse before allocating
        let mut hdr = vec![SCHEMA_VERSION, FrameKind::Request.as_u8()];
        hdr.extend_from_slice(&((MAX_PAYLOAD as u32) + 1).to_be_bytes());
        assert_eq!(decode_frame(&hdr), Err(FrameError::TooLarge(MAX_PAYLOAD + 1)));

        // an oversized payload cannot even be encoded
        let mut out = Vec::new();
        assert!(encode_frame(FrameKind::Request, &vec![0u8; MAX_PAYLOAD + 1], &mut out).is_err());
    }

    #[test]
    fn truncated_payloads_decode_to_none() {
        let full = sample_status().encode_for_test();
        // New format has 2 optional trailing u64s (ecn_ce, delay_events) for backward compat.
        // Truncations that remove only those optional fields should still parse as old version,
        // but any other truncation must fail.
        for cut in 0..full.len() {
            if let Some(decoded) = Response::decode(&full[..cut]) {
                let remaining = full.len() - cut;
                assert!(remaining <= 16, "cut at {cut} (missing {remaining} bytes) must not parse, got {decoded:?}");
            }
        }
        assert!(Response::decode(&full).is_some());

        // Old 8-field payload (truncate 16 bytes) must parse with 0 defaults
        let truncated = &full[..full.len() - 16];
        let dec = Response::decode(truncated).expect("old 8-field payload must parse");
        if let Response::Status(st) = dec {
            assert_eq!(st.counters.ecn_ce, 0);
            assert_eq!(st.counters.delay_events, 0);
        } else {
            panic!("expected status");
        }

        // 9-field payload (truncate 8 bytes) must parse with delay_events=0
        let truncated9 = &full[..full.len() - 8];
        let dec9 = Response::decode(truncated9).expect("9-field payload must parse");
        if let Response::Status(st) = dec9 {
            assert_eq!(st.counters.delay_events, 0);
        } else {
            panic!("expected status");
        }
    }

    #[test]
    fn request_decoder_rejects_trailing_bytes_and_noncanonical_booleans() {
        assert_eq!(Request::decode(&[Opcode::Ping.as_u8(), 0]), None);
        assert_eq!(Request::decode(&[Opcode::SetBondMode.as_u8(), 2]), None);
        assert_eq!(Request::decode(&[Opcode::SetBondMode.as_u8(), 1]), Some(Request::SetBondMode { enabled: true }));
    }

    #[test]
    fn response_and_event_decoders_reject_trailing_bytes() {
        let mut response = Response::Ack.encode();
        response.push(0);
        assert_eq!(Response::decode(&response), None);

        let mut event = Event::Warning { code: WARN_ALL_PATHS_DOWN }.encode();
        event.push(0);
        assert_eq!(Event::decode(&event), None);
    }

    #[test]
    fn every_request_roundtrips() {
        let requests = vec![
            Request::Ping,
            Request::GetStatus,
            Request::ListUplinks,
            Request::ListFlowRules,
            Request::RunSelfTest,
            Request::SetBondMode { enabled: true },
            Request::SetBondMode { enabled: false },
            Request::SetDuplicateMode { enabled: true },
            Request::SetAllowMetered { allowed: true },
            Request::SubscribeEvents { interval_hz: 1 },
            Request::GetSupportBundle,
            Request::RemoveFlowRule { id: 4242 },
            Request::SetFlowRule(sample_rule()),
        ];
        for req in requests {
            assert_eq!(Request::decode(&req.encode()), Some(req.clone()));
        }
    }

    #[test]
    fn every_response_roundtrips() {
        let responses = vec![
            Response::Pong { service_version: "0.1.0".to_string() },
            Response::Status(sample_status()),
            Response::Uplinks(sample_status().uplinks),
            Response::FlowRules(vec![sample_rule()]),
            Response::Ack,
            Response::SelfTest { passed: true, detail: "loopback ok".to_string() },
            Response::SupportBundle(format!("{{\"diagnostic\":\"{}\"}}", "x".repeat(1024))),
            Response::Error { status: Status::Unavailable, message: "no uplink".to_string() },
        ];
        for resp in responses {
            assert_eq!(Response::decode(&resp.encode()), Some(resp.clone()));
        }
    }

    #[test]
    fn every_event_roundtrips() {
        let events = vec![
            Event::PolicyChanged { revision: 12 },
            Event::UplinkStateChanged { id: 2, state: STATE_DOWN },
            Event::BondReverted { class: CLASS_BULK, gain_permille: -40 },
            Event::Stats(Counters { reinjections: 5, ..Counters::default() }),
            Event::Warning { code: WARN_ALL_PATHS_DOWN },
        ];
        for ev in events {
            assert_eq!(Event::decode(&ev.encode()), Some(ev.clone()));
        }
    }

    #[test]
    fn unicode_and_long_names_survive() {
        let rule = FlowRule { name: "vidéo-conférence 🎧".to_string(), ..sample_rule() };
        let back = FlowRule::decode_test(&rule.encode_for_test()).unwrap();
        assert_eq!(back.name, rule.name);
        let long = FlowRule { name: "x".repeat(400), ..sample_rule() };
        let back = FlowRule::decode_test(&long.encode_for_test()).unwrap();
        assert_eq!(back.name.len(), 255, "names are truncated, not malformed");
    }

    #[test]
    fn opcodes_are_stable_and_complete() {
        for op in Opcode::all() {
            assert_eq!(Opcode::from_u8(op.as_u8()), Some(op));
        }
        assert_eq!(Opcode::all().len(), 12);
        assert_eq!(Opcode::GetSupportBundle.as_u8(), 11);
        assert_eq!(Opcode::from_u8(200), None);
        for s in 0..=4u8 { assert!(Status::from_u8(s).is_some()); }
        assert_eq!(Status::from_u8(5), None);
    }

    impl StatusSnapshot {
        fn encode_for_test(&self) -> Vec<u8> { Response::Status(self.clone()).encode() }
    }
    impl FlowRule {
        fn encode_for_test(&self) -> Vec<u8> { Request::SetFlowRule(self.clone()).encode() }
        fn decode_test(payload: &[u8]) -> Option<FlowRule> {
            match Request::decode(payload)? {
                Request::SetFlowRule(r) => Some(r),
                _ => None,
            }
        }
    }
}
