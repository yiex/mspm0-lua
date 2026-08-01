#!/usr/bin/env python3
"""Install a prebuilt native-module set through the application UART."""

import argparse
import binascii
import re
import sys
import time
from pathlib import Path

import serial

from compose_firmware import load_inputs, prepare_segments, resolve_selection
from build_catalog_release import catalog_records, catalog_sha256
from module_bundle import build_bundle, validate_bundle
from upload_script import negotiate_baud, read_until, resolve_port


def write_line(ser, line):
    ser.write(line.encode("ascii") + b"\n")
    ser.flush()


def expect(ser, token, timeout, phase):
    data = read_until(ser, token, timeout)
    sys.stdout.write(data.decode("utf-8", errors="replace"))
    if token not in data:
        raise RuntimeError(f"{phase}: expected {token!r}")
    if b"MOD_ERR" in data and not token.startswith(b"MOD_ERR"):
        raise RuntimeError(f"{phase}: device rejected update")
    return data


def upload_hex(ser, name, payload, chunk_size=120):
    write_line(ser, f"<<<HEX {name}")
    expect(ser, b"SCRIPT_BEGIN", 3.0, "upload begin")
    for offset in range(0, len(payload), chunk_size):
        ser.write(binascii.hexlify(payload[offset:offset + chunk_size]) + b"\n")
        ser.flush()
        expect(ser, b"HEX_OK", 3.0, f"upload block {offset // chunk_size}")
    write_line(ser, ">>>HEX")
    done = expect(ser, b"SCRIPT_OK", max(8.0, len(payload) / 5000.0), "upload finish")
    if f"SCRIPT_OK {len(payload)}".encode("ascii") not in done:
        raise RuntimeError("upload finish: byte count mismatch")
    return done


def wait_for_module_update(ser, timeout=60.0):
    deadline = time.time() + timeout
    data = b""
    while time.time() < deadline:
        chunk = ser.read(max(1, min(ser.in_waiting, 256)))
        if chunk:
            data += chunk
            sys.stdout.write(chunk.decode("utf-8", errors="replace"))
            sys.stdout.flush()
            if b"MOD_ERR" in data:
                raise RuntimeError("flash install: device reported MOD_ERR")
            if b"MOD_READY" in data and b"MOD_DONE" in data and b"Idle" in data:
                return data
    raise RuntimeError("flash install: timed out before MOD_DONE/Idle")


def wait_for_script_done(ser, initial=b"", timeout=15.0):
    deadline = time.time() + timeout
    data = bytearray(initial)
    terminal = (b"SCRIPT_DONE OK", b"SCRIPT_DONE ERR", b"SCRIPT_DONE PENDING")
    while time.time() < deadline:
        if any(token in data for token in terminal):
            return bytes(data)
        chunk = ser.read(max(1, min(ser.in_waiting, 256)))
        if chunk:
            data.extend(chunk)
            sys.stdout.write(chunk.decode("utf-8", errors="replace"))
            sys.stdout.flush()
    return bytes(data)


def expected_fwinfo(manifest):
    catalog = manifest["catalog"]
    layout = manifest["layout"]
    return [
        f"FW_INFO {catalog['firmware_id']} {catalog['firmware_version']}",
        f"FW_TARGET {catalog['target']}",
        f"FW_ABI {catalog['core_abi']}",
        f"FW_MODULE_FORMAT {catalog['module_format']}",
        f"FW_NMUP_FORMAT {catalog['nmup_format']}",
        f"FW_SLOTS {layout['slot_count']} {layout['slot_size']}",
    ]


def validate_fwinfo(data, manifest, catalog_sha256):
    text = data.decode("ascii", errors="replace").replace("\r", "")
    lines = [line for line in text.split("\n") if line]
    expected = expected_fwinfo(manifest)
    expected.append(f"FW_CATALOG {catalog_sha256}")
    expected.append("FW_INFO_END")
    if lines[-len(expected):] != expected:
        raise RuntimeError(
            "fwinfo: connected firmware/catalog identity does not match the local release"
        )


def expected_slots(segments):
    return {
        int(segment["slot"]): (
            segment["name"],
            int(segment["size"]),
            f"{binascii.crc32(segment['data']) & 0xFFFFFFFF:08x}",
        )
        for segment in segments
    }


def parse_slots(data):
    slots = {}
    text = data.decode("ascii", errors="replace").replace("\r", "")
    for line in text.split("\n"):
        match = re.fullmatch(
            r"MOD_SLOT ([0-7]) ([a-z0-9_]+) ([0-9]+) ([0-9a-f]{8})", line
        )
        if match:
            slot = int(match.group(1))
            if slot in slots:
                raise RuntimeError(f"modstatus: duplicate slot {slot}")
            slots[slot] = (match.group(2), int(match.group(3)), match.group(4))
        elif line.startswith("MOD_SLOT "):
            raise RuntimeError(f"modstatus: invalid or BAD slot line: {line}")
    return slots


def luac_target(path):
    name = path.name
    allowed = all(
        "a" <= c <= "z" or "A" <= c <= "Z" or "0" <= c <= "9" or c in "_.-"
        for c in name
    )
    if path.suffix.lower() != ".luac" or not allowed or not 1 <= len(name) <= 28:
        raise ValueError(f"invalid dependency LUAC target name: {name}")
    if name == "main.luac":
        raise ValueError("dependency target must not be main.luac")
    return name


