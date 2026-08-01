# MSPM0G3507 modular Lua firmware: host integration guide

This document is the contract for a PC-side IDE, configurator, or production
programmer. It covers the modular firmware model, catalog validation, Lua API,
composition, flashing, LUAC upload, and combined deployment.

Current release identity: firmware/catalog `1.0.2`, catalog SHA-256
`c1609433a2bc70d4d991de00454f23f377fd71021f523447a3e00d61d4e2d23c`.

## 1. System model

The PC keeps a catalog that may be larger than the MCU Flash. A deployment
selects only the native modules required by the application. Selecting modules
does not compile C code.

Each native module is built once and linked into eight fixed-address variants.
Selection order determines slot order:

```text
requested:  rtc crc dac i2c
assigned:   rtc=slot0, crc=slot1, dac=slot2, i2c=slot3
addresses:  0x18000, 0x19000, 0x1A000, 0x1B000
```

The current catalog has 13 logical modules, 104 variants, and 228288 indexed
bytes. A single deployment can contain at most eight modules. Each selected
variant must fit its 4096-byte slot.

## 2. Memory and artifact contract

| Range | Size | Owner |
|---|---:|---|
| `0x00000..0x17EFF` | 95.75 KiB | boot, Lua VM, console, LittleFS support |
| `0x17F00..0x17FFF` | 256 B | native core API, ABI version 7 |
| `0x18000..0x1FFFF` | 32 KiB | eight native-module slots |
| external SPI Flash | device dependent | LittleFS and `.luac` files |

Important files:

| File | Purpose |
|---|---|
| `mspm0_lua/modules/modules.json` | layout, module names, named sets |
| `mspm0_lua/build_modules/index.json` | trusted catalog metadata |
| `mspm0_lua/build_modules/<name>/slot<N>/<name>.bin` | fixed-address variant |
| `mspm0_lua/build_modular/mspm0_lua_modular.bin` | core image |
| `mspm0_lua/build_composed/firmware_<label>.bin` | complete 128 KiB image |
| `mspm0_lua/build_composed/firmware_<label>.json` | segment report |
| `mspm0_lua/release/catalog_manifest.json` | immutable release identity and hashes |
| `mspm0_lua/release/mspm0-lua.api.json` | complete machine-readable Lua API |
| `mspm0_lua/release/test-vectors/` | NMUP vectors and verified transcript |

The host must use `index.json`; discovering arbitrary `.bin` files by walking
directories is not supported. Unindexed files such as an old `plug.bin` must
be ignored and are rejected by the supplied composer.

ABI v7 includes a shared unsigned 32-bit division entry. ADC, I2C, SPI, CAN,
and DAC use the core copy instead of carrying the same compiler runtime in each
module. This changes no Lua API or arithmetic result, but ABI v6 modules must
not be mixed with the v7 core.

## 3. Host workflow

### 3.1 Validate and preview

```powershell
python tools/compose_firmware.py --modules rtc crc dac i2c --plan-only
python tools/compose_firmware.py --modules --plan-only
```

`--set full` 仅用于 8 槽边界与恢复测试。正常开发由 IDE 从 Lua/API 使用情况生成
显式模块列表；不得把 `full` 当作默认选择。

Validation is completed before hardware access. It rejects:

- unknown, duplicate, conflicting, or more than eight selected modules;
- stale source hashes or a catalog from another layout/ABI;
- wrong slot address, image bounds, format-2 header, name, or Thumb init/deinit entry;
- file size, SHA-256, or payload CRC16 mismatch;
- paths escaping `build_modules`.

### 3.2 Compose a complete offline image

```powershell
python tools/compose_firmware.py --modules rtc crc dac i2c
python tools/compose_firmware.py --modules --output mspm0_lua/build_composed/firmware_core.bin
```

The output is always 131072 bytes. Unused Flash and slots contain `0xFF`.
Composition does not import or invoke a compiler.

### 3.3 First installation or core ABI upgrade

首次只组合 Core 和全空模块槽，然后使用生产烧录路径安装：

```powershell
python tools/compose_firmware.py --modules --output mspm0_lua/build_composed/firmware_core.bin
python tools/jlink_flash.py mspm0_lua/build_composed/firmware_core.bin
```

