#!/usr/bin/env python3
"""Download mspm0-sdk source tree as zip from GitHub."""
import io
import os
import zipfile
import urllib.request

URL = "https://github.com/TexasInstruments/mspm0-sdk/archive/refs/heads/main.zip"
OUT = os.path.abspath(os.path.join(os.path.dirname(__file__), "mspm0-sdk-main.zip"))
DEST = os.path.abspath(os.path.join(os.path.dirname(__file__), "mspm0-sdk"))


def hook(n, bs, ts):
    if n % 100 == 0:
        done = n * bs
        if ts > 0:
            print(f"  {done/1024/1024:.1f}/{ts/1024/1024:.1f} MB", flush=True)


def main():
    if not (os.path.exists(OUT) and os.path.getsize(OUT) > 1_000_000):
        print("download", URL)
        urllib.request.urlretrieve(URL, OUT, hook)
        print("saved", OUT, os.path.getsize(OUT))
    else:
        print("zip exists", os.path.getsize(OUT))

    # extract only source/ti and CMSIS
    keep_prefix = (
        "mspm0-sdk-main/source/ti/devices/",
        "mspm0-sdk-main/source/ti/driverlib/",
        "mspm0-sdk-main/source/third_party/CMSIS/",
    )
    os.makedirs(DEST, exist_ok=True)
    with zipfile.ZipFile(OUT, "r") as z:
        names = [n for n in z.namelist() if n.startswith(keep_prefix)]
        print("extract files", len(names))
        for n in names:
            # strip top folder
            rel = n[len("mspm0-sdk-main/") :]
            target = os.path.join(DEST, rel)
            if n.endswith("/"):
                os.makedirs(target, exist_ok=True)
                continue
            os.makedirs(os.path.dirname(target), exist_ok=True)
            with z.open(n) as src, open(target, "wb") as dst:
                dst.write(src.read())
    print("done", DEST)


if __name__ == "__main__":
    main()
