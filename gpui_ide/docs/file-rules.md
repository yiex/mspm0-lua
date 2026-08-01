# IDE data file rules

The IDE loads portable data from directories next to the executable. File
names are stable identifiers; JSON content is UTF-8 and unknown fields are
rejected.

## Chip

Path: `chips/<chip>.json`

```json
{
  "name": "mspm0g3507",
  "pins": {
    "PA0": ["GPIOA_DIO00", "UART0_TX", "I2C0_SDA"]
  }
}
```

`pins` contains every programmable chip pin. Each value is only the list of
hardware mux function names supported by that pin. Package numbers, board
headers, descriptions, versions and provenance do not belong in this file.

## Board

Path: `boards/<board>.json`

```json
{
  "name": "地猛星",
  "chip": "mspm0g3507",
  "pins": ["PA0", "PA1"],
  "flash": {
    "name": "W25Q32JVSSIQ",
    "storage": "external",
    "luac": true,
    "pins": { "SCK": "PB16", "CS": "PB17" }
  }
}
```

`pins` contains only pins physically exposed by the board. A future board may
use `{ "header name": "chip pin" }` instead of an array when its printed pin
names differ. `flash` is optional; when present, `luac` says that compiled Lua
files are stored there.

## API

Path: `apis/<chip>_lua.json`

The file describes only the newest installed firmware API. It has `chip`,
optional custom `types` and `enums`, plus `globals` and `modules`. A function
contains `name`, optional `description`, and one or more `overloads`. An
overload contains `params` and optional `returns`. A parameter contains
`name`, `type`, optional `optional`, and optional `resource`.

Pin parameters use a resource such as:

```json
{
  "kind": "pin",
  "scope": "board.exposed",
  "capability": { "class": "i2c", "signal": "SDA" },
  "bindings": { "route": "bus" }
}
```

Completion and diagnostics first resolve the function and overload from the
API file, then restrict candidates to the selected board's exposed pins, and
finally verify the required mux function against the board's chip file.

## Other directories

- `firmware/`: base firmware, module catalog and module images.
- `example/<project>/`: one complete project per directory.
- `font/`: OLED 自动取模使用的本地 TTF/TTC/OTF；中文和英文可在设置中分别选择。
- `config.json`: portable IDE preferences, including the remembered board.