This is required when the core, native API, slot layout, or ABI changes.

### 3.4 Normal module-only switching over CH340

```powershell
python tools/serial_module_set.py --modules i2c --port <serial-port>
python tools/serial_module_set.py --modules rtc crc dac i2c --port <serial-port>
python tools/serial_module_set.py --modules --port <serial-port>
```

This is the normal post-sale path. It uses UART0 through CH340, negotiates
460800 baud, and neither resets the MCU nor enters BSL. `--modules` with no
names installs an empty slot set. The host never calls a compiler or debugger.

The complete NMUP file is uploaded atomically to external LittleFS before the
internal module range is touched. The core validates it, records pending,
deinitializes the current modules, destroys the Lua VM, installs and verifies
all eight slots, then creates a new VM. The UART peripheral and connection stay
active.

Before any write, the reference tool issues `fwinfo` and rejects a firmware ID,
version, target, ABI, format, slot layout, or catalog hash mismatch. It compares
every current slot by slot number, module name, exact image size, and full-image
CRC32. An exact match skips the NMUP write and preserves internal-Flash erase
cycles.

### 3.5 One-command modules plus LUAC deployment

```powershell
python tools/deploy_bundle.py app.luac --modules gpio i2c tmr --port <serial-port>
python tools/deploy_bundle.py main.luac --modules i2c --dependency _oled_font.luac --dependency oled.luac --port <serial-port>
```

`deploy_bundle.py` 是传输参考工具；正式 IDE 自动从 Lua/API 推导上例的
`--modules` 列表，不要求用户手工维护。命令行工具故意要求显式给出 `--modules`
或 `--set`，避免遗漏参数时偷偷装入预设功能。

`_oled_font.luac` 和 `oled.luac` 由 IDE 自动生成/编译。普通命令行工具不会解析工程
文本或代替 IDE 取模；上面的第二条命令只说明最终传输顺序。

This command is an ordered, fail-fast host deployment job:

1. validate the module catalog and script before changing hardware;
2. upload and install the selected native modules through the application UART;
3. rebuild the Lua VM without resetting the MCU;
4. upload zero or more dependency LUAC files in topological order;
5. upload `app.luac` to LittleFS as `main.luac` through the same UART session;
6. report `BUNDLE_DEPLOY_OK` only after exact `SCRIPT_DONE OK`.

If module flashing fails, LUAC upload is not attempted. This prevents a new
script from running against a missing or incompatible native API.

The module set is a transaction, but the following LUAC replacement is a
separate transaction. If the module phase succeeds but LUAC upload fails, the
new modules remain installed. LittleFS writes through
`.upload.tmp` and replaces the target only after a complete upload, so the old
`main.luac` is not replaced by a partial file. The host must report the two
phase states separately and retry `upload_script.py` without reflashing the
already verified modules.

Use `upload_script.py --name test.luac` to save a non-boot file. A `main.luac` upload recreates
the Lua VM and runs immediately; other names are saved but not automatically
run. VM recreation first calls every loaded module's optional deinitializer in
reverse slot order. It then stops core software timers, releases peripheral
ownership, disables application GPIO output drivers, and clears application
pinmux entries. Console, SPI Flash, SWD, and other system-owned pins are kept.

## 4. Console and LUAC upload protocol

Application UART: UART0, PA10 TX, PA11 RX, 8-N-1. Every reset starts at 115200
baud. The host may negotiate 460800 for upload and debugging. The port is
normally the CH340 device (currently <serial-port>). Lines are LF terminated; CR is
ignored. The maximum accepted content is 255 characters per line.

Binary files use uppercase/lowercase hexadecimal text:

```text
host -> <<<HEX main.luac\n
MCU  -> SCRIPT_BEGIN\r\n
host -> <up to 254 hex characters>\n
MCU  -> HEX_OK\r\n
... one ACK per block ...
host -> >>>HEX\n
MCU  -> SCRIPT_OK <binary-byte-count>\r\n
MCU  -> SCRIPT_DONE OK\r\n
```

