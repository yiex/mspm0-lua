# Short demos (not SDK clones)

**Policy:** do **not** mirror every TI DriverLib example in Lua.  
SDK (`$MSPM0_SDK`) informs **C drivers**. Lua gets a **small
high-level surface** — see `LUA_API_STYLE.md`.

## Preferred demos

| Script | API | Purpose |
|---|---|---|
| `scripts/hi_led.lua` | `led.*` | Board LED / PWM |
| `scripts/hi_oled.lua` | `oled.*` | SSD1306 text + fixed-point num |
| `scripts/hi_imu_oled.lua` | `oled` + `uart` + `tmr` | Product-shaped realtime UI |
| `scripts/bytecode_peripheral_test.lua` | raw buses | CAN/SPI/I2C open smoke |

## Low-level / teaching only

`scripts/sdk_demos/01`–`13` remain as **escape-hatch** templates (`gpio`/`i2c`/`spi`).
Prefer rewriting product code with `led`/`oled`.

## Flash

`led` + `oled` (C font + I2C frames) are compiled into the bytecode image.
Measure free space with `python tools/build_fw.py` after each C feature.
