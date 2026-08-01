#!/usr/bin/env python3
"""Hold BOOT, pulse RST, flash via J-Link with retries, then run app."""
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from host_config import jlink_exe, luckfox_credentials, require

JLINK = jlink_exe()
DEFAULT_BIN = Path(__file__).resolve().parents[1] / "mspm0_lua" / "build" / "mspm0_lua.bin"


def ssh_connect():
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
    last = None
    for attempt in range(6):
        try:
            c.connect(
                host,
                username=user,
                password=password,
                timeout=20,
                allow_agent=False,
                look_for_keys=False,
                banner_timeout=30,
                auth_timeout=20,
            )
            return c
        except Exception as e:
            last = e
            print("ssh retry", attempt + 1, e)
            time.sleep(2)
    raise SystemExit(f"ssh failed: {last}")


def remote(c, cmd, timeout=20):
    print(">>>", cmd)
    _, o, e = c.exec_command(cmd, timeout=timeout)
    try:
        out = o.read().decode(errors="ignore")
    except Exception:
        out = ""
    try:
        err = e.read().decode(errors="ignore")
    except Exception:
        err = ""
    if out.strip():
        print(out.strip())
    if err.strip():
        print("ERR:", err[:300])


def cleanup_hold_boot(c):
    remote(c, "if test -f /tmp/hold_boot.pid; then kill $(cat /tmp/hold_boot.pid) 2>/dev/null || true; fi; rm -f /tmp/hold_boot.pid /tmp/hold_boot.ok")
    time.sleep(0.2)
    # C6 is BOOT. The board helper releases it to the normal external bias.
    remote(c, "python3 /root/boot.py boot-default")


def reset_application(c):
    """Pulse C7, the empirically verified application reset line."""
    remote(c, "python3 /root/boot.py reset --rst-seconds 0.15")


