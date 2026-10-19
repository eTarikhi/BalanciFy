//! Key storage abstraction (§9.2).
//!
//! Secrets must live in a DPAPI-protected store, TPM-backed where available,
//! and never in the JSON settings. This module defines the trait and provides
//! two implementations:
//!
//! * `MemoryKeyStore` — for tests and for the deterministic rig.
//! * `FileKeyStore` — for lab use, storing keys as raw bytes in a file with
//!   restrictive permissions (0600 on Unix). On Windows, when the `dpapi`
//!   feature is enabled, the file contents are protected via
//!   `CryptProtectData` (DPAPI). The DPAPI implementation is Windows-only and
//!   behind a feature so the crate stays cross-platform.
//!
//! The trait is sans-I/O in the sense that it takes paths as arguments; the
//! actual I/O happens inside the implementation. That keeps the handshake and
//! tunnel crates free of filesystem dependencies.

use crate::keys::{PinnedKeys, StaticKeypair};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    Io(String),
    Corrupt,
    NotFound,
    Dpapi(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(s) => write!(f, "I/O error: {s}"),
            StoreError::Corrupt => write!(f, "key file corrupt or wrong length"),
            StoreError::NotFound => write!(f, "key file not found"),
            StoreError::Dpapi(s) => write!(f, "DPAPI error: {s}"),
        }
    }
}

impl std::error::Error for StoreError {}

/// The abstraction over where static keys live.
pub trait KeyStore {
    /// Load the static keypair, or generate and save one if it does not exist.
    fn load_or_generate(&mut self) -> Result<StaticKeypair, StoreError>;

    /// Save a keypair (overwrites).
    fn save(&mut self, keypair: &StaticKeypair) -> Result<(), StoreError>;

    /// Load the pinned relay/client keys. Missing file => empty set.
    fn load_pinned(&self) -> Result<PinnedKeys, StoreError>;

    /// Save the pinned set.
    fn save_pinned(&mut self, pinned: &PinnedKeys) -> Result<(), StoreError>;
}

/// In-memory store: for tests, deterministic rigs, and as a fallback.
#[derive(Clone, Default)]
pub struct MemoryKeyStore {
    keypair: Option<StaticKeypair>,
    pinned: PinnedKeys,
}

impl std::fmt::Debug for MemoryKeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryKeyStore")
            .field("has_keypair", &self.keypair.is_some())
            .field("pinned_len", &self.pinned.len())
            .finish()
    }
}

impl MemoryKeyStore {
    pub fn new() -> Self { Self::default() }

    pub fn with_keypair(mut self, kp: StaticKeypair) -> Self {
        self.keypair = Some(kp);
        self
    }

    pub fn with_pinned(mut self, pinned: PinnedKeys) -> Self {
        self.pinned = pinned;
        self
    }
}

impl KeyStore for MemoryKeyStore {
    fn load_or_generate(&mut self) -> Result<StaticKeypair, StoreError> {
        if let Some(kp) = &self.keypair {
            Ok(kp.clone())
        } else {
            let kp = StaticKeypair::generate();
            self.keypair = Some(kp.clone());
            Ok(kp)
        }
    }

    fn save(&mut self, keypair: &StaticKeypair) -> Result<(), StoreError> {
        self.keypair = Some(keypair.clone());
        Ok(())
    }

    fn load_pinned(&self) -> Result<PinnedKeys, StoreError> { Ok(self.pinned.clone()) }

    fn save_pinned(&mut self, pinned: &PinnedKeys) -> Result<(), StoreError> {
        self.pinned = pinned.clone();
        Ok(())
    }
}

/// File-backed store. Lab form: raw 32-byte secret in a file with 0600
/// permissions. Production form (Windows, `dpapi` feature): DPAPI-protected.
#[derive(Debug, Clone)]
pub struct FileKeyStore {
    secret_path: PathBuf,
    pinned_path: PathBuf,
}

impl FileKeyStore {
    pub fn new(secret_path: impl Into<PathBuf>, pinned_path: impl Into<PathBuf>) -> Self {
        Self { secret_path: secret_path.into(), pinned_path: pinned_path.into() }
    }

