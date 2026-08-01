#!/usr/bin/env python3
"""Upload source or target bytecode to MSPM0 with the binary-safe HEX protocol."""
import argparse
import binascii
import sys
import time
from pathlib import Path

import serial
from serial.tools import list_ports


def resolve_port(requested):
    if requested and requested.lower() != "auto":
        return requested
    ports = list(list_ports.comports())
    for p in ports:
        if (p.vid, p.pid) == (0x1A86, 0x7523) or "CH340" in (p.description or "").upper():
            return p.device
    for p in ports:
        if "JLINK CDC" in (p.description or "").upper():
            return p.device
    raise SystemExit("no CH340/J-Link UART port found")


def read_until(ser, token, timeout):
    deadline = time.time() + timeout
    data = b""
    while time.time() < deadline and token not in data:
        # Avoid waiting for a full 128-byte read when a short acknowledgement
        # is already available; baud negotiation depends on a prompt switch.
        data += ser.read(max(1, min(ser.in_waiting, 128)))
    return data


def negotiate_baud(ser, connect_baud, target_baud):
    if target_baud == connect_baud:
        return
    ser.write(f"baud {target_baud}\n".encode("ascii"))
    ser.flush()
    switch_line = f"BAUD_SWITCH {target_baud}".encode("ascii")
    switching = read_until(ser, switch_line + b"\r\n", 2.0)
    if switch_line + b"\r\n" not in switching:
        raise RuntimeError(
            f"device did not accept baud switch from {connect_baud} to {target_baud}: "
            + switching.decode("utf-8", errors="replace").strip()
        )
    # Keep the host at the old divisor through the firmware's 300 ms guard.
    # Changing a CH340 divisor during that guard can put a stray byte into the
    # MCU RX path and prefix the confirmation command.
    time.sleep(0.35)
    ser.baudrate = target_baud
    time.sleep(0.2)
    ser.reset_input_buffer()
    # A divisor change can be observed as a stray character by the MCU RX.
    # Prefix the confirmation in the same USB write with a line terminator so
    # every earlier fragment is guaranteed to be discarded first.
    ser.write(f"\nbaud {target_baud}\n".encode("ascii"))
    ser.flush()
    ok_line = f"BAUD_OK {target_baud}".encode("ascii")
    confirmed = read_until(ser, ok_line + b"\r\n", 2.0)
    if ok_line + b"\r\n" not in confirmed:
        raise RuntimeError(
            f"device did not confirm {target_baud} baud: "
            + confirmed.decode("utf-8", errors="replace").strip()
        )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("script", type=Path, help="path to .lua or .luac")
    ap.add_argument("--name", help="LittleFS target (default: main.lua/main.luac)")
    ap.add_argument("--port", default="auto", help="COM port; auto prefers CH340")
    ap.add_argument("--connect-baud", type=int, default=115200,
                    help="current device baud before negotiation (default: 115200)")
    ap.add_argument("--baud", type=int, default=460800,
                    help="upload baud negotiated after connect (default: 460800)")
    ap.add_argument(
        "--chunk-size", type=int, default=120,
        help="binary bytes per acknowledged HEX line (1..127, default: 120)",
    )
    ap.add_argument(
        "--run-timeout", type=float, default=15.0,
        help="seconds to wait for main.lua/main.luac SCRIPT_DONE (default: 15)",
    )
    ap.add_argument("--no-run-listen", action="store_true")
    args = ap.parse_args()
    if not 1 <= args.chunk_size <= 127:
        ap.error("--chunk-size must be in 1..127 (firmware line limit is 255 chars)")
    if args.run_timeout <= 0:
        ap.error("--run-timeout must be positive")
    args.port = resolve_port(args.port)

    data = args.script.read_bytes()
    target = args.name or ("main.luac" if args.script.suffix.lower() == ".luac" else "main.lua")
    run_required = target in ("main.lua", "main.luac")

    ser = serial.Serial(args.port, args.connect_baud, timeout=0.2)
    # CH340 needs a short settle interval after the port is reopened.
    time.sleep(0.6)
    # drain
    ser.read(4096)

    try:
        negotiate_baud(ser, args.connect_baud, args.baud)
    except RuntimeError as error:
        print(error, file=sys.stderr)
        ser.close()
        return 2

    print(
        f"upload {args.script} -> {target} @ {args.port} "
        f"{args.baud} baud ({len(data)} bytes)"
    )
    ser.write(f"<<<HEX {target}\r\n".encode("ascii"))
    ser.flush()
    begin_deadline = time.time() + 3.0
    begin = b""
    while time.time() < begin_deadline and b"SCRIPT_BEGIN" not in begin:
        begin += ser.read(128)
    if b"SCRIPT_BEGIN" not in begin:
        sys.stdout.write(begin.decode("utf-8", errors="replace"))
        print("no SCRIPT_BEGIN", file=sys.stderr)
        ser.close()
        return 2
    for off in range(0, len(data), args.chunk_size):
        ser.write(binascii.hexlify(data[off:off + args.chunk_size]) + b"\r\n")
        ser.flush()
        ack_deadline = time.time() + 3.0
        ack = b""
        while time.time() < ack_deadline and b"HEX_OK" not in ack:
            ack += ser.read(64)
            if b"SCRIPT_ERR" in ack:
                break
        if b"HEX_OK" not in ack:
            sys.stdout.write(ack.decode("utf-8", errors="replace"))
            print("HEX block was not acknowledged", file=sys.stderr)
            ser.close()
            return 2
    ser.write(b">>>HEX\r\n")
    ser.flush()

    deadline = time.time() + max(12.0, len(data) / 2000.0)
    buf = b""
    ok = False
    run_ok = not run_required or args.no_run_listen
    run_done = not run_required or args.no_run_listen
    while time.time() < deadline:
        chunk = ser.read(256)
        if chunk:
            buf += chunk
            sys.stdout.write(chunk.decode("utf-8", errors="replace"))
            sys.stdout.flush()
            if b"SCRIPT_OK" in buf:
                ok = True
                if b"SCRIPT_DONE OK" in buf:
                    run_ok = True
                    run_done = True
                elif b"SCRIPT_DONE ERR" in buf or b"SCRIPT_DONE PENDING" in buf:
                    run_done = True
                break
            if b"SCRIPT_ERR" in buf:
                break
        else:
            time.sleep(0.05)

    if not args.no_run_listen:
        # SCRIPT_OK only acknowledges the atomic write.  Do not send another
        # command (especially a baud switch) while the uploaded main script
        # owns the interpreter; wait for its terminal marker instead.
        end = time.time() + args.run_timeout
        while time.time() < end:
            chunk = ser.read(256)
            if chunk:
                sys.stdout.write(chunk.decode("utf-8", errors="replace"))
                sys.stdout.flush()
                buf += chunk
                if b"SCRIPT_DONE OK" in buf:
                    run_ok = True
                    run_done = True
                    break
                if b"SCRIPT_DONE ERR" in buf or b"SCRIPT_DONE PENDING" in buf:
                    run_done = True
                    break

    success = ok and run_ok
    if run_required and not args.no_run_listen and not run_done:
        print(
            f"script did not finish within {args.run_timeout:g}s; "
            "leaving device at its current baud",
            file=sys.stderr,
        )
        ser.close()
        return 2
    if args.baud != args.connect_baud:
        try:
            negotiate_baud(ser, args.baud, args.connect_baud)
        except RuntimeError as error:
            print(f"failed to restore {args.connect_baud} baud: {error}", file=sys.stderr)
            success = False
    ser.close()
    return 0 if success else 2


if __name__ == "__main__":
    raise SystemExit(main())
