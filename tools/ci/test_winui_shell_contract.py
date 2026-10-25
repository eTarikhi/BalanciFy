from pathlib import Path
import unittest
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]
APP_DIR = ROOT / "windows" / "BalanciFy.App"
PROJECT = (APP_DIR / "BalanciFy.App.csproj").read_text(encoding="utf-8")
CLIENT = (APP_DIR / "IpcClient.cs").read_text(encoding="utf-8")
PREFERENCES = (APP_DIR / "UiPreferencesStore.cs").read_text(encoding="utf-8")
WINDOW = (APP_DIR / "MainWindow.xaml.cs").read_text(encoding="utf-8")
GUIDE = (APP_DIR / "README.md").read_text(encoding="utf-8")
IPC_PROTOCOL = (ROOT / "crates" / "ipc" / "src" / "lib.rs").read_text(encoding="utf-8")
CORE_RULES = (ROOT / "crates" / "core" / "src" / "rules.rs").read_text(encoding="utf-8")


class WinUiShellContractTests(unittest.TestCase):
    def test_project_and_xaml_files_are_well_formed_xml(self):
        for path in (
            APP_DIR / "BalanciFy.App.csproj",
            APP_DIR / "App.xaml",
            APP_DIR / "MainWindow.xaml",
        ):
            with self.subTest(path=path.name):
                ET.parse(path)

    def test_shell_targets_the_pinned_native_windows_stack(self):
        self.assertIn("net10.0-windows10.0.19041.0", PROJECT)
        self.assertIn('Microsoft.WindowsAppSDK" Version="2.5.1"', PROJECT)
        self.assertIn("<WindowsPackageType>None</WindowsPackageType>", PROJECT)
        self.assertIn("<WindowsAppSDKSelfContained>true</WindowsAppSDKSelfContained>", PROJECT)
        self.assertIn("win-x64;win-arm64", PROJECT)

    def test_shell_uses_only_local_named_pipe_for_service_state(self):
        self.assertIn('new NamedPipeClientStream(\n            ".",\n            PipeName', CLIENT)
        self.assertIn('"BalanciFy.Service.v1"', CLIENT)
        self.assertIn("TokenImpersonationLevel.Impersonation", CLIENT)
        self.assertNotIn("System.Net.Sockets", CLIENT)
        self.assertIn("GetStatusOpcode = 1", CLIENT)
        self.assertIn("ListFlowRulesOpcode = 3", CLIENT)
        self.assertIn("RunSelfTestOpcode = 9", CLIENT)
        self.assertIn("SetFlowRuleOpcode = 4", CLIENT)
        self.assertIn("RemoveFlowRuleOpcode = 5", CLIENT)
        self.assertIn("SetBondModeOpcode = 6", CLIENT)
        self.assertIn("SetDuplicateModeOpcode = 7", CLIENT)
        self.assertIn("SetAllowMeteredOpcode = 8", CLIENT)
        self.assertIn("SetFlowRuleAsync", WINDOW)
        self.assertIn("GetSupportBundleAsync", WINDOW)
        self.assertIn("GetRelaySnapshotAsync", WINDOW)
        self.assertIn("ExportSupportBundleButton_Click", WINDOW)
        self.assertIn("EditRuleButton_Click", WINDOW)
        self.assertIn("RemoveRuleButton_Click", WINDOW)
        self.assertIn("RulePinnedUplinkBox", WINDOW)
        self.assertIn("ApplySettingsButton_Click", WINDOW)

    def test_client_request_codes_match_rust_ipc_contract(self):
        opcodes = {
            "GetStatus": 1,
            "ListFlowRules": 3,
            "SetFlowRule": 4,
            "RemoveFlowRule": 5,
            "SetBondMode": 6,
            "SetDuplicateMode": 7,
            "SetAllowMetered": 8,
            "RunSelfTest": 9,
            "GetSupportBundle": 11,
        }
        for opcode, value in opcodes.items():
            with self.subTest(opcode=opcode):
                self.assertIn(f"Opcode::{opcode} => {value}", IPC_PROTOCOL)
        self.assertIn("FIRST_CUSTOM_RULE_ID: u32 = 1_000", CORE_RULES)
        self.assertIn("const RESP_SUPPORT_BUNDLE: u8 = 8", IPC_PROTOCOL)
        self.assertIn("id = 1_000", WINDOW)

    def test_ipc_decoder_is_bounded_and_big_endian(self):
        self.assertIn("private const int MaxPayload = 1 << 20", CLIENT)
        self.assertIn("ReadUInt32BigEndian", CLIENT)
        self.assertIn("ReadUInt64BigEndian", CLIENT)
        self.assertIn("UTF8Encoding StrictUtf8 = new(false, true)", CLIENT)
        self.assertIn("reader.RequireEnd()", CLIENT)
        self.assertIn("SupportBundleResponseTag = 8", CLIENT)
        self.assertIn("ReadString32()", CLIENT)
        self.assertIn("requestPayload.CopyTo(request, 6)", CLIENT)
        self.assertIn("AppendUInt32(payload, rule.Id)", CLIENT)
        self.assertIn("WriteUInt32BigEndian(encoded, value)", CLIENT)

    def test_home_surface_and_native_workflow_are_documented(self):
        self.assertIn("RefreshStatusAsync", WINDOW)
        self.assertIn("UplinkList.ItemsSource", WINDOW)
        self.assertIn("RulesList.Children.Add", WINDOW)
        self.assertIn("RunSelfTestAsync", WINDOW)
        self.assertIn("TimeoutException", WINDOW)
        self.assertIn("Windows 11 x64 or ARM64", GUIDE)
        self.assertIn("WSL is not a local prerequisite", GUIDE)
        self.assertIn("AutomationProperties.SetName(editButton", WINDOW)
        self.assertIn("AutomationProperties.SetName(removeButton", WINDOW)
        self.assertIn('AutomationProperties.LiveSetting="Polite"', (APP_DIR / "MainWindow.xaml").read_text(encoding="utf-8"))

    def test_relay_health_panel_uses_redacted_service_diagnostics(self):
        xaml = (APP_DIR / "MainWindow.xaml").read_text(encoding="utf-8")
        self.assertIn('Content="Relay health" Tag="relay"', xaml)
        self.assertIn('x:Name="RelayPanel"', xaml)
        self.assertIn('x:Name="RelayStatusText"', xaml)
        self.assertIn("GetRelaySnapshotAsync", WINDOW)
        self.assertIn("JsonDocument.Parse(bundle", CLIENT)
        self.assertIn('"relay_failover_count"', CLIENT)
        self.assertIn('"relay_failover_convergence_ms"', CLIENT)
        self.assertIn('"update_required"', CLIENT)
        self.assertIn("relay endpoints and identities", WINDOW.lower())

    def test_user_preferences_are_bounded_atomic_and_reapplied(self):
        self.assertIn("Environment.SpecialFolder.LocalApplicationData", PREFERENCES)
        self.assertIn('"BalanciFy",', PREFERENCES)
        self.assertIn('"ui-preferences.json"', PREFERENCES)
        self.assertIn("MaxFileBytes = 256 * 1024", PREFERENCES)
        self.assertIn("MaxCustomRules = 128", PREFERENCES)
        self.assertIn("preferences.Version != 1", PREFERENCES)
        self.assertIn("FileMode.CreateNew", PREFERENCES)
        self.assertIn("stream.Flush(flushToDisk: true)", PREFERENCES)
        self.assertIn("File.Move(temporaryPath, path, overwrite: true)", PREFERENCES)
        self.assertIn("ReconcileSavedRulesAsync", WINDOW)
        self.assertIn("currentRule != savedRule", WINDOW)
        self.assertIn("NextCustomRuleId(usedIds)", WINDOW)
        self.assertIn("SynchronizePreferencesAsync", WINDOW)
        self.assertIn("UiPreferencesStore.SaveAsync", WINDOW)
        self.assertIn("_serviceDisconnected", WINDOW)
        self.assertIn("%LOCALAPPDATA%", GUIDE)

    def test_native_full_profile_builds_winui_shell(self):
        runner = (ROOT / "tools" / "ci" / "local-ci-windows.ps1").read_text(encoding="utf-8")
        self.assertIn("Require-Command 'dotnet'", runner)
        self.assertIn("winui-shell-build", runner)
        self.assertIn("--runtime', 'win-x64", runner)
        workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        self.assertIn("actions/setup-dotnet@v5.0.0", workflow)
        self.assertIn("dotnet-version: '10.0.x'", workflow)
        self.assertIn("dotnet build windows/BalanciFy.App/BalanciFy.App.csproj", workflow)


if __name__ == "__main__":
    unittest.main()
