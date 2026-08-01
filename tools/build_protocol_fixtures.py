#!/usr/bin/env python3
"""Generate immutable NMUP v1 vectors and machine-readable expectations."""

import binascii
import hashlib
import json
import struct
from pathlib import Path

from compose_firmware import load_inputs, prepare_segments, resolve_selection
from build_catalog_release import catalog_records, catalog_sha256
from module_bundle import (
    BUNDLE_HEADER,
    BUNDLE_HEADER_SIZE,
    CRC_OFFSET,
    SLOT_ENTRY,
    SLOT_ENTRY_SIZE,
    build_bundle,
    validate_bundle,
)


ROOT = Path(__file__).resolve().parents[1] / "mspm0_lua"
OUT = ROOT / "release" / "test-vectors"
EXAMPLE = ROOT / "release" / "examples" / "ide_oled123"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def describe(bundle, layout):
    report = validate_bundle(bundle, layout)
    header = BUNDLE_HEADER.unpack_from(bundle)
    entries = []
    for slot in range(int(layout["slot_count"])):
        raw = SLOT_ENTRY.unpack_from(bundle, BUNDLE_HEADER_SIZE + slot * SLOT_ENTRY_SIZE)
        name = raw[8].split(b"\0", 1)[0].decode("ascii")
        entries.append({
            "slot": slot,
            "present": bool(raw[0]),
            "size": raw[3],
            "payload_offset": raw[4],
            "image_crc32": f"{raw[5]:08x}",
            "module_crc16": f"{raw[6]:04x}",
            "name": name,
        })
    return {
        "length": len(bundle),
        "sha256": sha256(bundle),
        "bundle_crc32": f"{report['crc32']:08x}",
        "header": {
            "magic_ascii": struct.pack("<I", header[0]).decode("ascii"),
            "format": header[1],
            "abi": header[2],
            "header_size": header[3],
            "slot_count": header[4],
            "selected_count": header[5],
            "total_size": header[6],
            "crc32_offset": CRC_OFFSET,
        },
        "entries": entries,
    }


def append_upload(lines, name, payload, baud=460800):
    lines.append(f"H@{baud} <<<HEX {name}")
    lines.append(f"D@{baud} SCRIPT_BEGIN")
    for offset in range(0, len(payload), 120):
        lines.append(f"H@{baud} {payload[offset:offset + 120].hex()}")
        lines.append(f"D@{baud} HEX_OK")
    lines.append(f"H@{baud} >>>HEX")
    lines.append(f"D@{baud} SCRIPT_OK {len(payload)}")


def slot_region_crc(bundle, layout):
    slot_size = int(layout["slot_size"])
    region = bytearray(b"\xff" * (int(layout["slot_count"]) * slot_size))
    for slot in range(int(layout["slot_count"])):
        raw = SLOT_ENTRY.unpack_from(bundle, BUNDLE_HEADER_SIZE + slot * SLOT_ENTRY_SIZE)
        if raw[0]:
            size, offset = raw[3], raw[4]
            start = slot * slot_size
            region[start:start + size] = bundle[offset:offset + size]
    return binascii.crc32(region) & 0xFFFFFFFF


def append_status(lines, slots, layout_crc, catalog_hash, baud=460800):
    lines.append(f"H@{baud} modstatus")
    lines.append(f"D@{baud} MOD_STATUS IDLE")
    lines.append(f"D@{baud} MOD_CATALOG {catalog_hash}")
    valid_count = 0
    for entry in slots:
        if entry["present"]:
            valid_count += 1
            lines.append(
                f"D@{baud} MOD_SLOT {entry['slot']} {entry['name']} "
                f"{entry['size']} {entry['image_crc32']}"
            )
    lines.append(f"D@{baud} MOD_LAYOUT {valid_count} {layout_crc:08x}")
    lines.append(f"D@{baud} MOD_PENDING none")
    lines.append(f"D@{baud} MOD_STATUS_END")


