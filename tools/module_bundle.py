#!/usr/bin/env python3
"""Build and validate transactional native-module update bundles."""

import binascii
import struct


BUNDLE_MAGIC = 0x50554D4E  # "NMUP"
BUNDLE_FORMAT = 1
BUNDLE_HEADER = struct.Struct("<IHHHBBII12s")
SLOT_ENTRY = struct.Struct("<BBHIIIHH8sI")
BUNDLE_HEADER_SIZE = BUNDLE_HEADER.size
SLOT_ENTRY_SIZE = SLOT_ENTRY.size
CRC_OFFSET = 16


class BundleError(ValueError):
    pass


def _fail(message):
    raise BundleError(message)


def _crc_with_zeroed_field(data):
    crc_data = bytearray(data)
    crc_data[CRC_OFFSET:CRC_OFFSET + 4] = b"\0" * 4
    return binascii.crc32(crc_data) & 0xFFFFFFFF


def build_bundle(manifest, selected, segments):
    layout = manifest["layout"]
    slot_count = int(layout["slot_count"])
    abi = int(layout["abi_version"])
    header_size = BUNDLE_HEADER_SIZE + slot_count * SLOT_ENTRY_SIZE
    by_slot = {int(segment["slot"]): segment for segment in segments}
    if len(by_slot) != len(segments) or len(segments) != len(selected):
        _fail("segment/selection mismatch")

    payload = bytearray()
    entries = bytearray()
    for slot in range(slot_count):
        segment = by_slot.get(slot)
        if segment is None:
            entries.extend(SLOT_ENTRY.pack(0, slot, 0, 0, 0, 0, 0, 0, b"", 0))
            continue
        data = bytes(segment["data"])
        name = segment["name"].encode("ascii")
        if len(name) > 7:
            _fail(f"module name does not fit bundle: {segment['name']}")
        module_crc16 = struct.unpack_from("<H", data, 12)[0]
        offset = header_size + len(payload)
        entries.extend(SLOT_ENTRY.pack(
            1, slot, 0, len(data), offset,
            binascii.crc32(data) & 0xFFFFFFFF,
            module_crc16, 0, name, 0,
        ))
        payload.extend(data)

    total_size = header_size + len(payload)
    header = BUNDLE_HEADER.pack(
        BUNDLE_MAGIC, BUNDLE_FORMAT, abi, header_size,
        slot_count, len(selected), total_size, 0, b"",
    )
    bundle = bytearray(header + entries + payload)
    struct.pack_into("<I", bundle, CRC_OFFSET, _crc_with_zeroed_field(bundle))
    validate_bundle(bundle, layout)
    return bytes(bundle)


def validate_bundle(data, layout):
    data = bytes(data)
    slot_count = int(layout["slot_count"])
    slot_size = int(layout["slot_size"])
    slot_base = int(layout["slot_base"])
    abi = int(layout["abi_version"])
    expected_header_size = BUNDLE_HEADER_SIZE + slot_count * SLOT_ENTRY_SIZE
    if len(data) < expected_header_size:
        _fail("truncated header")
    (magic, fmt, file_abi, header_size, file_slots, selected_count,
     total_size, expected_crc, reserved) = BUNDLE_HEADER.unpack_from(data)
    if magic != BUNDLE_MAGIC or fmt != BUNDLE_FORMAT:
        _fail("bundle magic/format")
    if file_abi != abi:
        _fail("bundle ABI")
    if file_slots != slot_count or header_size != expected_header_size:
        _fail("bundle layout")
    if total_size != len(data) or total_size > expected_header_size + slot_count * slot_size:
        _fail("bundle size")
    if reserved != b"\0" * len(reserved):
        _fail("bundle reserved")
    if expected_crc != _crc_with_zeroed_field(data):
        _fail("bundle CRC32")

    names = set()
    present_count = 0
    expected_offset = expected_header_size
    entries = []
    for slot in range(slot_count):
        entry = SLOT_ENTRY.unpack_from(data, BUNDLE_HEADER_SIZE + slot * SLOT_ENTRY_SIZE)
        present, entry_slot, reserved0, size, offset, image_crc, module_crc, reserved1, raw_name, flags = entry
        if entry_slot != slot or present not in (0, 1) or reserved0 or reserved1 or flags:
            _fail(f"slot{slot} entry")
        if not present:
            if size or offset or image_crc or module_crc or raw_name.strip(b"\0"):
                _fail(f"slot{slot} absent entry")
            entries.append(None)
            continue
        present_count += 1
        name_bytes = raw_name.split(b"\0", 1)[0]
        try:
            name = name_bytes.decode("ascii")
        except UnicodeDecodeError:
            _fail(f"slot{slot} name")
        if not name or len(name) > 7 or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789_" for c in name):
            _fail(f"slot{slot} name")
        if raw_name != name_bytes + b"\0" * (len(raw_name) - len(name_bytes)):
            _fail(f"slot{slot} name padding")
        if name in names:
            _fail("duplicate module name")
        names.add(name)
        if size <= 32 or size > slot_size or offset != expected_offset or offset + size > len(data):
            _fail(f"slot{slot} payload bounds")
        image = data[offset:offset + size]
        if binascii.crc32(image) & 0xFFFFFFFF != image_crc:
            _fail(f"slot{slot} image CRC32")
        magic, mod_fmt, mod_abi, image_size, payload_crc, mod_header, init, deinit = struct.unpack_from(
            "<IHHIHHII", image
        )
        mod_name = image[24:32].split(b"\0", 1)[0].decode("ascii", "strict")
        address = slot_base + slot * slot_size
        if magic != 0x444F4D4C or mod_fmt != 2 or mod_abi != abi or mod_header != 32:
            _fail(f"slot{slot} module header")
        if image_size != size or payload_crc != module_crc or mod_name != name:
            _fail(f"slot{slot} module metadata")
        if not init & 1 or not address + 32 <= (init & ~1) < address + size:
            _fail(f"slot{slot} init address")
        if deinit and (not deinit & 1 or not address + 32 <= (deinit & ~1) < address + size):
            _fail(f"slot{slot} deinit address")
        from compose_firmware import crc16_modbus
        if crc16_modbus(image[32:]) != payload_crc:
            _fail(f"slot{slot} payload CRC16")
        entries.append({"slot": slot, "name": name, "size": size, "offset": offset})
        expected_offset += size
    if present_count != selected_count or expected_offset != len(data):
        _fail("bundle selected count/length")
    return {
        "abi": abi,
        "selected_count": selected_count,
        "total_size": total_size,
        "crc32": expected_crc,
        "entries": entries,
    }
