"""Create the portable IDE metadata files from the reviewed detailed registry.

This is a one-time mechanical migration helper. Runtime code only reads the
flat chips/, boards/ and apis/ directories.
"""

from __future__ import annotations

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIRMWARE_ROOT = ROOT.parent / "mspm0_lua"


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )


def simplify_param(param: dict) -> dict:
    result = {"name": param["name"], "type": param["type"]}
    if param.get("optional"):
        result["optional"] = True
    if "resource" in param:
        result["resource"] = param["resource"]
    return result


def simplify_function(function: dict) -> dict:
    result = {"name": function["name"]}
    if function.get("description"):
        result["description"] = function["description"]
    result["overloads"] = []
    for overload in function["overloads"]:
        item = {"params": [simplify_param(p) for p in overload["params"]]}
        if overload.get("returns"):
            item["returns"] = [
                {key: value for key, value in returned.items() if key in {"name", "type"}}
                for returned in overload["returns"]
            ]
        result["overloads"].append(item)
    return result


def main() -> None:
    capability_data = read_json(
        FIRMWARE_ROOT / "docs/MSPM0G3507_64PIN_CAPABILITIES.json"
    )
    pins = {}
    for pin, entry in capability_data["pins"].items():
        gpio = f"GPIO{pin[1]}_DIO{int(pin[2:]):02d}"
        pins[pin] = sorted({gpio, *[item["signal"] for item in entry["functions"]]})
    for route in capability_data.get("adc_routes", []):
        pins[route["pin"]].append(f"A{route['instance']}_{route['channel']}")
        pins[route["pin"]].sort()
    write_json(ROOT / "chips/mspm0g3507.json", {"name": "mspm0g3507", "pins": pins})

    old_board = read_json(
        ROOT / "metadata/boards/dimengxing/mspm0g3507-v1.board.json"
    )
    exposed = [pin["name"] for pin in old_board["pins"] if pin.get("available")]
    board = {
        "name": "地猛星",
        "chip": "mspm0g3507",
        "pins": exposed,
        "flash": {
            "name": "W25Q32JVSSIQ",
            "storage": "external",
            "luac": True,
            "pins": {
                "POCI": "PB14",
                "PICO": "PB15",
                "SCK": "PB16",
                "CS": "PB17",
            },
        },
    }
    write_json(ROOT / "boards/LKDMX.json", board)

    detailed_api = read_json(
        ROOT / "metadata/apis/mspm0/mspm0-lua-modular.api.json"
    )
    modules = [
        {
            "name": module["name"],
            "description": module.get("description", ""),
            "functions": [simplify_function(function) for function in module["functions"]],
        }
        for module in detailed_api["modules"]
    ]
    i2c = next(module for module in modules if module["name"] == "i2c")
    write_on = next(function for function in i2c["functions"] if function["name"] == "write_on")
    route_params = write_on["overloads"][0]["params"][:3]
    modules.append({
        "name": "oled",
        "description": "按需上传的 SSD1306 OLED Lua 功能库；自动依赖原生 i2c 模块",
        "functions": [
            {
                "name": "open",
                "description": "显式指定 I2C 实例、SCL、SDA、7 位地址和速率",
                "overloads": [{"params": [
                    *route_params,
                    {"name": "address", "type": "integer"},
                    {"name": "hz", "type": "integer"},
                ]}],
            },
            {"name": "close", "description": "关闭显示", "overloads": [{"params": []}]},
            {"name": "clear", "description": "清屏", "overloads": [{"params": []}]},
            {"name": "fill", "description": "使用指定字节填充显存", "overloads": [{"params": [{"name": "value", "type": "integer"}]}]},
            {"name": "text", "description": "显示文本；IDE 按静态字号自动取模", "overloads": [{"params": [
                {"name": "x", "type": "integer"},
                {"name": "y", "type": "integer"},
                {"name": "text", "type": "string"},
                {"name": "size", "type": "integer"},
            ]}]},
            {"name": "number", "description": "显示定点数；IDE 自动包含数字、小数点和负号", "overloads": [{"params": [
                {"name": "x", "type": "integer"},
                {"name": "y", "type": "integer"},
                {"name": "value", "type": "integer"},
                {"name": "decimals", "type": "integer"},
                {"name": "size", "type": "integer"},
            ]}]},
        ],
    })
    api = {
        "chip": "mspm0g3507",
        "types": [value["id"] for value in detailed_api.get("types", [])],
        "enums": [value["id"] for value in detailed_api.get("enums", [])],
        "globals": [simplify_function(value) for value in detailed_api["globals"]],
        "modules": modules,
    }
    write_json(ROOT / "apis/mspm0g3507_lua.json", api)

    for project_file in (ROOT / "example").glob("*/mspm0_lua.json"):
        project = read_json(project_file)
        project.pop("target", None)
        write_json(project_file, project)


if __name__ == "__main__":
    main()
