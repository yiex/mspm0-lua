#!/usr/bin/env python3
"""Generate the production Chip/Board/API registry for the Dimengxing board."""

from __future__ import annotations

import copy
import json
import re
from pathlib import Path


IDE_ROOT = Path(__file__).resolve().parents[1]
FIRMWARE_ROOT = IDE_ROOT.parent / "mspm0_lua"
CAPABILITIES = FIRMWARE_ROOT / "docs" / "MSPM0G3507_64PIN_CAPABILITIES.json"
RELEASE_API = FIRMWARE_ROOT / "release" / "mspm0-lua.api.json"
CHIP_OUT = IDE_ROOT / "metadata" / "chips" / "ti" / "mspm0g3507-lqfp48.chip.json"
BOARD_OUT = IDE_ROOT / "metadata" / "boards" / "dimengxing" / "mspm0g3507-v1.board.json"
API_OUT = IDE_ROOT / "metadata" / "apis" / "mspm0" / "mspm0-lua-modular.api.json"


# MSPM0G3507 PT package positions from data sheet SLASEX6C, Table 6-2.
PACKAGE_POSITIONS = {
    "PA0": 1, "PA1": 2, "PA28": 3, "PA31": 5,
    "PA2": 8, "PA3": 9, "PA4": 10, "PA5": 11, "PA6": 12,
    "PA7": 13, "PB2": 14, "PB3": 15, "PA8": 16, "PA9": 17,
    "PA10": 18, "PA11": 19, "PB6": 20, "PB7": 21, "PB8": 22,
    "PB9": 23, "PB14": 24, "PB15": 25, "PB16": 26,
    "PA12": 27, "PA13": 28, "PA14": 29, "PA15": 30,
    "PA16": 31, "PA17": 32, "PA18": 33, "PA19": 34,
    "PA20": 35, "PB17": 36, "PB18": 37, "PB19": 38,
    "PA21": 39, "PA22": 40, "PB20": 41, "PB24": 42,
    "PA23": 43, "PA24": 44, "PA25": 45, "PA26": 46, "PA27": 47,
}


# Analog/system functions bonded on the PT package. These are normalized facts,
# not copied descriptions or table presentation from the vendor data sheet.
ANALOG_FUNCTIONS = {
    "PA2": ["ROSC"],
    "PA3": ["LFXIN"],
    "PA4": ["LFCLK_IN", "LFXOUT"],
    "PA5": ["HFXIN"],
    "PA6": ["HFCLK_IN", "HFXOUT"],
    "PA13": ["COMP0_IN2-"],
    "PA14": ["COMP0_IN2+", "A0_12"],
    "PA15": ["A1_0", "DAC_OUT", "OPA0_IN2+", "OPA1_IN2+", "COMP0_IN3+", "COMP1_IN3+"],
    "PA16": ["A1_1", "OPA1_OUT"],
    "PA17": ["A1_2", "OPA1_IN1-", "COMP0_IN1-"],
    "PA18": ["A1_3", "OPA1_IN1+", "COMP0_IN1+", "GPAMP_IN-"],
    "PA21": ["A1_7", "COMP2_IN1-", "VREF-"],
    "PA22": ["A0_7", "GPAMP_OUT", "OPA0_OUT"],
    "PA23": ["COMP1_IN1-", "VREF+"],
    "PA24": ["A0_3", "OPA0_IN1-"],
    "PA25": ["A0_2", "OPA0_IN1+"],
    "PA26": ["A0_1", "COMP0_IN0+", "OPA0_IN0+", "GPAMP_IN+"],
    "PA27": ["A0_0", "COMP0_IN0-", "OPA0_IN0-"],
    "PB17": ["A1_4", "COMP1_IN2-"],
    "PB18": ["A1_5", "COMP1_IN2+"],
    "PB19": ["A1_6", "COMP2_IN1+", "OPA1_IN0+"],
    "PB20": ["A0_6", "OPA1_IN0-"],
    "PB24": ["A0_5", "COMP1_IN1+"],
}


