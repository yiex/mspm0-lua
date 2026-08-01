#!/usr/bin/env python3
"""Download littlefs single-header-ish sources without git."""
import os
import urllib.request
import zipfile
import io

URL = "https://github.com/littlefs-project/littlefs/archive/refs/tags/v2.10.2.zip"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "mspm0_lua", "third_party", "littlefs")


def main():
    out = os.path.abspath(OUT)
    os.makedirs(out, exist_ok=True)
    print("download", URL)
    data = urllib.request.urlopen(URL, timeout=120).read()
    z = zipfile.ZipFile(io.BytesIO(data))
    # extract only needed files
    names = [n for n in z.namelist() if n.endswith(("lfs.c", "lfs.h", "lfs_util.h", "lfs_util.c"))]
    for n in names:
        base = os.path.basename(n)
        if not base:
            continue
        target = os.path.join(out, base)
        with z.open(n) as src, open(target, "wb") as dst:
            dst.write(src.read())
        print("wrote", target)
    print("done")


if __name__ == "__main__":
    main()
