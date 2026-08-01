#!/usr/bin/env python3
"""Build every module for every runtime slot; composition needs no compiler."""

import argparse
import hashlib
import json
import os
import struct
import subprocess
from pathlib import Path

from host_config import resolve_sdk, resolve_toolchain, tool_name


ROOT = Path(__file__).resolve().parents[1] / "mspm0_lua"
SDK = resolve_sdk(ROOT)
TC = resolve_toolchain() / "bin"
CC = TC / tool_name("arm-none-eabi-gcc")
OBJCOPY = TC / tool_name("arm-none-eabi-objcopy")
READELF = TC / tool_name("arm-none-eabi-readelf")
SIZE = TC / tool_name("arm-none-eabi-size")
LINKER = ROOT / "ld" / "native_module.lds"
MANIFEST = ROOT / "modules" / "modules.json"
HEADER_SIZE = 32
MODULE_MAGIC = 0x444F4D4C
MODULE_FORMAT = 2


def run(command, capture=False):
    print("+", " ".join(str(item) for item in command))
    return subprocess.run(
        command,
        cwd=ROOT,
        check=True,
        capture_output=capture,
        text=capture,
        encoding="utf-8" if capture else None,
        errors="ignore" if capture else None,
    )


def crc16_modbus(data):
    crc = 0xFFFF
    for value in data:
        crc ^= value
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc


def load_manifest():
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    layout = manifest["layout"]
    catalog = manifest.get("catalog", {})
    required_catalog = {
        "id", "version", "firmware_id", "firmware_version", "target",
        "core_abi", "module_format", "nmup_format",
    }
    if manifest.get("schema") != 1 or not required_catalog <= set(catalog):
        raise SystemExit("modules.json has incomplete catalog identity")
    if int(catalog["core_abi"]) != int(layout["abi_version"]):
        raise SystemExit("catalog core ABI differs from layout")
    names = set()
    for module in manifest["modules"]:
        name = module.get("name", "")
        source = module.get("source", "")
        if (not name.isascii() or not name or len(name) > 7 or
                not all(c.isalnum() or c == "_" for c in name)):
            raise SystemExit(f"invalid module name: {name!r}")
        if name in names:
            raise SystemExit(f"duplicate module name: {name}")
        source_path = (ROOT / "modules" / source).resolve()
        modules_dir = (ROOT / "modules").resolve()
        if source_path.parent != modules_dir or source_path.suffix != ".c" or not source_path.is_file():
            raise SystemExit(f"invalid source for {name}: {source}")
        conflicts = module.get("conflicts", [])
        if not isinstance(conflicts, list) or any(not isinstance(v, str) for v in conflicts):
            raise SystemExit(f"invalid conflicts for {name}")
        dependencies = module.get("dependencies", [])
        lua_modules = module.get("lua_modules", [])
        if (not isinstance(dependencies, list) or
                any(not isinstance(v, str) for v in dependencies)):
            raise SystemExit(f"invalid dependencies for {name}")
        if (not isinstance(lua_modules, list) or not lua_modules or
                any(not isinstance(v, str) or not v for v in lua_modules)):
            raise SystemExit(f"invalid Lua module mapping for {name}")
        if not isinstance(module.get("version"), str):
            raise SystemExit(f"invalid version for {name}")
        names.add(name)
    for module in manifest["modules"]:
        unknown = (set(module.get("conflicts", [])) |
                   set(module.get("dependencies", []))) - names
        if unknown:
            raise SystemExit(f"module {module['name']} conflicts with unknown modules: {sorted(unknown)}")
    for set_name, members in manifest.get("sets", {}).items():
        unknown = set(members) - names
        if unknown:
            raise SystemExit(f"set {set_name} has unknown modules: {unknown}")
    return manifest


