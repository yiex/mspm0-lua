#!/usr/bin/env python3
"""Dump TEXT/MTEXT entities from a DXF schematic and match pin labels."""
import argparse
import re
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dxf", type=Path, help="path to the DXF file")
    args = parser.parse_args()

    lines = args.dxf.read_text(errors="ignore").splitlines()
    texts = []
    i = 0
    stop = {
        "TEXT", "MTEXT", "LINE", "LWPOLYLINE", "CIRCLE", "INSERT", "POINT",
        "ARC", "SOLID", "HATCH", "DIMENSION", "LEADER", "SPLINE", "ELLIPSE",
        "ATTDEF", "ATTRIB", "VIEWPORT", "IMAGE", "WIPEOUT", "3DFACE",
        "ENDSEC", "SEQEND",
    }
    while i < len(lines):
        if lines[i].strip() in ("TEXT", "MTEXT"):
            ent = {"type": lines[i].strip()}
            j = i + 1
            while j + 1 < len(lines):
                code = lines[j].strip()
                val = lines[j + 1]
                if code == "0":
                    break
                if code == "10":
                    ent["x"] = float(val)
                elif code == "20":
                    ent["y"] = float(val)
                elif code == "1":
                    ent["s"] = val
                j += 2
            if "s" in ent and "x" in ent:
                texts.append(ent)
            i = j
        else:
            i += 1

    print("texts", len(texts))
    pat = re.compile(
        r"SPI_|BSL|340_|W25|PA1[01]|PB1[4-7]|PA18|UART|TX|RX|LED|CH340|"
        r"NRST|SWD|POCI|PICO|CS",
        re.I,
    )
    for t in texts:
        if pat.search(t["s"]):
            print(f"{t['x']:8.1f},{t['y']:8.1f}  {t['s']}")

    print("\n--- proximity SPI/BSL/UART ---")
    interest = [
        t for t in texts if re.search(r"SPI_|BSL|340_|W25Q", t["s"], re.I)
    ]
    pins = [
        t for t in texts
        if re.fullmatch(r"P[AB]\d+", t["s"]) or re.match(r"P[AB]\d+/", t["s"])
    ]
    for a in interest:
        nearest = sorted(
            pins,
            key=lambda p: (p["x"] - a["x"]) ** 2 + (p["y"] - a["y"]) ** 2,
        )[:5]
        print(
            a["s"], "->",
            [
                (p["s"], round(((p["x"] - a["x"]) ** 2 + (p["y"] - a["y"]) ** 2) ** 0.5, 1))
                for p in nearest
            ],
        )


if __name__ == "__main__":
    main()
