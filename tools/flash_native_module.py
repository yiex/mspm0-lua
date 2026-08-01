#!/usr/bin/env python3
"""Validate and install a native module without erasing the core firmware."""

import argparse
import struct
import subprocess
import tempfile
from pathlib import Path

from hold_boot_flash import (
    cleanup_hold_boot,
    reset_application,
    ssh_connect,
    start_hold_boot,
)
from host_config import jlink_exe


JLINK = jlink_exe()
SLOT_ADDR = 0x0001F800
SLOT_SIZE = 0x800
HEADER_SIZE = 32
MODULE_MAGIC = 0x444F4D4C


def crc16_modbus(data):
    crc = 0xFFFF
    for value in data:
        crc ^= value
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc


def validate(image):
    data = image.read_bytes()
    if len(data) < HEADER_SIZE or len(data) > SLOT_SIZE:
        raise SystemExit(f"invalid module size: {len(data)}")
    magic, fmt, abi, size, crc, header_size, entry = struct.unpack_from(
        "<IHHIHHI", data, 0)
    if magic != MODULE_MAGIC or fmt != 1 or abi != 1:
        raise SystemExit("invalid module magic/format/ABI")
    if size != len(data) or header_size != HEADER_SIZE:
        raise SystemExit("invalid module length/header")
    if not (SLOT_ADDR + HEADER_SIZE <= (entry & ~1) < SLOT_ADDR + size):
        raise SystemExit("module entry is outside its image")
    actual_crc = crc16_modbus(data[HEADER_SIZE:])
    if crc != actual_crc:
        raise SystemExit(
            f"module CRC mismatch: header 0x{crc:04X}, actual 0x{actual_crc:04X}"
        )
    return data[20:32].split(b"\0", 1)[0].decode("ascii", "strict")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("image", type=Path)
    parser.add_argument(
        "--hold-boot",
        action="store_true",
        help="hold BOOT through the helper board while J-Link writes the module",
    )
    args = parser.parse_args()
    image = args.image.resolve()
    name = validate(image)
    script = f"""
si 1
speed 2000
device MSPM0G3507
connect
halt
loadbin {image.as_posix()} 0x{SLOT_ADDR:08X}
verifybin {image.as_posix()} 0x{SLOT_ADDR:08X}
r
g
exit
"""
    with tempfile.NamedTemporaryFile(
        "w", suffix=".jlink", delete=False, encoding="ascii"
    ) as handle:
        handle.write(script)
        command_file = handle.name
    print(f"installing {name} at 0x{SLOT_ADDR:08X}")
    client = None
    try:
        if args.hold_boot:
            client = ssh_connect()
            start_hold_boot(client)
        result = subprocess.run(
            [str(JLINK), "-CommandFile", command_file, "-ExitOnError", "1"],
            text=True,
            encoding="utf-8",
            errors="ignore",
        )
    finally:
        if client is not None:
            cleanup_hold_boot(client)
            reset_application(client)
            client.close()
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
