# Dimengxing MSPM0G3507 pin multiplexing

This table was checked against the local 2024-12-23 schematic
`LUA_TI/SCH/SCH_Schematic_1_2026-07-18.pdf` and the MSPM0G3507 data-sheet
Table 6-2. The board uses the 48-pin package.

## Runtime API

All bonded PA/PB names are accepted by `gpio.*` and `irq.*`. Compact PINCM
tables + per-pin **owner claim** track conflicts.

```lua
gpio.mode("PA27", "out")
gpio.set("PA27", 1)
print(gpio.owner("PA27"))   -- "gpio" | "uart2" | "adc" | "free" | ...
gpio.release("PA27")

-- Route a digital alternate function. PF = data-sheet Pin Function number.
gpio.af("PA22", 5, 0)       -- TIMA0_C1 example
gpio.af("PA27", 0)          -- PF=0 analog (ADC/OPA/COMP)

-- ADC by pin name (claims + PF0) or channel number
local v = adc.read("PA27")  -- A0_0
local raw, pns = adc.capture("PA25", 256, 200, 1)
print(adc.channel("PA26"))  -- 1

-- PWM routes (≤2 independent + ≤2 complementary)
local a = pwm.open("PA14", 1000)   -- TIMG12
local b = pwm.open("PA23", 2000)   -- TIMG7
pwm.duty(a, 40); pwm.duty(b, 60)
local c0 = pwm.comp(20000, 50, 500)              -- TIMA0 PA8/PA22
local c1 = pwm.comp("PA15", "PB6", 20000, 50, 500) -- TIMA1
```

Errors use `pin:reason` — `pin` unknown, `locked` (SYS/console/Flash), `busy`
(other owner), `pair` (invalid complementary pair).

`gpio.af` only routes IOMUX; drivers (`uart`/`i2c`/`pwm`/…) open peripherals.

`adc.capture(ch, n, timeout_ms, rate)`：n 最多 256；rate 0≈2 MSPS，1≈95.8 kSPS，2≈7.78 kSPS。

**时钟脚（勿与应用 ADC/GPIO 混淆）**

| 功能 | 引脚 | 说明 |
|---|---|---|
| HFXT 40 MHz | PA5 / PA6 | SYS 锁定 |
| LFXT 32.768 kHz | PA3 / PA4 | 板载无源晶振；SYS 锁定 |
| SWD | PA19 / PA20 | SYS 锁定 |
| PA27 | A0_0 / TIMG8_C1 / CAN_RX 等 | **不是**晶振脚；可用作 ADC 或 QEI PHB |

## Board conflicts (enforced in firmware)

| Pins | Policy | Claim result |
|---|---|---|
| PB14..PB17 | Flash / LittleFS | `locked` for app owners (only SPI1 FS path) |
| PA10/PA11 | UART0 console | `locked` except `uart0`/SYS |
| PA19/PA20, PA3..PA6, PA2 | SWD / crystals / ROSC | `locked` (SYS) |
| PA14 | LED load | allowed; use `gpio` or `pwm` (TIMG12) |
| PA18 | BSL risk at reset | allowed after boot; avoid OLED at power-up |
| any pin | one owner | second open without close → `busy` |

Soft note: PA21/PA23 may be VREF; PA0/PA1 have I2C pull-ups.

## Expansion headers

PF1 is GPIO for every listed pin. `A:` lists analog functions selected with
PF=0. Other entries use `PF number:function`.

