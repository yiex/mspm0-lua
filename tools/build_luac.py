#!/usr/bin/env python3
"""Build the native Lua 5.5.1 / LUA_32BITS bytecode compiler.

The compiler is only needed by host tooling that prepares Lua bytecode for
the firmware (tools/compile_lua.py). Zig is a build-time dependency and is
downloaded with a pinned SHA-256 when not already available.
"""

from __future__ import annotations

import hashlib
import shutil
import subprocess
import urllib.request
import zipfile
from pathlib import Path


PROJECT = Path(__file__).resolve().parents[1]
LUA = PROJECT / "mspm0_lua" / "third_party" / "lua"
HOST_C = PROJECT / "tools" / "luac_mspm0.c"
COMPILER = Path(__file__).resolve().parent / "bin" / "luac_mspm0.exe"

ZIG_VERSION = "0.15.2"
ZIG_URL = (
    f"https://ziglang.org/download/{ZIG_VERSION}/"
    f"zig-x86_64-windows-{ZIG_VERSION}.zip"
)
ZIG_SHA256 = "3a0ed1e8799a2f8ce2a6e6290a9ff22e6906f8227865911fb7ddedc3cc14cb0c"
ZIG_CACHE = PROJECT / "tmp" / "ide-build" / f"zig-{ZIG_VERSION}"

LUA_SOURCES = [
    "lapi.c", "lauxlib.c", "lcode.c", "lctype.c", "ldebug.c", "ldo.c",
    "ldump.c", "lfunc.c", "lgc.c", "llex.c", "lmem.c", "lobject.c",
    "lopcodes.c", "lparser.c", "lstate.c", "lstring.c", "ltable.c",
    "ltm.c", "lundump.c", "lvm.c", "lzio.c",
]


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def find_zig() -> Path:
    command = shutil.which("zig")
    if command:
        return Path(command)
    cached = next(ZIG_CACHE.glob("**/zig.exe"), None) if ZIG_CACHE.is_dir() else None
    if cached:
        return cached
    archive = ZIG_CACHE.parent / f"zig-{ZIG_VERSION}.zip"
    archive.parent.mkdir(parents=True, exist_ok=True)
    if not archive.is_file() or file_sha256(archive) != ZIG_SHA256:
        print(f"Downloading Zig {ZIG_VERSION}...")
        urllib.request.urlretrieve(ZIG_URL, archive)
    if file_sha256(archive) != ZIG_SHA256:
        raise SystemExit("Zig archive SHA-256 mismatch")
    print("Extracting Zig...")
    ZIG_CACHE.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive) as bundle:
        bundle.extractall(ZIG_CACHE)
    cached = next(ZIG_CACHE.glob("**/zig.exe"), None)
    if not cached:
        raise SystemExit("zig.exe missing after extraction")
    return cached


def main() -> None:
    if not HOST_C.is_file():
        raise SystemExit(f"missing host compiler source: {HOST_C}")
    if not (LUA / "lua.h").is_file():
        raise SystemExit(f"missing Lua sources: {LUA}")
    zig = find_zig()
    COMPILER.parent.mkdir(parents=True, exist_ok=True)
    command = [
        str(zig), "cc", "-target", "x86_64-windows-gnu", "-std=c99",
        "-O2", "-s", "-DLUA_32BITS", f"-I{LUA}", str(HOST_C),
        *[str(LUA / name) for name in LUA_SOURCES], "-o", str(COMPILER),
    ]
    print("Building native target-ABI compiler...")
    subprocess.run(command, check=True)
    print(f"OK {COMPILER} ({COMPILER.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