The modular release accepts `<<<HEX` only; source `<<<LUA` is not compiled on
the MCU. The final `SCRIPT_DONE` is emitted only for `main.luac` because that
target runs immediately. Relevant errors are `SCRIPT_ERR name/fs`,
`open`, `line`, `hex`, `write`, `save`, or `OOM`. After any error the host must
abort the transfer and start a new `<<<HEX` transaction.

The supplied uploader defaults to 120 binary bytes per line. Hex encoding
produces 240 characters, below the 255-character firmware limit:

```powershell
python tools/upload_script.py app.luac --port <serial-port>
python tools/upload_script.py app.luac --name driver.luac --chunk-size 120
```

Valid LittleFS names contain only letters, digits, `_`, `.`, and `-`, and are
at most 28 characters. `require("driver")` resolves `driver.luac`; `runfile`
requires the explicit filename.

Console commands:

| Command | Result |
|---|---|
| `r` or `f` | recreate VM and run current boot script |
| `!` | request cooperative Lua stop |
| `ls` | `LS`, zero or more `F <name> <size>`, `LS_END` |
| `storageinfo` | exact external storage part, capacity, SPI instance and four pins |
| `fileinfo <name>` | exact file length and lowercase CRC32, or stable `FILE_ERR` reason |
| `format` | format external LittleFS |
| `baud 460800` | acknowledged switch from current rate to 460800 |
| `baud 115200` | switch an active high-speed session back to 115200 |
| `modstatus` | report pending state and valid slots, terminated by `MOD_STATUS_END` |
| `fwinfo` | report release identity and catalog hash, terminated by `FW_INFO_END` |
| `modapply <file>` | validate and atomically activate a complete NMUP module set |
| `bsl` | reset into TI ROM BSL; ROM UART is 9600 baud |

The modular core intentionally omits the old `get`, `rm`, `boot`, `help`, and
source-upload paths to preserve slot capacity. `fileinfo` errors are exactly
`INVALID_NAME`, `FS_NOT_MOUNTED`, `NOT_FOUND`, or `IO`; error lines terminate
the query without `FILE_END`.

`modstatus` reports `MOD_CATALOG <sha256>` before slot rows, then
`MOD_LAYOUT <valid-count> <full-32KiB-slot-region-crc32>` and
`MOD_PENDING none|invalid|<bundle-crc32>` before `MOD_STATUS_END`. Unknown or
missing summary rows indicate a different firmware contract; a host must not
start NMUP until `fwinfo`, catalog, layout, and pending state are understood.

## 5. Native Lua API summary

Only selected modules exist as Lua globals. A script must not call a module
that was not included in its deployment. Errors use short stable prefixes such
as `i2c:pin`, `uart:busy`, `adc:timeout`, or `dac:range`.

Pin strings use `PA0..PA31` and `PB0..PB27` for the maximum 64-pin package.
Peripheral `valid`/`route` helpers should be used by a graphical host before
generating a script. Pin and peripheral ownership is exclusive; close or
release a resource before assigning the same pin to another module.

### GPIO

```lua
gpio.mode(pin [, mode [, option [, feature [, invert]]]])
gpio.set(pin, value)       -- alias: write
gpio.get(pin)              -- alias: read
gpio.od_write(pin, release)
gpio.toggle(pin)
gpio.af(pin, pf [, input_enable])
gpio.release(pin)
gpio.owner(pin)            -- numeric owner, 0 means free
gpio.policy(pin)           -- reserved-pin policy bits
gpio.valid(pin)            -- boolean
```

Modes are `out`, `od`, `analog`, `in`, `in_pu`, and `in_pd`. Console and
board-critical pins may be policy protected.

### I2C

```lua
i2c.write(addr, data [, hz])
i2c.read(addr, count [, hz])
i2c.write_read(addr, write_data, read_count [, hz])
i2c.write_on(id, scl, sda, addr, data [, hz])
i2c.read_on(id, scl, sda, addr, count [, hz])
i2c.write_read_on(id, scl, sda, addr, write_data, count [, hz])
i2c.probe_on(id, scl, sda, addr [, hz])
i2c.recover(id, scl, sda)
i2c.valid(id, scl, sda)
```

