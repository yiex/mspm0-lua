#!/usr/bin/env python3
"""Test GPIO4_C1 high-impedance, high, and low states on a VisionG board."""

import argparse
import time

from visiong import Pin


PIN_NAME = "GPIO0_A4"


def set_state(pin, state):
    if state == "z":
        # Input mode with the internal pull resistor disabled = high impedance.
        pin.init(mode=Pin.IN, pull=Pin.PULL_NONE)
        print(f"{PIN_NAME}: HIGH-Z (input, pull disabled)")
    elif state == "high":
        pin.init(mode=Pin.OUT, value=1)
        print(f"{PIN_NAME}: HIGH")
    elif state == "low":
        pin.init(mode=Pin.OUT, value=0)
        print(f"{PIN_NAME}: LOW")
    else:
        raise ValueError(f"unsupported state: {state}")


def main():
    parser = argparse.ArgumentParser(description=f"Test {PIN_NAME} GPIO modes")
    parser.add_argument(
        "--state",
        choices=("z", "high", "low", "cycle"),
        default="cycle",
        help="state to apply; default: cycle",
    )
    parser.add_argument(
        "--seconds",
        type=float,
        default=3.0,
        help="how long to hold each state; default: 3 seconds",
    )
    args = parser.parse_args()

    if args.seconds <= 0:
        parser.error("--seconds must be greater than zero")

    pin = Pin(PIN_NAME, backend="auto")
    states = ("z", "high", "low") if args.state == "cycle" else (args.state,)

    try:
        for state in states:
            set_state(pin, state)
            print(f"Holding for {args.seconds:g} seconds...")
            time.sleep(args.seconds)
    finally:
        # Leave the pin in high-impedance input mode instead of leaving it driven.
        try:
            pin.init(mode=Pin.IN, pull=Pin.PULL_NONE)
            print(f"{PIN_NAME}: final state HIGH-Z")
        finally:
            pin.deinit()


if __name__ == "__main__":
    main()
