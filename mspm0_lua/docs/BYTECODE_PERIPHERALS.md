# Bytecode-only secondary peripherals

These drivers are registered only by the `bytecode` firmware. They keep UART0
on PA10/PA11 and LittleFS SPI1 on PB14..PB17 untouched. The source and
source-full profiles retain their previous APIs and Flash sizes.

## UART1, UART2 and UART3

```lua
uart.open(1, "PA17", "PA18", 115200)
uart.tx(1, "hello")
local data = uart.rx(1, 100, 64)
uart.close(1)
```

| ID | Default TX/RX | Other accepted TX | Other accepted RX |
|---:|---|---|---|
| 1 | PA17 / PA18 | PB6, PA8 | PA9, PB7 |
| 2 | PA23 / PA24 | PA21, PB17 | PA22, PB18 |
| 3 | PA26 / PA25 | PA14, PB2 | PB3, PA13 |

RX is **IRQ + 384-byte ring** (not DMA). The ring drops the oldest byte when
full so continuous streams (e.g. ATK module @ 115200) keep moving. While I2C is
busy-waiting, the firmware also polls UART FIFOs into the ring so Lua OLED
paints do not starve application UART RX.

PB17 is the external-Flash chip select and PA14 drives the LED, so those two
alternate routes should normally be avoided. UART0 continues to use
`uart.write(text)` and `uart.read(timeout_ms,max_bytes)` for the console.

ATK-601/901 demo (Lua parse, no C `imu.*`): UART2 **PA23=TX / PA24=RX** @ 115200
cross-wired to the module. Scripts: `hi_uart_atk.lua`, `hi_uart_atk_oled.lua`.

## I2C1

```lua
i2c.open(1, "PA15", "PA16", 100000)
assert(i2c.write(1, 0x3c, bytes(0x00, 0xae)))
-- Hot path: stack-buffered write, no Lua string alloc
assert(i2c.writev(1, 0x3c, 0x00, 0xae))
assert(i2c.writev(1, 0x3c, 0x40, 0, 0, 0, 0, 0, 0))
local value = i2c.write_read(1, 0x50, bytes(0), 8)
i2c.close(1)
```

Validated pairs are **PA15/PA16** (preferred) and PA17/PA18, in SCL/SDA order.
External pull-ups are required. **Do not hang OLED on PA18** (BSL risk at
reset). I2C ID 0 remains the existing PA1/PA0 controller.

Writes use a single START + full length with TX FIFO refill, bounded timeouts,
and soft recover on error. Prefer `i2c.writev` in OLED paint loops.

## SPI0

```lua
-- ID 1 denotes the new SPI0 path; ID 0 remains shared SPI1/W25Q.
spi.open(1, "PA12", "PA14", "PA13", "PA18", 1000000)
local rx = spi.xfer(1, bytes(0x9f, 0xff, 0xff, 0xff))
spi.close(1)
```

The order is SCK, PICO/MOSI, POCI/MISO, GPIO chip select, frequency. The
validated route is PA12/PA14/PA13. PA14 has the board LED load. UART0, SWD and
PB14..PB17 are rejected as application chip-select pins.

## Classic CAN

An external CAN transceiver is required for a real bus. Internal loopback works
without one.

```lua
can.open(500000, true) -- 125k, 250k, 500k or 1M; true=internal loopback
assert(can.send(0x321, bytes(0x12, 0x34), 100))
local id, data = can.recv(100)
can.close()
```

CAN uses PA26 TX / PA27 RX. This first implementation supports classic CAN,
11-bit identifiers and payloads from 0 to 8 bytes. Send and receive are polling
operations with bounded timeouts; an unacknowledged transmission is cancelled.

## High-level helpers (preferred)

```lua
-- PA14 status: gpio or pwm (no led.* module)
gpio.mode("PA14", "out"); gpio.toggle("PA14")
pwm.open("PA14", 1000); pwm.duty(0, 40); pwm.close(0)

oled.open()                 -- I2C1 PA15/PA16, 0x3C, 100k
oled.cursor(0, 0); oled.print("HI")
oled.num(18, 2, -550, 1)

-- ATK attitude: use uart + Lua (see hi_uart_atk.lua), not imu.*
uart.open(2, "PA23", "PA24", 115200)
local chunk = uart.rx(2, 10, 64)
```

C owns pinmux and SSD1306 (`board_oled`). Protocol glue stays in Lua.

## Board test

`scripts/bytecode_peripheral_test.lua` opens and closes every new controller and
performs two CAN internal-loopback transfers with a close/reopen between them.
On the physical board this printed
`PERIPHERAL_TEST_OK`; a reset reran it from LittleFS and then reached `Idle`.
Compile, flash and upload it with:

```text
python tools/compile_lua.py mspm0_lua/scripts/bytecode_peripheral_test.lua mspm0_lua/build_bytecode/bytecode_peripheral_test.luac
python tools/hold_boot_flash.py mspm0_lua/build_bytecode/mspm0_lua_bytecode.bin
python tools/upload_script.py mspm0_lua/build_bytecode/bytecode_peripheral_test.luac --name main.luac --port <serial-port>
```