The short forms use I2C1 on PA15/PA16 and remain callable firmware APIs, but
all shipped examples and IDE-generated code must use the `_on` forms with an
explicit instance and concrete pin strings. IDs are 0 or 1. Separate reads, writes,
and probes accept 7-bit and 10-bit addresses up to `0x3ff`; combined
write-then-read uses a repeated START and currently accepts 7-bit addresses
only. Transfers have bounded timeouts and bus recovery.

There is no native `oled.*` module in the modular catalog. SSD1306 examples are
Lua drivers over `i2c.write_on`; the reference route is I2C1, PA15 SCL, PA16
SDA, address `0x3c`, 100000 Hz.

### SPI

```lua
spi.xfer([cs], data [, hz [, mode [, lsb_first]]])
spi.xfer_on(id, sck, pico, poci, cs, data [, hz [, mode [, lsb_first]]])
spi.read_on(id, sck, pico, poci, cs, count [, fill [, hz [, mode [, lsb]]]])
spi.valid(id, sck, pico, poci)
```

IDs are 0 or 1; modes are 0..3. The short form uses SPI0 on PA12/PA14/PA13 and
defaults CS to PA18. SPI1 access is arbitrated with the external Flash.

### UART

```lua
uart.open(id [, tx [, rx [, baud [, bits [, parity [, stop]]]]]])
uart.tx(id, data)
uart.rx(id [, timeout_ms [, max_bytes]])
uart.close(id)
uart.valid(id, tx, rx)
```

IDs are 0..3. UART0 is shared with the console and is acquired temporarily;
UART1..3 have independent state. Parity is `none`, `even`, or `odd`.

### Timers and capture

```lua
tmr.start(id, period_ms)       -- software timer id 0..3
tmr.ready(id)                  -- read/clear boolean
tmr.take(id)                   -- read/clear accumulated hits
tmr.stop(id)
tmr.millis()
tmr.delay(ms)
tmr.hw_start(timer, ticks [, prescale [, periodic]])
tmr.hw_value(timer)
tmr.hw_ready(timer)
tmr.hw_stop(timer)
tmr.capture_open(timer, pin [, edge [, prescale]]) -- returns handle
tmr.capture_ready(handle)
tmr.capture_read(handle)
tmr.capture_close(handle, pin)
tmr.route(timer, pin)          -- channel or -1
```

Timer IDs: 0=TIMA0, 1=TIMA1, 2=TIMG0, 3=TIMG6, 4=TIMG7, 5=TIMG8,
6=TIMG12. Capture edge is 0 rising, 1 falling, 2 both.

### PWM

```lua
pwm.open(pin [, hz [, duty [, center [, invert]]]])
pwm.open_on(timer, pin, hz [, duty [, center [, invert]]])
pwm.duty(handle, percent)
pwm.close(handle, pin)
pwm.open_pair(timer, high_pin, low_pin, hz [, duty [, dead_ns [, center]]])
pwm.close_pair(handle, high_pin, low_pin)
pwm.route(timer, pin)
```

`duty` is 0..100. Complementary pairs are supported on TIMA0/TIMA1 and include
bounded dead-time conversion.

### ADC and DAC

```lua
adc.channel(pin)
adc.instance(pin)
adc.read(pin [, sample_cycles [, averages [, bits]]])
adc.read_mv(pin, vdda_mv [, sample_cycles [, averages [, bits]]])
adc.release(pin)

dac.open([bits [, reference [, external_pin_enable]]])
dac.write(code)
dac.write_mv(millivolts, reference_mv) -- returns generated code
dac.close()
```

ADC bits are 8, 10, or 12; averaging is 1..128 in powers of two. DAC bits are
8 or 12 and the external DAC0 output is PA15.

### CAN, CRC, comparator, RTC, and OPA

```lua
can.open([bitrate [, loopback [, tx [, rx]]]])
can.open_on(tx, rx [, bitrate [, loopback]])
can.send(id, data [, timeout_ms [, extended]]) -- classic payload <= 8 B
can.recv([timeout_ms])                         -- id, data, extended or no value
can.valid(tx, rx)
can.close()

crc.crc16(data [, initial]) -- CRC-16/MODBUS
crc.crc32(data [, initial])

comp.open(id, positive_pin, negative_pin [, fast [, hysteresis [, invert]]])
comp.read(id)
comp.close(id)

rtc.open()
rtc.set(year, month, day, weekday, hour, minute, second)
rtc.get() -- seven values in the same order
rtc.close()

opa.open(id [, psel [, nsel [, msel [, gain [, output [, chop [, high_gbw [, rri]]]]]]]])
opa.ready(id)
opa.close(id)
```