def compile_module(module):
    name = module["name"]
    source = ROOT / "modules" / module["source"]
    build = ROOT / "build_modules" / name
    build.mkdir(parents=True, exist_ok=True)
    obj = build / f"{name}.o"
    flags = [
        "-mcpu=cortex-m0plus", "-march=armv6-m", "-mthumb",
        "-mfloat-abi=soft", "-std=c99", "-Oz", "-g0",
        "-ffunction-sections", "-fdata-sections", "-fno-common",
        "-fno-unwind-tables", "-fno-asynchronous-unwind-tables",
        "-Wall", "-Wextra", "-Werror",
        "-DMSPM0_MODULAR_CORE", "-D__MSPM0G3507__",
        f"-I{ROOT / 'app'}", f"-I{ROOT / 'board'}",
        f"-I{SDK / 'source'}",
        f"-I{SDK / 'source' / 'third_party' / 'CMSIS' / 'Core' / 'Include'}",
    ]
    run([str(CC), *flags, "-c", str(source), "-o", str(obj)])
    return obj, flags


def link_variant(module, layout, obj, flags, slot):
    name = module["name"]
    address = int(layout["slot_base"]) + slot * int(layout["slot_size"])
    slot_size = int(layout["slot_size"])
    build = ROOT / "build_modules" / name / f"slot{slot}"
    build.mkdir(parents=True, exist_ok=True)
    elf = build / f"{name}.elf"
    image = build / f"{name}.bin"
    run([
        str(CC), *flags, "-nostartfiles", "-nostdlib", "-nodefaultlibs",
        "-Wl,--gc-sections", f"-Wl,-T,{LINKER}",
        f"-Wl,--defsym,NATIVE_MODULE_LINK_ADDR=0x{address:08X}",
        f"-Wl,-Map,{build / f'{name}.map'}", str(obj),
        f"-L{SDK / 'source' / 'ti' / 'driverlib' / 'lib' / 'gcc' / 'm0p' / 'mspm0g1x0x_g3x0x'}",
        "-Wl,--start-group", "-l:driverlib.a", "-lgcc", "-Wl,--end-group",
        "-o", str(elf),
    ])
    relocations = run([str(READELF), "-r", str(elf)], capture=True).stdout
    if "There are no relocations in this file" not in relocations:
        raise SystemExit(f"{name}: unresolved relocations are not allowed")
    run([str(OBJCOPY), "-O", "binary", str(elf), str(image)])

    data = bytearray(image.read_bytes())
    if len(data) <= HEADER_SIZE or len(data) > slot_size:
        raise SystemExit(f"{name}: size {len(data)} is outside 33..{slot_size}")
    struct.pack_into("<I", data, 8, len(data))
    crc = crc16_modbus(data[HEADER_SIZE:])
    struct.pack_into("<H", data, 12, crc)
    image.write_bytes(data)
    magic, fmt, abi, image_size, payload_crc, header_size, raw_entry, deinit = (
        struct.unpack_from("<IHHIHHII", data)
    )
    encoded_name = data[24:32].split(b"\0", 1)[0].decode("ascii", "strict")
    if (magic != MODULE_MAGIC or fmt != MODULE_FORMAT or
            abi != int(layout["abi_version"]) or image_size != len(data) or
            payload_crc != crc or header_size != HEADER_SIZE or
            encoded_name != name):
        raise SystemExit(f"{name}: generated module header mismatch")
    entry = raw_entry & ~1
    if not address + HEADER_SIZE <= entry < address + len(data):
        raise SystemExit(f"{name}: entry 0x{entry:08X} is outside slot image")
    if deinit and ((deinit & 1) == 0 or not
            address + HEADER_SIZE <= (deinit & ~1) < address + len(data)):
        raise SystemExit(f"{name}: deinit 0x{deinit:08X} is outside slot image")
    run([str(SIZE), str(elf)])
    print(
        f"OK {name}: slot {slot} @ 0x{address:08X}, "
        f"{len(data)} bytes, CRC16 0x{crc:04X}"
    )
    return {
        "slot": slot,
        "address": address,
        "size": len(data),
        "crc16": crc,
        "sha256": hashlib.sha256(data).hexdigest(),
        "image": image.relative_to(ROOT).as_posix(),
    }


