#!/usr/bin/env python3
"""Open a UART first, pulse the verified remote reset line, and capture boot."""

import argparse
import time

import serial

from hold_boot_flash import reset_application, ssh_connect


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", required=True)
    parser.add_argument("--baud", type=int, default=115200)
    parser.add_argument("--seconds", type=float, default=5.0)
    args = parser.parse_args()
    uart = serial.Serial(args.port, args.baud, timeout=0.1)
    uart.reset_input_buffer()
    client = ssh_connect()
    try:
        reset_application(client)
    finally:
        client.close()
    deadline = time.monotonic() + args.seconds
    data = bytearray()
    while time.monotonic() < deadline:
        data.extend(uart.read(256))
    uart.close()
    print(repr(bytes(data)))
    print(data.decode("utf-8", errors="replace"))
    return 0 if data else 1


if __name__ == "__main__":
    raise SystemExit(main())
