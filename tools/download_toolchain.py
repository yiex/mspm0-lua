#!/usr/bin/env python3
"""Download and extract the pinned ARM GNU toolchain for the current OS."""
import platform
import os
import shutil
import subprocess
import sys
import urllib.request
import zipfile

REL = "14.2.rel1"
BASE = (
    "https://developer.arm.com/-/media/Files/downloads/gnu/"
    f"{REL}/binrel/"
)
if os.name == "nt":
    ARCHIVE_NAME = f"arm-gnu-toolchain-{REL}-mingw-w64-i686-arm-none-eabi.zip"
    ARCHIVE_URL = BASE + ARCHIVE_NAME
    ARCHIVE_SUFFIX = ".zip"
elif platform.machine().lower() in ("x86_64", "amd64", "aarch64"):
    machine = "x86_64" if platform.machine().lower() in ("x86_64", "amd64") else "aarch64"
    ARCHIVE_NAME = f"arm-gnu-toolchain-{REL}-{machine}-arm-none-eabi.tar.xz"
    ARCHIVE_URL = BASE + ARCHIVE_NAME
    ARCHIVE_SUFFIX = ".tar.xz"
else:
    raise SystemExit(f"unsupported host platform: {platform.machine()}")

OUT_DIR = os.path.dirname(os.path.abspath(__file__))
ARCHIVE_PATH = os.path.join(OUT_DIR, ARCHIVE_NAME)
EXTRACT_DIR = os.path.join(OUT_DIR, "arm-gnu-toolchain")


def hook(n, bs, ts):
    if n % 80 == 0:
        done = n * bs
        if ts > 0:
            print(f"  {done / 1024 / 1024:.1f}/{ts / 1024 / 1024:.1f} MB", flush=True)
        else:
            print(f"  {done / 1024 / 1024:.1f} MB", flush=True)


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    if os.path.exists(ARCHIVE_PATH) and os.path.getsize(ARCHIVE_PATH) > 50_000_000:
        print("archive exists", ARCHIVE_PATH, os.path.getsize(ARCHIVE_PATH))
    else:
        print("download", ARCHIVE_URL)
        urllib.request.urlretrieve(ARCHIVE_URL, ARCHIVE_PATH, hook)
        print("saved", ARCHIVE_PATH, os.path.getsize(ARCHIVE_PATH))

    if not os.path.isdir(EXTRACT_DIR):
        print("extracting...")
        os.makedirs(EXTRACT_DIR, exist_ok=True)
        if ARCHIVE_SUFFIX == ".tar.xz":
            # GNU tar on Linux preserves the toolchain's symlink/hardlink
            # layout; --strip-components=1 removes the versioned top folder.
            subprocess.run(
                ["tar", "-xJf", ARCHIVE_PATH, "-C", EXTRACT_DIR,
                 "--strip-components=1"],
                check=True,
            )
        else:
            staging = os.path.join(OUT_DIR, ".toolchain_staging")
            if os.path.isdir(staging):
                shutil.rmtree(staging)
            os.makedirs(staging)
            with zipfile.ZipFile(ARCHIVE_PATH, "r") as z:
                z.extractall(staging)
            # Distribution archives contain a single versioned top-level
            # folder; flatten it so tools/arm-gnu-toolchain/bin exists.
            entries = [name for name in os.listdir(staging)
                       if os.path.isdir(os.path.join(staging, name))]
            if len(entries) == 1:
                source = os.path.join(staging, entries[0])
                for name in os.listdir(source):
                    shutil.move(os.path.join(source, name), EXTRACT_DIR)
                shutil.rmtree(source)
            else:
                for name in os.listdir(staging):
                    shutil.move(os.path.join(staging, name), EXTRACT_DIR)
            shutil.rmtree(staging)
        print("extracted to", EXTRACT_DIR)
    else:
        print("already extracted", EXTRACT_DIR)

    # Verify the expected layout before handing over to the build scripts.
    exe = "arm-none-eabi-gcc.exe" if os.name == "nt" else "arm-none-eabi-gcc"
    gcc = os.path.join(EXTRACT_DIR, "bin", exe)
    if not os.path.isfile(gcc):
        raise SystemExit(f"toolchain extraction failed: {gcc} is missing")
    print("GCC:", gcc)
    cc1 = None
    for subdir in ("lib", "libexec"):
        for root, dirs, files in os.walk(os.path.join(EXTRACT_DIR, subdir)):
            if "cc1" in files:
                cc1 = os.path.join(root, "cc1")
                break
        if cc1 is not None:
            break
    if cc1 is None:
        raise SystemExit(
            "toolchain extraction failed: cc1 not found under lib/ or libexec/"
        )
    print("CC1:", cc1)


if __name__ == "__main__":
    main()
