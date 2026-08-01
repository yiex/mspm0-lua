#!/usr/bin/env python3
"""Host-side corruption and contract tests for NMUP v1."""

import ast
import binascii
import struct
from pathlib import Path

from compose_firmware import load_inputs, prepare_segments, resolve_selection
from module_bundle import BundleError, CRC_OFFSET, build_bundle, validate_bundle


ROOT = Path(__file__).resolve().parents[1]


def rejected(data, layout, label):
    try:
        validate_bundle(data, layout)
    except (BundleError, UnicodeDecodeError):
        return
    raise AssertionError(f"accepted corrupt bundle: {label}")


def recrc(data):
    data = bytearray(data)
    data[CRC_OFFSET:CRC_OFFSET + 4] = b"\0" * 4
    struct.pack_into("<I", data, CRC_OFFSET, binascii.crc32(data) & 0xFFFFFFFF)
    return data


def main():
    manifest, catalog, known = load_inputs()
    selected, _ = resolve_selection(manifest, known, modules=["gpio", "i2c", "tmr"])
    segments = prepare_segments(manifest, catalog, known, selected, include_core=False)
    bundle = build_bundle(manifest, selected, segments)
    layout = manifest["layout"]
    report = validate_bundle(bundle, layout)
    assert report["selected_count"] == 3

    rejected(bundle[:-1], layout, "truncated")
    damaged = bytearray(bundle)
    damaged[-1] ^= 1
    rejected(damaged, layout, "bundle CRC")
    damaged = bytearray(bundle)
    struct.pack_into("<H", damaged, 6, layout["abi_version"] + 1)
    rejected(recrc(damaged), layout, "ABI")
    damaged = bytearray(bundle)
    damaged[32 + 1] = 7
    rejected(recrc(damaged), layout, "slot index")
    damaged = bytearray(bundle)
    damaged[32 + 24:32 + 32] = b"i2c\0\0\0\0\0"
    rejected(recrc(damaged), layout, "duplicate name")
    damaged = bytearray(bundle)
    first_payload = struct.unpack_from("<I", damaged, 32 + 8)[0]
    struct.pack_into("<I", damaged, first_payload + 16, 0x18001)
    image_size = struct.unpack_from("<I", damaged, 32 + 4)[0]
    image = damaged[first_payload:first_payload + image_size]
    struct.pack_into("<I", damaged, 32 + 12, binascii.crc32(image) & 0xFFFFFFFF)
    rejected(recrc(damaged), layout, "entry address")

    empty = build_bundle(manifest, [], [])
    assert validate_bundle(empty, layout)["selected_count"] == 0
    serial_source = (ROOT / "tools" / "serial_module_set.py").read_text(encoding="utf-8")
    imported = {node.names[0].name for node in ast.walk(ast.parse(serial_source))
                if isinstance(node, ast.Import)}
    assert "subprocess" not in imported
    assert "jlink" not in serial_source.lower()
    assert "bsl" in serial_source.lower()  # only the help text states it is not used
    print(f"MODULE_UPDATE_TEST_OK bytes={len(bundle)} corruptions=6 empty-set no-compiler no-jlink")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
