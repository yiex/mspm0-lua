#!/usr/bin/env python3
"""Audit modular API metadata and verify every exported symbol on a device.

The device phase deliberately reads function values only.  It proves module
registration and compiler injection without changing pin muxes or peripherals.
"""

import argparse
import json
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import serial


WORKSPACE = Path(__file__).resolve().parents[1]
FIRMWARE = WORKSPACE / "mspm0_lua"
API_PATH = FIRMWARE / "release" / "mspm0-lua.api.json"
MODULES_PATH = FIRMWARE / "modules"
CORE_PATH = FIRMWARE / "lua_bind" / "lua_bind_core.c"
COMPILER = Path(__file__).with_name("compile_lua.py")
MODULE_SET = Path(__file__).with_name("serial_module_set.py")


def c_exports(path, table):
    source = path.read_text(encoding="utf-8")
    match = re.search(
        rf"static const (?:native_lua_reg_t|luaL_Reg) {re.escape(table)}\[\] = \{{(.*?)\n\}};",
        source,
        re.DOTALL,
    )
    if not match:
        raise RuntimeError(f"registration table not found: {path}:{table}")
    return set(re.findall(r'\{\s*"([a-z0-9_]+)"\s*,', match.group(1)))


def metadata_exports(api):
    globals_ = {item["name"] for item in api["globals"]}
    modules = {
        item["name"]: {
            name
            for function in item["functions"]
            for name in (function["name"], *function.get("aliases", []))
        }
        for item in api["modules"]
    }
    return globals_, modules


def static_audit(api):
    globals_, modules = metadata_exports(api)
    core = c_exports(CORE_PATH, "k_core_globals") | {"print", "runfile", "require"}
    if globals_ != core:
        raise RuntimeError(f"globals mismatch: C-only={sorted(core - globals_)} metadata-only={sorted(globals_ - core)}")
    iq = c_exports(CORE_PATH, "k_iq_functions")
    if modules.get("iq") != iq:
        raise RuntimeError(f"iq mismatch: C-only={sorted(iq - modules.get('iq', set()))} metadata-only={sorted(modules.get('iq', set()) - iq)}")

    native = []
    for module in api["modules"]:
        extension = module.get("extensions", {}).get("mspm0.native_module")
        if not extension:
            continue
        name = extension["id"]
        native.append(name)
        exported = c_exports(MODULES_PATH / f"{name}.c", f"k_{name}_functions")
        expected = set(modules[name])
        if name == "tmr":
            expected.remove("every")
        if exported != expected:
            raise RuntimeError(f"{name} mismatch: C-only={sorted(exported - expected)} metadata-only={sorted(expected - exported)}")

    event = next(module for module in api["modules"] if module["name"] == "event")
    injected = event.get("extensions", {}).get("mspm0.compiler_injected", {})
    if (set(event["functions"][index]["name"] for index in range(len(event["functions"])))
            != {"run", "poll", "stop"}
            or injected.get("required_native_modules") != ["tmr"]):
        raise RuntimeError("event metadata must describe the compiler-injected tmr dispatcher")
    print(f"STATIC_API_OK globals={len(globals_)} iq={len(iq)} native_modules={len(native)}")
    return native


def read_installed_modules(port):
    with serial.Serial(port, 115200, timeout=0.05, xonxoff=False, rtscts=False, dsrdtr=False) as device:
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
        raise RuntimeError("COM port did not return MOD_STATUS_END")
    names = []
    for line in data.decode("ascii", errors="replace").replace("\r", "").split("\n"):
        match = re.fullmatch(r"MOD_SLOT [0-7] ([a-z0-9_]+) [0-9]+ [0-9a-f]{8}", line)
        if match:
            names.append(match.group(1))
    return names


def run(command):
    process = subprocess.run(command, cwd=WORKSPACE, text=True, capture_output=True)
    sys.stdout.write(process.stdout)
    sys.stderr.write(process.stderr)
    if process.returncode:
        raise RuntimeError(f"command failed ({process.returncode}): {' '.join(map(str, command))}")
    return process.stdout


def probe_source(api, modules):
    _, api_modules = metadata_exports(api)
    lines = ["print('API_SURFACE_BEGIN')"]
    if "tmr" in modules:
        for name in api_modules["event"]:
            lines.append(f"print('API', 'event.{name}', type(event.{name}))")
    for module in modules:
        for name in sorted(api_modules[module]):
            lines.append(f"print('API', '{module}.{name}', type({module}.{name}))")
    lines.append("print('API_SURFACE_END')")
    return "\n".join(lines) + "\n"


