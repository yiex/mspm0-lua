#!/usr/bin/env python3
"""Exercise the COM console like an IDE without replacing the boot script."""

import argparse
import binascii
import json
import time
from pathlib import Path

import serial

from hold_boot_flash import jlink_reset_application
from upload_script import negotiate_baud, read_until


ROOT = Path(__file__).resolve().parents[1]


def expect(ser, token, timeout, stage, transcript):
    started = time.monotonic()
    data = read_until(ser, token, timeout)
    elapsed = time.monotonic() - started
    transcript.append(
        {
            "stage": stage,
            "seconds": round(elapsed, 3),
            "received": data.decode("utf-8", errors="replace"),
        }
    )
    if token not in data:
        raise RuntimeError(f"{stage}: missing {token!r}; received {data!r}")
    return data


def write_line(ser, line):
    ser.write(line.encode("ascii") + b"\n")
    ser.flush()


def require_lines(data, expected, stage):
    text = data.decode("ascii", errors="replace")
    missing = [line for line in expected if line not in text.splitlines()]
    if missing:
        raise RuntimeError(f"{stage}: missing lines {missing!r}; received {text!r}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("script", type=Path)
    parser.add_argument("--port", required=True)
    parser.add_argument("--connect-baud", type=int, default=115200)
    parser.add_argument("--baud", type=int, default=460800)
    parser.add_argument("--chunk-size", type=int, default=120)
    parser.add_argument("--name", default="ide_smoke.luac")
    parser.add_argument("--boot-timeout", type=float, default=6.0)
    parser.add_argument(
        "--manifest", type=Path,
        default=ROOT / "mspm0_lua/release/catalog_manifest.json",
    )
    parser.add_argument(
        "--no-reset", action="store_true",
        help="use an already-running 115200 session without J-Link reset",
    )
    args = parser.parse_args()
    if not args.script.is_file():
        parser.error(f"missing script: {args.script}")
    if not args.manifest.is_file():
        parser.error(f"missing manifest: {args.manifest}")
    if not 1 <= args.chunk_size <= 127:
        parser.error("--chunk-size must be in 1..127")

    payload = args.script.read_bytes()
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    identity = [
        f"FW_INFO {manifest['firmware_id']} {manifest['firmware_version']}",
        f"FW_TARGET {manifest['target']}",
        f"FW_ABI {manifest['core_abi']}",
        f"FW_MODULE_FORMAT {manifest['module_format']}",
        f"FW_NMUP_FORMAT {manifest['nmup_format']}",
        f"FW_SLOTS {manifest['layout']['slot_count']} {manifest['layout']['slot_size']}",
        f"FW_CATALOG {manifest['catalog_sha256']}",
        "FW_INFO_END",
    ]
    transcript = []
    result = {
        "port": args.port,
        "reset": "none" if args.no_reset else "jlink",
        "connect_baud": args.connect_baud,
        "upload_baud": args.baud,
        "file": args.name,
        "bytes": len(payload),
        "stages": transcript,
        "ok": False,
    }
    # Match gpui_ide: keep modem-control lines deasserted before opening the
    # CH340, so opening the port cannot create a reset-like DTR/RTS edge.
    ser = serial.Serial(
        port=None,
        baudrate=args.connect_baud,
        bytesize=serial.EIGHTBITS,
        parity=serial.PARITY_NONE,
        stopbits=serial.STOPBITS_ONE,
        timeout=0.05,
        xonxoff=False,
        rtscts=False,
        dsrdtr=False,
    )
    ser.dtr = False
    ser.rts = False
    ser.port = args.port
    ser.open()
    try:
        ser.reset_input_buffer()
        if not args.no_reset:
            if not jlink_reset_application():
                raise RuntimeError("jlink_reset: failed to restart application")

        write_line(ser, "!")
        expect(ser, b"STOP", args.boot_timeout, "stop_115200", transcript)

        write_line(ser, "fwinfo")
        fwinfo = expect(ser, b"FW_INFO_END", 3.0, "fwinfo_115200", transcript)
        require_lines(fwinfo, identity, "fwinfo_115200")

        write_line(ser, "storageinfo")
        storage = expect(ser, b"STORAGE_END", 3.0, "storageinfo", transcript)
        require_lines(storage, [
            "STORAGE external_littlefs",
            "PART W25Q32JVSSIQ",
            "CAPACITY 4194304",
            "PINS SPI1 PB16 PB15 PB14 PB17",
            "STORAGE_END",
        ], "storageinfo")

        write_line(ser, "fileinfo does-not-exist.luac")
        missing = expect(ser, b"FILE_ERR", 3.0, "fileinfo_missing", transcript)
        require_lines(missing, ["FILE_ERR NOT_FOUND"], "fileinfo_missing")

        write_line(ser, "fileinfo bad/name")
        invalid = expect(ser, b"FILE_ERR", 3.0, "fileinfo_invalid", transcript)
        require_lines(invalid, ["FILE_ERR INVALID_NAME"], "fileinfo_invalid")

        write_line(ser, "modstatus")
        status = expect(ser, b"MOD_STATUS_END", 3.0, "modstatus_before", transcript)
        require_lines(status, [
            "MOD_STATUS IDLE",
            f"MOD_CATALOG {manifest['catalog_sha256']}",
            "MOD_PENDING none",
            "MOD_STATUS_END",
        ], "modstatus_before")
        if not any(line.startswith("MOD_LAYOUT ") for line in status.decode("ascii").splitlines()):
            raise RuntimeError("modstatus_before: missing MOD_LAYOUT")

        started = time.monotonic()
        negotiate_baud(ser, args.connect_baud, args.baud)
        transcript.append(
            {"stage": "negotiate_up", "seconds": round(time.monotonic() - started, 3)}
        )

        write_line(ser, f"<<<HEX {args.name}")
        expect(ser, b"SCRIPT_BEGIN", 3.0, "upload_begin", transcript)
        upload_started = time.monotonic()
        blocks = 0
        for offset in range(0, len(payload), args.chunk_size):
            ser.write(binascii.hexlify(payload[offset:offset + args.chunk_size]) + b"\n")
            ser.flush()
            expect(ser, b"HEX_OK", 3.0, f"block_{blocks}", transcript)
            blocks += 1
        write_line(ser, ">>>HEX")
        final = expect(ser, b"SCRIPT_OK", 6.0, "upload_finish", transcript)
        expected_size = f"SCRIPT_OK {len(payload)}".encode("ascii")
        if expected_size not in final:
            raise RuntimeError(f"upload_finish: expected {expected_size!r}, got {final!r}")
        result["upload_seconds"] = round(time.monotonic() - upload_started, 3)
        result["blocks"] = blocks

        write_line(ser, f"fileinfo {args.name}")
        fileinfo = expect(ser, b"FILE_END", 3.0, "fileinfo_uploaded", transcript)
        crc32 = binascii.crc32(payload) & 0xFFFFFFFF
        require_lines(fileinfo, [
            f"FILE {args.name} {len(payload)} {crc32:08x}",
            "FILE_END",
        ], "fileinfo_uploaded")

        started = time.monotonic()
        negotiate_baud(ser, args.baud, args.connect_baud)
        transcript.append(
            {"stage": "negotiate_down", "seconds": round(time.monotonic() - started, 3)}
        )
        write_line(ser, "fwinfo")
        restored = expect(ser, b"FW_INFO_END", 3.0, "fwinfo_restored_115200", transcript)
        require_lines(restored, identity, "fwinfo_restored_115200")
        result["ok"] = True
        print("IDE_SESSION_OK")
        return 0
    except Exception as error:
        result["error"] = str(error)
        print(f"IDE_SESSION_FAIL: {error}")
        return 1
    finally:
        ser.close()
        if not result["ok"] and not args.no_reset:
            # A failed IDE transaction may leave the parser in HEX mode or the
            # console at the negotiated baud. Reset restores the documented
            # 115200 recovery point for the next session.
            result["recovered_115200"] = jlink_reset_application()
        print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    raise SystemExit(main())
