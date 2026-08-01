#!/usr/bin/env python3
"""Shared host configuration for build and flash helpers.

Nothing here is machine-specific. Optional integration points are enabled
exclusively through environment variables:

- MSPM0_SDK          path to a TI MSPM0 SDK root (must contain source/ti/driverlib)
- MSPM0_TOOLCHAIN    path to an ARM GNU toolchain root (contains bin/)
- JLINK_EXE          full path to JLink.exe (defaults to common install paths)
- LUCKFOX_HOST / LUCKFOX_USER / LUCKFOX_PASS
                     optional SSH access used to hold/reset a board through a
                     host computer; the board helpers are skipped when unset
"""

import os
import sys
from pathlib import Path


def tool_name(base):
    """Return the executable name for the current platform."""
    return base if os.name != "nt" else base + ".exe"


def jlink_exe():
    """Return the J-Link executable, honoring JLINK_EXE when set."""
    override = os.environ.get("JLINK_EXE")
    if override:
        return Path(override)
    candidates = (
        Path(r"C:\Program Files\SEGGER\JLink\JLink.exe"),
        Path(r"C:\Program Files (x86)\SEGGER\JLink\JLink.exe"),
        Path("/opt/SEGGER/JLink/JLinkExe"),
    )
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return candidates[0]


def luckfox_credentials():
    """Return (host, user, password) when fully configured, otherwise None."""
    host = os.environ.get("LUCKFOX_HOST")
    user = os.environ.get("LUCKFOX_USER")
    password = os.environ.get("LUCKFOX_PASS")
    if host and user and password:
        return host, user, password
    return None


def resolve_sdk(project_root, local_relative="third_party/mspm0_sdk"):
    """Resolve the TI MSPM0 SDK root from MSPM0_SDK or the project tree."""
    override = os.environ.get("MSPM0_SDK")
    candidates = []
    if override:
        candidates.append(Path(override))
    candidates.append(project_root / local_relative)
    for candidate in candidates:
        if (candidate / "source" / "ti" / "driverlib").is_dir():
            return candidate.resolve()
    raise SystemExit(
        "No MSPM0 SDK found: set MSPM0_SDK or place the SDK at "
        f"{candidates[-1]}"
    )


def resolve_toolchain():
    """Resolve the ARM GNU toolchain root containing a bin/ directory.

    Honors MSPM0_TOOLCHAIN, otherwise defaults to tools/arm-gnu-toolchain.
    Distribution archives keep a versioned top-level folder; this helper
    descends into it when the root itself has no bin/.
    """
    override = os.environ.get("MSPM0_TOOLCHAIN")
    if override:
        root = Path(override)
    else:
        root = Path(__file__).resolve().parent / "arm-gnu-toolchain"
    if (root / "bin").is_dir():
        return root.resolve()
    # Search one level down for the versioned extraction folder.
    exe = "arm-none-eabi-gcc.exe" if os.name == "nt" else "arm-none-eabi-gcc"
    for candidate in sorted(root.glob("*/bin"), key=lambda p: len(p.parts)):
        if (candidate / exe).is_file():
            return candidate.parent.resolve()
    return root.resolve()


def require(module):
    """Fail with a readable message when an optional dependency is missing."""
    try:
        __import__(module)
    except ImportError as error:
        print(f"missing optional dependency: {module}", file=sys.stderr)
        print(f"install it with: python -m pip install {module}", file=sys.stderr)
        raise SystemExit(1) from error
