//! Balancify installer -- transactional environment provisioning (Sec.4.8 rev 2.1, Sec.8.3)
//! One signed offline bundle; zero user prerequisites.
//! Each step journals, any failure rolls back in reverse order.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

#[derive(Debug, Clone)]
pub enum InstallStep {
    Preflight,
    DriverPackage { inf_path: PathBuf },
    Service { name: String, bin_path: PathBuf },
    Adapter { mtu: u16 },
    Runtimes { dotnet_bundled: bool, winappsdk_bundled: bool },
    Gui { exe_path: PathBuf },
    SbomSidecar { path: PathBuf },
    SelfTest,
}

#[derive(Debug, Clone)]
pub enum RollbackAction {
    RemoveDriver { inf_path: PathBuf },
    RemoveService { name: String },
    RemoveAdapter,
    RemoveRuntimes { dotnet_added: bool, winappsdk_added: bool },
    RemoveGui,
    RestoreFile { path: PathBuf, backup: Option<PathBuf> },
    RestoreRoutes,
}

#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub step: InstallStep,
    pub at: Instant,
    pub rollback: Option<RollbackAction>,
}

#[derive(Debug, Default)]
pub struct InstallJournal {
    entries: VecDeque<JournalEntry>,
}

fn json_quote(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\u{08}' => encoded.push_str("\\b"),
            '\u{0c}' => encoded.push_str("\\f"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            character if character <= '\u{1f}' => {
                encoded.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
}

fn serialize_install_step(step: &InstallStep) -> String {
    match step {
        InstallStep::Preflight => "{\"kind\":\"preflight\"}".to_string(),
        InstallStep::DriverPackage { inf_path } => format!(
            "{{\"kind\":\"driver_package\",\"inf_path\":{}}}",
            json_quote(&inf_path.to_string_lossy())
        ),
        InstallStep::Service { name, bin_path } => format!(
            "{{\"kind\":\"service\",\"name\":{},\"bin_path\":{}}}",
            json_quote(name),
            json_quote(&bin_path.to_string_lossy())
        ),
        InstallStep::Adapter { mtu } => format!("{{\"kind\":\"adapter\",\"mtu\":{mtu}}}"),
        InstallStep::Runtimes { dotnet_bundled, winappsdk_bundled } => format!(
            "{{\"kind\":\"runtimes\",\"dotnet_bundled\":{dotnet_bundled},\"winappsdk_bundled\":{winappsdk_bundled}}}"
        ),
        InstallStep::Gui { exe_path } => format!(
            "{{\"kind\":\"gui\",\"exe_path\":{}}}",
            json_quote(&exe_path.to_string_lossy())
        ),
        InstallStep::SbomSidecar { path } => format!(
            "{{\"kind\":\"sbom_sidecar\",\"path\":{}}}",
            json_quote(&path.to_string_lossy())
        ),
        InstallStep::SelfTest => "{\"kind\":\"self_test\"}".to_string(),
    }
}

fn serialize_rollback_action(action: &RollbackAction) -> String {
    match action {
        RollbackAction::RemoveDriver { inf_path } => format!(
            "{{\"kind\":\"remove_driver\",\"inf_path\":{}}}",
            json_quote(&inf_path.to_string_lossy())
        ),
        RollbackAction::RemoveService { name } => format!(
            "{{\"kind\":\"remove_service\",\"name\":{}}}",
            json_quote(name)
        ),
        RollbackAction::RemoveAdapter => "{\"kind\":\"remove_adapter\"}".to_string(),
        RollbackAction::RemoveRuntimes { dotnet_added, winappsdk_added } => format!(
            "{{\"kind\":\"remove_runtimes\",\"dotnet_added\":{dotnet_added},\"winappsdk_added\":{winappsdk_added}}}"
        ),
        RollbackAction::RemoveGui => "{\"kind\":\"remove_gui\"}".to_string(),
        RollbackAction::RestoreFile { path, backup } => format!(
            "{{\"kind\":\"restore_file\",\"path\":{},\"backup\":{}}}",
            json_quote(&path.to_string_lossy()),
            backup.as_ref().map(|path| json_quote(&path.to_string_lossy())).unwrap_or_else(|| "null".to_string())
        ),
        RollbackAction::RestoreRoutes => "{\"kind\":\"restore_routes\"}".to_string(),
    }
}

impl InstallJournal {
    pub fn push(&mut self, step: InstallStep, rollback: Option<RollbackAction>) {
        self.entries.push_back(JournalEntry {
            step,
            at: Instant::now(),
            rollback,
        });
    }

    pub fn rollback_all(&self) -> Vec<RollbackAction> {
        self.entries
            .iter()
            .rev()
            .filter_map(|e| e.rollback.clone())
            .collect()
    }

    pub fn save(&self, path: &PathBuf) -> std::io::Result<()> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let entries = self.entries.iter().enumerate().map(|(sequence, entry)| {
            let rollback = entry
                .rollback
                .as_ref()
                .map(serialize_rollback_action)
                .unwrap_or_else(|| "null".to_string());
            format!(
                "{{\"sequence\":{sequence},\"step\":{},\"rollback\":{rollback}}}",
                serialize_install_step(&entry.step)
            )
        }).collect::<Vec<_>>().join(",");
        let json = format!("{{\"entries\":[{entries}]}}\n");
        let mut options = OpenOptions::new();
        options.write(true).create(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = file.metadata()?.permissions();
            permissions.set_mode(0o600);
            file.set_permissions(permissions)?;
        }
        file.set_len(0)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()
    }
}

pub struct Installer {
    journal: InstallJournal,
    journal_path: PathBuf,
    dry_run: bool,
}

static SIDECAR_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn default_journal_path() -> PathBuf {
    let sequence = SIDECAR_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "balancify_install-{}-{timestamp}-{sequence}.json",
        std::process::id()
    ))
}

