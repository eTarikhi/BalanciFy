#!/usr/bin/env python3
"""Validate the PE machine type of a compiled Windows driver image."""

from __future__ import annotations

import argparse
import struct
from pathlib import Path

MACHINE_TYPES = {"x64": 0x8664, "ARM64": 0xAA64}


def pe_machine(image: bytes) -> int:
    if len(image) < 64 or image[:2] != b"MZ":
        raise ValueError("missing or truncated DOS header")
    pe_offset = struct.unpack_from("<I", image, 0x3C)[0]
    if pe_offset > len(image) - 6 or image[pe_offset : pe_offset + 4] != b"PE\0\0":
        raise ValueError("missing or truncated PE signature")
    return struct.unpack_from("<H", image, pe_offset + 4)[0]


def verify_driver(path: Path, expected_arch: str) -> int:
    actual = pe_machine(path.read_bytes())
    expected = MACHINE_TYPES[expected_arch]
    if actual != expected:
        raise ValueError(
            f"{path}: expected {expected_arch} machine 0x{expected:04x}, got 0x{actual:04x}"
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
    print(f"Verified {args.architecture} driver PE machine type 0x{machine:04x}: {args.driver}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