def core_probe_source(api):
    globals_, modules = metadata_exports(api)
    lines = ["print('API_CORE_BEGIN')"]
    for name in sorted(globals_):
        lines.append(f"print('API', '{name}', type({name}))")
    for name in sorted(modules["iq"]):
        lines.append(f"print('API', 'iq.{name}', type(iq.{name}))")
    lines.append("print('API_CORE_END')")
    return "\n".join(lines) + "\n"


def assert_probe_output(output, api, modules):
    expected = set()
    _, api_modules = metadata_exports(api)
    if "tmr" in modules:
        expected.update(f"event.{name}" for name in api_modules["event"])
    for module in modules:
        expected.update(f"{module}.{name}" for name in api_modules[module])
    found = {
        match.group(1)
        for match in re.finditer(r"^API\s+([^\s]+)\s+function\s*$", output, re.MULTILINE)
    }
    missing = sorted(expected - found)
    wrong = sorted(found - expected)
    if "API_SURFACE_BEGIN" not in output or "API_SURFACE_END" not in output or missing or wrong:
        raise RuntimeError(f"device surface mismatch: missing={missing} unexpected={wrong}")
    print(f"DEVICE_API_OK modules={','.join(modules)} functions={len(expected)}")


def assert_core_probe_output(output, api):
    globals_, modules = metadata_exports(api)
    expected = set(globals_) | {f"iq.{name}" for name in modules["iq"]}
    found = {
        match.group(1)
        for match in re.finditer(r"^API\s+([^\s]+)\s+function\s*$", output, re.MULTILINE)
    }
    missing = sorted(expected - found)
    wrong = sorted(found - expected)
    if "API_CORE_BEGIN" not in output or "API_CORE_END" not in output or missing or wrong:
        raise RuntimeError(f"device core mismatch: missing={missing} unexpected={wrong}")
    print(f"DEVICE_CORE_OK functions={len(expected)}")


def restore_modules(port, modules):
    if modules:
        run([sys.executable, str(MODULE_SET), "--modules", *modules, "--port", port])
    else:
        run([sys.executable, str(MODULE_SET), "--set", "core", "--port", port])


def upload_completion_script(port, modules):
    with tempfile.TemporaryDirectory(prefix="mspm0-api-complete-") as directory:
        source = Path(directory) / "complete.lua"
        bytecode = source.with_suffix(".luac")
        source.write_text("print('API_SURFACE_TEST_COMPLETE')\n", encoding="ascii", newline="\n")
        run([sys.executable, str(COMPILER), str(source), str(bytecode)])
        command = [sys.executable, str(MODULE_SET)]
        if modules:
            command.extend(["--modules", *modules])
        else:
            command.extend(["--set", "core"])
        command.extend(["--port", port, "--luac", str(bytecode)])
        run(command)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", required=True)
    parser.add_argument("--skip-device", action="store_true")
    args = parser.parse_args()

    api = json.loads(API_PATH.read_text(encoding="utf-8"))
    native = static_audit(api)
    if args.skip_device:
        return 0

    original = read_installed_modules(args.port)
    groups = [native[:7], native[7:]]
    try:
        with tempfile.TemporaryDirectory(prefix="mspm0-api-surface-") as directory:
            directory = Path(directory)
            source = directory / "surface_core.lua"
            bytecode = source.with_suffix(".luac")
            source.write_text(core_probe_source(api), encoding="ascii", newline="\n")
            run([sys.executable, str(COMPILER), str(source), str(bytecode)])
            output = run([
                sys.executable, str(MODULE_SET), "--modules", *original,
                "--port", args.port, "--luac", str(bytecode),
            ])
            assert_core_probe_output(output, api)
            for index, modules in enumerate(groups, start=1):
                source = directory / f"surface_{index}.lua"
                bytecode = source.with_suffix(".luac")
                source.write_text(probe_source(api, modules), encoding="ascii", newline="\n")
                run([sys.executable, str(COMPILER), str(source), str(bytecode)])
                output = run([
                    sys.executable, str(MODULE_SET), "--modules", *modules,
                    "--port", args.port, "--luac", str(bytecode),
                ])
                assert_probe_output(output, api, modules)
    finally:
        restore_modules(args.port, original)
        upload_completion_script(args.port, original)
        print(f"DEVICE_MODULES_RESTORED {','.join(original) if original else 'core'}")
    print("MODULAR_API_SURFACE_OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
