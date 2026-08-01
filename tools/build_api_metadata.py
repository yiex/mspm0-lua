#!/usr/bin/env python3
"""Generate complete IDE metadata for the modular Lua firmware surface."""

import json
import re
from pathlib import Path

from build_catalog_release import catalog_records, catalog_sha256


ROOT = Path(__file__).resolve().parents[1] / "mspm0_lua"
OUTPUT = ROOT / "release" / "mspm0-lua.api.json"


def constraints(minimum=None, maximum=None, allowed=None, max_length=None):
    value = {}
    if minimum is not None:
        value["minimum"] = minimum
    if maximum is not None:
        value["maximum"] = maximum
    if allowed is not None:
        value["allowed_values"] = allowed
    if max_length is not None:
        value["max_length"] = max_length
    return value


def resource(kind, capability=None, signal=None, bindings=None):
    value = {"kind": kind, "scope": "board.exposed" if kind == "pin" else "chip.all"}
    if capability or signal:
        value["capability"] = {}
        if capability:
            value["capability"]["class"] = capability
        if signal:
            value["capability"]["signal"] = signal
    if bindings:
        value["bindings"] = bindings
    return value


def param(name, kind="integer", optional=False, default=None, limit=None,
          completion=None, owned_resource=None, variadic=False, description=None):
    value = {"name": name, "type": kind}
    if optional:
        value["optional"] = True
    if default is not None:
        value["default"] = default
    if limit:
        value["value_constraints"] = limit
    if completion:
        value["completion"] = completion
    if owned_resource:
        value["resource"] = owned_resource
    if variadic:
        value["variadic"] = True
    if description:
        value["description"] = description
    return value


def returned(kind, name=None, description=None):
    value = {"type": kind}
    if name:
        value["name"] = name
    if description:
        value["description"] = description
    return value


def effect(kind, target, lifetime="call", exclusive=False, notes=None):
    value = {"kind": kind, "target": target, "lifetime": lifetime}
    if exclusive:
        value["exclusive"] = True
    if notes:
        value["notes"] = notes
    return value


def function(module, name, params=None, returns=None, description=None,
             aliases=None, effects=None, blocking=False, errors=None):
    symbol = f"lua.{module + '.' if module else ''}{name}"
    value = {
        "name": name,
        "description": description or symbol,
        "overloads": [{
            "id": "default",
            "params": params or [],
            "returns": returns or [],
        }],
        "extensions": {
            "mspm0.symbol_id": symbol,
            "mspm0.blocking": blocking,
            "mspm0.errors": errors or ["argument"],
        },
    }
    if aliases:
        value["aliases"] = aliases
    if effects:
        value["overloads"][0]["effects"] = effects
    return value


PIN = lambda name, capability="gpio", signal=None, optional=False, default=None, bindings=None: param(
    name, "pin", optional=optional, default=default,
    completion={"kind": "resource", "quote": "double", "placeholder": f"{name} pin"},
    owned_resource=resource("pin", capability, signal, bindings),
)
BOOL = lambda name, optional=False, default=None: param(
    name, "boolean", optional=optional, default=default
)
def INT(name, minimum=None, maximum=None, optional=False, default=None,
        allowed=None):
    return param(
        name, "integer", optional=optional, default=default,
        limit=constraints(minimum, maximum, allowed) if
        (minimum is not None or maximum is not None or allowed is not None)
        else None,
    )


def STR(name, optional=False, default=None, allowed=None, max_length=None):
    return param(
        name, "string", optional=optional, default=default,
        limit=constraints(allowed=allowed, max_length=max_length) if
        (allowed is not None or max_length is not None) else None,
    )


