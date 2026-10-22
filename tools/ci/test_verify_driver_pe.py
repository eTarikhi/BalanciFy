import struct
import tempfile
import unittest
from pathlib import Path

from verify_driver_pe import pe_certificate_table, pe_machine, verify_driver


def make_pe(machine: int, certificate: tuple[int, int] = (0, 0)) -> bytes:
    pe_offset = 0x80
    optional_size = 0xF0
    image = bytearray(pe_offset + 24 + optional_size)
    image[:2] = b"MZ"
    struct.pack_into("<I", image, 0x3C, pe_offset)
    image[pe_offset : pe_offset + 4] = b"PE\0\0"
    struct.pack_into("<H", image, pe_offset + 4, machine)
    struct.pack_into("<H", image, pe_offset + 20, optional_size)
    optional_offset = pe_offset + 24
    struct.pack_into("<H", image, optional_offset, 0x20B)  # PE32+
    security_entry = optional_offset + 112 + 4 * 8
    struct.pack_into("<II", image, security_entry, *certificate)
    return bytes(image)


class DriverPeVerificationTests(unittest.TestCase):
    def test_reads_x64_and_arm64_machine_types_and_empty_certificate_table(self):
        for machine in (0x8664, 0xAA64):
            image = make_pe(machine)
            self.assertEqual(pe_machine(image), machine)
            self.assertEqual(pe_certificate_table(image), (0, 0))

    def test_rejects_wrong_architecture_for_expected_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            driver = Path(directory) / "driver.sys"
            driver.write_bytes(make_pe(0x8664))
            with self.assertRaisesRegex(ValueError, "expected ARM64.*got 0x8664"):
                verify_driver(driver, "ARM64")

    def test_rejects_signed_image_for_unsigned_ci_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            driver = Path(directory) / "driver.sys"
            driver.write_bytes(make_pe(0xAA64, certificate=(0x400, 0x80)))
            with self.assertRaisesRegex(ValueError, "expected an unsigned driver"):
                verify_driver(driver, "ARM64")

    def test_rejects_malformed_pe_image(self):
        with self.assertRaisesRegex(ValueError, "DOS header"):
            pe_machine(b"not a driver")


if __name__ == "__main__":
    unittest.main()
