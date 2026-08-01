#!/usr/bin/env python3
"""Compose selected prebuilt modules without invoking a C toolchain."""

import argparse
import hashlib
import json
import struct
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1] / "mspm0_lua"
MANIFEST = ROOT / "modules" / "modules.json"
CATALOG = ROOT / "build_modules" / "index.json"
FLASH_SIZE = 0x20000
MODULE_MAGIC = 0x444F4D4C
MODULE_FORMAT = 2
HEADER_SIZE = 32


def fail(message):
    raise SystemExit(message)


def crc16_modbus(data):
    crc = 0xFFFF
    for value in data:
        crc ^= value
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def validate_module(data, name, address, slot_size, abi_version):
    if len(data) <= HEADER_SIZE or len(data) > slot_size:
        fail(f"module {name} has invalid size {len(data)}")
    magic, fmt, abi, image_size, expected_crc, header_size, entry, deinit = (
        struct.unpack_from("<IHHIHHII", data)
    )
    encoded_name = data[24:32].split(b"\0", 1)[0].decode("ascii", "strict")
    if magic != MODULE_MAGIC or fmt != MODULE_FORMAT or abi != abi_version:
        fail(f"module {name} header/ABI mismatch")
    if image_size != len(data) or header_size != HEADER_SIZE or encoded_name != name:
        fail(f"module {name} metadata mismatch")
    if not entry & 1 or not address + HEADER_SIZE <= (entry & ~1) < address + image_size:
        fail(f"module {name} entry is outside slot at 0x{address:08X}")
    if deinit and (not deinit & 1 or not address + HEADER_SIZE <= (deinit & ~1) < address + image_size):
        fail(f"module {name} deinit is outside slot at 0x{address:08X}")
    actual_crc = crc16_modbus(data[HEADER_SIZE:])
    if actual_crc != expected_crc:
        fail(f"module {name} CRC mismatch: expected 0x{expected_crc:04X}, got 0x{actual_crc:04X}")


def load_inputs():
    if not MANIFEST.is_file():
        fail(f"missing manifest: {MANIFEST}")
    if not CATALOG.is_file():
        fail("missing module catalog; prebuild modules once before composing")
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    catalog = json.loads(CATALOG.read_text(encoding="utf-8"))
    if catalog.get("schema") != 2:
        fail("module catalog is obsolete; rebuild modules with build_native_module.py")
    if catalog.get("catalog") != manifest.get("catalog"):
        fail("module catalog identity differs from modules.json; rebuild modules")
    if catalog.get("modules_definition_sha256") != sha256(MANIFEST.read_bytes()):
        fail("modules.json changed since the catalog was built; rebuild modules")
    if catalog.get("layout") != manifest.get("layout"):
        fail("module catalog layout/ABI differs from modules.json; rebuild modules")
    known = {item["name"]: item for item in manifest["modules"]}
    if len(known) != len(manifest["modules"]):
        fail("duplicate module name in modules.json")
    return manifest, catalog, known


def resolve_selection(manifest, known, set_name=None, modules=None):
    if modules is not None:
        selected = modules
        label = "custom"
    else:
        if set_name not in manifest.get("sets", {}):
            fail(f"unknown set: {set_name}")
        selected = manifest["sets"][set_name]
        label = set_name
    duplicates = sorted({name for name in selected if selected.count(name) > 1})
    if duplicates:
        fail(f"duplicate modules are not allowed: {', '.join(duplicates)}")
    unknown = sorted(set(selected) - set(known))
    if unknown:
        fail(f"unknown modules: {', '.join(unknown)}")
    count = int(manifest["layout"]["slot_count"])
    if len(selected) > count:
        fail(
            f"selection needs {len(selected)} runtime slots, but MCU has {count}; "
            f"the catalog may be larger, choose at most {count} modules per firmware"
        )
    selected_set = set(selected)
    conflicts = []
    for name in selected:
        for other in known[name].get("conflicts", []):
            if other in selected_set:
                conflicts.append(tuple(sorted((name, other))))
    if conflicts:
        pairs = ", ".join(f"{a}<->{b}" for a, b in sorted(set(conflicts)))
        fail(f"module conflicts: {pairs}")
    return list(selected), label


