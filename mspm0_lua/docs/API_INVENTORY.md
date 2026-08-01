# Lua API inventory (bytecode)

Canonical: `API_REFERENCE.md`. Style: `LUA_API_STYLE.md`.

## Present

| Module | Notes |
|---|---|
| globals | print millis delay_ms yield stopped bytes byte runfile require |
| sys | mem gc resource |
| gpio irq uart i2c spi adc pwm | capture / comp PWM |
| tmr event task | no task.run |
| oled | display only；ATK 用 `uart` 脚本 |
| iq pid filt ramp util crc | util: no abs/min/max |
| btn enc cap qei wdt fs | |

## Removed

`led.*` · `task.run` · `requirefile` · `adc.mv` · `filt.update_clamp` ·  
`pid.step_err` · `imu.*` · `util.abs/min/max`

## Compose

```lua
gpio.mode("PA14","out"); gpio.toggle("PA14")
local id=pwm.open("PA14",1000); pwm.duty(id,40)
-- 第二路: pwm.open("PA23",2000) → TIMG7
-- 第二互补: pwm.comp("PA15","PB6",20000,50,500) → TIMA1
local raw=adc.read(0); local mv=raw and (raw*3300)//4095
y=filt.update(f, util.clamp(x,0,4095))
-- ATK: scripts/hi_uart_atk.lua (uart.rx + Lua parse)
```