HEADER_PINS = {
    "PA0", "PA1", "PA2", "PA7", "PA8", "PA9", "PA12", "PA13",
    "PA14", "PA15", "PA16", "PA17", "PA18", "PA21", "PA22",
    "PA23", "PA24", "PA25", "PA26", "PA27", "PA28", "PA31",
    "PB3", "PB6", "PB7", "PB8", "PB9", "PB17", "PB18", "PB19",
    "PB20", "PB24",
}
FIXED_PINS = {"PA3", "PA4", "PA5", "PA6", "PA10", "PA11", "PA19", "PA20", "PB14", "PB15", "PB16"}
LOCKED_PINS = {"PA2", "PA3", "PA4", "PA5", "PA6", "PA10", "PA11", "PA19", "PA20", "PB14", "PB15", "PB16", "PB17"}


def clean_id(value: str) -> str:
    value = re.sub(r"[^a-z0-9]+", ".", value.lower()).strip(".")
    return value or "unknown"


def direction(signal: str) -> str:
    if signal.endswith(("_TX", "_RTS", "_OUT", "_PICO")):
        return "output"
    if signal.endswith(("_RX", "_CTS", "_IN", "_POCI")):
        return "input"
    return "bidirectional"


def add_peripheral(peripherals: dict[str, dict], peripheral: str, cls: str, signal: str) -> None:
    match = re.search(r"(\d+)$", peripheral)
    instance = int(match.group(1)) if match else 0
    spec = peripherals.setdefault(
        peripheral,
        {"id": peripheral, "class": cls, "instance": instance, "signals": set()},
    )
    spec["signals"].add(signal)


def digital_capabilities(pin: str, values: dict, peripherals: dict[str, dict]) -> list[dict]:
    port = pin[1]
    number = int(pin[2:])
    caps = [{
        "id": f"{pin.lower()}.gpio",
        "function": f"GPIO{port}_DIO{number:02d}",
        "class": "gpio",
        "mode": "gpio",
        "direction": "bidirectional",
        "selector": 1,
    }]
    for item in values.get("functions", []):
        function = item["signal"]
        selector = item["pf"]
        specs: list[tuple[str, str | None, str | None]] = []
        if match := re.fullmatch(r"UART(\d+)_(TX|RX|CTS|RTS)", function):
            specs.append(("uart", f"UART{match.group(1)}", match.group(2)))
        elif match := re.fullmatch(r"I2C(\d+)_(SCL|SDA)", function):
            specs.append(("i2c", f"I2C{match.group(1)}", match.group(2)))
        elif match := re.fullmatch(r"SPI(\d+)_(.+)", function):
            peripheral = f"SPI{match.group(1)}"
            raw_signal = match.group(2)
            signals = []
            if raw_signal in {"SCLK", "SCK"}:
                signals.append("SCK")
            if "PICO" in raw_signal:
                signals.append("PICO")
            if "POCI" in raw_signal:
                signals.append("POCI")
            if cs := re.search(r"CS([0-3])", raw_signal):
                signals.append(f"CS{cs.group(1)}")
            for signal in dict.fromkeys(signals):
                specs.append(("spi", peripheral, signal))
        elif match := re.fullmatch(r"(TIMA\d+|TIMG\d+)_(.+)", function):
            peripheral = match.group(1)
            signal = match.group(2)
            specs.extend([("timer", peripheral, signal), ("pwm", peripheral, signal)])
        elif match := re.fullmatch(r"CAN(?:FD)?(\d+)_CAN(TX|RX)", function):
            specs.append(("can", f"CAN{match.group(1)}", match.group(2)))
        elif match := re.fullmatch(r"COMP(\d+)_OUT", function):
            specs.append(("comp", f"COMP{match.group(1)}", "OUT"))
        elif function == "RTC_RTC_OUT":
            specs.append(("rtc", "RTC0", "OUT"))
        else:
            specs.append(("system", None, None))

        for ordinal, (cls, peripheral, signal) in enumerate(specs):
            cap = {
                "id": f"{pin.lower()}.{clean_id(function)}.{cls}.{ordinal}",
                "function": function,
                "class": cls,
                "mode": "alternate" if cls != "system" else "system",
                "direction": direction(function),
                "selector": selector,
            }
            if peripheral and signal:
                cap.update({"peripheral": peripheral, "signal": signal, "route": peripheral.lower()})
                add_peripheral(peripherals, peripheral, "timer" if cls == "pwm" else cls, signal)
            caps.append(cap)
    return caps


