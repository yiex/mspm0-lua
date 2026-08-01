#!/usr/bin/env python3
"""Smoke test: compare bulk and paced UART upload behavior."""
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
    print("boot:", s.read(4096).decode("utf-8", errors="replace")[:400])

    bad = b'<<<LUA\nprint("LED blink")\ngpio.mode("PA14", "out")\nfor i = 1, 6 do\n  gpio.toggle("PA14")\n  delay_ms(120)\nend\nprint("done")\n>>>LUA\n'
    s.write(bad)
    time.sleep(2.0)
    print("BULK:", s.read(2048).decode("utf-8", errors="replace"))

    s.write(b"<<<LUA\r\n")
    time.sleep(0.15)
    print("begin", s.read(64))
    for line in [
        "print('LED blink')",
        "gpio.mode('PA14','out')",
        "for i=1,6 do",
        "  gpio.toggle('PA14')",
        "  delay_ms(120)",
        "end",
        "gpio.set('PA14',0)",
        "print('done')",
    ]:
        s.write(line.encode() + b"\r\n")
        time.sleep(0.04)
    s.write(b">>>LUA\r\n")
    time.sleep(2.5)
    print("PACED:", s.read(2048).decode("utf-8", errors="replace"))
    s.close()


if __name__ == "__main__":
    main()