def gpio_api():
    claim = [effect("claim", "pin", "until_release", True)]
    release = [effect("release", "pin", "call")]
    return [
        function("gpio", "mode", [PIN("pin"), STR("mode", True, "in", ["out", "od", "analog", "in", "in_pu", "in_pd"]), INT("option", 0, 3, True, 0), INT("feature", 0, 7, True, 0), BOOL("invert", True, False)], [], effects=claim, errors=["gpio:pin", "gpio:busy", "gpio:mode"]),
        function("gpio", "set", [PIN("pin"), INT("value", 0, 1)], [], aliases=["write"], effects=[effect("write", "pin")], errors=["gpio:pin", "gpio:owner"]),
        function("gpio", "od_write", [PIN("pin"), INT("release", 0, 1)], [], effects=[effect("write", "pin")]),
        function("gpio", "get", [PIN("pin")], [returned("integer", "level")], aliases=["read"], effects=[effect("read", "pin")]),
        function("gpio", "toggle", [PIN("pin")], [], effects=[effect("write", "pin")]),
        function("gpio", "af", [PIN("pin"), INT("pf", 0, 9), BOOL("input_enable", True, False)], [], effects=claim, errors=["gpio:pin", "gpio:busy", "gpio:pf"]),
        function("gpio", "release", [PIN("pin")], [], effects=release),
        function("gpio", "owner", [PIN("pin")], [returned("integer", "owner")]),
        function("gpio", "policy", [PIN("pin")], [returned("integer", "policy_bits")]),
        function("gpio", "valid", [STR("pin")], [returned("boolean")]),
    ]


def tmr_api():
    return [
        function("tmr", "start", [INT("id", 0, 3), INT("period_ms", 1, 0x7fffffff)], [returned("boolean")], effects=[effect("claim", "id", "until_release", True)]),
        function("tmr", "every", [INT("period_ms", 1, 0x7fffffff), param("callback", "function", optional=True)], [returned("integer", "id")], effects=[effect("claim", "id", "until_release", True)], errors=["tmr:period", "tmr:full"]),
        function("tmr", "ready", [INT("id", 0, 3)], [returned("boolean")], effects=[effect("read", "id")]),
        function("tmr", "take", [INT("id", 0, 3)], [returned("integer", "hits")], effects=[effect("read", "id")]),
        function("tmr", "stop", [INT("id", 0, 3)], [returned("boolean")], effects=[effect("release", "id")]),
        function("tmr", "millis", [], [returned("integer", "milliseconds")]),
        function("tmr", "delay", [INT("milliseconds", 0, 0x7fffffff)], [], blocking=True, errors=["STOP"]),
        function("tmr", "hw_start", [INT("timer", 0, 6), INT("ticks", 1, 0xffffffff), INT("prescale", 0, 255, True, 0), BOOL("periodic", True, True)], [returned("boolean")], effects=[effect("claim", "timer", "until_release", True)], errors=["tmr:id", "tmr:busy", "tmr:range"]),
        function("tmr", "hw_value", [INT("timer", 0, 6)], [returned("integer")]),
        function("tmr", "hw_ready", [INT("timer", 0, 6)], [returned("boolean")]),
        function("tmr", "hw_stop", [INT("timer", 0, 6)], [returned("boolean")], effects=[effect("release", "timer")]),
        function("tmr", "capture_open", [INT("timer", 0, 6), PIN("pin", "timer"), INT("edge", 0, 2, True, 0), INT("prescale", 0, 255, True, 0)], [returned("integer", "handle")], effects=[effect("claim", "timer", "handle", True), effect("claim", "pin", "handle", True)]),
        function("tmr", "capture_ready", [INT("handle", 0, 6)], [returned("boolean")]),
        function("tmr", "capture_read", [INT("handle", 0, 6)], [returned("integer", "ticks")]),
        function("tmr", "capture_close", [INT("handle", 0, 6), PIN("pin", "timer")], [returned("boolean")], effects=[effect("release", "handle"), effect("release", "pin")]),
        function("tmr", "route", [INT("timer", 0, 6), PIN("pin", "timer")], [returned("integer", "channel")]),
    ]


