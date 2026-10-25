from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
SERVER = (ROOT / "crates/ipc/src/server.rs").read_text(encoding="utf-8")
APP = (ROOT / "app/src/main.rs").read_text(encoding="utf-8")
SERVICE = (ROOT / "crates/service/src/bin/balancify-service.rs").read_text(encoding="utf-8")


class NamedPipeContractTests(unittest.TestCase):
    def test_server_uses_local_acl_pipe_and_prevents_pipe_squatting(self):
        self.assertIn('D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)', SERVER)
        self.assertIn("PIPE_REJECT_REMOTE_CLIENTS", SERVER)
        self.assertIn("FILE_FLAG_FIRST_PIPE_INSTANCE", SERVER)

    def test_client_token_is_checked_after_first_client_bytes_and_reverted(self):
        self.assertIn("ImpersonateNamedPipeClient(handle)", SERVER)
        self.assertIn("CheckTokenMembership(", SERVER)
        self.assertIn("RevertToSelf()", SERVER)
        read_impl = SERVER.split("impl Read for NamedPipeStream", 1)[1].split("impl Write for NamedPipeStream", 1)[0]
        self.assertLess(read_impl.index("validate_client_session(self.handle)?"), read_impl.index("return Ok(read as usize)"))
        self.assertIn("GetNamedPipeClientSessionId", SERVER)

    def test_service_and_lab_shell_share_the_windows_pipe_name(self):
        self.assertIn("WINDOWS_SERVICE_PIPE_NAME", SERVICE)
        self.assertIn("NamedPipeStream::connect(WINDOWS_SERVICE_PIPE_NAME)", APP)
        self.assertIn('BALANCIFY_IPC_TRANSPORT', APP)
        self.assertIn('Ok(value) if value == "loopback-tcp"', APP)


if __name__ == "__main__":
    unittest.main()
