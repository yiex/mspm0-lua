#!/usr/bin/env python3
"""Build mspm0_lua firmware without GNU make (Windows-friendly)."""
import os
import subprocess
import sys
from pathlib import Path

from host_config import resolve_sdk, resolve_toolchain, tool_name

ROOT = Path(__file__).resolve().parents[1] / "mspm0_lua"
SDK = resolve_sdk(ROOT)
TC = resolve_toolchain() / "bin"
print("SDK =", SDK)
CC = TC / tool_name("arm-none-eabi-gcc")
OBJCOPY = TC / tool_name("arm-none-eabi-objcopy")
SIZE = TC / tool_name("arm-none-eabi-size")
PROFILE = os.environ.get("MSPM0_PROFILE", "bytecode").lower()
if PROFILE not in ("source", "source_full", "bytecode", "modular"):
    raise SystemExit(
        "MSPM0_PROFILE must be source, source_full, bytecode, or modular"
    )
BUILD_NAMES = {
    "source": "build",
    "source_full": "build_source_full",
    "bytecode": "build_bytecode",
    "modular": "build_modular",
}
BUILD = ROOT / BUILD_NAMES[PROFILE]
LDS = ROOT / "ld" / "mspm0g3507.lds"
NAME = {
    "source": "mspm0_lua",
    "source_full": "mspm0_lua_source_full",
    "bytecode": "mspm0_lua_bytecode",
    "modular": "mspm0_lua_modular",
}[PROFILE]
LFS_BLOCKS = int(os.environ.get("MSPM0_LFS_BLOCKS", "0"), 0)
UART_SELFTEST = os.environ.get("MSPM0_UART_SELFTEST", "0") == "1"
SKIP_BOOT_SCRIPT = os.environ.get("MSPM0_SKIP_BOOT_SCRIPT", "0") == "1"
if LFS_BLOCKS != 0 and (LFS_BLOCKS < 16 or LFS_BLOCKS > 4096):
    raise SystemExit("MSPM0_LFS_BLOCKS must be 0 (JEDEC auto) or 16..4096")

CPUFLAGS = ["-mcpu=cortex-m0plus", "-march=armv6-m", "-mthumb", "-mfloat-abi=soft"]
CFLAGS = CPUFLAGS + [
    "-std=c99", "-Oz", "-g0", "-ffunction-sections", "-fdata-sections",
    "-fno-common", "-fno-exceptions", "-flto", "-Wall",
    "-D__MSPM0G3507__", "-DLUA_32BITS", "-DBOARD_UART_IRQ=1",
    f"-I{ROOT/'app'}", f"-I{ROOT/'board'}", f"-I{ROOT/'lua_bind'}",
    f"-I{ROOT/'third_party'/'lua'}",
    f"-I{ROOT/'third_party'/'littlefs'}",
    f"-I{SDK/'source'}",
    f"-I{SDK/'source'/'third_party'/'CMSIS'/'Core'/'Include'}",
    "-DLFS_NO_MALLOC",
    "-DLFS_NO_ASSERT",
    "-DLFS_NO_DEBUG",
    "-DLFS_NO_WARN",
    "-DLFS_NO_ERROR",
    "-DLFS_NAME_MAX=31",
    f"-DLFS_BLOCK_COUNT={LFS_BLOCKS}",
]
if PROFILE in ("bytecode", "modular"):
    CFLAGS.append("-DLUA_BINARY_ONLY")
    if PROFILE == "modular":
        CFLAGS.extend(["-DMSPM0_MODULAR_CORE", "-DLUA_SOURCE_FULL_TIGHT"])
elif PROFILE == "source":
    CFLAGS.append("-DLUA_SOURCE_COMPACT")
elif PROFILE == "source_full":
    CFLAGS.append("-DLUA_SOURCE_FULL_TIGHT")
if UART_SELFTEST:
    CFLAGS.append("-DBOARD_UART_SELFTEST=1")
if SKIP_BOOT_SCRIPT:
    CFLAGS.append("-DBOARD_SKIP_BOOT_SCRIPT=1")
LDFLAGS = CPUFLAGS + [
    "-nostartfiles", "-flto", "-Wl,--gc-sections",
    f"-Wl,-T,{LDS}", f"-Wl,-Map,{BUILD/NAME}.map",
    f"-L{SDK/'source'/'ti'/'driverlib'/'lib'/'gcc'/'m0p'/'mspm0g1x0x_g3x0x'}",
    "-Wl,--start-group", "-l:driverlib.a", "-lc", "-lm", "-lgcc", "-Wl,--end-group",
    "--specs=nano.specs", "--specs=nosys.specs",
]
if PROFILE == "modular":
    LDFLAGS.append("-Wl,--defsym,MODULAR_LAYOUT=1")

