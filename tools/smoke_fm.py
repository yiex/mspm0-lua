#!/usr/bin/env python3
"""Smoke test: flash a firmware image and exercise LittleFS commands."""
import argparse
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from host_config import jlink_exe

sys.stdout.reconfigure(encoding="utf-8", errors="replace")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", type=Path, default=None, help="firmware image to flash")
    parser.add_argument("--port", default=None, help="UART port (e.g. COMx or /dev/ttyUSB0)")
    args = parser.parse_args()
    binp = args.bin or (
        Path(__file__).resolve().parents[1] / "mspm0_lua" / "build" / "mspm0_lua.bin"
    )
    if not binp.is_file():
        raise SystemExit(f"missing firmware image: {binp}")
    if args.port is None:
        raise SystemExit("--port is required")
    import serial

    script = f"""
si 1
speed 2000
device MSPM0G3507
connect
halt
loadbin {binp.as_posix()} 0x0
verifybin {binp.as_posix()} 0x0
r
g
exit
"""
    p = Path(tempfile.gettempdir()) / "f.jlink"
    p.write_text(script)
    r = subprocess.run(
        [str(jlink_exe()), "-CommandFile", str(p)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="ignore",
    )
    print("flash", "Verify successful" in r.stdout)
    s = serial.Serial(args.port, 115200, timeout=0.3)
    s.dtr = False
    s.rts = False
    s.reset_input_buffer()
    time.sleep(2.5)
    print("boot:", s.read(4096).decode("utf-8", errors="replace")[:500])
    s.write(b"ls\r\n")
    time.sleep(0.8)
    print("ls:", s.read(2048).decode("utf-8", errors="replace"))
    s.write(b"<<<LUA blink.lua\r\n")
    time.sleep(0.1)
    for line in ["print('n')", "gpio.mode('PA14','out')", "gpio.set('PA14',0)"]:
        s.write(line.encode() + b"\r\n")
        time.sleep(0.04)
    s.write(b">>>LUA\r\n")
    time.sleep(1.5)
    print("save:", s.read(1024).decode("utf-8", errors="replace"))
    s.write(b"ls\r\n")
    time.sleep(0.8)
    print("ls2:", s.read(1024).decode("utf-8", errors="replace"))
    s.write(b"boot blink.lua\r\n")
    time.sleep(1.0)
    print("bootcmd:", s.read(512).decode("utf-8", errors="replace"))
    s.close()


if __name__ == "__main__":
    main()
