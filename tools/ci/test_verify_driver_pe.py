import struct
import tempfile
import unittest
from pathlib import Path

from verify_driver_pe import pe_machine, verify_driver


def make_pe(machine: int) -> bytes:
    image = bytearray(0x80 + 6)
    image[:2] = b"MZ"
    struct.pack_into("<I", image, 0x3C, 0x80)
    image[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<H", image, 0x84, machine)
    return bytes(image)


class DriverPeVerificationTests(unittest.TestCase):
    def test_reads_x64_and_arm64_machine_types(self):
        self.assertEqual(pe_machine(make_pe(0x8664)), 0x8664)
        self.assertEqual(pe_machine(make_pe(0xAA64)), 0xAA64)

    def test_rejects_wrong_architecture_for_expected_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            driver = Path(directory) / "driver.sys"
            driver.write_bytes(make_pe(0x8664))
            with self.assertRaisesRegex(ValueError, "expected ARM64.*got 0x8664"):
                verify_driver(driver, "ARM64")

    def test_rejects_malformed_pe_image(self):
        with self.assertRaisesRegex(ValueError, "DOS header"):
            pe_machine(b"not a driver")


if __name__ == "__main__":
    unittest.main()
