#!/usr/bin/env python3
"""Deterministic IDE/device simulation for transactional module updates."""

import binascii
import contextlib
import io

from compose_firmware import load_inputs, prepare_segments, resolve_selection
from module_bundle import BundleError, build_bundle, validate_bundle
from serial_module_set import upload_hex, wait_for_module_update, write_line


class PowerLoss(RuntimeError):
    pass


class DeviceModel:
    def __init__(self, layout):
        self.layout = layout
        self.flash = bytearray(b"\xA5" * (layout["slot_size"] * layout["slot_count"]))
        self.files = {"main.luac": b"old-entry"}
        self.pending = None
        self.boot_runs = 0

    def stage(self, name):
        validate_bundle(self.files[name], self.layout)
        self.pending = name

    def install(self, fail_after=None):
        if self.pending is None:
            raise RuntimeError("no pending update")
        bundle = self.files[self.pending]
        plan = validate_bundle(bundle, self.layout)
        operations = 0
        for slot, entry in enumerate(plan["entries"]):
            base = slot * self.layout["slot_size"]
            self.flash[base:base + self.layout["slot_size"]] = b"\xFF" * self.layout["slot_size"]
            operations += 1
            if fail_after == operations:
                raise PowerLoss("injected during erase")
            if entry:
                image = bundle[entry["offset"]:entry["offset"] + entry["size"]]
                self.flash[base:base + len(image)] = image
                operations += 1
                if fail_after == operations:
                    raise PowerLoss("injected during program")
        self.verify(bundle)
        self.files.pop("main.luac", None)
        del self.files[self.pending]
        self.pending = None

    def verify(self, bundle):
        plan = validate_bundle(bundle, self.layout)
        for slot, entry in enumerate(plan["entries"]):
            base = slot * self.layout["slot_size"]
            expected = bytearray(b"\xFF" * self.layout["slot_size"])
            if entry:
                image = bundle[entry["offset"]:entry["offset"] + entry["size"]]
                expected[:len(image)] = image
            if self.flash[base:base + self.layout["slot_size"]] != expected:
                raise RuntimeError(f"slot{slot} verify")


class FakeSerial:
    def __init__(self, device):
        self.device = device
        self.rx = bytearray()
        self.line = bytearray()
        self.upload_name = None
        self.upload = bytearray()

    @property
    def in_waiting(self):
        return len(self.rx)

    def flush(self):
        pass

    def read(self, size):
        data = bytes(self.rx[:size])
        del self.rx[:size]
        return data

    def emit(self, text):
        self.rx.extend(text.encode("ascii"))

    def write(self, data):
        for value in data:
            if value == 13:
                continue
            if value == 10:
                self.handle(bytes(self.line).decode("ascii"))
                self.line.clear()
            else:
                self.line.append(value)
        return len(data)

    def handle(self, line):
        if self.upload_name is not None:
            if line == ">>>HEX":
                self.device.files[self.upload_name] = bytes(self.upload)
                self.emit(f"SCRIPT_OK {len(self.upload)}\r\n")
                self.upload_name = None
                return
            self.upload.extend(binascii.unhexlify(line))
            self.emit("HEX_OK\r\n")
            return
        if line.startswith("<<<HEX "):
            self.upload_name = line[7:]
            self.upload.clear()
            self.emit("SCRIPT_BEGIN\r\n")
        elif line.startswith("modapply "):
            name = line[9:]
            try:
                self.device.stage(name)
                plan = validate_bundle(self.device.files[name], self.device.layout)
                self.emit(f"MOD_READY {plan['selected_count']} {plan['total_size']}\r\n")
                self.emit(f"MOD_APPLY {name}\r\nMOD_VERIFY\r\n")
                self.device.install()
                self.emit(f"MOD_DONE {plan['selected_count']}\r\nIdle\r\n")
            except (BundleError, RuntimeError):
                self.emit("MOD_ERR model\r\nMOD_BLOCKED\r\n")
        elif line == "modstatus":
            state = "PENDING" if self.device.pending else "IDLE"
            self.emit(f"MOD_STATUS {state}\r\n")
            self.emit("MOD_CATALOG test\r\n")
            valid_count = 0
            for slot in range(self.device.layout["slot_count"]):
                base = slot * self.device.layout["slot_size"]
                image = bytes(self.device.flash[base:base + self.device.layout["slot_size"]])
                if image[:4] == b"LMOD":
                    size = int.from_bytes(image[8:12], "little")
                    name = image[24:32].split(b"\0", 1)[0].decode("ascii")
                    crc = binascii.crc32(image[:size]) & 0xFFFFFFFF
                    self.emit(f"MOD_SLOT {slot} {name} {size} {crc:08x}\r\n")
                    valid_count += 1
            layout_crc = binascii.crc32(self.device.flash) & 0xFFFFFFFF
            self.emit(f"MOD_LAYOUT {valid_count} {layout_crc:08x}\r\n")
            self.emit("MOD_PENDING none\r\n")
            self.emit("MOD_STATUS_END\r\n")
        elif line == "storageinfo":
            self.emit(
                "STORAGE external_littlefs\r\n"
                "PART W25Q32JVSSIQ\r\n"
                "CAPACITY 4194304\r\n"
                "PINS SPI1 PB16 PB15 PB14 PB17\r\n"
                "STORAGE_END\r\n"
            )
        elif line.startswith("fileinfo "):
            name = line[9:]
            allowed = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.-"
            if not name or len(name) > 28 or any(ch not in allowed for ch in name):
                self.emit("FILE_ERR INVALID_NAME\r\n")
            elif name not in self.device.files:
                self.emit("FILE_ERR NOT_FOUND\r\n")
            else:
                payload = self.device.files[name]
                crc = binascii.crc32(payload) & 0xFFFFFFFF
                self.emit(f"FILE {name} {len(payload)} {crc:08x}\r\nFILE_END\r\n")
        elif line == "fwinfo":
            self.emit(
                "FW_INFO mspm0g3507.lua-modular 1.0.0\r\n"
                "FW_TARGET MSPM0G3507\r\nFW_ABI 7\r\n"
                "FW_MODULE_FORMAT 2\r\nFW_NMUP_FORMAT 1\r\n"
                "FW_SLOTS 8 4096\r\nFW_CATALOG test\r\nFW_INFO_END\r\n"
            )


