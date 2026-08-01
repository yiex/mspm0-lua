# LittleFS Lua modules

The bytecode firmware supports two ways to split an application across files:

- `runfile("setup.luac")` executes a side-effect script and returns success.
- `require("driver")` loads `driver.luac`, returns its exported value and caches
  it for the lifetime of the current Lua VM. An explicit `.luac` suffix is also
  accepted, for example `require("driver.luac")`.

Only target Lua 5.5.1 `LUA_32BITS` bytecode is accepted. Every file is streamed
from LittleFS through `lua_load`; source and bytecode are not copied into a
whole-file RAM buffer.

## Library pattern

```lua
-- sensor.lua -> compile/save as sensor.luac
local M = {}

function M.read()
  return 123
end

return M
```

```lua
-- main.lua -> compile/upload as main.luac
local sensor = require("sensor")
print(sensor.read())
```

Modules may require other modules. Avoid depending on exports from a circular
dependency: during a cycle, the not-yet-finished module is represented by
`true`, which prevents recursion but cannot expose its final table yet.

## SSD1306 OLED example

`scripts/oled_ssd1306.lua` is a small 128x64 I2C driver. Prefer **PA15/PA16**
and address `0x3c` (avoid PA18/BSL). External pull-ups are required.

```powershell
python tools/compile_lua.py mspm0_lua/scripts/oled_ssd1306.lua mspm0_lua/build_bytecode/oled_ssd1306.luac
python tools/upload_script.py mspm0_lua/build_bytecode/oled_ssd1306.luac --name oled_ssd1306.luac --port <serial-port>
python tools/compile_lua.py mspm0_lua/scripts/oled_demo.lua mspm0_lua/build_bytecode/oled_demo.luac
python tools/upload_script.py mspm0_lua/build_bytecode/oled_demo.luac --name main.luac --port <serial-port>
```

Fonts can be separate `.luac` modules, but **24 KiB Lua heap** cannot hold a
full 6x8 ASCII table plus large drivers plus a heavy main at once. For
IMU+OLED realtime, prefer a **single-file** app with a tiny digit glyph table
and `i2c.writev` (see `scripts/hi_uart_atk_oled.lua` and
`IMU_OLED_REALTIME.md`).
Use `require` for cold menus/config, not for the 20 ms paint path.

## Board verification

On <serial-port>, `oled_ssd1306.luac` was installed as a 1,190-byte optional module and
`module_require_test.luac` was installed as `main.luac`. The test required the
same module once without and once with the suffix, verified that both results
were the same table, and printed `MODULE_REQUIRE_OK`. A hardware reset loaded
the external `main.luac`, required the persisted module again, printed the same
marker and reached `Idle`.