    fn read_secret_file(path: &Path) -> Result<[u8; 32], StoreError> {
        let raw = std::fs::read(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound { StoreError::NotFound }
            else { StoreError::Io(e.to_string()) }
        })?;
        // DPAPI: file contains protected blob; otherwise raw 32 bytes.
        // For migration, if DPAPI unprotect fails and raw is exactly 32 bytes,
        // treat raw as secret (old file) and next save will protect it.
        let data = match dpapi_unprotect(&raw) {
            Ok(d) => d,
            Err(StoreError::Dpapi(_)) => {
                if raw.len() == 32 {
                    raw.clone()
                } else {
                    return Err(StoreError::Corrupt);
                }
            }
            Err(e) => return Err(e),
        };
        if data.len() != 32 { return Err(StoreError::Corrupt); }
        let mut out = [0u8; 32];
        out.copy_from_slice(&data);
        Ok(out)
    }

    fn write_secret_file(path: &Path, secret: &[u8; 32]) -> Result<(), StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::Io(e.to_string()))?;
        }
        let protected = dpapi_protect(secret)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &protected).map_err(|e| StoreError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, path).map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(())
    }

    fn read_pinned_file(path: &Path) -> Result<PinnedKeys, StoreError> {
        let raw = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(PinnedKeys::new()),
            Err(e) => return Err(StoreError::Io(e.to_string())),
        };
        if raw.is_empty() { return Ok(PinnedKeys::new()); }
        // Try DPAPI unprotect; on non-DPAPI builds this is identity.
        // For migration, if DPAPI unprotect fails and raw is multiple of 32,
        // treat raw as pinned keys (old file).
        let data = match dpapi_unprotect(&raw) {
            Ok(d) => d,
            Err(StoreError::Dpapi(_)) => {
                if raw.len() % 32 == 0 {
                    raw.clone()
                } else {
                    return Err(StoreError::Corrupt);
                }
            }
            Err(e) => return Err(e),
        };
        if data.len() % 32 != 0 { return Err(StoreError::Corrupt); }
        let mut pinned = PinnedKeys::new();
        for chunk in data.chunks(32) {
            let mut key = [0u8; 32];
            key.copy_from_slice(chunk);
            pinned.pin(key);
        }
        Ok(pinned)
    }

    fn write_pinned_file(path: &Path, pinned: &PinnedKeys) -> Result<(), StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::Io(e.to_string()))?;
        }
        let mut raw = Vec::with_capacity(pinned.len() * 32);
        for k in pinned.iter() { raw.extend_from_slice(k); }
        let protected = dpapi_protect(&raw)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &protected).map_err(|e| StoreError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, path).map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(())
    }
}

impl KeyStore for FileKeyStore {
    fn load_or_generate(&mut self) -> Result<StaticKeypair, StoreError> {
        match Self::read_secret_file(&self.secret_path) {
            Ok(secret) => Ok(StaticKeypair::from_bytes(secret)),
            Err(StoreError::NotFound) => {
                let kp = StaticKeypair::generate();
                Self::write_secret_file(&self.secret_path, &kp.secret_bytes())?;
                Ok(kp)
            }
            Err(e) => Err(e),
        }
    }

    fn save(&mut self, keypair: &StaticKeypair) -> Result<(), StoreError> {
        Self::write_secret_file(&self.secret_path, &keypair.secret_bytes())
    }

    fn load_pinned(&self) -> Result<PinnedKeys, StoreError> {
        Self::read_pinned_file(&self.pinned_path)
    }

    fn save_pinned(&mut self, pinned: &PinnedKeys) -> Result<(), StoreError> {
        Self::write_pinned_file(&self.pinned_path, pinned)
    }
}

// ------------------------------------------------------------------ DPAPI
// First-party DPAPI wrapper, no third-party crate: calls CryptProtectData /
// CryptUnprotectData from crypt32.dll directly via FFI. Behind `dpapi` feature
// and Windows only, so the crate stays cross-platform and the audit stays clean.
// This satisfies §9.2: secrets in DPAPI-protected store, TPM-backed where
// available (DPAPI uses TPM if present), never in JSON settings.

