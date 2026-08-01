#!/usr/bin/env python3
"""Host-side regression and negative tests for modular composition."""

import inspect

import compose_firmware as compose


def rejects(call, expected):
    try:
        call()
    except SystemExit as error:
        if expected not in str(error):
            raise AssertionError(f"expected {expected!r}, got {str(error)!r}")
        return
    raise AssertionError(f"expected rejection containing {expected!r}")


def main():
    manifest, catalog, known = compose.load_inputs()
    slot_count = int(manifest["layout"]["slot_count"])
    assert len(known) > slot_count, "catalog must be allowed to exceed MCU slots"
    assert set(catalog["modules"]) == set(known)
    for name, module in catalog["modules"].items():
        assert len(module["variants"]) == slot_count, name
        assert {item["slot"] for item in module["variants"]} == set(range(slot_count))

    selected = ["rtc", "crc", "dac", "i2c"]
    segments = compose.prepare_segments(manifest, catalog, known, selected)
    assert [item["name"] for item in segments[1:]] == selected
    assert [item["slot"] for item in segments[1:]] == [0, 1, 2, 3]
    i2c_slot3 = segments[-1]
    i2c_slot0 = compose.prepare_segments(
        manifest, catalog, known, ["i2c"]
    )[1]
    assert i2c_slot0["path"] != i2c_slot3["path"]
    assert i2c_slot0["data"] != i2c_slot3["data"]

    rejects(
        lambda: compose.resolve_selection(
            manifest, known, modules=["i2c", "i2c"]
        ),
        "duplicate",
    )
    rejects(
        lambda: compose.resolve_selection(
            manifest, known, modules=list(known)[:slot_count + 1]
        ),
        "runtime slots",
    )

    address = i2c_slot0["address"]
    data = i2c_slot0["data"]
    compose.validate_module(
        data, "i2c", address, int(manifest["layout"]["slot_size"]),
        int(manifest["layout"]["abi_version"]),
    )
    bad_abi = bytearray(data)
    bad_abi[6] ^= 1
    rejects(
        lambda: compose.validate_module(
            bad_abi, "i2c", address, 4096,
            int(manifest["layout"]["abi_version"]),
        ),
        "ABI mismatch",
    )
    bad_crc = bytearray(data)
    bad_crc[-1] ^= 1
    rejects(
        lambda: compose.validate_module(
            bad_crc, "i2c", address, 4096,
            int(manifest["layout"]["abi_version"]),
        ),
        "CRC mismatch",
    )
    bad_deinit = bytearray(data)
    bad_deinit[20:24] = (address + 4).to_bytes(4, "little")
    rejects(
        lambda: compose.validate_module(
            bad_deinit, "i2c", address, 4096,
            int(manifest["layout"]["abi_version"]),
        ),
        "deinit",
    )
    rejects(
        lambda: compose.validate_module(
            data, "i2c", address + 4096, 4096,
            int(manifest["layout"]["abi_version"]),
        ),
        "outside slot",
    )
    source = inspect.getsource(compose)
    assert "subprocess" not in source and "arm-none-eabi" not in source
    print(
        f"CATALOG_TEST_OK modules={len(known)} slots={slot_count} "
        "abi crc address deinit capacity duplicate no-compiler"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
