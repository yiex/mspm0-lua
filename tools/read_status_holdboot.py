#!/usr/bin/env python3
"""Hold BOOT, connect J-Link, read status mailbox (no reflash)."""
import subprocess
import time
from pathlib import Path

from hold_boot_flash import (
    JLINK,
    cleanup_hold_boot,
    reset_application,
    remote,
    ssh_connect,
    start_hold_boot,
)


def main():
    c = ssh_connect()
    start_hold_boot(c, pulse_reset=False)
    print("BOOT hold ready without reset (fail-safe max 60s)")

    # IMPORTANT: do not reset before reading mailbox; just halt and read
    script = """
si 1
speed 2000
device MSPM0G3507
connect
halt
mem32 0x20200100 4
mem32 0x00000000 2
exit
"""
    cmd = Path(__file__).resolve().parent / "tmp_st_hold.jlink"
    cmd.write_text(script, encoding="ascii")
    try:
        r = subprocess.run(
            [str(JLINK), "-CommandFile", str(cmd)],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="ignore",
        )
        print(r.stdout[-2500:])
    finally:
        cleanup_hold_boot(c)
        reset_application(c)
        c.close()

    for line in r.stdout.splitlines():
        if "20200100" in line.lower() and "=" in line:
            parts = line.split("=")[1].strip().split()
            magic = int(parts[0], 16)
            flags = int(parts[1], 16)
            jedec = int(parts[2], 16)
            raw = int(parts[3], 16) if len(parts) > 3 else 0
            print(f"magic=0x{magic:08X} flags=0x{flags:08X} jedec=0x{jedec:06X} raw=0x{raw:08X}")
            bits = {
                1: "BOOT",
                2: "UART_OK",
                4: "LUA_OK",
                8: "LUA_RUN",
                16: "SPI_OK",
                32: "SPI_FAIL",
                64: "DEMO_DONE",
                128: "LFS_OK",
                256: "SCRIPT_EXT",
                512: "UART_LB_OK",
                1024: "UART_LB_FAIL",
                2048: "UART_RX_HIT",
                4096: "PUTS_OK",
                8192: "POST_LED",
                16384: "HFXT_OK",
                32768: "HFXT_FAIL",
                65536: "NATIVE_MODULE_OK",
            }
            names = [n for b, n in bits.items() if flags & b]
            print("flags:", ", ".join(names))
            return 0 if magic == 0x4C554131 and flags & 0x00010000 else 1
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
