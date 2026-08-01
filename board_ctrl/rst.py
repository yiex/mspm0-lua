#!/usr/bin/env python3
#Drive GPIO0_A4 low briefly, then leave it high-impedance."""

import os
import time

from visiong import Pin

#boot-1c7(high-enable,low-defult) rst-1c6(low-enable,high-impedance-defalue)
PIN_NAME = "GPIO1_C6"


def main():
    pin = Pin(PIN_NAME, backend="reg")

    pin.init(mode=Pin.OUT, value=0)
    print(f"{PIN_NAME}: LOW; holding for 1 second", flush=True)
    time.sleep(1.0)

    pin.init(mode=Pin.IN, pull=Pin.PULL_NONE)
    print(f"{PIN_NAME}: HIGH-Z; exiting without restoring the original state", flush=True)

    # Normal Python shutdown destroys Pin/PinMux and VisionG restores its
    # saved pinmux state.  os._exit() intentionally skips that cleanup so
    # GPIO0_A4 remains in the configured high-impedance state.
    os._exit(0)


if __name__ == "__main__":
    main()