def build_module(module, layout):
    obj, flags = compile_module(module)
    variants = [
        link_variant(module, layout, obj, flags, slot)
        for slot in range(int(layout["slot_count"]))
    ]
    source_sha256 = hashlib.sha256(
        (ROOT / "modules" / module["source"]).read_bytes()
    ).hexdigest()
    build_identity = {
        "module": module["name"],
        "version": module["version"],
        "source_sha256": source_sha256,
        "variants": [item["sha256"] for item in variants],
    }
    build_id = hashlib.sha256(json.dumps(
        build_identity, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")).hexdigest()
    for variant in variants:
        variant.update({
            "module": module["name"],
            "module_version": module["version"],
            "target": "MSPM0G3507",
            "abi_version": int(layout["abi_version"]),
            "module_format": 2,
            "build_id": build_id,
        })
    return {
        "name": module["name"],
        "display_name": module["display_name"],
        "version": module["version"],
        "source": module["source"],
        "source_sha256": source_sha256,
        "build_id": build_id,
        "lua_modules": module["lua_modules"],
        "dependencies": module.get("dependencies", []),
        "conflicts": module.get("conflicts", []),
        "resources": module.get("resources", []),
        "resident": bool(module.get("resident", False)),
        "variants": variants,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "modules", nargs="*", help="module names; default builds every module"
    )
    args = parser.parse_args()
    manifest = load_manifest()
    known = {module["name"]: module for module in manifest["modules"]}
    selected = args.modules or list(known)
    unknown = set(selected) - set(known)
    if unknown:
        raise SystemExit(f"unknown modules: {', '.join(sorted(unknown))}")
    results = [
        build_module(known[name], manifest["layout"])
        for name in selected
    ]
    index = ROOT / "build_modules" / "index.json"
    catalog_modules = {}
    if index.is_file():
        try:
            previous = json.loads(index.read_text(encoding="utf-8"))
            if (previous.get("schema") == 2 and
                    previous.get("layout") == manifest["layout"] and
                    previous.get("catalog") == manifest["catalog"]):
                valid_names = set(known)
                catalog_modules = {
                    name: item for name, item in previous.get("modules", {}).items()
                    if (name in valid_names and
                        item.get("version") == known[name].get("version") and
                        item.get("source") == known[name].get("source"))
                }
        except (OSError, ValueError, TypeError):
            pass
    catalog_modules.update({item["name"]: item for item in results})
    catalog = {
        "schema": 2,
        "catalog": manifest["catalog"],
        "modules_definition_sha256": hashlib.sha256(
            MANIFEST.read_bytes()
        ).hexdigest(),
        "layout": manifest["layout"],
        "modules": catalog_modules,
    }
    index.write_text(json.dumps(catalog, indent=2) + "\n", encoding="utf-8")
    # Only index.json defines deployable module images.  Previous toolchain
    # layouts left top-level *.bin files behind; keeping them risks an old
    # artifact being picked up by packaging tools or by a manual deployment.
    module_root = (ROOT / "build_modules").resolve()
    indexed_images = set()
    for item in catalog_modules.values():
        for variant in item["variants"]:
            image = (ROOT / variant["image"]).resolve()
            try:
                image.relative_to(module_root)
            except ValueError as error:
                raise SystemExit(f"catalog image escapes build_modules: {image}") from error
            indexed_images.add(image)
    removed = []
    for image in module_root.rglob("*.bin"):
        if image.resolve() not in indexed_images:
            image.unlink()
            removed.append(image.relative_to(module_root).as_posix())
    for directory in sorted(module_root.rglob("*"), key=lambda path: len(path.parts), reverse=True):
        if directory.is_dir() and not any(directory.iterdir()):
            directory.rmdir()
    if removed:
        print("pruned", ", ".join(sorted(removed)))
    print("index", index)


if __name__ == "__main__":
    main()
