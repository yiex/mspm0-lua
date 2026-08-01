#!/usr/bin/env python3
"""Format LittleFS and install the default target-ABI Lua files over UART."""
import argparse
import subprocess
import sys
import time
from pathlib import Path

import serial

from upload_script import resolve_port

ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = ROOT / "mspm0_lua" / "scripts"
OUT = ROOT / "mspm0_lua" / "build_bytecode" / "lfs_init"


def format_lfs(port: str, baud: int) -> None:
    with serial.Serial(port, baud, timeout=0.2) as ser:
        time.sleep(0.1)
        ser.reset_input_buffer()
        ser.write(b"format\r\n")
        ser.flush()
        deadline = time.time() + 20
        data = b""
        while time.time() < deadline:
            data += ser.read(256)
            if b"FORMAT_OK" in data:
                print(data.decode("utf-8", errors="replace"), end="", flush=True)
                return
            if b"FORMAT_ERR" in data:
                break
        raise SystemExit("LittleFS format failed: " + data.decode("utf-8", errors="replace"))


def run(*args: str) -> None:
    subprocess.run([sys.executable, *args], cwd=ROOT, check=True)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", default="auto", help="COM port; auto prefers CH340")
    ap.add_argument("--baud", type=int, default=115200)
    ap.add_argument("--scripts", type=Path, default=SCRIPTS)
    ap.add_argument("--include-tests", action="store_true")
    args = ap.parse_args()
    port = resolve_port(args.port)
    sources = sorted(args.scripts.resolve().glob("*.lua"))
    if not args.include_tests:
        sources = [p for p in sources if p.name != "large_stream_test.lua"]
    if not sources:
        raise SystemExit(f"no .lua files in {args.scripts}")
    OUT.mkdir(parents=True, exist_ok=True)
    compiled = []
    for src in sources:
        name = "main.luac" if src.name == "main.lua" else src.with_suffix(".luac").name
        dst = OUT / name
        run("tools/compile_lua.py", str(src), str(dst))
        compiled.append((dst, name))

    print(f"Formatting LittleFS via {port}...", flush=True)
    format_lfs(port, args.baud)
    time.sleep(1.0)
    # Install optional modules first; main.luac runs immediately after upload.
    compiled.sort(key=lambda item: item[1] == "main.luac")
    for path, name in compiled:
        run("tools/upload_script.py", str(path), "--name", name,
            "--port", port, "--baud", str(args.baud))
        time.sleep(0.8)
    print(f"SPI-Flash initialization complete: {len(compiled)} files")


if __name__ == "__main__":
    main()
