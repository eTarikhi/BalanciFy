//! Versioned, service-owned machine policy persistence.
//!
//! This is deliberately a small fixed binary schema rather than a new runtime
//! JSON dependency. It contains only policy toggles and bounded custom rules;
//! secrets, relay endpoints, identities, and transient link/session state are
//! excluded. The directory ACL/mode is established before any file is read.

use crate::ipc::{self, FlowRule};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAGIC: &[u8; 4] = b"BFPY";
const VERSION: u16 = 1;
const MAX_CUSTOM_RULES: usize = ipc::MAX_CUSTOM_POLICY_RULES;
const MAX_RULE_NAME_CHARS: usize = 120;
const MAX_RULE_NAME_BYTES: usize = MAX_RULE_NAME_CHARS * 4;
const MAX_FILE_BYTES: usize = 64 * 1024;
const PRIMARY_FILE: &str = "machine-policy.bin";
const BACKUP_FILE: &str = "machine-policy.bak";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachinePolicy {
    pub bond_enabled: bool,
    pub duplicate_mode: bool,
    pub allow_metered: bool,
    pub custom_rules: Vec<FlowRule>,
}

impl Default for MachinePolicy {
    fn default() -> Self {
        Self {
            bond_enabled: true,
            duplicate_mode: false,
            allow_metered: false,
            custom_rules: Vec::new(),
        }
    }
}

impl MachinePolicy {
    pub fn validate(&self) -> io::Result<()> {
        if self.custom_rules.len() > MAX_CUSTOM_RULES {
            return Err(invalid_data("machine policy contains too many custom rules"));
        }
        let mut ids = std::collections::BTreeSet::new();
        for rule in &self.custom_rules {
            if rule.id < balancify_core::FIRST_CUSTOM_RULE_ID || !ids.insert(rule.id) {
                return Err(invalid_data("machine policy has a reserved or duplicate custom rule id"));
            }
            if rule.name.trim().is_empty()
                || rule.name.chars().count() > MAX_RULE_NAME_CHARS
                || rule.name.len() > MAX_RULE_NAME_BYTES
            {
                return Err(invalid_data("machine policy has an invalid custom rule name"));
            }
            if rule.class > ipc::CLASS_PINNED
                || (rule.class != ipc::CLASS_PINNED && rule.pinned_uplink != 0)
                || rule.port_start > rule.port_end
            {
                return Err(invalid_data("machine policy has an invalid custom rule field"));
            }
        }
        Ok(())
    }

    fn encode(&self) -> io::Result<Vec<u8>> {
        self.validate()?;
        let mut rules = self.custom_rules.clone();
        rules.sort_by_key(|rule| rule.id);
        let mut bytes = Vec::with_capacity(16 + rules.len() * 40);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_be_bytes());
        let flags = u8::from(self.bond_enabled)
            | (u8::from(self.duplicate_mode) << 1)
            | (u8::from(self.allow_metered) << 2);
        bytes.push(flags);
        bytes.extend_from_slice(&(rules.len() as u16).to_be_bytes());
        for rule in &rules {
            let name = rule.name.as_bytes();
            bytes.extend_from_slice(&rule.id.to_be_bytes());
            bytes.push(rule.class);
            bytes.push(rule.pinned_uplink);
            bytes.extend_from_slice(&rule.priority.to_be_bytes());
            bytes.push(rule.proto);
            bytes.extend_from_slice(&rule.port_start.to_be_bytes());
            bytes.extend_from_slice(&rule.port_end.to_be_bytes());
            bytes.push(u8::from(rule.duplicate));
            bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
            bytes.extend_from_slice(name);
        }
        if bytes.len() + 4 > MAX_FILE_BYTES {
            return Err(invalid_data("encoded machine policy exceeds its file-size limit"));
        }
        let checksum = checksum32(&bytes);
        bytes.extend_from_slice(&checksum.to_be_bytes());
        Ok(bytes)
    }

    fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 13 || bytes.len() > MAX_FILE_BYTES {
            return Err(invalid_data("machine policy file has an invalid size"));
        }
        let body_len = bytes.len() - 4;
        let expected_checksum = u32::from_be_bytes(bytes[body_len..].try_into().unwrap());
        if checksum32(&bytes[..body_len]) != expected_checksum {
            return Err(invalid_data("machine policy checksum does not match"));
        }
        let mut reader = PolicyReader::new(&bytes[..body_len]);
        if reader.take(4)? != MAGIC {
            return Err(invalid_data("machine policy magic is invalid"));
        }
        let version = reader.u16()?;
        if version != VERSION {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("unsupported machine policy schema version {version}"),
            ));
        }
        let flags = reader.u8()?;
        if flags & !0x07 != 0 {
            return Err(invalid_data("machine policy contains unknown flags"));
        }
        let count = reader.u16()? as usize;
        if count > MAX_CUSTOM_RULES {
            return Err(invalid_data("machine policy contains too many custom rules"));
        }
        let mut custom_rules = Vec::with_capacity(count);
        for _ in 0..count {
            let id = reader.u32()?;
            let class = reader.u8()?;
            let pinned_uplink = reader.u8()?;
            let priority = reader.u32()?;
            let proto = reader.u8()?;
            let port_start = reader.u16()?;
            let port_end = reader.u16()?;
            let duplicate = reader.boolean()?;
            let name_len = reader.u16()? as usize;
            if name_len == 0 || name_len > MAX_RULE_NAME_BYTES {
                return Err(invalid_data("machine policy rule name length is invalid"));
            }
            let name = std::str::from_utf8(reader.take(name_len)?)
                .map_err(|_| invalid_data("machine policy rule name is not valid UTF-8"))?
                .to_owned();
            custom_rules.push(FlowRule {
                id,
                name,
                class,
                pinned_uplink,
                priority,
                proto,
                port_start,
                port_end,
                duplicate,
            });
        }
        if !reader.is_empty() {
            return Err(invalid_data("machine policy contains trailing data"));
        }
        let policy = Self {
            bond_enabled: flags & 0x01 != 0,
            duplicate_mode: flags & 0x02 != 0,
            allow_metered: flags & 0x04 != 0,
            custom_rules,
        };
        policy.validate()?;
        Ok(policy)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedMachinePolicy {
    pub policy: MachinePolicy,
    pub recovered_from_backup: bool,
}