| Pin | Analog (PF0) | Digital alternate functions |
|---|---|---|
| PA0 | - | 2:UART0_TX, 3:I2C0_SDA, 4:TIMA0_C0, 5:TIMA_FAL1, 6:TIMG8_C1, 7:FCC_IN |
| PA1 | - | 2:UART0_RX, 3:I2C0_SCL, 4:TIMA0_C1, 5:TIMA_FAL2, 6:TIMG8_IDX, 7:TIMG8_C0 |
| PA28 | - | 2:UART0_TX, 3:I2C0_SDA, 4:TIMA0_C3, 5:TIMA_FAL0, 6:TIMG7_C0, 7:TIMA1_C0 |
| PA31 | - | 2:UART0_RX, 3:I2C0_SCL, 4:TIMA0_C3N, 5:TIMG12_C1, 6:CLK_OUT, 7:TIMG7_C1, 8:TIMA1_C1 |
| PA2 | ROSC | 2:TIMG8_C1, 3:SPI0_CS0, 4:TIMG7_C1, 5:SPI1_CS0 |
| PB24 | A0_5, COMP1_IN1+ | 2:SPI0_CS3, 3:SPI0_CS1, 4:TIMA0_C3, 5:TIMG12_C1, 6:TIMA0_C1N, 7:TIMA1_C0N |
| PB20 | A0_6, OPA1_IN0- | 2:SPI0_CS2, 3:SPI1_CS0, 4:TIMA0_C2, 5:TIMG12_C0, 6:TIMA_FAL1, 7:TIMA0_C1, 8:TIMA1_C1N |
| PB19 | A1_6, COMP2_IN1+, OPA1_IN0+ | 2:COMP2_OUT, 3:SPI0_POCI, 4:TIMG8_C1, 5:UART0_CTS, 6:TIMG7_C1 |
| PB18 | A1_5, COMP1_IN2+ | 2:UART2_RX, 3:SPI0_SCK, 4:SPI1_CS2, 5:TIMA1_C1, 6:TIMA0_C2N |
| PB17 | A1_4, COMP1_IN2- | 2:UART2_TX, 3:SPI0_PICO, 4:SPI1_CS1, 5:TIMA1_C0, 6:TIMA0_C2 |
| PA7 | - | 2:COMP0_OUT, 3:CLK_OUT, 4:TIMG8_C0, 5:TIMA0_C2, 6:TIMG8_IDX, 7:TIMG7_C1, 8:TIMA0_C1 |
| PB3 | - | 2:UART3_RX, 3:UART2_RTS, 4:I2C1_SDA, 5:TIMA0_C3N, 6:UART1_RTS, 7:TIMG6_C1, 8:TIMA1_C1 |
| PB8 | - | 2:UART1_CTS, 3:SPI1_PICO, 4:TIMA0_C0, 5:COMP1_OUT |
| PA9 | - | 2:UART1_RX, 3:SPI0_PICO, 4:UART0_CTS, 5:TIMA0_C1, 6:RTC_OUT, 7:TIMA0_C0N, 8:TIMA1_C1N, 9:CLK_OUT |
| PB6 | - | 2:UART1_TX, 3:SPI1_CS0, 4:SPI0_CS1, 5:TIMG8_C0, 6:UART2_CTS, 7:TIMG6_C0, 8:TIMA1_C0N |
| PB7 | - | 2:UART1_RX, 3:SPI1_POCI, 4:SPI0_CS2, 5:TIMG8_C1, 6:UART2_RTS, 7:TIMG6_C1, 8:TIMA1_C1N |
| PA27 | A0_0, COMP0_IN0-, OPA0_IN0- | 2:RTC_OUT, 3:SPI1_CS1, 4:TIMG8_C1, 5:TIMA_FAL2, 6:CAN_RX, 7:TIMG7_C1 |
| PA26 | A0_1, COMP0_IN0+, OPA0_IN0+, GPAMP_IN+ | 2:UART3_TX, 3:SPI1_CS0, 4:TIMG8_C0, 5:TIMA_FAL0, 6:CAN_TX, 7:TIMG7_C0 |
| PA25 | A0_2, OPA0_IN1+ | 2:UART3_RX, 3:SPI1_CS3, 4:TIMG12_C1, 5:TIMA0_C3, 6:TIMA0_C1N |
| PA24 | A0_3, OPA0_IN1- | 2:UART2_RX, 3:SPI0_CS2, 4:TIMA0_C3N, 5:TIMG0_C1, 6:UART3_RTS, 7:TIMG7_C1, 8:TIMA1_C1 |
| PA23 | COMP1_IN1-, VREF+ | 2:UART2_TX, 3:SPI0_CS3, 4:TIMA0_C3, 5:TIMG0_C0, 6:UART3_CTS, 7:TIMG7_C0, 8:TIMG8_C0 |
| PA22 | A0_7, GPAMP_OUT, OPA0_OUT | 2:UART2_RX, 3:TIMG8_C1, 4:UART1_RTS, 5:TIMA0_C1, 6:CLK_OUT, 7:TIMA0_C0N, 8:TIMG6_C1 |
| PA21 | A1_7, COMP2_IN1-, VREF- | 2:UART2_TX, 3:TIMG8_C0, 4:UART1_CTS, 5:TIMA0_C0, 6:TIMG6_C0 |
| PB9 | - | 2:UART1_RTS, 3:SPI1_SCK, 4:TIMA0_C1, 5:TIMA0_C0N |
| PA18 | A1_3, OPA1_IN1+, COMP0_IN1+, GPAMP_IN- | 2:UART1_RX, 3:SPI1_PICO, 4:I2C1_SDA, 5:TIMA0_C3N, 6:TIMG7_C1, 7:TIMA1_C1 |
| PA17 | A1_2, OPA1_IN1-, COMP0_IN1- | 2:UART1_TX, 3:SPI1_SCK, 4:I2C1_SCL, 5:TIMA0_C3, 6:TIMG7_C0, 7:TIMA1_C0 |
| PA16 | A1_1, OPA1_OUT | 2:COMP2_OUT, 3:SPI1_POCI, 4:I2C1_SDA, 5:TIMA1_C1, 6:TIMA1_C1N, 7:TIMA0_C2N, 8:FCC_IN |
| PA15 | A1_0, DAC_OUT, OPA0_IN2+, OPA1_IN2+, COMP0_IN3+, COMP1_IN3+ | 2:UART0_RTS, 3:SPI1_CS2, 4:I2C1_SCL, 5:TIMA1_C0, 6:TIMG8_IDX, 7:TIMA1_C0N, 8:TIMA0_C2 |
| PA14 | A0_12, COMP0_IN2+ | 2:UART0_CTS, 3:SPI0_PICO, 4:UART3_TX, 5:TIMG12_C0, 6:CLK_OUT |
| PA13 | COMP0_IN2- | 2:UART3_RTS, 3:SPI0_POCI, 4:UART3_RX, 5:TIMG0_C1, 6:CAN_RX, 7:TIMA0_C3N |
| PA12 | - | 2:UART3_CTS, 3:SPI0_SCK, 4:TIMG0_C0, 5:CAN_TX, 6:TIMA0_C3, 7:FCC_IN |

