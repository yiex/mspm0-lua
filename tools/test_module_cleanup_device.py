#!/usr/bin/env python3
"""Exercise live module cleanup on a real modular firmware device."""

import argparse
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import serial


WORKSPACE = Path(__file__).resolve().parents[1]
COMPILER = WORKSPACE / "tools" / "compile_lua.py"
MODULE_SET = WORKSPACE / "tools" / "serial_module_set.py"


def installed_modules(port):
    with serial.Serial(port, 115200, timeout=0.05, xonxoff=False,
                       rtscts=False, dsrdtr=False) as device:
        device.dtr = False
        device.rts = False
        time.sleep(0.25)
        device.reset_input_buffer()
        device.write(b"modstatus\n")
        deadline = time.time() + 3.0
        data = b""
        while time.time() < deadline and b"MOD_STATUS_END" not in data:
            data += device.read(max(1, min(device.in_waiting, 256)))
    if b"MOD_STATUS_END" not in data:
        raise RuntimeError("device did not return MOD_STATUS_END")
    return [match.group(1) for match in re.finditer(
        rb"^MOD_SLOT [0-7] ([a-z0-9_]+) [0-9]+ [0-9a-f]{8}\r?$",
        data, re.MULTILINE)]


def run(command):
    completed = subprocess.run(command, cwd=WORKSPACE, text=True,
                               capture_output=True)
    sys.stdout.write(completed.stdout)
    sys.stderr.write(completed.stderr)
    if completed.returncode:
        raise RuntimeError("command failed: " + " ".join(map(str, command)))
    return completed.stdout


def compile_source(directory, name, source):
    lua = directory / f"{name}.lua"
    luac = directory / f"{name}.luac"
    lua.write_text(source, encoding="ascii", newline="\n")
    run([sys.executable, str(COMPILER), str(lua), str(luac)])
    return luac


def install_and_run(port, modules, bytecode, marker):
    output = run([sys.executable, str(MODULE_SET), "--modules", *modules,
                  "--port", port, "--luac", str(bytecode)])
    if marker not in output:
        raise RuntimeError(f"missing device marker: {marker}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", required=True)
    args = parser.parse_args()

    original = installed_modules(args.port)
    try:
        with tempfile.TemporaryDirectory(prefix="mspm0-module-cleanup-") as raw:
            directory = Path(raw)
            pwm_open = compile_source(directory, "pwm_open", """
local handle = pwm.open('PA14', 1000, 10)
print('PWM_OPEN', handle)
""")
            gpio_after_pwm = compile_source(directory, "gpio_after_pwm", """
gpio.mode('PA14', 'out')
gpio.set('PA14', 0)
print('PWM_CLEANUP_OK')
""")
            capture_open = compile_source(directory, "capture_open", """
local handle = tmr.capture_open(0, 'PA0')
print('CAPTURE_OPEN', handle)
""")
            gpio_after_capture = compile_source(directory, "gpio_after_capture", """
gpio.mode('PA0', 'out')
gpio.set('PA0', 0)
print('CAPTURE_CLEANUP_OK')
""")
            install_and_run(args.port, ["pwm"], pwm_open, "PWM_OPEN")
            install_and_run(args.port, ["gpio"], gpio_after_pwm,
                            "PWM_CLEANUP_OK")
            install_and_run(args.port, ["tmr"], capture_open, "CAPTURE_OPEN")
            install_and_run(args.port, ["gpio"], gpio_after_capture,
                            "CAPTURE_CLEANUP_OK")
    finally:
        command = [sys.executable, str(MODULE_SET), "--port", args.port]
        command.extend(["--modules", *original] if original else ["--set", "core"])
        run(command)
    print("MODULE_CLEANUP_DEVICE_OK pwm capture restored")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