def analog_capabilities(pin: str, peripherals: dict[str, dict]) -> list[dict]:
    caps = []
    for ordinal, function in enumerate(ANALOG_FUNCTIONS.get(pin, [])):
        peripheral = None
        signal = None
        if match := re.fullmatch(r"A([01])_(\d+)", function):
            cls, peripheral, signal = "adc", f"ADC{match.group(1)}", f"CH{match.group(2)}"
        elif match := re.fullmatch(r"COMP(\d+)_(.+)", function):
            cls, peripheral, signal = "comp", f"COMP{match.group(1)}", match.group(2)
        elif match := re.fullmatch(r"OPA(\d+)_(.+)", function):
            cls, peripheral, signal = "opa", f"OPA{match.group(1)}", match.group(2)
        elif function.startswith("GPAMP_"):
            cls, peripheral, signal = "gpamp", "GPAMP0", function.removeprefix("GPAMP_")
        elif function == "DAC_OUT":
            cls, peripheral, signal = "dac", "DAC0", "OUT"
        elif function.startswith("VREF"):
            cls, peripheral, signal = "vref", "VREF0", function.removeprefix("VREF")
        else:
            cls = "system"
        cap = {
            "id": f"{pin.lower()}.{clean_id(function)}.analog.{ordinal}",
            "function": function,
            "class": cls,
            "mode": "analog" if cls != "system" else "system",
            "direction": "analog" if cls != "system" else "none",
            "selector": 0,
        }
        if peripheral and signal:
            cap.update({"peripheral": peripheral, "signal": signal, "route": peripheral.lower()})
            add_peripheral(peripherals, peripheral, cls, signal)
        caps.append(cap)
    return caps


def build_chip(source: dict) -> dict:
    peripherals: dict[str, dict] = {}
    pins = []
    for pin, position in sorted(PACKAGE_POSITIONS.items(), key=lambda pair: pair[1]):
        caps = digital_capabilities(pin, source["pins"][pin], peripherals)
        caps.extend(analog_capabilities(pin, peripherals))
        pins.append({
            "id": pin,
            "package_positions": [str(position)],
            "electrical": {"input": True, "output": True, "analog": pin in ANALOG_FUNCTIONS},
            "capabilities": caps,
        })
    peripheral_list = []
    for spec in sorted(peripherals.values(), key=lambda value: value["id"]):
        spec = copy.deepcopy(spec)
        spec["signals"] = sorted(spec["signals"])
        peripheral_list.append(spec)
    return {
        "$schema": "../../docs/metadata-standard/schemas/chip.schema.json",
        "schema_version": "1.0.0",
        "kind": "chip",
        "id": "ti.mspm0g3507.lqfp48",
        "version": "1.0.0",
        "vendor": "Texas Instruments",
        "family": "mspm0g",
        "model": "MSPM0G3507",
        "display_name": "MSPM0G3507SPT (LQFP-48)",
        "package": {"id": "pt", "name": "LQFP-48", "pin_count": 48, "variant": "SPT"},
        "naming": {"pin_pattern": "^P[AB](?:[0-9]|[12][0-9]|3[01])$", "case_sensitive": False, "canonical_case": "upper"},
        "quality": {"status": "verified", "coverage": "complete", "reviewed_by": "project source and data-sheet audit", "reviewed_at": "2026-07-27"},
        "features": ["gpio", "adc", "can", "comp", "dac", "i2c", "opa", "pwm", "spi", "timer", "uart"],
        "peripherals": peripheral_list,
        "pins": pins,
        "constraints": [{
            "id": "pt.package.only",
            "kind": "package",
            "severity": "error",
            "members": sorted(PACKAGE_POSITIONS),
            "message": "Only GPIOs bonded on the MSPM0G3507 PT package are present.",
        }],
        "provenance": {
            "sources": [
                {"title": "MSPM0G350x data sheet pin attributes", "revision": "SLASEX6C"},
                {"title": "TI MSPM0G350x device header PINCM definitions", "revision": "local MSPM0 SDK snapshot"},
            ],
            "license": "CC0-1.0 for project-authored factual normalization; source documents retain their original rights",
            "generated_by": "gpui_ide/tools/generate_target_metadata.py",
        },
    }