APP = [
    ROOT / "app" / "main.c",
    ROOT / "app" / "lua_runtime.c",
    ROOT / "board" / "ti_msp_dl_config.c",
    ROOT / "board" / "board_uart.c",
    ROOT / "board" / "board_uart_app.c",
    ROOT / "board" / "board_delay.c",
    ROOT / "board" / "board_pins.c",
    ROOT / "board" / "board_resource.c",
    ROOT / "board" / "board_irq.c",
    ROOT / "board" / "board_dma.c",
    ROOT / "board" / "board_pwm.c",
    ROOT / "board" / "board_iq.c",
    ROOT / "board" / "board_pid.c",
    ROOT / "board" / "board_filt.c",
    ROOT / "board" / "board_btn.c",
    ROOT / "board" / "board_enc.c",
    ROOT / "board" / "board_wdt.c",
    ROOT / "board" / "board_ramp.c",
    ROOT / "board" / "board_util.c",
    ROOT / "board" / "board_crc.c",
    ROOT / "board" / "board_cap.c",
    ROOT / "board" / "board_qei.c",
    ROOT / "board" / "board_adc.c",
    # board_can.c dropped: free Flash for ADC DMA + complementary PWM
    ROOT / "board" / "board_i2c.c",
    ROOT / "board" / "board_i2c1.c",
    ROOT / "board" / "board_oled.c",
    ROOT / "board" / "board_spi.c",
    ROOT / "board" / "board_spi0.c",
    ROOT / "board" / "board_spiflash.c",
    ROOT / "board" / "board_lfs.c",
    ROOT / "board" / "board_status.c",
    ROOT / "board" / "startup_mspm0g350x_gcc.c",
    ROOT / "lua_bind" / "lua_bind.c",
    ROOT / "third_party" / "littlefs" / "lfs.c",
    ROOT / "third_party" / "littlefs" / "lfs_util.c",
]
if PROFILE == "modular":
    APP = [
        ROOT / "app" / "main.c",
        ROOT / "app" / "lua_runtime.c",
        ROOT / "app" / "native_module.c",
        ROOT / "app" / "module_update.c",
        ROOT / "board" / "ti_msp_dl_config.c",
        ROOT / "board" / "board_uart.c",
        ROOT / "board" / "board_delay.c",
        ROOT / "board" / "board_pins.c",
        ROOT / "board" / "board_resource.c",
        ROOT / "board" / "board_irq.c",
        ROOT / "board" / "board_dma.c",
        ROOT / "board" / "board_wdt.c",
        ROOT / "board" / "board_iq.c",
        ROOT / "board" / "board_crc.c",
        ROOT / "board" / "board_spiflash.c",
        ROOT / "board" / "board_lfs.c",
        ROOT / "board" / "board_status.c",
        ROOT / "board" / "startup_mspm0g350x_gcc.c",
        ROOT / "lua_bind" / "lua_bind_core.c",
        ROOT / "third_party" / "littlefs" / "lfs.c",
        ROOT / "third_party" / "littlefs" / "lfs_util.c",
    ]
elif PROFILE == "bytecode":
    APP.insert(2, ROOT / "app" / "native_module.c")
LUA = [
    "lapi.c", "lauxlib.c", "lbaselib.c", "lcode.c", "lctype.c", "ldebug.c",
    "ldo.c", "lfunc.c", "lgc.c", "llex.c", "lmem.c", "lobject.c",
    "lopcodes.c", "lparser.c", "lstate.c", "lstring.c", "ltable.c",
    "ltm.c", "lundump.c", "lvm.c", "lzio.c",
    # omitted: lstrlib lmathlib linit lcorolib liolib loslib loadlib ltablib ldump
]
if PROFILE in ("bytecode", "modular"):
    LUA = [s for s in LUA if s not in ("lcode.c", "llex.c", "lparser.c")]
SRCS = APP + [ROOT / "third_party" / "lua" / s for s in LUA]


def run(cmd):
    print("+", " ".join(str(c) for c in cmd[:6]), "...")
    r = subprocess.run(cmd, cwd=ROOT)
    if r.returncode != 0:
        raise SystemExit(r.returncode)


def main():
    if PROFILE == "modular":
        run([
            sys.executable,
            str(Path(__file__).resolve().parent / "build_catalog_release.py"),
            "--prepare-only",
        ])
    BUILD.mkdir(parents=True, exist_ok=True)
    print("Profile =", PROFILE)
    if LFS_BLOCKS:
        lfs_kib = LFS_BLOCKS * 4
        lfs_size = f"{lfs_kib // 1024} MiB" if lfs_kib % 1024 == 0 else f"{lfs_kib} KiB"
        print(f"LittleFS = {LFS_BLOCKS} x 4 KiB = {lfs_size}")
    else:
        print("LittleFS = full chip (JEDEC capacity auto-detect)")
    flags_file = BUILD / "cflags.txt"
    flags_text = "\n".join(str(flag) for flag in CFLAGS) + "\n"
    flags_changed = not flags_file.exists() or flags_file.read_text() != flags_text
    project_headers = [
        header
        for directory in (ROOT / "app", ROOT / "board", ROOT / "lua_bind")
        for header in directory.glob("*.h")
    ]
    objs = []
    for src in SRCS:
        obj = BUILD / (src.stem + ".o")
        objs.append(obj)
        # Rebuild when shared project headers change.
        need = flags_changed or not obj.exists()
        if obj.exists():
            ot = obj.stat().st_mtime
            need = need or ot < src.stat().st_mtime
            if not need:
                for hdr in project_headers:
                    if ot < hdr.stat().st_mtime:
                        need = True
                        break
        if not need:
            continue
        run([str(CC), *CFLAGS, "-c", str(src), "-o", str(obj)])
    flags_file.write_text(flags_text)
    elf = BUILD / f"{NAME}.elf"
    run([str(CC), *[str(o) for o in objs], *LDFLAGS, "-o", str(elf)])
    binp = BUILD / f"{NAME}.bin"
    run([str(OBJCOPY), "-O", "binary", str(elf), str(binp)])
    run([str(SIZE), str(elf)])
    print("OK", elf, binp)


if __name__ == "__main__":
    main()