#[derive(Debug, Clone)]
pub struct MachinePolicyStore {
    directory: PathBuf,
}

impl MachinePolicyStore {
    /// Opens a dedicated machine-policy directory and restricts it to the
    /// service/admin trust boundary before reading existing state.
    pub fn open(directory: impl AsRef<Path>) -> io::Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        fs::create_dir_all(&directory)?;
        let metadata = fs::symlink_metadata(&directory)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid_data("machine-policy directory is not a trusted directory"));
        }
        secure_path(&directory, true)?;
        Ok(Self { directory })
    }

    pub fn load(&self) -> io::Result<LoadedMachinePolicy> {
        let primary_path = self.directory.join(PRIMARY_FILE);
        let backup_path = self.directory.join(BACKUP_FILE);
        match read_policy(&primary_path) {
            Ok(Some(policy)) => Ok(LoadedMachinePolicy { policy, recovered_from_backup: false }),
            Ok(None) => match read_policy(&backup_path)? {
                Some(policy) => {
                    self.install_recovered(&policy)?;
                    Ok(LoadedMachinePolicy { policy, recovered_from_backup: true })
                }
                None => Ok(LoadedMachinePolicy { policy: MachinePolicy::default(), recovered_from_backup: false }),
            },
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                match read_policy(&backup_path) {
                    Ok(Some(policy)) => {
                        self.install_recovered(&policy)?;
                        Ok(LoadedMachinePolicy { policy, recovered_from_backup: true })
                    }
                    Ok(None) => Err(error),
                    Err(backup_error) => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("machine policy and recovery backup are invalid: {error}; {backup_error}"),
                    )),
                }
            }
            Err(error) => Err(error),
        }
    }

    /// Persist replacement via a unique, same-directory temporary. The
    /// previous valid primary becomes the recovery backup before promotion.
    pub fn save(&self, policy: &MachinePolicy) -> io::Result<()> {
        let bytes = policy.encode()?;
        let temporary = self.write_temporary(&bytes)?;
        let primary = self.directory.join(PRIMARY_FILE);
        let backup = self.directory.join(BACKUP_FILE);
        if path_exists(&primary)? {
            remove_if_exists(&backup)?;
            fs::rename(&primary, &backup)?;
        }
        if let Err(error) = fs::rename(&temporary, &primary) {
            // Keep the backup intact. A subsequent service start recovers it.
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = sync_directory(&self.directory) {
            // The file replacement was visible but its directory entry was not
            // confirmed durable. Restore the last known generation where possible.
            let _ = fs::remove_file(&primary);
            if path_exists(&backup).unwrap_or(false) {
                let _ = fs::rename(&backup, &primary);
            }
            let _ = sync_directory(&self.directory);
            return Err(error);
        }
        Ok(())
    }

    fn install_recovered(&self, policy: &MachinePolicy) -> io::Result<()> {
        let bytes = policy.encode()?;
        let temporary = self.write_temporary(&bytes)?;
        let primary = self.directory.join(PRIMARY_FILE);
        remove_if_exists(&primary)?;
        if let Err(error) = fs::rename(&temporary, &primary) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        sync_directory(&self.directory)
    }

    fn write_temporary(&self, bytes: &[u8]) -> io::Result<PathBuf> {
        for _ in 0..128 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = self.directory.join(format!(
                ".machine-policy.{}.{}.tmp",
                std::process::id(),
                sequence,
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = match options.open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            drop(file);
            if let Err(error) = secure_path(&path, false) {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            return Ok(path);
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "could not allocate a unique machine-policy temporary"))
    }
}

fn read_policy(path: &Path) -> io::Result<Option<MachinePolicy>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(invalid_data("machine policy path is not a regular file"));
        }
        Ok(metadata) if metadata.len() > MAX_FILE_BYTES as u64 => {
            return Err(invalid_data("machine policy file exceeds its size limit"));
        }
        Ok(_) => {}
    }
    secure_path(path, false)?;
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    Read::take(&mut file, (MAX_FILE_BYTES + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(invalid_data("machine policy file exceeds its size limit"));
    }
    MachinePolicy::decode(&bytes).map(Some)
}

