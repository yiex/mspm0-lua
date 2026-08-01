#!/usr/bin/env python3
"""Flash binary to MSPM0G3507 via SEGGER J-Link.

Optionally pulses RST on Luckfox before connect.
"""
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from host_config import jlink_exe, luckfox_credentials, require

JLINK = jlink_exe()
DEVICE = "MSPM0G3507"


def luckfox_reset():
    creds = luckfox_credentials()
    if not creds:
        print("LUCKFOX_* not set; skipping board reset helper")
        return
    host, user, password = creds
    require("paramiko")
    import paramiko
    try:
        c = paramiko.SSHClient()
        c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        c.connect(
            host,
            username=user,
            password=password,
            timeout=8,
            allow_agent=False,
            look_for_keys=False,
        )
        c.exec_command("python3 /root/boot.py status-setup", timeout=10)
        time.sleep(0.1)
        c.exec_command("python3 /root/boot.py reset --rst-seconds 0.05", timeout=10)
        c.close()
        print("Luckfox RST pulse done")
        time.sleep(0.3)
    except Exception as e:
        print("Luckfox reset skipped:", e)


def main():
    if len(sys.argv) < 2:
        print("usage: jlink_flash.py firmware.bin")
        return 1
    bin_path = Path(sys.argv[1]).resolve()
    if not bin_path.exists():
        print("missing", bin_path)
        return 1

    luckfox_reset()

    # loadbin erases only the sectors covered by the image. A chip erase also
    # targets the locked BCR configuration sector and fails on MSPM0G3507.
    script = f"""
si 1
speed 4000
device {DEVICE}
connect
halt
loadbin {bin_path.as_posix()} 0x00000000
verifybin {bin_path.as_posix()} 0x00000000
r
g
exit
"""
    with tempfile.NamedTemporaryFile("w", suffix=".jlink", delete=False, encoding="ascii") as f:
        f.write(script)
        cmdfile = f.name
    print("flashing", bin_path)
    r = subprocess.run(
        [str(JLINK), "-CommandFile", cmdfile, "-ExitOnError", "1"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="ignore",
    )
    out = (r.stdout or "") + (r.stderr or "")
    print(out[-3000:])
    hard_error = any(marker in out for marker in (
        "Failed to erase", "ERROR: Erase", "Failed to program",
        "Verify failed", "Could not connect",
    ))
    ok = not hard_error and (
        "Verify successful" in out or
        "Contents already match" in out or
        ("Verifying binary" in out and "Failed" not in out)
    )
    if not ok:
        print("flash may have failed, code", r.returncode)
        return r.returncode or 1
    print("flash script finished")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