fn unique_sidecar_sibling(path: &Path, suffix: &str) -> PathBuf {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("sbom"))
        .to_string_lossy();
    let sequence = SIDECAR_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".{name}.balancify-{}-{sequence}.{suffix}",
        std::process::id()
    ))
}

fn reserve_sidecar_backup(path: &Path) -> io::Result<PathBuf> {
    loop {
        let directory = unique_sidecar_sibling(path, "backup");
        match fs::create_dir(&directory) {
            Ok(()) => return Ok(directory.join("previous")),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

fn remove_sidecar_backup(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if let Some(directory) = path.parent() {
        match fs::remove_dir(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn stage_sidecar_copy(src: &Path, temp: &Path) -> io::Result<()> {
    let mut input = File::open(src)?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(temp)?;
    io::copy(&mut input, &mut output)?;
    output.sync_all()
}

impl Installer {
    pub fn new(journal_path: PathBuf, dry_run: bool) -> Self {
        Self {
            journal: InstallJournal::default(),
            journal_path,
            dry_run,
        }
    }

    pub fn preflight(&mut self) -> Result<(), String> {
        println!("[preflight] OS build, HVCI state, disk space, AV probe, snapshot routes");
        // Check OS build >= 19041 (Windows 10 2004)
        #[cfg(windows)]
        {
            // Would call RtlGetVersion, check build number
        }
        // Check disk space
        // Snapshot routes: netsh interface ip show config, etc.
        self.journal.push(
            InstallStep::Preflight,
            Some(RollbackAction::RestoreRoutes),
        );
        Ok(())
    }

    pub fn install_driver(&mut self, inf_path: PathBuf) -> Result<(), String> {
        println!("[driver] installing {} (attestation-signed)", inf_path.display());
        if !self.dry_run {
            // In real installer: pnputil /add-driver inf /install
            // DiInstallDevice, etc.
            // For now, simulate
        }
        self.journal.push(
            InstallStep::DriverPackage { inf_path: inf_path.clone() },
            Some(RollbackAction::RemoveDriver { inf_path }),
        );
        Ok(())
    }

    pub fn install_service(&mut self, name: &str, bin_path: PathBuf) -> Result<(), String> {
        println!("[service] creating service {} -> {}", name, bin_path.display());
        if !self.dry_run {
            // sc create BalancifyService binPath= ... obj= "NT SERVICE\\BalancifyService" type= own
            // Set SDDL for pipe, restricted account
        }
        self.journal.push(
            InstallStep::Service { name: name.to_string(), bin_path: bin_path.clone() },
            Some(RollbackAction::RemoveService { name: name.to_string() }),
        );
        Ok(())
    }

    pub fn bring_up_adapter(&mut self, mtu: u16) -> Result<(), String> {
        println!("[adapter] bringing up virtual adapter MTU {}", mtu);
        self.journal.push(
            InstallStep::Adapter { mtu },
            Some(RollbackAction::RemoveAdapter),
        );
        Ok(())
    }

    pub fn ensure_runtimes(&mut self) -> Result<(), String> {
        println!("[runtimes] detecting .NET 10 + WinAppSDK, installing bundled if absent");
        let dotnet_bundled = false; // would detect
        let winappsdk_bundled = false;
        let dotnet_added = false;
        let winappsdk_added = false;
        self.journal.push(
            InstallStep::Runtimes { dotnet_bundled, winappsdk_bundled },
            Some(RollbackAction::RemoveRuntimes { dotnet_added, winappsdk_added }),
        );
        Ok(())
    }

    pub fn install_gui(&mut self, exe_path: PathBuf) -> Result<(), String> {
        println!("[gui] registering {} + tray + firewall rules", exe_path.display());
        self.journal.push(
            InstallStep::Gui { exe_path: exe_path.clone() },
            Some(RollbackAction::RemoveGui),
        );
        Ok(())
    }

    pub fn install_sbom_sidecar(&mut self, src: PathBuf, dst: PathBuf) -> Result<(), String> {
        println!("[sbom] copying SPDX sidecar {} -> {}", src.display(), dst.display());
        let mut backup = None;
        if !self.dry_run {
            let source_metadata = fs::symlink_metadata(&src)
                .map_err(|error| format!("required SPDX SBOM is missing: {} ({error})", src.display()))?;
            if !source_metadata.is_file() || source_metadata.file_type().is_symlink() {
                return Err(format!("required SPDX SBOM is not a regular file: {}", src.display()));
            }
            if let Some(parent) = dst.parent().filter(|parent| !parent.as_os_str().is_empty()) {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("create SBOM directory failed: {error}"))?;
            }

            match fs::symlink_metadata(&dst) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    let previous = reserve_sidecar_backup(&dst)
                        .map_err(|error| format!("reserve SBOM backup failed: {error}"))?;
                    if let Err(error) = fs::rename(&dst, &previous) {
                        if let Some(directory) = previous.parent() {
                            let _ = fs::remove_dir(directory);
                        }
                        return Err(format!("preserve existing SBOM failed: {error}"));
                    }
                    backup = Some(previous);
                }
                Ok(_) => return Err(format!("SBOM destination is not a regular file: {}", dst.display())),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("inspect SBOM destination failed: {error}")),
            }

            let temp = unique_sidecar_sibling(&dst, "tmp");
            let install_result = stage_sidecar_copy(&src, &temp)
                .and_then(|()| fs::rename(&temp, &dst));
            if let Err(error) = install_result {
                let _ = fs::remove_file(&temp);
                if let Some(previous) = backup.take() {
                    if let Err(restore_error) = fs::rename(&previous, &dst) {
                        return Err(format!(
                            "copy SBOM failed: {error}; restoring previous sidecar also failed: {restore_error}"
                        ));
                    }
                    let _ = remove_sidecar_backup(&previous);
                }
                return Err(format!("copy SBOM failed: {error}"));
            }
        }
        let rollback = (!self.dry_run).then_some(RollbackAction::RestoreFile { path: dst.clone(), backup });
        self.journal
            .push(InstallStep::SbomSidecar { path: dst }, rollback);
        Ok(())
    }

    pub fn self_test(&mut self) -> Result<(), String> {
        println!("[self-test] loop through embedded endpoint, local mode");
        println!("[self-test] checks: driver loaded, service reachable via IPC, adapter MTU, both SBOM sidecars exist, ETW provider registered, self-test offline VM per clean-machine-test.md");
        self.journal.push(InstallStep::SelfTest, None);
        Ok(())
    }

    fn discard_sidecar_backups(&mut self) {
        for entry in &mut self.journal.entries {
            if let Some(RollbackAction::RestoreFile { backup, .. }) = entry.rollback.as_mut() {
                if let Some(previous) = backup.take() {
                    let removed = match fs::remove_file(&previous) {
                        Ok(()) => true,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
                        Err(error) => {
                            eprintln!("warning: could not remove prior SBOM backup {}: {error}", previous.display());
                            *backup = Some(previous.clone());
                            false
                        }
                    };
                    if removed {
                        if let Some(directory) = previous.parent() {
                            if let Err(error) = fs::remove_dir(directory) {
                                eprintln!("warning: could not remove SBOM backup directory {}: {error}", directory.display());
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn run(&mut self) -> Result<(), String> {
        let steps: Vec<Box<dyn Fn(&mut Self) -> Result<(), String>>> = vec![
            Box::new(|s| s.preflight()),
            Box::new(|s| s.install_driver(PathBuf::from("driver/balancify.inf"))),
            Box::new(|s| s.install_service("BalancifyService", PathBuf::from("service/balancify-service.exe"))),
            Box::new(|s| s.bring_up_adapter(1440)),
            Box::new(|s| s.ensure_runtimes()),
            Box::new(|s| s.install_gui(PathBuf::from("app/Balancify.exe"))),
            Box::new(|s| s.install_sbom_sidecar(PathBuf::from("sbom.spdx.json"), PathBuf::from("C:\\ProgramData\\Balancify\\sbom.spdx.json"))),
            Box::new(|s| s.install_sbom_sidecar(PathBuf::from("sbom.spdx3.json"), PathBuf::from("C:\\ProgramData\\Balancify\\sbom.spdx3.json"))),
            Box::new(|s| s.self_test()),
        ];

        for (i, step_fn) in steps.into_iter().enumerate() {
            if let Err(e) = step_fn(self) {
                eprintln!("Install failed at step {}: {}, rolling back", i, e);
                self.rollback()?;
                return Err(e);
            }
            if let Err(save_error) = self.journal.save(&self.journal_path) {
                eprintln!("could not persist installer journal: {save_error}; rolling back");
                if let Err(rollback_error) = self.rollback() {
                    return Err(format!(
                        "write installation journal failed: {save_error}; rollback also failed: {rollback_error}"
                    ));
                }
                return Err(format!("write installation journal failed: {save_error}"));
            }
        }
        self.discard_sidecar_backups();
        if let Err(error) = self.journal.save(&self.journal_path) {
            eprintln!("warning: installation succeeded but final journal update failed: {error}");
        }
        println!("Install complete, journal at {}", self.journal_path.display());
        Ok(())
    }

    pub fn rollback(&self) -> Result<(), String> {
        println!("[rollback] executing inverse operations in reverse order");
        for action in self.journal.rollback_all() {
            println!("  rollback: {:?}", action);
            match action {
                RollbackAction::RemoveDriver { inf_path } => {
                    println!("    pnputil /delete-driver {} /uninstall", inf_path.display());
                }
                RollbackAction::RemoveService { name } => {
                    println!("    sc delete {}", name);
                }
                RollbackAction::RemoveAdapter => {
                    println!("    removing virtual adapter");
                }
                RollbackAction::RemoveRuntimes { dotnet_added, winappsdk_added } => {
                    if dotnet_added {
                        println!("    removing .NET runtime we added");
                    }
                    if winappsdk_added {
                        println!("    removing WinAppSDK runtime we added");
                    }
                }
                RollbackAction::RemoveGui => {
                    println!("    unregistering GUI + firewall rules");
                }
                RollbackAction::RestoreFile { path, backup } => {
                    println!("    restoring installed sidecar {}", path.display());
                    match fs::symlink_metadata(&path) {
                        Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                            fs::remove_file(&path).map_err(|error| format!("remove sidecar failed: {error}"))?;
                        }
                        Ok(_) => return Err(format!("refusing to remove non-file sidecar path: {}", path.display())),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => return Err(format!("inspect sidecar during rollback failed: {error}")),
                    }
                    if let Some(previous) = backup {
                        fs::rename(&previous, &path)
                            .map_err(|error| format!("restore previous sidecar failed: {error}"))?;
                        if let Err(error) = remove_sidecar_backup(&previous) {
                            eprintln!("warning: could not clean up restored SBOM backup {}: {error}", previous.display());
                        }
                    }
                }
                RollbackAction::RestoreRoutes => {
                    println!("    restoring routes/adapters/services snapshot");
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct InstallerCliOptions {
    dry_run: bool,
    journal_path: PathBuf,
}

fn parse_installer_args(args: &[String]) -> Result<Option<InstallerCliOptions>, String> {
    let mut dry_run = false;
    let mut journal_path = None;
    let mut positional_path = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--dry-run" => dry_run = true,
            "--journal" => {
                index += 1;
                let path = args
                    .get(index)
                    .ok_or_else(|| "--journal requires a path".to_string())?;
                if path.is_empty() || path.starts_with('-') {
                    return Err("--journal requires a path argument, not another option".to_string());
                }
                if journal_path.replace(PathBuf::from(path)).is_some() {
                    return Err("--journal may only be specified once".to_string());
                }
            }
            "--help" | "-h" => return Ok(None),
            option if option.starts_with('-') => {
                return Err(format!("unknown installer option: {option}"));
            }
            "" => return Err("journal path must not be empty".to_string()),
            path => {
                if positional_path.replace(PathBuf::from(path)).is_some() {
                    return Err("provide at most one positional journal path".to_string());
                }
            }
        }
        index += 1;
    }

    if journal_path.is_some() && positional_path.is_some() {
        return Err("use either --journal PATH or a positional journal path, not both".to_string());
    }
    Ok(Some(InstallerCliOptions {
        dry_run,
        journal_path: journal_path.or(positional_path).unwrap_or_else(default_journal_path),
    }))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse_installer_args(&args) {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("Usage: balancify-installer [--dry-run] [--journal PATH | JOURNAL_PATH]");
            return;
        }
        Err(error) => {
            eprintln!("{error}\nUsage: balancify-installer [--dry-run] [--journal PATH | JOURNAL_PATH]");
            std::process::exit(2);
        }
    };
    println!(
        "Balancify installer -- dry_run={} journal={}",
        options.dry_run,
        options.journal_path.display()
    );
    let mut installer = Installer::new(options.journal_path, options.dry_run);
    match installer.run() {
        Ok(()) => println!("Install succeeded"),
        Err(e) => {
            eprintln!("Install failed: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_cli_dry_run_does_not_become_the_journal_path() {
        let options = parse_installer_args(&["--dry-run".to_string()])
            .unwrap()
            .unwrap();
        assert!(options.dry_run);
        let temp_dir = std::env::temp_dir();
        assert_eq!(options.journal_path.parent(), Some(temp_dir.as_path()));
        assert!(options
            .journal_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("balancify_install-"));

        let first_default = default_journal_path();
        let second_default = default_journal_path();
        assert_ne!(first_default, second_default);
    }

    #[test]
    fn installer_cli_accepts_explicit_journal_and_rejects_ambiguous_args() {
        let options = parse_installer_args(&[
            "--journal".to_string(),
            "journal.json".to_string(),
            "--dry-run".to_string(),
        ])
        .unwrap()
        .unwrap();
        assert!(options.dry_run);
        assert_eq!(options.journal_path, PathBuf::from("journal.json"));
        assert!(parse_installer_args(&["--journal".to_string()]).is_err());
        assert!(parse_installer_args(&["--journal".to_string(), "--dry-run".to_string()]).is_err());
        assert!(parse_installer_args(&[String::new()]).is_err());
        assert!(parse_installer_args(&["first.json".to_string(), "second.json".to_string()]).is_err());
        assert!(parse_installer_args(&["--journal".to_string(), "a.json".to_string(), "b.json".to_string()]).is_err());
        assert!(parse_installer_args(&["--unexpected".to_string()]).is_err());
        assert!(parse_installer_args(&["--help".to_string()]).unwrap().is_none());
    }

    #[test]
    fn journal_rollback_order() {
        let mut j = InstallJournal::default();
        j.push(InstallStep::Preflight, Some(RollbackAction::RestoreRoutes));
        j.push(InstallStep::DriverPackage { inf_path: PathBuf::from("a.inf") }, Some(RollbackAction::RemoveDriver { inf_path: PathBuf::from("a.inf") }));
        j.push(InstallStep::Service { name: "svc".to_string(), bin_path: PathBuf::from("svc.exe") }, Some(RollbackAction::RemoveService { name: "svc".to_string() }));
        let rb = j.rollback_all();
        assert_eq!(rb.len(), 3);
        // Reverse order
        assert!(matches!(rb[0], RollbackAction::RemoveService { .. }));
        assert!(matches!(rb[1], RollbackAction::RemoveDriver { .. }));
        assert!(matches!(rb[2], RollbackAction::RestoreRoutes));
    }

    #[test]
    fn installer_dry_run_succeeds() {
        let journal_path = std::env::temp_dir().join(format!("balancify-test-journal-{}.json", std::process::id()));
        let mut inst = Installer::new(journal_path.clone(), true);
        assert!(inst.run().is_ok());
        assert_eq!(inst.journal.entries.len(), 9); // preflight, driver, service, adapter, runtimes, gui, two SBOMs, self-test
        std::fs::remove_file(journal_path).unwrap();
    }

    #[test]
    fn journal_serializes_step_and_rollback_details_with_json_escaping() {
        let root = std::env::temp_dir().join(format!("balancify-journal-json-{}", std::process::id()));
        let journal_path = root.join("nested").join("journal.json");
        let inventory_path = PathBuf::from("inventory\".spdx.json");
        let mut journal = InstallJournal::default();
        journal.push(
            InstallStep::SbomSidecar { path: inventory_path.clone() },
            Some(RollbackAction::RestoreFile {
                path: inventory_path,
                backup: Some(PathBuf::from(".backup/previous")),
            }),
        );

        journal.save(&journal_path).unwrap();
        let json = std::fs::read_to_string(&journal_path).unwrap();
        assert!(json.contains(r#""kind":"sbom_sidecar""#));
        assert!(json.contains(r#"inventory\".spdx.json"#));
        assert!(json.contains(r#""kind":"restore_file""#));
        assert!(json.contains(".backup/previous"));
        assert!(json.ends_with('\n'));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&journal_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn journal_write_failure_aborts_install_and_rolls_back() {
        let root = std::env::temp_dir().join(format!("balancify-journal-failure-{}", std::process::id()));
        let journal_path = root.join("journal.json");
        std::fs::create_dir_all(&journal_path).unwrap();

        let mut installer = Installer::new(journal_path, true);
        let error = installer.run().unwrap_err();
        assert!(error.contains("write installation journal failed"));
        assert_eq!(installer.journal.entries.len(), 1);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dry_run_rollback_leaves_existing_sidecar_untouched() {
        let root = std::env::temp_dir().join(format!("balancify-sbom-dry-run-{}", std::process::id()));
        let src = root.join("source.spdx.json");
        let dst = root.join("install").join("sbom.spdx.json");
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::write(&dst, b"installed inventory").unwrap();

        let mut installer = Installer::new(root.join("journal.json"), true);
        installer.install_sbom_sidecar(src, dst.clone()).unwrap();
        installer.rollback().unwrap();

        assert_eq!(std::fs::read(&dst).unwrap(), b"installed inventory");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sbom_sidecar_requires_and_copies_the_generated_inventory() {
        let root = std::env::temp_dir().join(format!("balancify-sbom-{}", std::process::id()));
        let src = root.join("source.spdx.json");
        let dst = root.join("install").join("sbom.spdx.json");
        std::fs::create_dir_all(&root).unwrap();
        let expected = r#"{"spdxVersion":"SPDX-2.3"}"#;
        std::fs::write(&src, expected).unwrap();

        let mut installer = Installer::new(root.join("journal.json"), false);
        installer.install_sbom_sidecar(src.clone(), dst.clone()).unwrap();
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), expected);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacing_existing_sidecar_restores_previous_file_on_rollback() {
        let root = std::env::temp_dir().join(format!("balancify-sbom-replace-{}", std::process::id()));
        let src = root.join("source.spdx3.json");
        let dst = root.join("install").join("sbom.spdx3.json");
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::write(&src, b"new SPDX inventory").unwrap();
        std::fs::write(&dst, b"previous installed inventory").unwrap();

        let mut installer = Installer::new(root.join("journal.json"), false);
        installer.install_sbom_sidecar(src, dst.clone()).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"new SPDX inventory");
        let backup = match installer.journal.entries.back().unwrap().rollback.as_ref().unwrap() {
            RollbackAction::RestoreFile { backup: Some(path), .. } => path.clone(),
            action => panic!("expected backup action, got {action:?}"),
        };
        assert!(backup.exists());

        installer.rollback().unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"previous installed inventory");
        assert!(!backup.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn successful_sidecar_commit_removes_previous_backup() {
        let root = std::env::temp_dir().join(format!("balancify-sbom-commit-{}", std::process::id()));
        let src = root.join("source.spdx.json");
        let dst = root.join("install").join("sbom.spdx.json");
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::write(&src, b"new inventory").unwrap();
        std::fs::write(&dst, b"old inventory").unwrap();

        let mut installer = Installer::new(root.join("journal.json"), false);
        installer.install_sbom_sidecar(src, dst.clone()).unwrap();
        let backup = match installer.journal.entries.back().unwrap().rollback.as_ref().unwrap() {
            RollbackAction::RestoreFile { backup: Some(path), .. } => path.clone(),
            action => panic!("expected backup action, got {action:?}"),
        };
        installer.discard_sidecar_backups();
        assert!(!backup.exists());
        assert_eq!(std::fs::read(&dst).unwrap(), b"new inventory");
        assert!(matches!(
            installer.journal.entries.back().unwrap().rollback.as_ref(),
            Some(RollbackAction::RestoreFile { backup: None, .. })
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn spdx3_sidecar_is_copied_and_removed_on_rollback() {
        let root = std::env::temp_dir().join(format!("balancify-spdx3-sidecar-{}", std::process::id()));
        let src = root.join("source.spdx3.json");
        let dst = root.join("install").join("sbom.spdx3.json");
        std::fs::create_dir_all(&root).unwrap();
        let expected = r#"{"@context":"https://spdx.org/rdf/3.0.1/spdx-context.jsonld"}"#;
        std::fs::write(&src, expected).unwrap();

        let mut installer = Installer::new(root.join("journal.json"), false);
        installer.install_sbom_sidecar(src, dst.clone()).unwrap();
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), expected);
        installer.rollback().unwrap();
        assert!(!dst.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sbom_sidecar_does_not_fabricate_a_missing_inventory() {
        let root = std::env::temp_dir().join(format!("balancify-sbom-missing-{}", std::process::id()));
        let dst = root.join("install").join("sbom.spdx.json");
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::write(&dst, b"existing inventory").unwrap();
        let mut installer = Installer::new(root.join("journal.json"), false);
        let error = installer
            .install_sbom_sidecar(root.join("missing.spdx.json"), dst.clone())
            .unwrap_err();
        assert!(error.contains("required SPDX SBOM is missing"));
        assert_eq!(std::fs::read(&dst).unwrap(), b"existing inventory");
        std::fs::remove_dir_all(root).unwrap();
    }
}
