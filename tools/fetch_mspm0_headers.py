#!/usr/bin/env python3
"""Fetch minimal MSPM0G3507 headers/startup/linker from TI public mspm0-sdk."""
import os
import urllib.request

BASE = "https://raw.githubusercontent.com/TexasInstruments/mspm0-sdk/main"
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "mspm0_lua", "third_party", "mspm0"))

FILES = [
    # device header tree (subset; enough for register access patterns used by DriverLib samples)
    "source/ti/devices/msp/m0p/mspm0g350x.h",
    "source/ti/devices/msp/msp.h",
    "source/ti/devices/msp/peripherals/hw_gpio.h",
    "source/ti/devices/msp/peripherals/hw_uart.h",
    "source/ti/devices/msp/peripherals/hw_spi.h",
    "source/ti/devices/msp/peripherals/hw_sysctl.h",
    "source/ti/devices/msp/peripherals/hw_iomux.h",
    "source/ti/devices/msp/peripherals/hw_vref.h",
    "source/ti/devices/msp/peripherals/hw_flashctl.h",
    "source/ti/devices/msp/peripherals/m0p/hw_sysctl_mspm0g1x0x_g3x0x.h",
    "source/ti/devices/msp/peripherals/m0p/sysctl/hw_sysctl_mspm0g1x0x_g3x0x.h",
    "source/ti/devices/msp/peripherals/m0p/hw_cpuss.h",
    "source/ti/devices/msp/peripherals/hw_crcp.h",
    "source/ti/devices/DeviceFamily.h",
    "source/ti/devices/msp/peripherals/hw_memctl.h",
    # startup / linker
    "source/ti/devices/msp/m0p/startup_system_files/gcc/startup_mspm0g350x_gcc.c",
    "source/ti/devices/msp/m0p/linker_files/gcc/mspm0g3507.lds",
    # driverlib core used by many examples (download larger set later if needed)
    "source/ti/driverlib/dl_common.h",
    "source/ti/driverlib/dl_common.c",
    "source/ti/driverlib/dl_gpio.h",
    "source/ti/driverlib/dl_gpio.c",
    "source/ti/driverlib/dl_uart.h",
    "source/ti/driverlib/dl_uart.c",
    "source/ti/driverlib/dl_uart_main.h",
    "source/ti/driverlib/dl_uart_main.c",
    "source/ti/driverlib/dl_spi.h",
    "source/ti/driverlib/dl_spi.c",
    "source/ti/driverlib/dl_sysctl.h",
    "source/ti/driverlib/dl_sysctl_mspm0g1x0x_g3x0x.c",
    "source/ti/driverlib/dl_sysctl.h",
    "source/ti/driverlib/m0p/dl_sysctl.h",
    "source/ti/driverlib/m0p/sysctl/dl_sysctl_mspm0g1x0x_g3x0x.h",
    "source/ti/driverlib/dl_vref.h",
    "source/ti/driverlib/dl_vref.c",
    "source/ti/driverlib/dl_flashctl.h",
    "source/ti/driverlib/dl_flashctl.c",
    "source/ti/driverlib/dl_interrupt.h",
    "source/ti/driverlib/dl_interrupt.c",
    "source/ti/driverlib/dl_timer.h",
    "source/ti/driverlib/dl_timer.c",
]


def fetch(rel):
    url = f"{BASE}/{rel}"
    dest = os.path.join(ROOT, rel)
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    if os.path.exists(dest) and os.path.getsize(dest) > 0:
        print("skip", rel)
        return True
    print("GET", rel)
    try:
        with urllib.request.urlopen(url, timeout=60) as r:
            data = r.read()
        with open(dest, "wb") as f:
            f.write(data)
        print(" ", len(data), "bytes")
        return True
    except Exception as e:
        print(" FAIL", e)
        return False


def main():
    ok = bad = 0
    for rel in FILES:
        if fetch(rel):
            ok += 1
        else:
            bad += 1
    print(f"done ok={ok} fail={bad} root={ROOT}")


if __name__ == "__main__":
    main()
