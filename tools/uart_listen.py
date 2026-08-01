#!/usr/bin/env python3
"""Listen on a serial port and print UART traffic."""
import argparse
import sys
import time

import serial


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", required=True)
    ap.add_argument("--baud", type=int, default=115200)
    ap.add_argument("--seconds", type=float, default=5.0)
    args = ap.parse_args()

    ser = serial.Serial(args.port, args.baud, timeout=0.2)
    print(f"listen {args.port} {args.baud} for {args.seconds}s")
    end = time.time() + args.seconds
    while time.time() < end:
        d = ser.read(256)
        if d:
            sys.stdout.write(d.decode("utf-8", errors="replace"))
            sys.stdout.flush()
    ser.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