def event_api():
    return [
        function("event", "run", [], [], blocking=True, errors=["STOP", "callback error"]),
        function("event", "poll", [], [returned("integer", "callbacks_dispatched")], errors=["callback error"]),
        function("event", "stop", [], [], errors=[]),
    ]


def pwm_api():
    return [
        function("pwm", "open", [PIN("pin", "pwm"), INT("hz", 1, 1000000, True, 1000), INT("duty", 0, 100, True, 50), BOOL("center", True, False), BOOL("invert", True, False)], [returned("integer", "handle")], effects=[effect("claim", "pin", "handle", True), effect("claim", "timer", "handle", True)]),
        function("pwm", "open_on", [INT("timer", 0, 6), PIN("pin", "pwm"), INT("hz", 1, 1000000), INT("duty", 0, 100, True, 50), BOOL("center", True, False), BOOL("invert", True, False)], [returned("integer", "handle")], effects=[effect("claim", "pin", "handle", True), effect("claim", "timer", "handle", True)]),
        function("pwm", "duty", [INT("handle", 0, 6), INT("percent", 0, 100)], [returned("boolean")], effects=[effect("write", "handle")]),
        function("pwm", "close", [INT("handle", 0, 6), PIN("pin", "pwm")], [returned("boolean")], effects=[effect("release", "handle"), effect("release", "pin")]),
        function("pwm", "open_pair", [INT("timer", 0, 1), PIN("high_pin", "pwm"), PIN("low_pin", "pwm"), INT("hz", 1, 1000000), INT("duty", 0, 100, True, 50), INT("dead_ns", 0, 1000000, True, 0), BOOL("center", True, False)], [returned("integer", "handle")], effects=[effect("claim", "timer", "handle", True), effect("claim", "high_pin", "handle", True), effect("claim", "low_pin", "handle", True)]),
        function("pwm", "close_pair", [INT("handle", 0, 1), PIN("high_pin", "pwm"), PIN("low_pin", "pwm")], [returned("boolean")], effects=[effect("release", "handle"), effect("release", "high_pin"), effect("release", "low_pin")]),
        function("pwm", "route", [INT("timer", 0, 6), PIN("pin", "pwm")], [returned("integer", "channel")]),
    ]


def adc_api():
    common = [PIN("pin", "adc"), INT("sample_cycles", 4, 1024, True, 32), INT("averages", 1, 128, True, 1, [1, 2, 4, 8, 16, 32, 64, 128]), INT("bits", optional=True, default=12, allowed=[8, 10, 12])]
    return [
        function("adc", "channel", [PIN("pin", "adc")], [returned("integer", "channel")]),
        function("adc", "instance", [PIN("pin", "adc")], [returned("integer", "adc_id")]),
        function("adc", "read", common, [returned("integer", "code")], blocking=True, effects=[effect("claim", "pin", "call", True), effect("claim", "adc", "call", True)], errors=["adc:pin", "adc:busy", "adc:timeout"]),
        function("adc", "read_mv", [PIN("pin", "adc"), INT("vdda_mv", 1, 10000), *common[1:]], [returned("integer", "millivolts")], blocking=True, effects=[effect("claim", "pin", "call", True), effect("claim", "adc", "call", True)]),
        function("adc", "release", [PIN("pin", "adc")], [returned("boolean")], effects=[effect("release", "pin")]),
    ]


