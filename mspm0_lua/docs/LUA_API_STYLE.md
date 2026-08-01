# Lua API style (high-level first)

## Goal

- **Lua = orchestration** (policy, state machine, protocol glue)
- **C = hot path** (pinmux, bus frames, font paint, ring/ISR)
- **Not** a 1:1 port of TI DriverLib

## Layers

```
Lua app  →  oled / pid / filt / btn / enc …   (product verbs)
         →  gpio / pwm / uart / i2c / spi / adc      (buses + protocol glue)
C board_ →  IRQ ring, I2C, OLED glyphs, DMA capture
```

Protocol devices (e.g. ATK IMU) stay in **Lua on `uart.*`**, not a C product module.

## Consolidation rules

| Prefer | Avoid / removed |
|---|---|
| `gpio` + `pwm.open("PA14")` | `led.*` (deleted) |
| `event.run()` | `task.run` alias (deleted) |
| `require("mod")` | `requirefile` alias (deleted) |
| `uart` + Lua ATK parse | `imu.*` C module (deleted) |
| `adc.read` then `raw*vdda//4095` | `adc.mv` (deleted) |
| `filt.update(util.clamp(...))` | `filt.update_clamp` (deleted) |
| `pid.step(id, sp, fb, dt)` | `pid.step_err` (deleted) |
| Lua `a<b and a or b` | `util.min`/`max`/`abs` (deleted) |

Keep C helpers only when they save cycles or avoid allocation in a real loop.

## Preferred surface (bytecode)

| Module | Verbs |
|---|---|
| `gpio` | `mode` `set` `get` `toggle` `af` `owner` `release` |
| `pwm` | `open` `duty` `close` `comp` `comp_duty` `comp_close` |
| `uart` | `write` `read` + `open`/`tx`/`rx`/`close` (1..3) |
| `i2c` / `spi` / `adc` | open/xfer/capture as documented |
| `tmr` / `irq` / `event` / `task` | timers, edges, dispatch, coroutines |
| `oled` | SSD1306；ATK 姿态见 `hi_uart_atk*.lua` |
| `iq` / `pid` / `filt` / `ramp` | fixed-point control |
| `btn` / `enc` / `cap` / `qei` | inputs |
| `util` | `clamp` `deadzone` `map` `med3` `slew` `avg` `sign` |
| `crc` / `wdt` / `fs` / `sys` | protocol, watchdog, storage, mem |

## Related

- `API_REFERENCE.md` — full reference  
- `BYTECODE_PERIPHERALS.md` — bus pin tables  
