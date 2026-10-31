from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
SERVICE = (ROOT / "crates/service/src/lib.rs").read_text(encoding="utf-8")
STORE = (ROOT / "crates/service/src/machine_policy.rs").read_text(encoding="utf-8")
SERVICE_BIN = (ROOT / "crates/service/src/bin/balancify-service.rs").read_text(encoding="utf-8")
IPC = (ROOT / "crates/ipc/src/lib.rs").read_text(encoding="utf-8")
CLIENT = (ROOT / "windows/BalanciFy.App/IpcClient.cs").read_text(encoding="utf-8")
WINDOW = (ROOT / "windows/BalanciFy.App/MainWindow.xaml.cs").read_text(encoding="utf-8")
XAML = (ROOT / "windows/BalanciFy.App/MainWindow.xaml").read_text(encoding="utf-8")
LEGACY = (ROOT / "windows/BalanciFy.App/LegacyPolicySnapshotStore.cs").read_text(encoding="utf-8")
ETW = (ROOT / "crates/service/src/etw.rs").read_text(encoding="utf-8")


class MachinePolicyContractTests(unittest.TestCase):
    def test_policy_schema_is_versioned_bounded_and_excludes_runtime_state(self):
        for text in (
            'const MAGIC: &[u8; 4] = b"BFPY"',
            "const VERSION: u16 = 1",
            "MAX_CUSTOM_RULES: usize = ipc::MAX_CUSTOM_POLICY_RULES",
            "const MAX_FILE_BYTES: usize = 64 * 1024",
            "pub bond_enabled: bool",
            "pub duplicate_mode: bool",
            "pub allow_metered: bool",
            "pub custom_rules: Vec<FlowRule>",
            "fn checksum32(bytes: &[u8]) -> u32",
        ):
            with self.subTest(contract=text):
                self.assertIn(text, STORE)
        for excluded in ("relay_primary", "static_keypair", "sessions:", "uplink_rtt"):
            with self.subTest(excluded=excluded):
                self.assertNotIn(excluded, STORE)
        self.assertNotIn("serde", (ROOT / "crates/service/Cargo.toml").read_text(encoding="utf-8"))

    def test_atomic_write_keeps_a_recovery_generation_and_syncs_before_ack(self):
        writer = STORE.split("fn write_temporary", 1)[1].split("fn read_policy", 1)[0]
        self.assertIn("create_new(true)", writer)
        self.assertIn("write_all(bytes)", writer)
        self.assertIn("file.sync_all()", writer)
        save = STORE.split("pub fn save(&self", 1)[1].split("fn install_recovered", 1)[0]
        self.assertLess(save.index("self.write_temporary"), save.index("fs::rename(&primary, &backup)"))
        self.assertLess(save.index("fs::rename(&primary, &backup)"), save.index("fs::rename(&temporary, &primary)"))
        self.assertLess(save.index("fs::rename(&temporary, &primary)"), save.index("sync_directory(&self.directory)"))
        self.assertIn("PRIMARY_FILE", save)
        self.assertIn("BACKUP_FILE", save)

    def test_recovery_fails_closed_on_future_schema_and_invalid_generations(self):
        load = STORE.split("pub fn load(&self)", 1)[1].split("pub fn save(&self", 1)[0]
        self.assertIn("read_policy(&primary_path, self.enforce_windows_acl)", load)
        self.assertIn("read_policy(&backup_path, self.enforce_windows_acl)", load)
        self.assertIn("io::ErrorKind::Unsupported", STORE)
        self.assertIn("unsupported machine policy schema version", STORE)
        self.assertIn("install_recovered(&policy)", load)
        self.assertIn("store_roundtrips_and_recovers_previous_atomic_generation", STORE)
        self.assertIn("store_fails_closed_when_primary_and_backup_are_both_corrupt", STORE)
        self.assertIn("policy_decoder_rejects_corruption_future_versions_and_trailing_bytes", STORE)
        self.assertIn("store_does_not_downgrade_a_future_primary_to_an_older_backup", STORE)
        self.assertIn("store_ignores_uncommitted_temporary_files", STORE)

    def test_policy_store_permissions_are_explicit_on_windows_and_unix(self):
        self.assertIn('"O:SYG:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"', STORE)
        self.assertIn("OWNER_SECURITY_INFORMATION", STORE)
        self.assertIn("PROTECTED_DACL_SECURITY_INFORMATION", STORE)
        self.assertIn("SetFileSecurityW", STORE)
        production_open = STORE.split("pub fn open(", 1)[1].split("#[cfg(test)]", 1)[0]
        self.assertIn("secure_path(&directory, true)?", production_open)
        self.assertIn("open_without_platform_acl_for_test", STORE)
        self.assertIn("#[cfg(test)]\n    pub(crate) fn open_without_platform_acl_for_test", STORE)
        self.assertIn("options.mode(0o600)", STORE)
        self.assertIn("if is_directory { 0o700 } else { 0o600 }", STORE)
        self.assertIn("store_uses_private_unix_modes_and_rejects_symlink_policy_files", STORE)

    def test_test_store_skips_windows_acl_for_reads_writes_and_recovery_only(self):
        production_open = STORE.split("pub fn open(", 1)[1].split("#[cfg(test)]", 1)[0]
        test_open = STORE.split("pub(crate) fn open_without_platform_acl_for_test", 1)[1].split("pub fn load", 1)[0]
        policy_guard = STORE.split("fn secure_policy_path(", 1)[1].split("fn secure_path(", 1)[0]
        self.assertIn("enforce_windows_acl: true", production_open)
        self.assertIn("enforce_windows_acl: false", test_open)
        self.assertIn("secure_policy_path(&path, false, self.enforce_windows_acl)", STORE)
        self.assertIn("read_policy(&primary_path, self.enforce_windows_acl)", STORE)
        self.assertIn("read_policy(&backup_path, self.enforce_windows_acl)", STORE)
        self.assertIn("if enforce_windows_acl {", policy_guard)
        self.assertIn("secure_path(path, is_directory)", policy_guard)
        self.assertIn("let _ = enforce_windows_acl;\n        secure_path(path, is_directory)", policy_guard)

    def test_ignored_windows_acl_test_checks_the_production_open_policy(self):
        test_name = "windows_real_open_sets_system_owner_and_restricted_dacl"
        start = STORE.index(f"fn {test_name}()")
        attributes = STORE[STORE.rfind("#[cfg(windows)]", 0, start):start]
        test = STORE[start:]
        self.assertIn("#[cfg(windows)]", attributes)
        self.assertIn("#[test]", attributes)
        self.assertIn("#[ignore = \"requires LocalSystem", attributes)
        self.assertIn("MachinePolicyStore::open(&directory)", test)
        self.assertIn('assert_eq!(sid_to_string(owner), "S-1-5-18"', test)
        self.assertIn("SE_DACL_PROTECTED", test)
        self.assertIn('"S-1-5-32-544"', test)
        self.assertIn('"S-1-5-11"', test)
        self.assertIn('"S-1-5-32-545"', test)

    def test_unsecured_test_constructor_is_cfg_test_only_and_has_no_runtime_callers(self):
        helper = "open_without_platform_acl_for_test"
        self.assertIn("#[cfg(test)]\n    pub(crate) fn " + helper, STORE)
        store_tests = STORE.index("#[cfg(test)]\nmod tests")
        self.assertEqual(STORE[:store_tests].count(helper), 1, "only the cfg(test) declaration may precede its test module")
        service_tests = SERVICE.index("#[cfg(test)]\nmod tests")
        self.assertNotIn(helper, SERVICE[:service_tests])
        self.assertNotIn(helper, SERVICE_BIN)

    def test_service_loads_policy_before_ipc_and_persists_before_mutating_live_state(self):
        ctor = SERVICE.split("pub fn new_with_machine_policy_store", 1)[1].split("// ------------------------------------------------------------- topology", 1)[0]
        self.assertLess(ctor.index("store.load()?"), ctor.index("Self::new(cfg"))
        self.assertLess(
            SERVICE_BIN.index("Service::new_with_machine_policy_store"),
            SERVICE_BIN.index('startup_step("start local IPC v1 listener", start_ipc_listener)?'),
        )
        commit = SERVICE.split("fn commit_machine_policy", 1)[1].split("fn machine_policy_error_response", 1)[0]
        self.assertLess(commit.index("store.save(&candidate)"), commit.index("self.cfg.bond_enabled ="))
        self.assertLess(commit.index("store.save(&candidate)"), commit.index("self.rules = candidate_rules"))
        self.assertIn("the live policy was not changed", SERVICE)
        self.assertIn("machine_policy_survives_service_restart_and_restores_rules", SERVICE)
        self.assertIn("failed_machine_policy_write_does_not_mutate_live_service_state", SERVICE)

    def test_bulk_policy_ipc_is_bounded_and_import_preserves_conflicting_rules(self):
        self.assertIn("ApplyMachinePolicy", IPC)
        self.assertIn("ApplyMachinePolicy => 12", IPC)
        self.assertIn("MAX_CUSTOM_POLICY_RULES: usize = 128", IPC)
        self.assertIn("if count > MAX_CUSTOM_POLICY_RULES { return None; }", IPC)
        self.assertIn("apply_machine_policy_rejects_rule_counts_above_the_contract_limit", IPC)
        self.assertIn("same_rule_ignoring_id", SERVICE)
        self.assertIn("legacy_policy_import_merges_rules_remaps_conflicts_and_is_idempotent", SERVICE)
        self.assertIn("ApplyMachinePolicyOpcode = 12", CLIENT)
        self.assertIn("MaxCustomPolicyRules = 128", CLIENT)
        self.assertIn("ApplyMachinePolicyAsync", WINDOW)
        self.assertIn("ImportLegacyPolicyButton_Click", WINDOW)

    def test_shell_never_replays_or_rewrites_legacy_policy_implicitly(self):
        self.assertIn("LegacyPolicySnapshotStore.LoadAsync", WINDOW)
        self.assertIn("LegacyPolicySnapshotStore.Delete()", WINDOW)
        self.assertNotIn("SynchronizePreferencesAsync", WINDOW)
        self.assertNotIn("ReconcileSavedRulesAsync", WINDOW)
        self.assertNotIn("LegacyPolicySnapshotStore.SaveAsync", WINDOW)
        self.assertNotIn("FileMode.CreateNew", LEGACY)
        self.assertIn("Import it explicitly as machine-wide policy", WINDOW)
        self.assertIn("machine-wide settings", WINDOW)
        self.assertIn('x:Name="ImportLegacyPolicyButton"', XAML)

    def test_policy_write_failure_has_dedicated_redacted_event_id(self):
        self.assertIn("EVT_POLICY_PERSISTENCE_FAILED: u16 = 700", ETW)
        self.assertIn("machine policy persistence failed kind={:?}", SERVICE)
        self.assertIn("service policy could not be saved; the live policy was not changed", SERVICE)


if __name__ == "__main__":
    unittest.main()