def main():
    parser = argparse.ArgumentParser(
        description="Switch native modules without reset, BSL, BOOT, J-Link, or compilation."
    )
    choice = parser.add_mutually_exclusive_group(required=True)
    choice.add_argument("--set", dest="set_name")
    choice.add_argument("--modules", nargs="*", help="explicit modules in slot order; empty means core only")
    parser.add_argument("--port", default="auto")
    parser.add_argument("--connect-baud", type=int, default=115200)
    parser.add_argument("--baud", type=int, default=460800)
    parser.add_argument("--name", default="modules.upd", help="temporary LittleFS bundle name")
    parser.add_argument("--chunk-size", type=int, default=120)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--plan-only", action="store_true")
    parser.add_argument("--keep-bundle", action="store_true")
    parser.add_argument(
        "--dependency", type=Path, action="append", default=[],
        help="dependency .luac to upload by basename; repeat in dependency order",
    )
    parser.add_argument("--luac", type=Path, help="upload this main.luac after modules activate")
    args = parser.parse_args()
    if not 1 <= args.chunk_size <= 127:
        parser.error("--chunk-size must be in 1..127")

    manifest, catalog, known = load_inputs()
    selected, label = resolve_selection(manifest, known, args.set_name, args.modules)
    segments = prepare_segments(manifest, catalog, known, selected, include_core=False)
    bundle = build_bundle(manifest, selected, segments)
    report = validate_bundle(bundle, manifest["layout"])
    local_catalog_sha256 = catalog_sha256(catalog_records()[2])
    output = (args.output or Path(__file__).resolve().parents[1] / "mspm0_lua" /
              "build_composed" / f"modules_{label}.upd").resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(bundle)
    print(f"NMUP {label}: {len(selected)}/8 modules, {len(bundle)} bytes, CRC32 {report['crc32']:08x}")
    for segment in segments:
        print(f"  slot{segment['slot']} {segment['name']} {segment['size']} bytes")
    if args.plan_only:
        return 0
    if args.luac and (not args.luac.is_file() or args.luac.suffix.lower() != ".luac"):
        parser.error("--luac must be an existing .luac file")
    dependencies = []
    dependency_names = set()
    for dependency in args.dependency:
        dependency = dependency.resolve()
        if not dependency.is_file():
            parser.error(f"dependency does not exist: {dependency}")
        try:
            target = luac_target(dependency)
        except ValueError as error:
            parser.error(str(error))
        if target in dependency_names:
            parser.error(f"duplicate dependency target: {target}")
        dependency_names.add(target)
        dependencies.append((target, dependency))

    port = resolve_port(args.port)
    ser = serial.Serial(port=None, baudrate=args.connect_baud, timeout=0.05,
                        xonxoff=False, rtscts=False, dsrdtr=False)
    ser.dtr = False
    ser.rts = False
    ser.port = port
    ser.open()
    active_baud = args.connect_baud
    try:
        time.sleep(0.25)
        ser.reset_input_buffer()
        write_line(ser, "fwinfo")
        identity = expect(ser, b"FW_INFO_END\r\n", 3.0, "firmware identity")
        validate_fwinfo(identity, manifest, local_catalog_sha256)
        write_line(ser, "modstatus")
        before = expect(ser, b"MOD_STATUS_END\r\n", 3.0, "preflight")
        needs_update = parse_slots(before) != expected_slots(segments)
        if not needs_update and not dependencies and not args.luac:
            print("MOD_LAYOUT_MATCH")
            print("SERIAL_MODULE_SET_OK")
            return 0
        negotiate_baud(ser, args.connect_baud, args.baud)
        active_baud = args.baud
        if needs_update:
            print(f"UPLOAD {len(bundle)} bytes @ {args.baud}")
            upload_hex(ser, args.name, bundle, args.chunk_size)
            write_line(ser, f"modapply {args.name}")
            wait_for_module_update(ser)
        else:
            print("MOD_LAYOUT_MATCH")
        for target, dependency in dependencies:
            data = dependency.read_bytes()
            print(f"LUAC_DEP {target} {len(data)} bytes")
            upload_hex(ser, target, data, args.chunk_size)
        if args.luac:
            luac = args.luac.read_bytes()
            print(f"LUAC_MAIN {len(luac)} bytes")
            upload_result = upload_hex(ser, "main.luac", luac, args.chunk_size)
            run_result = wait_for_script_done(ser, upload_result)
            if b"SCRIPT_DONE OK" not in run_result:
                raise RuntimeError("LUAC run: device did not report SCRIPT_DONE OK")
        write_line(ser, "modstatus")
        status = expect(ser, b"MOD_STATUS_END\r\n", 5.0, "postflight")
        if b"MOD_STATUS IDLE" not in status:
            raise RuntimeError("postflight: module update is still pending")
        if parse_slots(status) != expected_slots(segments):
            raise RuntimeError("postflight: module slot name/size/CRC differs from selection")
        negotiate_baud(ser, args.baud, args.connect_baud)
        active_baud = args.connect_baud
        print("SERIAL_MODULE_SET_OK")
        return 0
    except Exception as error:
        print(f"SERIAL_MODULE_SET_FAIL: {error}", file=sys.stderr)
        if active_baud != 115200:
            try:
                # An invalid line aborts a possibly incomplete HEX transaction.
                write_line(ser, "zz")
                time.sleep(0.1)
                negotiate_baud(ser, active_baud, 115200)
                print("SERIAL_RECOVERED_115200", file=sys.stderr)
            except Exception as recovery_error:
                print(f"SERIAL_RECOVERY_UNCERTAIN: {recovery_error}", file=sys.stderr)
        return 2
    finally:
        ser.close()
        if not args.keep_bundle and output.exists():
            output.unlink()


if __name__ == "__main__":
    raise SystemExit(main())
