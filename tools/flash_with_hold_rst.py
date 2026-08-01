#!/usr/bin/env python3
"""Hold RST through a helper board, then flash the MCU via J-Link."""
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from host_config import jlink_exe, luckfox_credentials, require

JLINK = jlink_exe()
BIN = Path(__file__).resolve().parents[1] / "mspm0_lua" / "build" / "mspm0_lua.bin"


def main():
    bin_path = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else BIN
    if not bin_path.is_file():
        raise SystemExit(f"missing {bin_path}")
    creds = luckfox_credentials()
    if not creds:
        raise SystemExit(
            "LUCKFOX_HOST / LUCKFOX_USER / LUCKFOX_PASS must be set "
            "to use the board hold/reset helpers"
        )
    require("paramiko")
    import paramiko

    host, user, password = creds
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(
        host,
        username=user,
        password=password,
        timeout=10,
        allow_agent=False,
        look_for_keys=False,
    )
    hold = r"""
from visiong import Pin
import time, os
boot = Pin("GPIO1_C6", backend="reg")
rst = Pin("GPIO1_C7", backend="reg")
boot.init(mode=Pin.OUT, value=0)
rst.init(mode=Pin.OUT, value=0)
print("RST held LOW", flush=True)
time.sleep(12)
rst.init(mode=Pin.IN, pull=Pin.PULL_NONE)
print("RST Hi-Z", flush=True)
os._exit(0)
"""
    sftp = c.open_sftp()
    with sftp.file("/root/hold_rst.py", "w") as f:
        f.write(hold)
    sftp.close()
    c.exec_command("python3 /root/hold_rst.py > /tmp/hold_rst.log 2>&1 &")
    time.sleep(1.5)
    print("RST held, connecting J-Link...")

    script = f"""
si 1
speed 1000
device MSPM0G3507
connect
halt
erase 0x00000000 0x0001FFFF
loadbin {bin_path.as_posix()} 0x0
verifybin {bin_path.as_posix()} 0x0
r
g
exit
"""
    with tempfile.NamedTemporaryFile(
        "w", suffix=".jlink", delete=False, encoding="ascii"
    ) as handle:
        handle.write(script)
        cmdfile = Path(handle.name)
    r = subprocess.run(
        [str(JLINK), "-CommandFile", str(cmdfile)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="ignore",
    )
    out = (r.stdout or "") + (r.stderr or "")
    print(out[-3500:])
    time.sleep(2)
    i, o, e = c.exec_command("cat /tmp/hold_rst.log")
    print("hold log:", o.read().decode(errors="ignore"))
    c.close()
    hard_error = any(marker in out for marker in (
        "Failed to erase", "Failed to halt", "Failed to prepare",
        "Failed to download", "Verify failed", "Could not connect",
        "Error: ResetTarget failed",
    ))
    verified = "Verify successful" in out or "Contents already match" in out
    if r.returncode != 0 or hard_error or not verified:
        print("flash failed", file=sys.stderr)
        return r.returncode or 1
    print("flash verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
