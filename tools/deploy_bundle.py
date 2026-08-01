#!/usr/bin/env python3
"""Deploy native modules and LUAC through one application-UART session."""

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SERIAL_TOOL = ROOT / "tools" / "serial_module_set.py"


def run(command):
    print("+", " ".join(str(item) for item in command), flush=True)
    result = subprocess.run(command, cwd=ROOT)
    if result.returncode != 0:
        raise SystemExit(result.returncode)


def main():
    parser = argparse.ArgumentParser(
        description=(
            "Validate, install, and activate native modules, then upload main.luac. "
            "All phases use the normal application UART without reset or BSL."
        )
    )
    parser.add_argument("script", type=Path, help="target Lua .luac file")
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument("--set", dest="set_name", help="named set from modules.json")
    selection.add_argument(
        "--modules", nargs="+", help="explicit modules in deterministic slot order"
    )
    parser.add_argument("--name", default="main.luac", help="must be main.luac")
    parser.add_argument(
        "--dependency", type=Path, action="append", default=[],
        help="dependency .luac; repeat in topological upload order",
    )
    parser.add_argument("--port", default="auto", help="COM port; auto prefers CH340")
    parser.add_argument("--connect-baud", type=int, default=115200)
    parser.add_argument("--baud", type=int, default=460800)
    parser.add_argument("--chunk-size", type=int, default=120)
    parser.add_argument(
        "--plan-only", action="store_true",
        help="validate and print the combined job without touching hardware",
    )
    args = parser.parse_args()

    script = args.script.resolve()
    if not script.is_file():
        parser.error(f"script does not exist: {script}")
    if script.suffix.lower() != ".luac":
        parser.error("modular firmware accepts target .luac only")
    if args.name != "main.luac":
        parser.error("combined deployment target is fixed to main.luac")
    if not 1 <= args.chunk_size <= 127:
        parser.error("--chunk-size must be in 1..127")
    for dependency in args.dependency:
        if not dependency.is_file() or dependency.suffix.lower() != ".luac":
            parser.error(f"dependency must be an existing .luac: {dependency}")

    command = [
        sys.executable, str(SERIAL_TOOL),
        "--port", args.port,
        "--connect-baud", str(args.connect_baud),
        "--baud", str(args.baud),
        "--chunk-size", str(args.chunk_size),
        "--luac", str(script),
    ]
    for dependency in args.dependency:
        command.extend(("--dependency", str(dependency.resolve())))
    if args.set_name:
        command.extend(("--set", args.set_name))
    else:
        command.append("--modules")
        command.extend(args.modules)
    if args.plan_only:
        command.append("--plan-only")
    run(command)
    print("BUNDLE_DEPLOY_OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
