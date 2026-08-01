#!/usr/bin/env python3
"""Verify core/module images and read status with fail-safe BOOT control."""
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
    root = Path(__file__).resolve().parents[1] / "mspm0_lua"
    core = root / "build_bytecode" / "mspm0_lua_bytecode.bin"
    module = root / "build_modules" / "plug" / "plug.bin"
    if not core.exists() or not module.exists():
        raise SystemExit("build the bytecode core and plug module first")
    c = ssh_connect()
    try:
        # Preserve the application's SRAM mailbox while making SWD visible.
        start_hold_boot(c, pulse_reset=False)
        time.sleep(0.5)
        script = f"""si 1
speed 2000
device MSPM0G3507
connect
halt
verifybin {core.as_posix()} 0x00000000
verifybin {module.as_posix()} 0x0001F800
mem32 0x20200100 4
exit
"""
        cmd = Path(__file__).resolve().parent / "tmp_verify.jlink"
        cmd.write_text(script, encoding="ascii")
        result = subprocess.run(
            [str(JLINK), "-CommandFile", str(cmd)],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="ignore",
        )
        print(result.stdout[-2500:])
    finally:
        cleanup_hold_boot(c)
        reset_application(c)
        c.close()

    verify_ok = result.stdout.count("Verify successful.") == 2
    module_ok = any(
        "20200100" in line and "=" in line and
        len(line.split("=")[1].strip().split()) >= 2 and
        int(line.split("=")[1].strip().split()[1], 16) & 0x00010000
        for line in result.stdout.splitlines()
    )
    print("images match:", verify_ok)
    print("native module flag:", module_ok)
    return 0 if verify_ok and module_ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