def jlink_reset_application(verify_path=None) -> bool:
    """Start the application after BOOT has been released."""
    verify = ""
    if verify_path is not None:
        verify = f"verifybin {Path(verify_path).resolve().as_posix()} 0x00000000\n"
    script = f"""
si 1
speed 1000
device MSPM0G3507
connect
reset
halt
{verify}go
exit
"""
    with tempfile.NamedTemporaryFile(
        "w", suffix=".jlink", delete=False, encoding="ascii"
    ) as handle:
        handle.write(script)
        cmdfile = Path(handle.name)
    result = subprocess.run(
        [str(JLINK), "-CommandFile", str(cmdfile), "-ExitOnError", "1"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="ignore",
    )
    if result.returncode != 0:
        print((result.stdout or "")[-1200:])
    elif verify_path is not None:
        print(f"core verified unchanged: {Path(verify_path).resolve()}")
    return result.returncode == 0


def start_hold_boot(c, pulse_reset=True):
    reset_sequence = (
        "    rst.init(mode=Pin.OUT, value=0)\n"
        "    time.sleep(0.15)\n"
        "    rst.init(mode=Pin.IN, pull=Pin.PULL_NONE)\n"
        if pulse_reset else
        "    rst.init(mode=Pin.IN, pull=Pin.PULL_NONE)\n"
    )
    ready_message = (
        "RST=Hi-Z BOOT held (max 60s)"
        if pulse_reset else
        "BOOT held without reset (max 60s)"
    )
    hold = (
        "from visiong import Pin\n"
        "import os, signal, time\n"
        "stop = False\n"
        "def stopped(sig, frame):\n"
        "    global stop\n"
        "    stop = True\n"
        "signal.signal(signal.SIGTERM, stopped)\n"
        "signal.signal(signal.SIGINT, stopped)\n"
        "boot = Pin('GPIO1_C6', backend='reg')\n"
        "rst = Pin('GPIO1_C7', backend='reg')\n"
        "try:\n"
        "    boot.init(mode=Pin.OUT, value=1)\n"
        "    time.sleep(0.05)\n"
        + reset_sequence +
        f"    print('{ready_message}', flush=True)\n"
        "    open('/tmp/hold_boot.ok','w').write('1')\n"
        "    deadline = time.monotonic() + 60.0\n"
        "    while not stop and time.monotonic() < deadline:\n"
        "        time.sleep(0.25)\n"
        "finally:\n"
        "    boot.init(mode=Pin.IN, pull=Pin.PULL_NONE)\n"
        "    rst.init(mode=Pin.IN, pull=Pin.PULL_NONE)\n"
        "    try: os.remove('/tmp/hold_boot.ok')\n"
        "    except OSError: pass\n"
        "    print('BOOT/RST released', flush=True)\n"
    )
    sftp = c.open_sftp()
    with sftp.file("/root/hold_boot_bg.py", "w") as f:
        f.write(hold)
    sftp.close()
    cleanup_hold_boot(c)
    remote(c, "rm -f /tmp/hold_boot.log")
    time.sleep(0.3)
    remote(c, "nohup python3 /root/hold_boot_bg.py > /tmp/hold_boot.log 2>&1 & echo $! > /tmp/hold_boot.pid")
    for _ in range(20):
        time.sleep(0.25)
        _, o, _ = c.exec_command("test -f /tmp/hold_boot.ok && cat /tmp/hold_boot.log || echo WAIT")
        txt = o.read().decode(errors="ignore")
        if "BOOT held" in txt or "RST=Hi-Z" in txt:
            print(txt)
            return
    _, o, _ = c.exec_command("cat /tmp/hold_boot.log 2>/dev/null; ps | grep hold_boot || true")
    print("hold status:", o.read().decode(errors="ignore"))


def jlink_flash(bin_path: Path) -> bool:
    image = bin_path.read_bytes()
    if len(image) > 0x18000:
        erase_end = 0x1FFFF
    elif len(image) > 0x17F00 and image[0x17F00:0x17F04] == b"CAPI":
        erase_end = 0x17FFF
    else:
        erase_end = 0x1F7FF
    # Erase only the address range represented by this image. An unbounded
    # chip erase also targets locked BCR.
    script = f"""
si 1
speed 1000
device MSPM0G3507
connect
halt
erase 0x00000000 0x{erase_end:08X}
loadbin {bin_path.as_posix()} 0x0
verifybin {bin_path.as_posix()} 0x0
exit
"""
    with tempfile.NamedTemporaryFile(
        "w", suffix=".jlink", delete=False, encoding="ascii"
    ) as handle:
        handle.write(script)
        cmdfile = Path(handle.name)
    for attempt in range(1, 5):
        print(f"=== J-Link attempt {attempt} ===")
        r = subprocess.run(
            [str(JLINK), "-CommandFile", str(cmdfile)],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="ignore",
        )
        out = r.stdout or ""
        print(out[-2200:])
        if "Verify successful" in out or (
            "O.K." in out and "loadbin" in out.lower() and "Could not connect" not in out[-500:]
        ):
            if "Verify successful" in out or "Contents already match" in out or "Program & Verify" in out:
                return True
            if "O.K." in out and "Downloading file" in out:
                return True
            if "Contents already match" in out:
                return True
        time.sleep(0.8)
    return False


def main():
    bin_path = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else DEFAULT_BIN
    if not bin_path.exists():
        raise SystemExit(f"missing {bin_path}")

    c = ssh_connect()
    ok = False
    try:
        start_hold_boot(c)
        time.sleep(0.5)
        ok = jlink_flash(bin_path)
        if not ok:
            print("standard BSL sequence failed; retrying with BOOT hold only")
            cleanup_hold_boot(c)
            start_hold_boot(c, pulse_reset=False)
            time.sleep(0.5)
            ok = jlink_flash(bin_path)
    finally:
        # Always release the GPIOs; the remote helper also self-releases after
        # 60 seconds if this host process is killed or loses its SSH session.
        cleanup_hold_boot(c)
        if not ok or not jlink_reset_application():
            reset_application(c)
        c.close()

    print("flash ok?", ok)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
