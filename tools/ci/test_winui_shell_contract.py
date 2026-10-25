from pathlib import Path
import unittest
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]
APP_DIR = ROOT / "windows" / "BalanciFy.App"
PROJECT = (APP_DIR / "BalanciFy.App.csproj").read_text(encoding="utf-8")
CLIENT = (APP_DIR / "IpcClient.cs").read_text(encoding="utf-8")
WINDOW = (APP_DIR / "MainWindow.xaml.cs").read_text(encoding="utf-8")
GUIDE = (APP_DIR / "README.md").read_text(encoding="utf-8")


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
        self.assertNotIn("SetBondModeOpcode", CLIENT)

    def test_ipc_decoder_is_bounded_and_big_endian(self):
        self.assertIn("private const int MaxPayload = 1 << 20", CLIENT)
        self.assertIn("ReadUInt32BigEndian", CLIENT)
        self.assertIn("ReadUInt64BigEndian", CLIENT)
        self.assertIn("UTF8Encoding StrictUtf8 = new(false, true)", CLIENT)
        self.assertIn("reader.RequireEnd()", CLIENT)

    def test_home_surface_and_native_workflow_are_documented(self):
        self.assertIn("RefreshStatusAsync", WINDOW)
        self.assertIn("UplinkList.ItemsSource", WINDOW)
        self.assertIn("RulesList.ItemsSource", WINDOW)
        self.assertIn("RunSelfTestAsync", WINDOW)
        self.assertIn("TimeoutException", WINDOW)
        self.assertIn("Windows 11 x64 or ARM64", GUIDE)
        self.assertIn("WSL is not a local prerequisite", GUIDE)

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