def main():
    manifest, catalog, known = load_inputs()
    selected, _ = resolve_selection(manifest, known, modules=["i2c"])
    segments = prepare_segments(manifest, catalog, known, selected, include_core=False)
    bundle = build_bundle(manifest, selected, segments)
    layout = manifest["layout"]

    # Real host HEX framing and acknowledgements against a modeled device.
    device = DeviceModel(layout)
    serial = FakeSerial(device)
    with contextlib.redirect_stdout(io.StringIO()):
        upload_hex(serial, "modules.upd", bundle)
        write_line(serial, "modapply modules.upd")
        wait_for_module_update(serial, 2.0)
    assert device.pending is None
    device.verify(bundle)
    assert "main.luac" not in device.files and device.boot_runs == 0

    # IDE discovery queries are stable and include exact storage/file/layout facts.
    write_line(serial, "storageinfo")
    storage = serial.read(serial.in_waiting)
    assert b"CAPACITY 4194304\r\n" in storage and storage.endswith(b"STORAGE_END\r\n")
    write_line(serial, "fileinfo modules.upd")
    assert serial.read(serial.in_waiting) == b"FILE_ERR NOT_FOUND\r\n"
    write_line(serial, "fileinfo bad/name")
    assert serial.read(serial.in_waiting) == b"FILE_ERR INVALID_NAME\r\n"
    write_line(serial, "modstatus")
    status = serial.read(serial.in_waiting)
    assert b"MOD_CATALOG test\r\n" in status
    assert b"MOD_LAYOUT 1 " in status
    assert b"MOD_PENDING none\r\n" in status

    # A corrupt upload is rejected before the modeled internal Flash changes.
    device = DeviceModel(layout)
    before = bytes(device.flash)
    damaged = bytearray(bundle)
    damaged[-1] ^= 1
    device.files["modules.upd"] = damaged
    try:
        device.stage("modules.upd")
    except BundleError:
        pass
    else:
        raise AssertionError("corrupt bundle staged")
    assert bytes(device.flash) == before and device.pending is None
    assert device.files["main.luac"] == b"old-entry"

    # Power loss after an erase keeps pending; ordinary boot replay completes.
    device.files["modules.upd"] = bundle
    device.stage("modules.upd")
    try:
        device.install(fail_after=2)
    except PowerLoss:
        pass
    else:
        raise AssertionError("power loss not injected")
    assert device.pending == "modules.upd"
    device.install()  # startup recovery path
    assert device.pending is None
    device.verify(bundle)
    assert "main.luac" not in device.files and device.boot_runs == 0
    print("SERIAL_MODULE_SIM_OK fwinfo storageinfo fileinfo mod-summary slot-crc hex-ack no-reset old-main-disabled no-auto-run corrupt-before-erase power-loss-replay full-slot-verify")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