fn path_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn checksum32(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(windows)]
    let _ = path;
    Ok(())
}

fn secure_path(path: &Path, is_directory: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if is_directory { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(windows)]
    {
        let _ = is_directory;
        set_windows_service_acl(path)?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, is_directory);
        return Err(io::Error::new(io::ErrorKind::Unsupported, "machine-policy permissions are unsupported on this platform"));
    }
    Ok(())
}

#[cfg(windows)]
fn set_windows_service_acl(path: &Path) -> io::Result<()> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    const SDDL_REVISION_1: u32 = 1;
    const OWNER_SECURITY_INFORMATION: u32 = 0x0000_0001;
    const GROUP_SECURITY_INFORMATION: u32 = 0x0000_0002;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const PROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x8000_0000;

    let path: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let sddl: Vec<u16> = "O:SYG:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"
        .encode_utf16().chain(std::iter::once(0)).collect();
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    let mut descriptor_size = 0u32;
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(), SDDL_REVISION_1, &mut descriptor, &mut descriptor_size,
        )
    } == 0 {
        return Err(last_windows_error());
    }
    let result = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            OWNER_SECURITY_INFORMATION
                | GROUP_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    unsafe { LocalFree(descriptor); }
    if result == 0 { Err(last_windows_error()) } else { Ok(()) }
}

#[cfg(windows)]
fn last_windows_error() -> io::Error {
    io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

#[cfg(windows)]
#[link(name = "advapi32")]
extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        string: *const u16,
        revision: u32,
        descriptor: *mut *mut std::ffi::c_void,
        size: *mut u32,
    ) -> i32;
    fn SetFileSecurityW(
        name: *const u16,
        information: u32,
        descriptor: *mut std::ffi::c_void,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn GetLastError() -> u32;
}

