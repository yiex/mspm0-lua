#!/usr/bin/env python3
import json
import os
import urllib.request

API = "https://api.github.com/repos/TexasInstruments/mspm0-sdk/git/trees/main?recursive=1"
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "mspm0_lua", "third_party", "mspm0"))
RAW = "https://raw.githubusercontent.com/TexasInstruments/mspm0-sdk/main"


def main():
    print("listing tree...")
    with urllib.request.urlopen(API, timeout=120) as r:
        tree = json.load(r)["tree"]
    want_sub = (
        "source/ti/devices/",
        "source/ti/driverlib/",
        "source/third_party/CMSIS/Core/Include/",
    )
    files = []
    for t in tree:
        if t["type"] != "blob":
            continue
        p = t["path"]
        if not any(p.startswith(s) for s in want_sub):
            continue
        # skip huge / unused
        if "/docs/" in p or p.endswith(".html") or "/keil/" in p or "/iar/" in p:
            continue
        if p.endswith((".c", ".h", ".lds", ".S", ".s")):
            files.append(p)
    print("candidates", len(files))
    # prioritize
    prio = [p for p in files if any(x in p for x in (
        "mspm0g350", "startup_mspm0g350", "dl_sysctl", "dl_uart", "dl_gpio",
        "dl_spi", "dl_common", "dl_interrupt", "hw_sysctl", "CMSIS", "core_cm0",
        "msp.h", "DeviceFamily", "hw_iomux", "hw_gpio", "hw_uart", "hw_spi",
        "hw_cpuss", "hw_flashctl", "linker_files/gcc/mspm0g3507"
    ))]
    print("prio", len(prio))
    ok = fail = 0
    for rel in prio:
        dest = os.path.join(ROOT, rel)
        if os.path.exists(dest) and os.path.getsize(dest) > 0:
            continue
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        url = f"{RAW}/{rel}"
        try:
            with urllib.request.urlopen(url, timeout=60) as r:
                data = r.read()
            with open(dest, "wb") as f:
                f.write(data)
            print("OK", rel, len(data))
            ok += 1
        except Exception as e:
            print("FAIL", rel, e)
            fail += 1
    print("done ok", ok, "fail", fail)


if __name__ == "__main__":
    main()