def board_pin_id(pin: str) -> str:
    return f"header.{pin.lower()}" if pin in HEADER_PINS else f"onboard.{pin.lower()}"


def build_board() -> dict:
    pins = []
    for pin in sorted(HEADER_PINS | FIXED_PINS, key=lambda value: PACKAGE_POSITIONS[value]):
        preference = 10
        if pin in {"PA15", "PA16"}:
            preference = 40
        elif pin in {"PA17", "PA18", "PA12", "PA13", "PA14", "PA23", "PA24", "PA26", "PA27"}:
            preference = 20
        item = {
            "id": board_pin_id(pin),
            "name": pin,
            "chip_pin": pin,
            "available": pin in HEADER_PINS and pin not in LOCKED_PINS,
            "preference": preference,
        }
        if pin == "PA14":
            item["aliases"] = ["LED"]
            item["notes"] = "Board LED load; usable after explicitly claiming the pin."
        elif pin == "PA18":
            item["notes"] = "Exposed but has bootloader-entry risk during reset."
        pins.append(item)

    def connection(signal: str, pin: str, function: str | None = None, releasable: bool = False) -> dict:
        value = {
            "signal": signal,
            "chip_pin": pin,
            "board_pin": board_pin_id(pin),
            "exclusive": True,
            "releasable": releasable,
        }
        if function:
            value["function"] = function
        return value

    reserved = []
    reasons = {
        "PA2": "ROSC resistor", "PA3": "LFXT input", "PA4": "LFXT output",
        "PA5": "HFXT input", "PA6": "HFXT output", "PA10": "CH340 application console TX",
        "PA11": "CH340 application console RX", "PA19": "SWDIO", "PA20": "SWCLK",
        "PB14": "W25Q32 POCI", "PB15": "W25Q32 PICO", "PB16": "W25Q32 SCK", "PB17": "W25Q32 chip select",
    }
    for pin, reason in reasons.items():
        reserved.append({"chip_pin": pin, "reason": reason, "severity": "error", "releasable": False})
    reserved.append({"chip_pin": "PA18", "reason": "Bootloader invocation risk during reset", "severity": "warning", "releasable": True})

    return {
        "$schema": "../../docs/metadata-standard/schemas/board.schema.json",
        "schema_version": "1.0.0",
        "kind": "board",
        "id": "dimengxing.mspm0g3507.v1",
        "version": "1.0.0",
        "vendor": "Dimengxing",
        "name": "Dimengxing MSPM0G3507 core board",
        "revision": "verified-2026-07",
        "display_name": "Dimengxing MSPM0G3507 (LQFP-48)",
        "chip_ref": {"id": "ti.mspm0g3507.lqfp48", "version": "^1.0.0"},
        "compatibility": {"api_ids": ["mspm0g3507.lua-modular"], "firmware_abis": ["mspm0g3507.lua-modular.abi7"]},
        "features": ["external_littlefs", "serial_script_upload", "native_module_slots"],
        "quality": {"status": "verified", "coverage": "complete", "reviewed_by": "schematic and firmware pin-policy audit", "reviewed_at": "2026-07-27"},
        "pins": pins,
        "onboard_devices": [
            {"id": "ch340", "kind": "usb_uart_bridge", "name": "CH340 application console", "connections": [
                connection("MCU_TX", "PA10", "UART0_TX"), connection("MCU_RX", "PA11", "UART0_RX"),
            ]},
            {"id": "w25q32", "kind": "external_flash", "name": "W25Q32 SPI NOR", "part": "W25Q32JVSSIQ", "connections": [
                connection("POCI", "PB14", "SPI1_POCI"), connection("PICO", "PB15", "SPI1_PICO"),
                connection("SCK", "PB16", "SPI1_SCLK"), connection("CS", "PB17", "SPI1_CS1_POCI1"),
            ]},
            {"id": "status-led", "kind": "led", "name": "User LED", "connections": [connection("ANODE", "PA14", releasable=True)]},
        ],
        "reserved_pins": reserved,
        "memory_devices": [{
            "id": "script-flash", "kind": "external_spi_nor", "name": "W25Q32 script flash",
            "part": "W25Q32JVSSIQ", "capacity_bytes": 4194304, "erase_block_bytes": 4096,
            "writable": True,
            "interface": {"kind": "spi", "peripheral": "SPI1", "route": "spi1", "device": "w25q32"},
            "filesystem": {"kind": "littlefs", "mount_point": "/", "format_on_first_use": True},
        }],
        "artifact_targets": [{
            "id": "lua-bytecode-default", "kind": "lua_bytecode", "storage": "script-flash",
            "base_path": "/", "filename_template": "{module}.luac", "runtime_path_template": "/{module}.luac",
            "upload_strategy": "serial_protocol", "priority": 100,
            "required_features": ["external_littlefs"],
        }],
        "provenance": {
            "sources": [
                {"title": "Dimengxing board schematic", "revision": "local schematic 2026-07-18"},
                {"title": "Firmware board pin policy and validated driver routes", "revision": "workspace snapshot 2026-07-27"},
            ],
            "license": "CC0-1.0 for project-authored factual normalization; schematic and component documentation retain their original rights",
            "generated_by": "gpui_ide/tools/generate_target_metadata.py",
        },
    }


