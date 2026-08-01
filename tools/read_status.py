#!/usr/bin/env python3
"""Read status mailbox at fixed 0x20200100 (no Luckfox hold required if SWD works)."""
import subprocess
import tempfile
from pathlib import Path

from host_config import jlink_exe

JLINK = jlink_exe()
ADDR = 0x20200100

script = f"""
si 1
speed 2000
device MSPM0G3507
connect
halt
mem32 0x{ADDR:08X} 4
regs
exit
"""

def main():
    with tempfile.NamedTemporaryFile("w", suffix=".jlink", delete=False, encoding="ascii") as f:
        f.write(script)
        cmd = f.name
    r = subprocess.run(
        [str(JLINK), "-CommandFile", cmd],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="ignore",
    )
    print(r.stdout[-2000:])
    key = f"{ADDR:08X}".lower()
    for line in r.stdout.splitlines():
        if key in line.lower() and "=" in line:
            parts = line.split("=")[1].strip().split()
            magic = int(parts[0], 16)
            flags = int(parts[1], 16)
            jedec = int(parts[2], 16)
            raw = int(parts[3], 16) if len(parts) > 3 else 0
            print(f"magic=0x{magic:08X} flags=0x{flags:08X} jedec=0x{jedec:06X} raw=0x{raw:08X}")
            bits = {
                1: "BOOT", 2: "UART_OK", 4: "LUA_OK", 8: "LUA_RUN",
                16: "SPI_OK", 32: "SPI_FAIL", 64: "DEMO_DONE",
                128: "LFS_OK", 256: "SCRIPT_EXT",
                512: "UART_LB_OK", 1024: "UART_LB_FAIL",
                2048: "UART_RX_HIT", 4096: "PUTS_OK", 8192: "POST_LED",
                16384: "HFXT_OK", 32768: "HFXT_FAIL",
                65536: "NATIVE_MODULE_OK",
            }
            print("flags:", ", ".join(n for b, n in bits.items() if flags & b) or "(none)")
            return 0 if magic == 0x4C554131 else 1
    return 1

if __name__ == "__main__":
    raise SystemExit(main())
