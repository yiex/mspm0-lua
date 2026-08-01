# IDE automatic OLED font example

`main.lua` explicitly uses I2C1 on PA15/PA16 at 100 kHz and displays `123`
at 16 px. The IDE detects `require("oled")`, selects only the native `i2c`
module, rasterizes the required 16 px glyphs, and uploads in this order:

1. `_oled_font.luac`
2. `oled.luac`
3. `main.luac`

Digits `0` through `9`, decimal point, minus sign, and space are included for
every active OLED size so values that are only known at runtime remain usable.
