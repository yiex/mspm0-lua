#!/usr/bin/env python3
"""Control MSPM0 BOOT and RST via Luckfox VisionG GPIOs.

Pin map (per board wiring instruction):
  Control A -> GPIO1_C6
  Control B -> GPIO1_C7

Normal idle is Hi-Z on both lines. Hardware testing established that a short
GPIO1_C6 low pulse resets and starts the application.  The working flash-entry
sequence still asserts C6 high and pulses C7 low, so the legacy names/actions
are retained for compatibility and ``app-reset`` is explicit.

Note: older rst.py used GPIO1_C6 for reset. This file follows the current
wiring description. Use --pin-check if the board behaves inverted.
"""

from __future__ import annotations

import argparse
import os
import time

from visiong import Pin

PIN_BOOT = "GPIO1_C6"
PIN_RST = "GPIO1_C7"


def set_high_z(pin: Pin) -> None:
    pin.init(mode=Pin.IN, pull=Pin.PULL_NONE)


def set_out(pin: Pin, value: int) -> None:
    pin.init(mode=Pin.OUT, value=value)


def boot_default(pin: Pin) -> None:
    """Release control A; external board bias selects the normal boot state."""
    set_high_z(pin)


def boot_press(pin: Pin, seconds: float) -> None:
    """Assert BOOT (high) for `seconds`."""
    set_out(pin, 1)
    time.sleep(seconds)


def rst_default(pin: Pin) -> None:
    """RST idle: high, then release to Hi-Z (external pull-up holds it)."""
    set_out(pin, 1)
    time.sleep(0.01)
    set_high_z(pin)


def rst_press(pin: Pin, seconds: float) -> None:
    """Pulse RST low, then Hi-Z."""
    set_out(pin, 0)
    time.sleep(seconds)
    set_high_z(pin)


def enter_bsl(boot: Pin, rst: Pin, hold: float, rst_ms: float) -> None:
    """Hold BOOT high while resetting into BSL."""
    boot_press(boot, 0.0)
    time.sleep(0.01)
    rst_press(rst, rst_ms)
    time.sleep(hold)
    boot_default(boot)


def main() -> None:
    parser = argparse.ArgumentParser(description="MSPM0 BOOT/RST control on Luckfox")
    parser.add_argument(
        "action",
        choices=(
            "boot-default",
            "boot-press",
            "boot-hold",
            "rst-default",
            "rst-press",
            "reset",
            "app-reset",
            "bsl",
            "status-setup",
            "release",
        ),
        help="action to perform",
    )
    parser.add_argument("--seconds", type=float, default=0.5, help="hold time in seconds")
    parser.add_argument("--rst-seconds", type=float, default=0.1, help="RST low pulse width")
    parser.add_argument(
        "--no-exit-skip-cleanup",
        action="store_true",
        help="allow normal Python exit (VisionG may restore pinmux)",
    )
    args = parser.parse_args()
    if args.seconds < 0 or args.rst_seconds <= 0:
        parser.error("times must be valid (>0 for rst pulse)")

    boot = Pin(PIN_BOOT, backend="reg")
    rst = Pin(PIN_RST, backend="reg")

    if args.action == "boot-default":
        boot_default(boot)
        print(f"{PIN_BOOT}: Hi-Z (released)", flush=True)
    elif args.action == "boot-press":
        boot_press(boot, args.seconds)
        print(f"{PIN_BOOT}: HIGH held {args.seconds:g}s", flush=True)
        boot_default(boot)
        print(f"{PIN_BOOT}: Hi-Z (released)", flush=True)
    elif args.action == "boot-hold":
        boot_press(boot, 0.0)
        print(f"{PIN_BOOT}: HIGH (held; process will skip cleanup by default)", flush=True)
        time.sleep(args.seconds)
    elif args.action == "rst-default":
        rst_default(rst)
        print(f"{PIN_RST}: HIGH then Hi-Z (RST idle)", flush=True)
    elif args.action in ("rst-press", "reset"):
        rst_press(rst, args.rst_seconds)
        print(f"{PIN_RST}: LOW {args.rst_seconds:g}s -> Hi-Z (reset pulse)", flush=True)
    elif args.action == "app-reset":
        set_high_z(rst)
        rst_press(boot, args.rst_seconds)
        print(
            f"{PIN_BOOT}: LOW {args.rst_seconds:g}s -> Hi-Z "
            "(verified application reset)",
            flush=True,
        )
    elif args.action == "bsl":
        print("Enter BSL: BOOT=HIGH, pulse RST, hold BOOT, release BOOT", flush=True)
        enter_bsl(boot, rst, hold=args.seconds, rst_ms=args.rst_seconds)
        print("BSL sequence done; BOOT idle LOW", flush=True)
    elif args.action == "status-setup":
        boot_default(boot)
        set_high_z(rst)
        print(f"setup idle: {PIN_BOOT}=Hi-Z, {PIN_RST}=Hi-Z", flush=True)
    elif args.action == "release":
        set_high_z(boot)
        set_high_z(rst)
        print(f"released: {PIN_BOOT}=Hi-Z, {PIN_RST}=Hi-Z", flush=True)

    if not args.no_exit_skip_cleanup:
        # Keep pinmux/state as left by this script (same trick as rst.py).
        os._exit(0)


if __name__ == "__main__":
    main()
