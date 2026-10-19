//! ETW logging stub -- Windows Event Tracing for Windows (Sec.8.2 Diagnostics)
//! On Windows, logs to ETW provider; on non-Windows, falls back to eprintln.
//! Minimal dependencies, no external crate, to keep policy clean.

use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtwLevel {
    Info,
    Warning,
    Error,
    Verbose,
}

#[derive(Debug, Clone)]
pub struct EtwEvent {
    pub level: EtwLevel,
    pub id: u16,
    pub message: String,
    pub at: Instant,
}

pub struct EtwProvider {
    provider_name: String,
    #[cfg(windows)]
    handle: Option<WindowsEtwHandle>,
}

#[cfg(windows)]
struct WindowsEtwHandle {
    // In real impl: REGHANDLE from EventRegister
    _private: (),
}

impl EtwProvider {
    pub fn new(name: &str) -> Self {
        #[cfg(windows)]
        {
            // Real impl would call EventRegister with provider GUID
            // For stub, just store name
            Self {
                provider_name: name.to_string(),
                handle: Some(WindowsEtwHandle { _private: () }),
            }
        }
        #[cfg(not(windows))]
        {
            Self {
                provider_name: name.to_string(),
            }
        }
    }

    pub fn log(&self, level: EtwLevel, id: u16, msg: &str) {
        let ev = EtwEvent {
            level,
            id,
            message: msg.to_string(),
            at: Instant::now(),
        };
        #[cfg(windows)]
        {
            // Real impl: EventWrite with level, id, message
            // For stub, use OutputDebugString + ETW fallback
            let _ = &ev;
            // In real driver, KdPrintEx or EventWrite
            eprintln!("[ETW {} {:?} {}] {}", self.provider_name, level, id, msg);
        }
        #[cfg(not(windows))]
        {
            let _ = ev;
            eprintln!("[ETW {} {:?} {}] {}", self.provider_name, level, id, msg);
        }
    }

    pub fn info(&self, id: u16, msg: &str) {
        self.log(EtwLevel::Info, id, msg);
    }

    pub fn warn(&self, id: u16, msg: &str) {
        self.log(EtwLevel::Warning, id, msg);
    }

    pub fn error(&self, id: u16, msg: &str) {
        self.log(EtwLevel::Error, id, msg);
    }

    pub fn verbose(&self, id: u16, msg: &str) {
        self.log(EtwLevel::Verbose, id, msg);
    }
}

// Well-known event IDs (Sec.8.2)
pub const EVT_SERVICE_START: u16 = 100;
pub const EVT_SERVICE_STOP: u16 = 101;
pub const EVT_UPLINK_UP: u16 = 200;
pub const EVT_UPLINK_DOWN: u16 = 201;
pub const EVT_BOND_REVERTED: u16 = 300;
pub const EVT_REKEY: u16 = 400;
pub const EVT_ECN_CE: u16 = 401;
pub const EVT_DELAY_CONGESTION: u16 = 402;
pub const EVT_RELAY_FAILOVER: u16 = 500;
pub const EVT_AUTH_FAILED: u16 = 600;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn etw_provider_logs() {
        let prov = EtwProvider::new("BalancifyTest");
        prov.info(EVT_SERVICE_START, "service start");
        prov.warn(EVT_UPLINK_DOWN, "uplink down");
        prov.error(EVT_AUTH_FAILED, "auth failed");
    }
}