def prepare_segments(manifest, catalog, known, selected, include_core=True):
    layout = manifest["layout"]
    slot_base = int(layout["slot_base"])
    slot_size = int(layout["slot_size"])
    abi_version = int(layout["abi_version"])
    segments = []
    if include_core:
        core_path = ROOT / "build_modular" / "mspm0_lua_modular.bin"
        if not core_path.is_file():
            fail("missing modular core; build it once before composing")
        core = core_path.read_bytes()
        if not core or len(core) > slot_base:
            fail(f"modular core size {len(core)} overlaps module slots")
        segments.append({
            "name": "core", "address": 0, "size": len(core),
            "sha256": sha256(core), "path": core_path, "data": core,
        })
    catalog_root = (ROOT / "build_modules").resolve()
    for slot, name in enumerate(selected):
        module_catalog = catalog.get("modules", {}).get(name)
        if not module_catalog:
            fail(f"module {name} is not prebuilt; run build_native_module.py {name}")
        if module_catalog.get("source") != known[name].get("source"):
            fail(f"module {name} source metadata changed; rebuild it")
        if module_catalog.get("version") != known[name].get("version"):
            fail(f"module {name} version metadata changed; rebuild it")
        source_path = ROOT / "modules" / known[name]["source"]
        if module_catalog.get("source_sha256") != sha256(source_path.read_bytes()):
            fail(f"module {name} source changed since it was prebuilt; rebuild it")
        variants = module_catalog.get("variants", [])
        matches = [item for item in variants if item.get("slot") == slot]
        if len(matches) != 1:
            fail(f"module {name} has no unique prebuilt slot{slot} variant")
        item = matches[0]
        address = slot_base + slot * slot_size
        if item.get("address") != address:
            fail(f"module {name} slot{slot} catalog address mismatch")
        if (item.get("module") != name or
                item.get("module_version") != known[name].get("version") or
                item.get("target") != manifest["catalog"]["target"] or
                item.get("abi_version") != abi_version or
                item.get("module_format") != manifest["catalog"]["module_format"] or
                item.get("build_id") != module_catalog.get("build_id")):
            fail(f"module {name} slot{slot} identity mismatch")
        path = (ROOT / item.get("image", "")).resolve()
        try:
            path.relative_to(catalog_root)
        except ValueError:
            fail(f"module {name} image escapes build_modules")
        if not path.is_file():
            fail(f"missing prebuilt module variant: {name}/slot{slot}")
        data = path.read_bytes()
        if len(data) != item.get("size") or sha256(data) != item.get("sha256"):
            fail(f"module {name}/slot{slot} size or SHA-256 mismatch")
        validate_module(data, name, address, slot_size, abi_version)
        segments.append({
            "name": name, "slot": slot, "address": address,
            "size": len(data), "sha256": sha256(data),
            "path": path, "data": data,
        })
    return segments


def public_segment(segment):
    return {key: value for key, value in segment.items() if key not in ("path", "data")}


def main():
    parser = argparse.ArgumentParser()
    choice = parser.add_mutually_exclusive_group(required=True)
    choice.add_argument("--set", dest="set_name")
    choice.add_argument("--modules", nargs="*", help="explicit module names in slot order")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--plan-only", action="store_true", help="validate and print mapping without writing firmware")
    args = parser.parse_args()

    manifest, catalog, known = load_inputs()
    selected, label = resolve_selection(manifest, known, args.set_name, args.modules)
    segments = prepare_segments(manifest, catalog, known, selected)
    print(f"PLAN {label}: {len(selected)}/{manifest['layout']['slot_count']} runtime slots")
    for segment in segments:
        suffix = "" if segment["name"] == "core" else f" slot{segment['slot']}"
        print(f"  {segment['name']:<8}{suffix:<7} 0x{segment['address']:08X} {segment['size']} bytes")
    if args.plan_only:
        return 0

    image = bytearray(b"\xFF" * FLASH_SIZE)
    for segment in segments:
        address = segment["address"]
        image[address:address + segment["size"]] = segment["data"]
    output = (args.output or ROOT / "build_composed" / f"firmware_{label}.bin").resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(image)
    report = output.with_suffix(".json")
    report.write_text(json.dumps({
        "schema": 2, "set": label, "modules": selected,
        "flash_size": FLASH_SIZE, "image_sha256": sha256(image),
        "segments": [public_segment(item) for item in segments],
    }, indent=2) + "\n", encoding="utf-8")
    print(f"COMPOSED {output} ({len(image)} bytes), SHA-256 {sha256(image)}")
    print("report", report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
