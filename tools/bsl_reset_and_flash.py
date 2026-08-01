#!/usr/bin/env python3
"""Enter BSL (hold BOOT, pulse RST), then flash via J-Link."""
import subprocess
import tempfile
import time
from pathlib import Path

from host_config import jlink_exe, luckfox_credentials, require

JLINK = jlink_exe()
BIN = Path(__file__).resolve().parents[1] / "mspm0_lua" / "build" / "mspm0_lua.bin"


def ssh():
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
        timeout=12,
        allow_agent=False,
        look_for_keys=False,
    )
    return c


def run_remote(c, cmd, timeout=20):
    print(">>>", cmd)
    _, o, e = c.exec_command(cmd, timeout=timeout)
    out = o.read().decode(errors="ignore")
    err = e.read().decode(errors="ignore")
    if out.strip():
        print(out, end="" if out.endswith("\n") else "\n")
    if err.strip():
        print("ERR:", err[:400])


def main():
    if not BIN.exists():
        raise SystemExit(f"missing bin: {BIN}")

    c = ssh()
    # 1) idle, then BSL sequence: BOOT high + RST pulse, hold BOOT
    run_remote(c, "python3 /root/boot.py status-setup")
    time.sleep(0.2)
    # hold BOOT high, pulse RST, keep BOOT for a while (BSL entry)
    run_remote(c, "python3 /root/boot.py bsl --seconds 2.0 --rst-seconds 0.1")
    time.sleep(0.5)

    script = f"""
si 1
speed 2000
device MSPM0G3507
connect
halt
erase
loadbin {BIN.as_posix()} 0x0
verifybin {BIN.as_posix()} 0x0
r
g
exit
"""
    with tempfile.NamedTemporaryFile(
        "w", suffix=".jlink", delete=False, encoding="ascii"
    ) as handle:
        handle.write(script)
        cmdfile = Path(handle.name)
    print("J-Link flash after BSL...")
    r = subprocess.run(
        [str(JLINK), "-CommandFile", str(cmdfile)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="ignore",
    )
    print((r.stdout or "")[-3500:])
    print((r.stderr or "")[-500:])

    # release boot / normal reset so app can run
    time.sleep(0.3)
    run_remote(c, "python3 /root/boot.py boot-default")
    run_remote(c, "python3 /root/boot.py reset --rst-seconds 0.08")
    c.close()
    print("done")


if __name__ == "__main__":
    main()