def i2c_api():
    hz = INT("hz", 10000, 1000000, True, 100000)
    short_addr = INT("address", 0, 1023)
    bus = param("id", "integer", limit=constraints(0, 1),
                owned_resource=resource("peripheral", "i2c", bindings={"peripheral": "bus"}))
    scl = PIN("scl", "i2c", "SCL", bindings={"peripheral": "bus"})
    sda = PIN("sda", "i2c", "SDA", bindings={"peripheral": "bus"})
    call_claim = [effect("claim", "id", "call", True), effect("claim", "scl", "call", True), effect("claim", "sda", "call", True)]
    return [
        function("i2c", "write", [short_addr, STR("data", max_length=4095), hz], [returned("boolean")], blocking=True, errors=["i2c:pin", "i2c:busy", "i2c:timeout", "i2c:nack"]),
        function("i2c", "read", [short_addr, INT("count", 0, 256), hz], [returned("string", "data")], blocking=True),
        function("i2c", "write_read", [INT("address", 0, 127), STR("write_data", max_length=4095), INT("read_count", 0, 256), hz], [returned("string", "data")], blocking=True),
        function("i2c", "write_on", [bus, scl, sda, short_addr, STR("data", max_length=4095), hz], [returned("boolean")], effects=call_claim, blocking=True),
        function("i2c", "read_on", [bus, scl, sda, short_addr, INT("count", 0, 256), hz], [returned("string", "data")], effects=call_claim, blocking=True),
        function("i2c", "write_read_on", [bus, scl, sda, INT("address", 0, 127), STR("write_data", max_length=4095), INT("read_count", 0, 256), hz], [returned("string", "data")], effects=call_claim, blocking=True),
        function("i2c", "probe_on", [bus, scl, sda, short_addr, hz], [returned("boolean")], effects=call_claim, blocking=True),
        function("i2c", "recover", [bus, scl, sda], [returned("boolean")], effects=call_claim, blocking=True),
        function("i2c", "valid", [bus, STR("scl"), STR("sda")], [returned("boolean")]),
        function("i2c", "bytes", [param("values", "integer", variadic=True, limit=constraints(0, 255))], [returned("string", "data")], errors=["i2c:byte"]),
    ]


def spi_api():
    hz = INT("hz", 1, 20000000, True, 1000000)
    mode = INT("mode", 0, 3, True, 0)
    lsb = BOOL("lsb_first", True, False)
    bus = param("id", "integer", limit=constraints(0, 1),
                owned_resource=resource("peripheral", "spi", bindings={"peripheral": "bus"}))
    pins = [
        PIN("sck", "spi", "SCK", bindings={"peripheral": "bus"}),
        PIN("pico", "spi", "PICO", bindings={"peripheral": "bus"}),
        PIN("poci", "spi", "POCI", bindings={"peripheral": "bus"}),
        PIN("cs", "gpio"),
    ]
    return [
        function("spi", "xfer", [PIN("cs", "gpio", optional=True, default="PA18"), STR("data", max_length=512), hz, mode, lsb], [returned("string", "data")], blocking=True, errors=["spi:size", "spi:config", "spi:timeout"]),
        function("spi", "xfer_on", [bus, *pins, STR("data", max_length=512), hz, mode, lsb], [returned("string", "data")], blocking=True, effects=[effect("claim", "id", "call", True), *[effect("claim", item["name"], "call", True) for item in pins]], errors=["spi:size", "spi:config", "spi:timeout"]),
        function("spi", "read_on", [bus, *pins, INT("count", 0, 512), INT("fill", 0, 255, True, 255), hz, mode, lsb], [returned("string", "data")], blocking=True, errors=["spi:size", "spi:config", "spi:timeout"]),
        function("spi", "valid", [INT("id", 0, 1), STR("sck"), STR("pico"), STR("poci")], [returned("boolean")]),
        function("spi", "bytes", [param("values", "integer", variadic=True, limit=constraints(0, 255))], [returned("string", "data")], errors=["spi:byte"]),
    ]


