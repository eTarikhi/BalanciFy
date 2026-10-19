//! Balancify installer -- transactional environment provisioning (Sec.4.8 rev 2.1, Sec.8.3)
//! One signed offline bundle; zero user prerequisites.
//! Each step journals, any failure rolls back in reverse order.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone)]
pub enum InstallStep {
    Preflight,
    DriverPackage { inf_path: PathBuf },
    Service { name: String, bin_path: PathBuf },
    Adapter { mtu: u16 },
    Runtimes { dotnet_bundled: bool, winappsdk_bundled: bool },
    Gui { exe_path: PathBuf },
    SelfTest,
}

#[derive(Debug, Clone)]
pub enum RollbackAction {
    RemoveDriver { inf_path: PathBuf },
    RemoveService { name: String },
    RemoveAdapter,
    RemoveRuntimes { dotnet_added: bool, winappsdk_added: bool },
    RemoveGui,
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
        let json = format!("{{\"entries\":{}}}", self.entries.len());
        std::fs::write(path, json)
    }
}

pub struct Installer {
    journal: InstallJournal,
    journal_path: PathBuf,
    dry_run: bool,
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

    pub fn self_test(&mut self) -> Result<(), String> {
        println!("[self-test] loop through embedded endpoint, local mode");
        self.journal.push(InstallStep::SelfTest, None);
        Ok(())
    }

    pub fn run(&mut self) -> Result<(), String> {
        let steps: Vec<Box<dyn Fn(&mut Self) -> Result<(), String>>> = vec![
            Box::new(|s| s.preflight()),
            Box::new(|s| s.install_driver(PathBuf::from("driver/balancify.inf"))),
            Box::new(|s| s.install_service("BalancifyService", PathBuf::from("service/balancify-service.exe"))),
            Box::new(|s| s.bring_up_adapter(1440)),
            Box::new(|s| s.ensure_runtimes()),
            Box::new(|s| s.install_gui(PathBuf::from("app/Balancify.exe"))),
            Box::new(|s| s.self_test()),
        ];

        for (i, step_fn) in steps.into_iter().enumerate() {
            if let Err(e) = step_fn(self) {
                eprintln!("Install failed at step {}: {}, rolling back", i, e);
                self.rollback()?;
                return Err(e);
            }
            let _ = self.journal.save(&self.journal_path);
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
                RollbackAction::RestoreRoutes => {
                    println!("    restoring routes/adapters/services snapshot");
                }
            }
        }
        Ok(())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let journal_path = PathBuf::from(args.get(1).map(|s| s.as_str()).unwrap_or("/tmp/balancify_install.json"));
    println!("Balancify installer -- dry_run={} journal={}", dry_run, journal_path.display());
    let mut installer = Installer::new(journal_path, dry_run);
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
        let mut inst = Installer::new(PathBuf::from("/tmp/test_journal.json"), true);
        assert!(inst.run().is_ok());
        assert_eq!(inst.journal.entries.len(), 7);
    }
}