struct PolicyReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> PolicyReader<'a> {
    fn new(bytes: &'a [u8]) -> Self { Self { bytes, position: 0 } }
    fn is_empty(&self) -> bool { self.position == self.bytes.len() }
    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let end = self.position.checked_add(count).ok_or_else(|| invalid_data("machine policy offset overflow"))?;
        if end > self.bytes.len() { return Err(invalid_data("machine policy file is truncated")); }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }
    fn u8(&mut self) -> io::Result<u8> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> io::Result<u16> { Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap())) }
    fn u32(&mut self) -> io::Result<u32> { Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap())) }
    fn boolean(&mut self) -> io::Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(invalid_data("machine policy boolean has an invalid encoding")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("balancify-machine-policy-{}-{sequence}", std::process::id()))
    }

    fn sample_policy() -> MachinePolicy {
        MachinePolicy {
            bond_enabled: false,
            duplicate_mode: true,
            allow_metered: true,
            custom_rules: vec![FlowRule {
                id: 1_000,
                name: "video call 🎥".to_string(),
                class: ipc::CLASS_PINNED,
                pinned_uplink: 4,
                priority: 17,
                proto: 17,
                port_start: 5000,
                port_end: 5010,
                duplicate: true,
            }],
        }
    }

    #[test]
    fn policy_encoding_roundtrips_with_bounded_unicode_rules() {
        let policy = sample_policy();
        let encoded = policy.encode().unwrap();
        assert!(encoded.len() <= MAX_FILE_BYTES);
        assert_eq!(MachinePolicy::decode(&encoded).unwrap(), policy);
    }

    #[test]
    fn policy_decoder_rejects_corruption_future_versions_and_trailing_bytes() {
        let encoded = sample_policy().encode().unwrap();
        let mut corrupt = encoded.clone();
        corrupt[7] ^= 0x80;
        assert_eq!(MachinePolicy::decode(&corrupt).unwrap_err().kind(), io::ErrorKind::InvalidData);

        let mut future = encoded.clone();
        future[4..6].copy_from_slice(&(VERSION + 1).to_be_bytes());
        let body_len = future.len() - 4;
        let sum = checksum32(&future[..body_len]);
        future[body_len..].copy_from_slice(&sum.to_be_bytes());
        assert_eq!(MachinePolicy::decode(&future).unwrap_err().kind(), io::ErrorKind::Unsupported);

        let mut trailing = sample_policy().encode().unwrap();
        trailing.insert(trailing.len() - 4, 0);
        let body_len = trailing.len() - 4;
        let sum = checksum32(&trailing[..body_len]);
        trailing[body_len..].copy_from_slice(&sum.to_be_bytes());
        assert_eq!(MachinePolicy::decode(&trailing).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn policy_validation_rejects_duplicate_reserved_and_oversized_rules() {
        let mut policy = sample_policy();
        policy.custom_rules.push(policy.custom_rules[0].clone());
        assert!(policy.validate().is_err());
        policy.custom_rules.clear();
        policy.custom_rules.push(FlowRule { id: 2, ..sample_policy().custom_rules[0].clone() });
        assert!(policy.validate().is_err());
        policy.custom_rules = (0..=MAX_CUSTOM_RULES).map(|index| FlowRule {
            id: balancify_core::FIRST_CUSTOM_RULE_ID + index as u32,
            ..sample_policy().custom_rules[0].clone()
        }).collect();
        assert!(policy.validate().is_err());
    }

    #[test]
    fn store_roundtrips_and_recovers_previous_atomic_generation() {
        let dir = test_dir();
        let store = MachinePolicyStore::open(&dir).unwrap();
        let first = sample_policy();
        store.save(&first).unwrap();
        let second = MachinePolicy { duplicate_mode: false, ..first.clone() };
        store.save(&second).unwrap();
        fs::write(dir.join(PRIMARY_FILE), b"torn write").unwrap();
        let recovered = store.load().unwrap();
        assert_eq!(recovered.policy, first);
        assert!(recovered.recovered_from_backup);
        assert_eq!(store.load().unwrap().policy, first);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_does_not_downgrade_a_future_primary_to_an_older_backup() {
        let dir = test_dir();
        let store = MachinePolicyStore::open(&dir).unwrap();
        store.save(&MachinePolicy::default()).unwrap();
        store.save(&sample_policy()).unwrap();
        let primary = dir.join(PRIMARY_FILE);
        let mut future = fs::read(&primary).unwrap();
        future[4..6].copy_from_slice(&(VERSION + 1).to_be_bytes());
        let body_len = future.len() - 4;
        let checksum = checksum32(&future[..body_len]);
        future[body_len..].copy_from_slice(&checksum.to_be_bytes());
        fs::write(primary, future).unwrap();
        assert_eq!(store.load().unwrap_err().kind(), io::ErrorKind::Unsupported);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_ignores_uncommitted_temporary_files() {
        let dir = test_dir();
        let store = MachinePolicyStore::open(&dir).unwrap();
        fs::write(dir.join(".machine-policy.stale.tmp"), b"incomplete transaction").unwrap();
        assert_eq!(store.load().unwrap().policy, MachinePolicy::default());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_fails_closed_when_primary_and_backup_are_both_corrupt() {
        let dir = test_dir();
        let store = MachinePolicyStore::open(&dir).unwrap();
        store.save(&sample_policy()).unwrap();
        store.save(&MachinePolicy::default()).unwrap();
        fs::write(dir.join(PRIMARY_FILE), b"bad primary").unwrap();
        fs::write(dir.join(BACKUP_FILE), b"bad backup").unwrap();
        assert_eq!(store.load().unwrap_err().kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn store_uses_private_unix_modes_and_rejects_symlink_policy_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_dir();
        let store = MachinePolicyStore::open(&dir).unwrap();
        store.save(&sample_policy()).unwrap();
        assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(dir.join(PRIMARY_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        let external = dir.with_extension("external");
        fs::write(&external, b"do not follow").unwrap();
        fs::remove_file(dir.join(PRIMARY_FILE)).unwrap();
        std::os::unix::fs::symlink(&external, dir.join(PRIMARY_FILE)).unwrap();
        assert_eq!(store.load().unwrap_err().kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_file(external);
    }
}