def uart_api():
    bus = param("id", "integer", limit=constraints(0, 3),
                owned_resource=resource("peripheral", "uart", bindings={"peripheral": "bus"}))
    tx = PIN("tx", "uart", "TX", True, bindings={"peripheral": "bus"})
    rx = PIN("rx", "uart", "RX", True, bindings={"peripheral": "bus"})
    return [
        function("uart", "open", [bus, tx, rx, INT("baud", 300, 4000000, True, 115200), INT("bits", 5, 8, True, 8), STR("parity", True, "none", ["none", "even", "odd"]), INT("stop", optional=True, default=1, allowed=[1, 2])], [returned("boolean")], effects=[effect("claim", "id", "until_release", True), effect("claim", "tx", "until_release", True), effect("claim", "rx", "until_release", True)], errors=["uart:id", "uart:pin", "uart:busy", "uart:format"]),
        function("uart", "close", [INT("id", 0, 3)], [returned("boolean")], effects=[effect("release", "id")]),
        function("uart", "tx", [INT("id", 0, 3), STR("data", max_length=4096)], [returned("integer", "bytes_written")], blocking=True),
        function("uart", "rx", [INT("id", 0, 3), INT("timeout_ms", 0, 60000, True, 0), INT("max_bytes", 1, 4096, True, 256)], [returned("string", "data")], blocking=True),
        function("uart", "valid", [INT("id", 0, 3), STR("tx"), STR("rx")], [returned("boolean")]),
    ]


def can_api():
    tx = PIN("tx", "can", "TX", True, bindings={"peripheral": "can"})
    rx = PIN("rx", "can", "RX", True, bindings={"peripheral": "can"})
    return [
        function("can", "open", [INT("bitrate", optional=True, default=500000, allowed=[125000, 250000, 500000, 1000000]), BOOL("loopback", True, False), tx, rx], [returned("boolean")], effects=[effect("claim", "can", "until_release", True), effect("claim", "tx", "until_release", True), effect("claim", "rx", "until_release", True)]),
        function("can", "open_on", [tx, rx, INT("bitrate", optional=True, default=500000, allowed=[125000, 250000, 500000, 1000000]), BOOL("loopback", True, False)], [returned("boolean")]),
        function("can", "close", [], [returned("boolean")], effects=[effect("release", "can")]),
        function("can", "send", [INT("id", 0, 0x1fffffff), STR("data", max_length=8), INT("timeout_ms", 0, 60000, True, 100), BOOL("extended", True, False)], [returned("boolean")], blocking=True),
        function("can", "recv", [INT("timeout_ms", 0, 60000, True, 0)], [returned("integer", "id"), returned("string", "data"), returned("boolean", "extended")], blocking=True),
        function("can", "valid", [STR("tx"), STR("rx")], [returned("boolean")]),
    ]


def analog_misc_api():
    return {
        "dac": [
            function("dac", "open", [INT("bits", optional=True, default=12, allowed=[8, 12]), INT("reference", 0, 3, True, 0), BOOL("external_pin_enable", True, True)], [returned("boolean")], effects=[effect("claim", "dac", "until_release", True), effect("claim", "PA15", "until_release", True)]),
            function("dac", "write", [INT("code", 0, 4095)], [returned("boolean")], effects=[effect("write", "dac")]),
            function("dac", "write_mv", [INT("millivolts", 0, 10000), INT("reference_mv", 1, 10000)], [returned("integer", "code")], effects=[effect("write", "dac")]),
            function("dac", "close", [], [returned("boolean")], effects=[effect("release", "dac"), effect("release", "PA15")]),
        ],
        "crc": [
            function("crc", "crc16", [STR("data"), INT("initial", 0, 0xffff, True, 0xffff)], [returned("integer")]),
            function("crc", "crc32", [STR("data"), INT("initial", optional=True, default=-1)], [returned("integer")]),
        ],
        "comp": [
            function("comp", "open", [INT("id", 0, 2), PIN("positive_pin", "comp"), PIN("negative_pin", "comp"), BOOL("fast", True, False), INT("hysteresis", 0, 3, True, 0), BOOL("invert", True, False)], [returned("boolean")], effects=[effect("claim", "id", "until_release", True), effect("claim", "positive_pin", "until_release", True), effect("claim", "negative_pin", "until_release", True)]),
            function("comp", "read", [INT("id", 0, 2)], [returned("boolean")]),
            function("comp", "close", [INT("id", 0, 2)], [returned("boolean")], effects=[effect("release", "id")]),
        ],
        "rtc": [
            function("rtc", "open", [], [returned("boolean")], effects=[effect("claim", "rtc", "until_release", True)]),
            function("rtc", "set", [INT("year", 1, 4095), INT("month", 1, 12), INT("day", 1, 31), INT("weekday", 0, 6), INT("hour", 0, 23), INT("minute", 0, 59), INT("second", 0, 59)], [returned("boolean")]),
            function("rtc", "get", [], [returned("integer", name) for name in ("year", "month", "day", "weekday", "hour", "minute", "second")]),
            function("rtc", "close", [], [returned("boolean")], effects=[effect("release", "rtc")]),
        ],
        "opa": [
            function("opa", "open", [INT("id", 0, 1), INT("psel", 0, 7, True, 0), INT("nsel", 0, 7, True, 0), INT("msel", 0, 3, True, 0), INT("gain", 0, 7, True, 0), BOOL("output", True, True), BOOL("chop", True, False), BOOL("high_gbw", True, True), BOOL("rri", True, False)], [returned("boolean")], effects=[effect("claim", "id", "until_release", True)]),
            function("opa", "ready", [INT("id", 0, 1)], [returned("boolean")]),
            function("opa", "close", [INT("id", 0, 1)], [returned("boolean")], effects=[effect("release", "id")]),
        ],
    }