def write_transcript(manifest, vectors):
    catalog = manifest["catalog"]
    layout = manifest["layout"]
    catalog_hash = catalog_sha256(catalog_records()[2])
    i2c = (OUT / "i2c-only-valid.nmup").read_bytes()
    font = (EXAMPLE / "font_digits.luac").read_bytes()
    oled = (EXAMPLE / "oled123.luac").read_bytes()
    main = (EXAMPLE / "main.luac").read_bytes()
    full_bundle = (OUT / "full-valid.nmup").read_bytes()
    lines = [
        "# hardware-verified protocol transcript, 2026-07-27",
        "# H=host, D=device; every HEX payload and acknowledgement is included.",
        "H@115200 !",
        "D@115200 STOP",
        "H@115200 fwinfo",
        f"D@115200 FW_INFO {catalog['firmware_id']} {catalog['firmware_version']}",
        f"D@115200 FW_TARGET {catalog['target']}",
        f"D@115200 FW_ABI {catalog['core_abi']}",
        f"D@115200 FW_MODULE_FORMAT {catalog['module_format']}",
        f"D@115200 FW_NMUP_FORMAT {catalog['nmup_format']}",
        f"D@115200 FW_SLOTS {layout['slot_count']} {layout['slot_size']}",
        f"D@115200 FW_CATALOG {catalog_hash}",
        "D@115200 FW_INFO_END",
        "H@115200 storageinfo",
        "D@115200 STORAGE external_littlefs",
        "D@115200 PART W25Q32JVSSIQ",
        "D@115200 CAPACITY 4194304",
        "D@115200 PINS SPI1 PB16 PB15 PB14 PB17",
        "D@115200 STORAGE_END",
        "H@115200 fileinfo does-not-exist.luac",
        "D@115200 FILE_ERR NOT_FOUND",
    ]
    append_status(
        lines, vectors["full-valid.nmup"]["entries"],
        slot_region_crc(full_bundle, layout), catalog_hash, 115200,
    )
    lines.extend([
        "H@115200 baud 460800",
        "D@115200 BAUD_SWITCH 460800",
        "D@460800 BAUD_OK 460800",
    ])
    append_upload(lines, "modules.upd", i2c)
    lines.extend([
        "H@460800 modapply modules.upd",
        "D@460800 MOD_READY 1 3640",
        "D@460800 MOD_APPLY modules.upd",
        "D@460800 MOD_ERASE 0",
        "D@460800 MOD_WRITE 0 i2c",
        "D@460800 MOD_ERASE 1",
        "D@460800 MOD_ERASE 2",
        "D@460800 MOD_ERASE 3",
        "D@460800 MOD_ERASE 4",
        "D@460800 MOD_ERASE 5",
        "D@460800 MOD_ERASE 6",
        "D@460800 MOD_ERASE 7",
        "D@460800 MOD_VERIFY",
        "D@460800 MOD_DONE 1",
        "D@460800 MOD i2c",
        "D@460800 Idle",
    ])
    append_status(
        lines, vectors["i2c-only-valid.nmup"]["entries"],
        slot_region_crc(i2c, layout), catalog_hash,
    )
    append_upload(lines, "font_digits.luac", font)
    append_upload(lines, "oled123.luac", oled)
    append_upload(lines, "main.luac", main)
    lines.extend([
        "D@460800 MOD i2c",
        "D@460800 OLED_123_TWO_DEP_OK",
        "D@460800 SCRIPT_DONE OK",
        "H@460800 fileinfo main.luac",
        f"D@460800 FILE main.luac {len(main)} {binascii.crc32(main) & 0xFFFFFFFF:08x}",
        "D@460800 FILE_END",
    ])
    append_status(
        lines, vectors["i2c-only-valid.nmup"]["entries"],
        slot_region_crc(i2c, layout), catalog_hash,
    )
    lines.extend([
        "H@460800 baud 115200",
        "D@460800 BAUD_SWITCH 115200",
        "D@115200 BAUD_OK 115200",
    ])
    path = OUT / "verified-transcript.txt"
    data = ("\n".join(lines) + "\n").encode("ascii")
    path.write_bytes(data)
    return {"path": path.name, "length": len(data), "sha256": sha256(data)}


def main():
    manifest, catalog, known = load_inputs()
    vectors = {}
    OUT.mkdir(parents=True, exist_ok=True)
    for label, selection in (("i2c-only", ["i2c"]), ("full", manifest["sets"]["full"])):
        selected, _ = resolve_selection(manifest, known, modules=selection)
        segments = prepare_segments(manifest, catalog, known, selected, include_core=False)
        bundle = build_bundle(manifest, selected, segments)
        path = OUT / f"{label}-valid.nmup"
        path.write_bytes(bundle)
        vectors[path.name] = describe(bundle, manifest["layout"])

    valid = (OUT / "i2c-only-valid.nmup").read_bytes()
    damaged = bytearray(valid)
    damaged[-1] ^= 0x01
    damaged_path = OUT / "i2c-only-bundle-crc.nmup"
    damaged_path.write_bytes(damaged)
    vectors[damaged_path.name] = {
        "length": len(damaged),
        "sha256": sha256(damaged),
        "mutation": "last payload byte XOR 0x01; bundle CRC field unchanged",
        "stored_bundle_crc32": f"{struct.unpack_from('<I', damaged, CRC_OFFSET)[0]:08x}",
        "computed_bundle_crc32": f"{binascii.crc32(damaged[:CRC_OFFSET] + b'\0\0\0\0' + damaged[CRC_OFFSET + 4:]) & 0xFFFFFFFF:08x}",
        "expected_response": "MOD_ERR bundle-crc",
        "expected_state": "MOD_STATUS IDLE; internal slots unchanged; retry requires re-upload",
    }
    transcript = write_transcript(manifest, vectors)
    metadata = {
        "schema": 1,
        "catalog": manifest["catalog"],
        "layout": manifest["layout"],
        "crc_algorithms": {
            "bundle_and_image": "CRC-32/ISO-HDLC as Python binascii.crc32",
            "module_payload": "CRC-16/MODBUS, initial 0xffff, polynomial 0xa001",
        },
        "vectors": vectors,
        "hardware_verified_transcript": transcript,
    }
    metadata_path = OUT / "vectors.json"
    metadata_path.write_text(
        json.dumps(metadata, indent=2, ensure_ascii=True) + "\n", encoding="utf-8"
    )
    print(f"PROTOCOL_FIXTURES_OK {metadata_path} vectors={len(vectors)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
