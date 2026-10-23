import struct
import tempfile
import unittest
from pathlib import Path

from verify_driver_pe import (
    REQUIRED_DLL_CHARACTERISTICS,
    pe_certificate_table,
    pe_dll_characteristics,
    pe_machine,
    verify_driver,
)


def make_pe(
    machine: int,
    certificate: tuple[int, int] = (0, 0),
    dll_characteristics: int = 0x01C0,
    pe32: bool = False,
) -> bytes:
    pe_offset = 0x80
    optional_size = 0xF0
    image = bytearray(pe_offset + 24 + optional_size)
    image[:2] = b"MZ"
    struct.pack_into("<I", image, 0x3C, pe_offset)
    image[pe_offset : pe_offset + 4] = b"PE\0\0"
    struct.pack_into("<H", image, pe_offset + 4, machine)
    struct.pack_into("<H", image, pe_offset + 20, optional_size)
    optional_offset = pe_offset + 24
    struct.pack_into("<H", image, optional_offset, 0x10B if pe32 else 0x20B)
    struct.pack_into("<H", image, optional_offset + (66 if pe32 else 70), dll_characteristics)
    security_directory_offset = 96 if pe32 else 112
    security_entry = optional_offset + security_directory_offset + 4 * 8
    struct.pack_into("<II", image, security_entry, *certificate)
    return bytes(image)


class DriverPeVerificationTests(unittest.TestCase):
    def test_reads_architecture_mitigations_and_empty_certificate_table(self):
        required_flags = sum(REQUIRED_DLL_CHARACTERISTICS.values())
        for machine, pe32 in ((0x8664, False), (0xAA64, False), (0x14C, True)):
            image = make_pe(machine, dll_characteristics=required_flags, pe32=pe32)
            self.assertEqual(pe_machine(image), machine)
            self.assertEqual(pe_dll_characteristics(image), required_flags)
            self.assertEqual(pe_certificate_table(image), (0, 0))

    def test_rejects_wrong_architecture_for_expected_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            driver = Path(directory) / "driver.sys"
            driver.write_bytes(make_pe(0x8664))
            with self.assertRaisesRegex(ValueError, "expected ARM64.*got 0x8664"):
                verify_driver(driver, "ARM64")

    def test_rejects_image_missing_hvci_related_linker_flags(self):
        flags = REQUIRED_DLL_CHARACTERISTICS["DYNAMIC_BASE"] | REQUIRED_DLL_CHARACTERISTICS["NX_COMPAT"]
        with tempfile.TemporaryDirectory() as directory:
            driver = Path(directory) / "driver.sys"
            driver.write_bytes(make_pe(0xAA64, dll_characteristics=flags))
            with self.assertRaisesRegex(ValueError, "missing required PE DLL characteristics: FORCE_INTEGRITY"):
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
