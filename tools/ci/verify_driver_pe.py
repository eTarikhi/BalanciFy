#!/usr/bin/env python3
"""Validate a compiled Windows driver's PE architecture and unsigned status."""

from __future__ import annotations

import argparse
import struct
from pathlib import Path

MACHINE_TYPES = {"x64": 0x8664, "ARM64": 0xAA64}


def _pe_offsets(image: bytes) -> tuple[int, int, int]:
    if len(image) < 64 or image[:2] != b"MZ":
        raise ValueError("missing or truncated DOS header")
    pe_offset = struct.unpack_from("<I", image, 0x3C)[0]
    if pe_offset > len(image) - 24 or image[pe_offset : pe_offset + 4] != b"PE\0\0":
        raise ValueError("missing or truncated PE signature")

    machine = struct.unpack_from("<H", image, pe_offset + 4)[0]
    optional_size = struct.unpack_from("<H", image, pe_offset + 20)[0]
    optional_offset = pe_offset + 24
    if optional_size < 2 or optional_offset + optional_size > len(image):
        raise ValueError("missing or truncated PE optional header")
    return machine, optional_offset, optional_size


def pe_machine(image: bytes) -> int:
    return _pe_offsets(image)[0]


def pe_certificate_table(image: bytes) -> tuple[int, int]:
    _, optional_offset, optional_size = _pe_offsets(image)
    magic = struct.unpack_from("<H", image, optional_offset)[0]
    if magic == 0x10B:  # PE32
        directories_offset = 96
    elif magic == 0x20B:  # PE32+
        directories_offset = 112
    else:
        raise ValueError(f"unsupported PE optional-header magic 0x{magic:04x}")

    security_entry = directories_offset + 4 * 8  # IMAGE_DIRECTORY_ENTRY_SECURITY
    if optional_size < security_entry + 8:
        raise ValueError("truncated PE data-directory table")
    return struct.unpack_from("<II", image, optional_offset + security_entry)


def verify_driver(path: Path, expected_arch: str) -> int:
    image = path.read_bytes()
    actual = pe_machine(image)
    expected = MACHINE_TYPES[expected_arch]
    if actual != expected:
        raise ValueError(
            f"{path}: expected {expected_arch} machine 0x{expected:04x}, got 0x{actual:04x}"
        )
    certificate_offset, certificate_size = pe_certificate_table(image)
    if certificate_offset or certificate_size:
        raise ValueError(
            f"{path}: expected an unsigned driver, PE certificate table is "
            f"offset=0x{certificate_offset:x}, size={certificate_size}"
        )
    return actual


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("driver", type=Path)
    parser.add_argument("architecture", choices=tuple(MACHINE_TYPES))
    args = parser.parse_args()
    try:
        machine = verify_driver(args.driver, args.architecture)
    except (OSError, ValueError) as exc:
        parser.error(str(exc))
    print(
        f"Verified unsigned {args.architecture} driver PE machine type "
        f"0x{machine:04x}: {args.driver}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
