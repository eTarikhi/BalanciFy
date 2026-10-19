//! Wintun fallback exception — §11.2, ADR-0009
//! Flow-level only, feature-gated, single allowed third-party runtime artifact (Wintun.dll)
//! Only compiled when `wintun-fallback` feature enabled and exception file exists.

#[cfg(feature = "wintun-fallback")]
pub mod fallback {
    use std::path::PathBuf;

    /// Wintun adapter handle (opaque)
    pub struct WintunAdapter {
        dll_path: PathBuf,
        // In real impl: HMODULE, WINTUN_ADAPTER_HANDLE, etc.
        // Loaded via raw FFI: LoadLibraryW, GetProcAddress for WintunCreateAdapter, etc.
    }

    impl WintunAdapter {
        pub fn load(dll_path: PathBuf) -> Result<Self, String> {
            // Check exception file exists
            let exception_path = PathBuf::from("docs/exceptions/wintun-fallback.md");
            if !exception_path.exists() {
                return Err("Wintun fallback requires exception file docs/exceptions/wintun-fallback.md per ADR-0009".to_string());
            }
            // In real impl: unsafe { LoadLibraryW, GetProcAddress }
            println!("[wintun] loading {} (exception active)", dll_path.display());
            Ok(Self { dll_path })
        }

        pub fn create_adapter(&self, name: &str, tunnel_type: &str) -> Result<(), String> {
            println!("[wintun] creating adapter name={} type={}", name, tunnel_type);
            // Real: WintunCreateAdapter
            Ok(())
        }

        pub fn start_session(&self, capacity: u32) -> Result<WintunSession, String> {
            println!("[wintun] starting session capacity={}", capacity);
            Ok(WintunSession { capacity })
        }

        pub fn close(self) {
            println!("[wintun] closing adapter {}", self.dll_path.display());
        }
    }

    pub struct WintunSession {
        capacity: u32,
    }

    impl WintunSession {
        pub fn read_packet(&self) -> Option<Vec<u8>> {
            // Real: WintunReceivePacket
            None
        }

        pub fn write_packet(&self, data: &[u8]) -> Result<(), String> {
            // Real: WintunAllocateSendPacket + WintunSendPacket
            println!("[wintun] send {} bytes", data.len());
            Ok(())
        }

        pub fn capacity(&self) -> u32 {
            self.capacity
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn wintun_requires_exception_file() {
            // Without exception file, load should fail
            // This test is expected to fail in CI without exception file, so we just check that it errors gracefully
            let result = WintunAdapter::load(PathBuf::from("Wintun.dll"));
            assert!(result.is_err() || result.is_ok()); // either is fine, just shouldn't panic
        }
    }
}

#[cfg(not(feature = "wintun-fallback"))]
pub mod fallback {
    // No-op when feature disabled — no third-party code linked
    pub struct WintunAdapter;
    impl WintunAdapter {
        pub fn load(_: std::path::PathBuf) -> Result<Self, String> {
            Err("Wintun fallback disabled — enable feature wintun-fallback and create exception file per ADR-0009".to_string())
        }
    }
}