PB8 is duplicated on the two expansion headers. The debug header additionally
exposes PA10, PA11, PA19/SWDIO and PA20/SWCLK.

## Driver status

| Peripheral | Native | Validated pins / notes |
|---|---|---|
| GPIO / IRQ | Yes | All bonded; claim + `owner`/`release` |
| ADC0 | Yes | ch0..7 **or** pin: PA27/26/25/24, PB24/20, PA22 → A0_0..7; `capture` DMA |
| TIMG12 PWM | Yes | CCP0 **PA14/PB20**；CCP1 **PA25/PA31/PB24**（板载 LED 用此或 gpio） |
| TIMG7 PWM | Yes | CCP0 **PA17/PA23/PA28**；CCP1 **PA7/PA18/PA24/PB19**（与 TIMG12 可并发） |
| TIMA0 comp PWM | Yes | C0+C0N：PA8/PA0/PA21/PB8 × PA22/PA9/PB9 |
| TIMA1 comp PWM | Yes | C0+C0N：PA15/PA17/PA28/PB2 × PA15(CMPL)/PB6/PB24 |
| I2C0 / I2C1 | Yes | claim on open; I2C1 defaults PA15/16 |
| SPI0 | Yes | claim 4 pins; reject console/Flash |
| SPI1 / LittleFS | Yes | PB14..17 locked |
| UART0 | Yes | PA10/11 locked console |
| UART1/2/3 | Yes | header TX/RX in `BYTECODE_PERIPHERALS.md` |
| OLED | Yes | `oled` on I2C1；串口姿态用 `uart` + 脚本 |
| CAP (TIMG6) | Yes | CCP0 **PA21/PB6/PB2**；CCP1 **PA22/PB7/PB3** |
| QEI (TIMG8) | Yes | fixed **PA26** CCP0 / **PA27** CCP1 |
| CAN | Dropped from default bytecode | restore `board_can.c` if needed |
| COMP/OPA/DAC/VREF | route only | `gpio.af` + future drivers |
