#!/usr/bin/env python3
"""Build minimal UART-only probe firmware."""
import os
import subprocess
import sys
from pathlib import Path

from host_config import resolve_sdk, resolve_toolchain, tool_name

ROOT = Path(__file__).resolve().parents[1] / "mspm0_lua"
SDK = resolve_sdk(ROOT)
TC = resolve_toolchain() / "bin"
CC = TC / tool_name("arm-none-eabi-gcc")
OBJCOPY = TC / tool_name("arm-none-eabi-objcopy")
SIZE = TC / tool_name("arm-none-eabi-size")
BUILD = ROOT / "build"
LDS = ROOT / "ld" / "mspm0g3507.lds"
NAME = "mspm0_uart_only"

CPUFLAGS = ["-mcpu=cortex-m0plus", "-march=armv6-m", "-mthumb", "-mfloat-abi=soft"]
CFLAGS = CPUFLAGS + [
    "-std=c99", "-Os", "-g0", "-ffunction-sections", "-fdata-sections",
    "-fno-common", "-Wall", "-D__MSPM0G3507__",
    f"-I{ROOT / 'board'}",
    f"-I{SDK / 'source'}",
    f"-I{SDK / 'source' / 'third_party' / 'CMSIS' / 'Core' / 'Include'}",
]
if os.environ.get("MSPM0_UART_IRQ", "0") == "1":
    CFLAGS.append("-DBOARD_UART_IRQ=1")
LDFLAGS = CPUFLAGS + [
    "-nostartfiles", "-Wl,--gc-sections",
    f"-Wl,-T,{LDS}", f"-Wl,-Map,{BUILD / NAME}.map",
    f"-L{SDK / 'source' / 'ti' / 'driverlib' / 'lib' / 'gcc' / 'm0p' / 'mspm0g1x0x_g3x0x'}",
    "-Wl,--start-group", "-l:driverlib.a", "-lc", "-lm", "-lgcc", "-Wl,--end-group",
    "--specs=nano.specs", "--specs=nosys.specs",
]
SRCS = [
    ROOT / "app" / "main_uart_only.c",
    ROOT / "board" / "ti_msp_dl_config.c",
    ROOT / "board" / "board_uart.c",
    ROOT / "board" / "board_delay.c",
    ROOT / "board" / "board_status.c",
    ROOT / "board" / "startup_mspm0g350x_gcc.c",
]


def run(cmd):
    print("+", " ".join(str(c) for c in cmd[:5]), "...")
    r = subprocess.run(cmd)
    if r.returncode != 0:
        raise SystemExit(r.returncode)


def main():
    BUILD.mkdir(parents=True, exist_ok=True)
    objs = []
    for src in SRCS:
        obj = BUILD / (src.stem + ".o")
        objs.append(obj)
        run([str(CC), *CFLAGS, "-c", str(src), "-o", str(obj)])
    elf = BUILD / f"{NAME}.elf"
    run([str(CC), *[str(o) for o in objs], *LDFLAGS, "-o", str(elf)])
    binp = BUILD / f"{NAME}.bin"
    run([str(OBJCOPY), "-O", "binary", str(elf), str(binp)])
    run([str(SIZE), str(elf)])
    print("OK", elf, binp)


if __name__ == "__main__":
    main()