def iq_api():
    iq = lambda name: INT(name, -0x80000000, 0x7fffffff)
    unary = lambda name, argument="value": function(
        "iq", name, [iq(argument)], [returned("integer", "iq16")]
    )
    return [
        unary("from", "integer"),
        unary("from_x10", "value_x10"),
        unary("from_x100", "value_x100"),
        unary("to_x10", "iq16"),
        unary("to_x100", "iq16"),
        unary("to_x1000", "iq16"),
        function("iq", "mul", [iq("a"), iq("b")], [returned("integer", "iq16")]),
        function("iq", "div", [iq("a"), iq("b")], [returned("integer", "iq16")]),
        unary("sin_deg", "degrees_x10"),
        unary("cos_deg", "degrees_x10"),
        function("iq", "atan2_deg", [iq("y"), iq("x")], [returned("integer", "degrees_x10")]),
    ]


def oled_api():
    """Public surface of the Lua OLED library shipped with the firmware.

    This is a Lua dependency rather than a native module.  Keeping it in the
    catalog makes its I2C route subject to the same board/chip solver as the
    native peripheral APIs.
    """
    bus = param("id", "integer", limit=constraints(0, 1),
                owned_resource=resource("peripheral", "i2c", bindings={"peripheral": "bus"}))
    scl = PIN("scl", "i2c", "SCL", bindings={"peripheral": "bus"})
    sda = PIN("sda", "i2c", "SDA", bindings={"peripheral": "bus"})
    return [
        function("oled", "open", [
            bus, scl, sda, INT("address", 0x08, 0x77),
            INT("hz", 10000, 1000000),
        ], [], errors=["oled:i2c_id", "oled:pins", "oled:address", "oled:hz", "oled:i2c_route", "oled:i2c_write"]),
        function("oled", "close", [], [], errors=[]),
        function("oled", "fill", [INT("value", 0, 255, True, 255)], []),
        function("oled", "clear", [], []),
        function("oled", "text", [
            INT("x", 0, 127), INT("y", 0, 63), STR("value"),
            INT("size", allowed=[16]),
        ], [], errors=["oled:position", "oled:text_bounds", "oled:text_not_rasterized"]),
        function("oled", "number", [
            INT("x", 0, 127), INT("y", 0, 63), INT("value"),
            INT("decimals", 0, 3), INT("size", allowed=[16]),
        ], [], errors=["oled:number", "oled:position", "oled:text_bounds"]),
    ]


def core_module_entry(module_id, functions, description):
    return {
        "name": module_id,
        "description": description,
        "functions": functions,
        "extensions": {"mspm0.core_resident": True},
    }


