# Prebuilt modular firmware catalog

For PC IDE/provisioning integration, the complete protocol, Lua API, flashing,
and combined module-plus-LUAC workflow are documented in `HOST_INTEGRATION.md`.

The modular profile has two independent capacities:

- The PC-side catalog may contain any number of prebuilt feature modules and
  may be much larger than the MCU Flash.
- One firmware selection may contain up to eight modules because the MCU has
  eight 4 KiB runtime slots.

Selecting modules never invokes the C compiler. A module is compiled once and
linked into eight address variants (`slot0` through `slot7`). The composer
chooses the correct prebuilt variant when assigning the selected modules to
runtime slots.

## Flash layout

| Address range | Size | Purpose |
|---|---:|---|
| `0x00000000..0x00017EFF` | 95.75 KiB | boot, Lua VM, UART upload, LittleFS core |
| `0x00017F00..0x00017FFF` | 256 B | Core ABI v7 table |
| `0x00018000..0x0001FFFF` | 32 KiB | eight 4 KiB runtime slots |

The firmware loader accepts module format 2 and verifies magic, format, ABI,
image bounds, Thumb init/deinit entries, name, and CRC16 before executing a
module. Empty and invalid slots are not executed. Each runtime slot owns 32
bytes of core RAM state, so moving a module does not alias another module's
state. Before recreating the Lua VM, loaded modules are deinitialized in reverse
slot order; the core then stops software timers and electrically disconnects
all remaining application-owned pins.

## Catalog

| Module | Lua surface | Maximum variant size |
|---|---|---:|
| `uart` | UART0..3, all TX/RX routes, format and timeout control | 3,532 B |
| `gpio` | all 60 GPIO, PF0..PF9, pull/invert/hysteresis/drive | 1,624 B |
| `tmr` | software timers and hardware count/capture routes | 3,568 B |
| `pwm` | all PWM routes, shared channels, complementary/dead time | 3,884 B |
| `adc` | ADC0/ADC1, all 17 external inputs, resolution/averaging | 1,656 B |
| `i2c` | I2C0/I2C1, all routes, 7/10-bit, recovery | 3,352 B |
| `spi` | SPI0/SPI1, all routes, modes 0..3 and bit order | 2,420 B |
| `can` | MCAN pin pairs, standard/extended IDs and loopback | 2,628 B |
| `dac` | DAC0 8/12-bit, reference choice, raw/millivolt output | 1,136 B |
| `crc` | CRC16-Modbus and CRC32 | 332 B |
| `comp` | COMP0..2 with all external 64-pin analog input routes | 1,440 B |
| `rtc` | binary calendar, Gregorian validation and safe reads | 1,412 B |
| `opa` | OPA0/1 input mux, PGA gain, chopping, GBW and RRI | 1,552 B |

The catalog currently contains 13 modules and 104 variants totaling 228,288
bytes. This is intentionally larger than the device Flash. Every variant has
zero mutable `.data`/`.bss` and zero relocations.

ABI v7 exports the core's existing unsigned 32-bit division routine. ADC, I2C,
SPI, CAN, and DAC call that entry instead of linking a private copy of the ARM
division runtime into every variant. UART and PWM retain local copies because
TI driverlib code linked into those modules calls the compiler helper directly;
removing their copy would leave an unresolved driverlib dependency.

Release 1.0.2 uses a 98,184-byte core binary (its fixed API table extends the
file length) with SHA-256
`11d14d270e84251e0e2484bb4a753694e39977325b9d7715eef66aee6d0c6d02`.
Its catalog SHA-256 is
`c1609433a2bc70d4d991de00454f23f377fd71021f523447a3e00d61d4e2d23c`.

## Build once

Rebuild the core only after a core resource or ABI implementation change.
Rebuild a module only after changing its C source:

```powershell
$env:MSPM0_PROFILE = "modular"
python tools/build_fw.py
python tools/build_native_module.py
python tools/build_native_module.py i2c dac
```

The module catalog is `mspm0_lua/build_modules/index.json`; binaries are under
`mspm0_lua/build_modules/<name>/slot<N>/<name>.bin`. The index records source
and binary SHA-256 values. Composition rejects changed source, stale layout,
wrong ABI/address, bad metadata, oversize images, and CRC/SHA mismatch.

## Compose without compiling

Module order is slot order and is deterministic:

```powershell
python tools/compose_firmware.py --modules
python tools/compose_firmware.py --modules i2c
python tools/compose_firmware.py --modules rtc crc dac i2c
python tools/compose_firmware.py --modules opa comp rtc dac crc gpio i2c uart
python tools/compose_firmware.py --modules rtc crc --plan-only
```

The catalog can exceed eight modules. A single selection cannot: selecting
nine produces a capacity error instead of rebuilding or silently omitting a
module. Duplicate, unknown, and manifest-declared conflicting modules are also
rejected. Complete images and JSON segment reports are written under
`mspm0_lua/build_composed/`.

## Install and switch safely

Core/ABI 升级后先安装 Core 和全空槽；功能模块仍由之后的 IDE 运行事务按需上传：

```powershell
python tools/compose_firmware.py --modules --output mspm0_lua/build_composed/firmware_core.bin
python tools/jlink_flash.py mspm0_lua/build_composed/firmware_core.bin
```

After that one-time core installation, normal selection changes use only the
CH340 application UART. They do not reset the MCU, enter BSL, change BOOT, or
invoke J-Link:

```powershell
python tools/serial_module_set.py --modules i2c --port <serial-port>
python tools/serial_module_set.py --modules rtc crc dac i2c --port <serial-port>
python tools/serial_module_set.py --modules --port <serial-port>
```

`full` 集合只用于 8 槽容量边界和恢复测试，不是基础固件或日常默认配置。正式 IDE
根据可达 Lua 源码中的 API 使用情况生成模块列表，并与 `.luac` 一起完成两阶段部署。

The host creates one complete eight-slot NMUP transaction and uploads it to
LittleFS. The core validates bundle CRC32, ABI/layout, every fixed-address
module header, image CRC32, and payload CRC16 before creating a durable pending
record. It then deinitializes modules in reverse order, destroys the Lua VM,
erases `0x18000..0x1FFFF`, programs hardware-generated ECC from SRAM, verifies
every programmed byte and every erased tail, clears pending, and creates a new
VM. The MCU and UART session stay running throughout.

Power loss is recoverable. The update bundle is retained until the pending
record is cleared. On the next ordinary application boot, pending installation
runs before any module is registered. Validation or Flash failure leaves the
record in place and keeps Lua blocked, while UART upload, `modstatus`, and a new
`modapply` remain available for recovery.

`flash_module_set.py` remains a manufacturing/recovery tool for old base
firmware. It is not part of the normal customer workflow.

The status mailbox slot mask is based on occupied slots, not module identity.
Because assignment is contiguous, zero modules gives `0x00`, four gives
`0x0F`, and eight gives `0xFF`.

## Lua examples

Public examples always name concrete hardware pins. OLED is a Lua SSD1306
driver over the `i2c` module, not a native `oled.*` module; use I2C1 with PA15
SCL and PA16 SDA explicitly. Legacy scripts under `mspm0_lua/scripts` are
diagnostic history and are not the current IDE example contract.

```lua
-- DAC_OUT is PA15. open(bits, reference 0..3, external_pin_enabled).
dac.open(12, 0, 1)
local code = dac.write_mv(1650, 3300)
dac.close()

local modbus = crc.crc16("123456789") -- 0x4B37
local ethernet = crc.crc32("123456789") -- signed Lua integer for 0xCBF43926

-- COMP0 external positive PA26 and negative PA27.
comp.open(0, "PA26", "PA27", 1, 0, 0)
local high = comp.read(0)
comp.close(0)

rtc.open()
rtc.set(2026, 7, 26, 0, 12, 0, 0)
local year, month, day, dow, hour, minute, second = rtc.get()
rtc.close()

-- OPA0: psel=IN0+, nsel=RTAP, msel=GND, gain index 0 (1x).
opa.open(0, 1, 4, 2, 0, 1, 0, 1, 0)
local ready = opa.ready(0)
opa.close(0)
```

Run host-side catalog and corruption tests with:

```powershell
python tools/test_module_catalog.py
python tools/test_module_update_bundle.py
python tools/test_serial_module_update.py
```

## Compatibility rule

The runtime binaries are fixed-address variants, not position-independent
code. Changing `native_core_api_t`, slot layout, or resource semantics requires
one core/catalog rebuild. Normal feature selection after that uses only the
prebuilt catalog.