CAN bitrates are 125k, 250k, 500k, or 1M. Comparator IDs are 0..2. OPA IDs are
0..1; selector values map directly to the tables documented in `opa.c`.

## 6. Baud-rate and throughput policy

The firmware boots at 115200 and the supplied uploader negotiates 460800 by
default. The switch protocol is:

```text
host @115200 -> baud 460800\n
MCU  @115200 -> BAUD_SWITCH 460800\r\n
MCU switches UART divisor
MCU  @460800 -> BAUD_OK 460800\r\n
```

The host starts payload transfer only after the second acknowledgement. A
reset always recovers 115200. To disable negotiation use `--baud 115200`. If a
non-reset device is already at 460800, reconnect with
`--connect-baud 460800 --baud 460800`.

At 115200 8-N-1, raw capacity is about 11.52 KiB/s; hexadecimal transport
halves that before ACK overhead. Raising the block size from 64 to 120 bytes
reduces ACK round trips without changing firmware or boot compatibility.

The hardware clocks can accurately generate 460800 in both modes:

| UART clock | Approximate baud error at 460800 |
|---:|---:|
| 40 MHz HFXT path | about +0.16% |
| 32 MHz fallback | about -0.22% |

460800 is therefore the selected protocol rate. 921600 is intentionally not
the default because CH340 variants, cabling, and host-driver latency have a
larger reliability spread. On negotiation timeout the host must not send a
payload. Probe both supported rates, then issue `baud 115200` at the active
rate; a reset remains a fallback but is not required for recovery.

## 7. Application-UART native-module update protocol

Pure-UART update is supported by the modular core. `<<<HEX` first stores an
NMUP v1 bundle in external LittleFS; `modapply` then installs it into internal
Flash. The application does not invoke ROM BSL.

NMUP v1 uses a 32-byte little-endian header followed by eight fixed 32-byte
slot entries and contiguous payloads. The header carries `NMUP`, format 1, ABI
7, header size 288, slot count 8, selected count, exact file size, and standard
CRC32. Each present entry carries its slot, image size/offset, image CRC32,
the module's CRC16, and zero-padded module name. Empty entries explicitly erase
their slots. Reserved fields must be zero.

Device sequence:

```text
host: <<<HEX modules.upd ... >>>HEX
MCU:  SCRIPT_OK <bytes>
host: modapply modules.upd
MCU:  MOD_READY <count> <bytes>
MCU:  MOD_APPLY modules.upd
MCU:  MOD_ERASE <slot> / MOD_WRITE <slot> <name>
MCU:  MOD_VERIFY
MCU:  MOD_DONE <count>
MCU:  MOD <name> ... Idle
```

The pending record is committed only after three validation passes over the
complete bundle. Lua C pointers are removed before erase. Internal programming
uses TI FlashCTL RAM functions with hardware-generated ECC and interrupts
masked only around each Flash command. Readback verification occurs before the
pending record and bundle are deleted.

If power fails after pending commit, the next normal boot prints `MOD_RECOVER`
and repeats installation before creating Lua. Any error prints `MOD_ERR
<reason>` and `MOD_BLOCKED`; no module is registered. The host may upload a
replacement `modules.upd` and issue `modapply` again without BSL or reset.

CRC32 and CRC16 detect corruption; they do not authenticate an adversarial
sender. The current application UART is a trusted development interface: a
party with console access can install native machine code. Products that expose
the connector to untrusted users need a separately provisioned signature check
and command authentication policy before enabling `modapply`.

## 8. Regression commands

```powershell
python -m py_compile tools/upload_script.py tools/module_bundle.py tools/serial_module_set.py tools/deploy_bundle.py
python tools/test_module_catalog.py
python tools/test_module_update_bundle.py
python tools/test_serial_module_update.py
python tools/compose_firmware.py --set full --plan-only
```

Expected catalog result:

```text
CATALOG_TEST_OK modules=13 slots=8 abi crc address deinit capacity duplicate no-compiler
```