def lua_library_entry(module_id, functions, description, files):
    return {
        "name": module_id,
        "description": description,
        "functions": functions,
        "extensions": {
            "mspm0.lua_library": {
                "files": files,
                "uploaded_with_project": True,
            }
        },
    }


def compiler_injected_module_entry(module_id, functions, description, required_native_modules, trigger_symbols):
    return {
        "name": module_id,
        "description": description,
        "functions": functions,
        "extensions": {
            "mspm0.compiler_injected": {
                "runtime": "timer_event_dispatcher",
                "required_native_modules": required_native_modules,
                "trigger_symbols": trigger_symbols,
            }
        },
    }


def module_entry(module_id, functions, module_meta):
    return {
        "name": module_id,
        "description": module_meta["display_name"],
        "functions": functions,
        "availability": {"firmware_features_all": [f"native.{module_id}"]},
        "extensions": {
            "mspm0.native_module": {
                "id": module_id,
                "minimum_version": module_meta["version"],
                "dependencies": module_meta.get("dependencies", []),
                "conflicts": module_meta.get("conflicts", []),
                "resources": module_meta.get("resources", []),
            }
        },
    }


def c_reg_exports(path, table):
    source = path.read_text(encoding="utf-8")
    match = re.search(
        rf"static const (?:native_lua_reg_t|luaL_Reg) {re.escape(table)}\[\] = \{{(.*?)\n\}};",
        source,
        re.DOTALL,
    )
    if not match:
        raise SystemExit(f"API audit: missing C registration table {table} in {path}")
    return set(re.findall(r'\{\s*"([a-z0-9_]+)"\s*,', match.group(1)))


def audit_exports(functions, globals_api, module_meta):
    for module_id, api_functions in functions.items():
        source = ROOT / "modules" / module_meta[module_id]["source"]
        c_names = c_reg_exports(source, f"k_{module_id}_functions")
        if module_id == "tmr":
            c_names.add("every")
        api_names = {
            name
            for item in api_functions
            for name in (item["name"], *item.get("aliases", []))
        }
        if c_names != api_names:
            raise SystemExit(
                f"API audit {module_id}: C-only={sorted(c_names - api_names)} "
                f"metadata-only={sorted(api_names - c_names)}"
            )
    core_c = c_reg_exports(
        ROOT / "lua_bind" / "lua_bind_core.c", "k_core_globals"
    ) | {"print", "runfile", "require"}
    core_api = {item["name"] for item in globals_api}
    if core_c != core_api:
        raise SystemExit(
            f"API audit globals: C-only={sorted(core_c - core_api)} "
            f"metadata-only={sorted(core_api - core_c)}"
        )
    iq_c = c_reg_exports(
        ROOT / "lua_bind" / "lua_bind_core.c", "k_iq_functions"
    )
    iq_metadata = {item["name"] for item in iq_api()}
    if iq_c != iq_metadata:
        raise SystemExit(
            f"API audit iq: C-only={sorted(iq_c - iq_metadata)} "
            f"metadata-only={sorted(iq_metadata - iq_c)}"
        )