#[cfg(all(windows, feature = "dpapi"))]
mod dpapi {
    use crate::store::StoreError;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            p_data_in: *const DataBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut core::ffi::c_void,
            p_prompt_struct: *mut core::ffi::c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            p_data_in: *const DataBlob,
            ppsz_data_descr: *mut *mut u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut core::ffi::c_void,
            p_prompt_struct: *mut core::ffi::c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(h_mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        fn GetLastError() -> u32;
    }

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, StoreError> {
        if data.len() > u32::MAX as usize {
            return Err(StoreError::Dpapi("data too large".into()));
        }
        let blob_in = DataBlob {
            cb_data: data.len() as u32,
            pb_data: data.as_ptr() as *mut u8,
        };
        let mut blob_out = DataBlob { cb_data: 0, pb_data: core::ptr::null_mut() };
        let ok = unsafe {
            CryptProtectData(
                &blob_in,
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut blob_out,
            )
        };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            return Err(StoreError::Dpapi(format!("CryptProtectData failed: {err}")));
        }
        let out = unsafe {
            let slice = core::slice::from_raw_parts(blob_out.pb_data, blob_out.cb_data as usize);
            let v = slice.to_vec();
            LocalFree(blob_out.pb_data as *mut _);
            v
        };
        Ok(out)
    }

    pub fn unprotect(blob: &[u8]) -> Result<Vec<u8>, StoreError> {
        if blob.len() > u32::MAX as usize {
            return Err(StoreError::Dpapi("blob too large".into()));
        }
        let blob_in = DataBlob {
            cb_data: blob.len() as u32,
            pb_data: blob.as_ptr() as *mut u8,
        };
        let mut blob_out = DataBlob { cb_data: 0, pb_data: core::ptr::null_mut() };
        let ok = unsafe {
            CryptUnprotectData(
                &blob_in,
                core::ptr::null_mut(),
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut blob_out,
            )
        };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            return Err(StoreError::Dpapi(format!("CryptUnprotectData failed: {err}")));
        }
        let out = unsafe {
            let slice = core::slice::from_raw_parts(blob_out.pb_data, blob_out.cb_data as usize);
            let v = slice.to_vec();
            LocalFree(blob_out.pb_data as *mut _);
            v
        };
        Ok(out)
    }
}

#[cfg(all(windows, feature = "dpapi"))]
fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, StoreError> { dpapi::protect(data) }
#[cfg(all(windows, feature = "dpapi"))]
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, StoreError> { dpapi::unprotect(blob) }

#[cfg(not(all(windows, feature = "dpapi")))]
fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, StoreError> { Ok(data.to_vec()) }
#[cfg(not(all(windows, feature = "dpapi")))]
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, StoreError> { Ok(blob.to_vec()) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_store_generates_and_remembers() {
        let mut store = MemoryKeyStore::new();
        let kp1 = store.load_or_generate().expect("generate");
        let kp2 = store.load_or_generate().expect("load");
        assert_eq!(kp1.public_bytes(), kp2.public_bytes(), "second load returns same key");
    }

    #[test]
    fn memory_store_pinned_roundtrips() {
        let mut store = MemoryKeyStore::new();
        let mut pinned = PinnedKeys::new();
        pinned.pin([1u8; 32]);
        pinned.pin([2u8; 32]);
        store.save_pinned(&pinned).unwrap();
        let loaded = store.load_pinned().unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.is_pinned(&[1u8; 32]));
    }

    #[test]
    fn file_store_roundtrips() {
        let dir = std::env::temp_dir().join(format!("balancify-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let secret_path = dir.join("secret.key");
        let pinned_path = dir.join("pinned.keys");
        let mut store = FileKeyStore::new(&secret_path, &pinned_path);

        let kp = store.load_or_generate().expect("generate");
        let kp2 = store.load_or_generate().expect("load again");
        assert_eq!(kp.public_bytes(), kp2.public_bytes());

        let mut pinned = PinnedKeys::new();
        pinned.pin([3u8; 32]);
        store.save_pinned(&pinned).unwrap();
        let loaded = store.load_pinned().unwrap();
        assert_eq!(loaded.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_store_rejects_corrupt() {
        let dir = std::env::temp_dir().join(format!("balancify-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let secret_path = dir.join("secret.key");
        let pinned_path = dir.join("pinned.keys");
        std::fs::write(&secret_path, b"too short").unwrap();
        let mut store = FileKeyStore::new(&secret_path, &pinned_path);
        assert_eq!(store.load_or_generate().unwrap_err(), StoreError::Corrupt);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