def resource_param(overload: dict, name: str) -> dict | None:
    return next((item for item in overload.get("params", []) if item.get("name") == name), None)


def bind_bus_and_pins(overload: dict, cls: str, bus: str, pins: list[str]) -> None:
    bus_param = resource_param(overload, bus)
    if bus_param is not None:
        bus_param["resource"] = {
            "kind": "peripheral", "scope": "chip.all", "capability": {"class": cls},
            "bindings": {"id": f"{cls}_instance"},
        }
    for pin_name in pins:
        pin = resource_param(overload, pin_name)
        if pin is not None and "resource" in pin:
            pin["resource"]["bindings"] = {
                "peripheral": f"{cls}_instance", "route": f"{cls}_route",
            }


def build_api() -> dict:
    api = json.loads(RELEASE_API.read_text(encoding="utf-8"))
    quality = api.setdefault("quality", {})
    quality["reviewed_at"] = "2026-07-27"
    quality["notes"] = "IDE metadata patch: adds structured peripheral/route bindings without changing firmware ABI or callable symbols."
    for module in api.get("modules", []):
        name = module.get("name")
        for function in module.get("functions", []):
            overloads = function.get("overloads", [])
            if not overloads:
                continue
            overload = overloads[0]
            if name == "uart" and function.get("name") == "open":
                bind_bus_and_pins(overload, "uart", "id", ["tx", "rx"])
            elif name == "i2c" and function.get("name") in {"write_on", "read_on", "write_read_on", "probe_on", "recover"}:
                bind_bus_and_pins(overload, "i2c", "id", ["scl", "sda"])
            elif name == "spi" and function.get("name") in {"xfer_on", "read_on"}:
                bind_bus_and_pins(overload, "spi", "id", ["sck", "pico", "poci"])
            elif name == "can" and function.get("name") in {"open", "open_on"}:
                for pin_name in ["tx", "rx"]:
                    pin = resource_param(overload, pin_name)
                    if pin is not None and "resource" in pin:
                        pin["resource"]["bindings"] = {"peripheral": "can_instance", "route": "can_route"}
    provenance = api.setdefault("provenance", {})
    provenance["generated_by"] = (
        "gpui_ide/tools/generate_target_metadata.py from firmware release API "
        + api["version"]
    )
    return api


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    source = json.loads(CAPABILITIES.read_text(encoding="utf-8"))
    write_json(CHIP_OUT, build_chip(source))
    write_json(BOARD_OUT, build_board())
    write_json(API_OUT, build_api())
    print(f"METADATA_GENERATED chip={CHIP_OUT} board={BOARD_OUT} api={API_OUT}")


if __name__ == "__main__":
    main()