def main():
    modules_manifest, _, records = catalog_records()
    build_id = catalog_sha256(records)
    module_meta = {item["name"]: item for item in modules_manifest["modules"]}
    functions = {
        "gpio": gpio_api(), "tmr": tmr_api(), "pwm": pwm_api(),
        "adc": adc_api(), "i2c": i2c_api(), "spi": spi_api(),
        "uart": uart_api(), "can": can_api(), **analog_misc_api(),
    }
    globals_api = [
        function("", "print", [param("values", "any", variadic=True)], [], blocking=True, errors=[]),
        function("", "delay_ms", [INT("milliseconds", 0, 0x7fffffff)], [], blocking=True, errors=["STOP"]),
        function("", "millis", [], [returned("integer", "milliseconds")], errors=[]),
        function("", "yield", [], [], blocking=True, errors=["STOP"]),
        function("", "stopped", [], [returned("boolean")], errors=[]),
        function("", "byte", [STR("data", max_length=4096), INT("index", 1, 4096, True, 1)], [returned("integer", "value")], errors=[]),
        function("", "runfile", [STR("name", max_length=28)], [returned("boolean")], blocking=True, errors=["file name", "bytecode only", "file open", "file read"]),
        function("", "require", [STR("name", max_length=28)], [returned("any", "module")], blocking=True, errors=["module name", "file open", "file read", "module runtime error"]),
    ]
    audit_exports(functions, globals_api, module_meta)
    api = {
        "$schema": "https://schemas.mspm0-lua.dev/metadata/v1/api.schema.json",
        "schema_version": "1.0.0",
        "kind": "api",
        "id": "mspm0g3507.lua-modular",
        "version": modules_manifest["catalog"]["version"],
        "firmware": {
            "name": "MSPM0G3507 modular Lua firmware",
            "version": modules_manifest["catalog"]["firmware_version"],
            "abi": "mspm0g3507.lua-modular.abi7",
            "lua_version": "5.5.0",
            "features": [
                "serial.hex_upload", "serial.module_update",
                "external_littlefs", "multi_luac", "fwinfo",
                *[f"native.{name}" for name in functions],
            ],
            "build_id": build_id,
        },
        "compatibility": {
            "chip_ids": ["ti.mspm0g3507.lqfp48", "ti.mspm0g3507.lqfp64"]
        },
        "quality": {
            "status": "verified",
            "coverage": "complete",
            "reviewed_by": "firmware-source-audit",
            "reviewed_at": "2026-07-27",
            "notes": "Every modular-core global, native module export, and bundled Lua library is represented. Lua base-language builtins remain language metadata, not firmware extensions."
        },
        "types": [
            {"id": "pin", "kind": "alias", "lua_type": "string", "base": "string", "description": "PA0..PA31 or PB0..PB27, filtered by Chip and Board metadata."}
        ],
        "enums": [],
        "globals": globals_api,
        "modules": [
            core_module_entry("iq", iq_api(), "Resident Q16.16 fixed-point math"),
            compiler_injected_module_entry(
                "event", event_api(), "Compiler-injected timer callback dispatcher",
                ["tmr"], ["tmr.every", "event.run", "event.poll", "event.stop"],
            ),
            lua_library_entry(
                "oled", oled_api(), "Bundled SSD1306 OLED Lua library",
                ["oled.lua", "_oled_font.lua"],
            ),
            *[module_entry(name, functions[name], module_meta[name]) for name in functions],
        ],
        "diagnostics": [
            {"code": "API001", "severity": "error", "message": "API symbol is not present in the selected firmware catalog."},
            {"code": "MOD001", "severity": "error", "message": "Required native module is not selected."},
            {"code": "PIN004", "severity": "error", "message": "Pins do not share the requested peripheral route."}
        ],
        "provenance": {
            "sources": [
                {"title": "mspm0_lua modular C binding tables", "revision": build_id},
                {"title": "TI MSPM0 SDK DriverLib", "revision": "2.11.00.07"}
            ],
            "license": "Project firmware metadata, distributed with the firmware catalog",
            "generated_by": "tools/build_api_metadata.py"
        },
        "extensions": {
            "mspm0.catalog_sha256": build_id,
            "mspm0.module_format": 2,
            "mspm0.nmup_format": 1,
            "mspm0.runtime_model": {
                "callback_context": "Lua main context only",
                "interrupt_context": "ISRs never enter Lua",
                "native_module_limit": 8,
                "vm_rebuilt_after_module_update": True,
            },
        },
    }
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(
        json.dumps(api, indent=2, ensure_ascii=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"API_METADATA {OUTPUT} modules={len(functions)} functions={sum(len(v) for v in functions.values()) + len(globals_api)} build={build_id}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
