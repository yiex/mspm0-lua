#!/usr/bin/env python3
"""Install a validated prebuilt module selection without rebuilding the core."""

import argparse
import subprocess
import tempfile
from pathlib import Path

from hold_boot_flash import (
    JLINK,
    cleanup_hold_boot,
    jlink_reset_application,
    ssh_connect,
    start_hold_boot,
)
from compose_firmware import (
    load_inputs,
    prepare_segments,
    resolve_selection,
)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("set_name", nargs="?", help="named set from modules.json")
    parser.add_argument("--modules", nargs="*", help="explicit module names in slot order")
    args = parser.parse_args()
    if args.modules is not None and args.set_name is not None:
        raise SystemExit("use a named set or --modules, not both")
    if args.modules is None and args.set_name is None:
        raise SystemExit("provide a named set or --modules")

    manifest, catalog, known = load_inputs()
    selected, label = resolve_selection(
        manifest, known, args.set_name, args.modules
    )
    # Validate every byte before connecting to hardware or erasing anything.
    segments = prepare_segments(manifest, catalog, known, selected)
    core = segments[0]
    modules = segments[1:]
    layout = manifest["layout"]
    slot_base = int(layout["slot_base"])
    slot_end = slot_base + int(layout["slot_size"]) * int(layout["slot_count"]) - 1

    core_region_file = tempfile.NamedTemporaryFile(
        "wb", suffix=".bin", delete=False
    )
    core_region_file.write(
        core["data"] + b"\xFF" * (slot_base - len(core["data"]))
    )
    core_region_file.close()
    core_region_path = Path(core_region_file.name)

    commands = [
        "si 1", "speed 1000", "device MSPM0G3507", "connect", "halt",
        # This must succeed before the module region is touched.
        f"verifybin {core_region_path.as_posix()} 0x00000000",
        f"erase 0x{slot_base:08X} 0x{slot_end:08X}",
    ]
    for segment in modules:
        commands.append(f"loadbin {segment['path'].as_posix()} 0x{segment['address']:08X}")
        commands.append(f"verifybin {segment['path'].as_posix()} 0x{segment['address']:08X}")
    commands.append("exit")

    with tempfile.NamedTemporaryFile(
        "w", suffix=".jlink", delete=False, encoding="ascii"
    ) as handle:
        handle.write("\n".join(commands) + "\n")
        command_file = Path(handle.name)
    print(f"install {label}: {', '.join(selected) or '(core only)'}")
    for segment in modules:
        print(f"  slot{segment['slot']} {segment['name']} @ 0x{segment['address']:08X}")

    client = None
    result = None
    started = False
    try:
        client = ssh_connect()
        start_hold_boot(client)
        result = subprocess.run(
            [str(JLINK), "-CommandFile", str(command_file), "-ExitOnError", "1"],
            text=True, encoding="utf-8", errors="ignore",
        )
    finally:
        try:
            if client is not None:
                cleanup_hold_boot(client)
                if result is not None and result.returncode == 0:
                    # BOOT must be released before checking address zero;
                    # ROM BSL remaps the vector region while BOOT is held.
                    started = jlink_reset_application(core_region_path)
                else:
                    print("flash failed; application was not reset or started")
                client.close()
        finally:
            command_file.unlink(missing_ok=True)
            core_region_path.unlink(missing_ok=True)
    return 0 if started else 1


if __name__ == "__main__":
    raise SystemExit(main())
