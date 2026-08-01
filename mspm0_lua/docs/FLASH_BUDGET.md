# 128 KiB Flash budget and external-code profiles

Verified on 2026-07-23 with Lua 5.5.1, `LUA_32BITS`, GCC 14.2 LTO and an
MSPM0G3507 (128 KiB internal Flash, 32 KiB SRAM).

| Profile | Internal Flash | Free | BSS | Lua heap | APIs |
|---|---:|---:|---:|---:|---|
| `bytecode` (default) | 116,864 B text / 117,336 B bin | **13,736 B free** | 28,016 B BSS | 22 KiB | +IRQ events + cooperative tasks + `adc.capture` DMA + `pwm.comp`; **CAN dropped** |
| `source` | 129,312 B | 1,760 B | 18,968 B | 24 KiB | Source parser plus GPIO, IRQ, timer, PWM, UART, LittleFS |
| `source_full` (experimental) | 130,960 B | 112 B | 19,008 B | 24 KiB | Source parser, all hardware APIs, all bonded GPIO names and generic `gpio.af` routing |

The old full source-parser image used 130,040 B and had a 1,536-byte script
buffer. Streaming support adds file-management code, so keeping the parser and
all hardware bindings originally overflowed Flash by 840 B. The experimental
`source_full` profile now fits by removing the built-in demo, legacy line upload,
and the replaceable `get/rm/boot/help` console commands. It retains `format`,
`ls`, binary-safe HEX upload, source execution, `runfile`, and every hardware
binding. With only 112 B free it has no safe growth margin, so `bytecode` remains
the default. Removing `lcode.c`, `llex.c` and `lparser.c` saves about 18 KiB.

The bytecode-only secondary-peripheral drivers, VM stop hook, event dispatcher,
cooperative tasks and module loader leave 13,736 B for later features. They are
excluded from both source-parser profiles
by LTO and compile-time registration guards, so the 112-byte `source_full`
margin is unchanged. See `BYTECODE_PERIPHERALS.md`.

The Dimengxing pin-name implementation was later replaced by a compact PA/PB
parser plus PINCM index arrays. This expanded GPIO/IRQ from 15 names to every
bonded pin of the board's 48-pin package and also made room for
`gpio.af(pin, pf, input_enable)`, while reducing the final image by 72 B. See
`DIMENGXING_PINMUX.md` for the expansion-header matrix and board conflicts.

The Lua allocator is now a reusable fixed-heap allocator with split, coalesce and
in-place growth. The former bump allocator ignored frees and leaked every
reallocation until the VM was recreated.

## External Flash and streaming

LittleFS now defaults to the full JEDEC-reported device capacity. The connected
device reports `EF4016`, so the configured volume is 2^22 = 4 MiB. Set
`MSPM0_LFS_BLOCKS` only to override auto-detection; `0` means auto. A geometry
change can format an incompatible existing volume.

Scripts are never copied into a whole-file RAM buffer. `lua_load` requests the
next 128-byte LittleFS block through a reader callback. UART uploads are written
to `.upload.tmp` as they arrive and atomically renamed after a successful close.
`get` and `boot` also copy in bounded chunks.

Two upload transports are available:

- `<<<LUA [name]` / `>>>LUA`: legacy line-oriented source upload.
- `<<<HEX name` / `>>>HEX`: binary-safe streaming upload for source or `.luac`.

HEX uses per-block `HEX_OK` acknowledgements. This prevents the CH340 RX stream
from outrunning a worst-case NOR page program and makes large transfers reliable.
The `format` command recreates LittleFS and reports the detected byte capacity.

The Python uploader and the IDE use HEX, so neither binary NUL bytes nor long
source lines impose a script-size limit. Practical script complexity is still
bounded by the 24 KiB Lua heap, not by source-file length.

At boot, `main.luac` is preferred. Source-capable profiles fall back to
`main.lua`; compact `source` also has a built-in demo. The bytecode profile
accepts only binary chunks.
`runfile("module.luac")` lets a main script load optional side-effect modules from
LittleFS only when a feature is used.

For normal libraries, `require("module")` or `require("module.luac")`
streams the target bytecode from LittleFS, returns the module's value and caches
it in the Lua registry. A module normally ends with `return M`, where `M` is a
table of driver functions. Requiring the same file again returns the identical
table without rerunning its top-level code. A temporary cache sentinel prevents
accidental circular imports from recursing forever.

The bytecode profile installs a count hook every 1024 Lua VM instructions. The
host `!` command can therefore interrupt even `while true do end`; scripts do not
have to call `yield()` or `delay_ms()` to remain recoverable. A boot-file upload
reports `SCRIPT_OK` after the atomic save and `SCRIPT_DONE OK|ERR` only after the
script exits. Host tools must wait for `SCRIPT_DONE` before sending `ls` or other
console commands.

The bytecode profile accepts integer strings at runtime but intentionally does
not parse decimal strings as floats (`tonumber("1.5")` returns nil). Source is
compiled on the host, so float constants and float arithmetic in bytecode still
work. Omitting the unused `strtof` path avoids about 17 KiB of soft-float/newlib
code on the 128 KiB target. Source-capable profiles retain normal float parsing.

## Build and deploy

Default full-feature bytecode firmware:

```powershell
python tools/build_fw.py
python tools/init_spiflash.py
```

`init_spiflash.py` auto-selects the CH340 (currently <serial-port>), formats the full
LittleFS volume, compiles every default source with the target ABI, installs
optional modules first, and installs/runs `main.luac` last. Use `--include-tests`
to also install `large_stream_test.luac`.

For a single file:

```powershell
python tools/compile_lua.py app.lua app.luac
python tools/upload_script.py app.luac --name app.luac
```

`compile_lua.py` invokes the native Windows compiler built by
`tools/build_luac.py` from the exact vendored Lua 5.5.1 sources with
`LUA_32BITS`. Do not use an unrelated system `luac`, because Lua version and
numeric ABI must match.

### GPUI IDE and LED smoke test

End users run the packaged IDE release:

```text
gpui_ide\dist\Lua IDE.exe
```

The IDE embeds the same target-ABI compiler, connects to the CH340 attached to
PA10/PA11 and performs the normal bytecode workflow:

- **Compile source and upload** compiles the editor with the exact vendored Lua
  5.5.1 + `LUA_32BITS` ABI, writes `main.luac`, and runs it. This is the normal
  workflow for the default full-feature firmware.
- **Compile/download .luac** produces a local bytecode file without using UART.
- **Upload existing .luac** accepts an already compiled target-ABI file.
- **Compile and save module** writes a named `.luac` for later `runfile()` use.

The IDE supports only the current `bytecode` firmware, `main.luac`, named
`.luac` modules and binary-safe HEX upload. Historical source upload, line
transport, `main.lua`, `source` and `source_full` compatibility were removed.

The supplied LED test can also be compiled manually:

```powershell
python tools/compile_lua.py mspm0_lua/scripts/led_blink.lua mspm0_lua/build_bytecode/led_blink.luac
```

The test drives PA14 for six 150 ms on/off cycles and prints
`LED_BLINK_START` / `LED_BLINK_DONE`. It was also uploaded and run over <serial-port>:

```powershell
python tools/upload_script.py mspm0_lua/build_bytecode/led_blink.luac --port <serial-port>
```

The `source` and `source_full` measurements at the top of this document are kept
only as historical size records. They are no longer release targets and future
firmware/IDE work updates only `bytecode`.

## Board verification

Both paths were tested with files larger than the removed 1.5 KiB buffer:

- 2,733-byte `large_stream_test.luac`: printed `STREAM_OK 2520 20100`.
- approximately 2.9 KiB generated `main.lua`: printed `SOURCE_STREAM_OK 20100`.

The final bytecode firmware was flashed and verified by J-Link. The PA10/PA11
CH340 is <serial-port>; <serial-port> is the J-Link CDC port. UART RX now uses a one-entry FIFO
interrupt and a single-producer ring buffer. Full-format initialization, three
acknowledged uploads, cold boot from `main.luac`, and directory listing were all
verified over <serial-port>.

The secondary-peripheral test was also compiled to target bytecode and run from
LittleFS. UART1, I2C1 and SPI0 each opened and closed, then MCAN completed two
open/loopback/close cycles at different bitrates. A subsequent GPIO1_C6
reset printed `PERIPHERAL_TEST_OK` followed by `Idle`, confirming that the test
terminates and that the same external `main.luac` boots cleanly. MCAN uses a
1 ms power-island settling delay and checks its functional-clock handshake;
the generic 16-cycle delay was too short for reliable run-time power-up at
80 MHz.

For the connected control board, both control GPIOs must normally be Hi-Z. After
J-Link halts/programs the MCU, merely releasing the pins does not resume it; an
empirically verified GPIO1_C6 low pulse followed by Hi-Z starts the application.
`hold_boot_flash.py` now performs that pulse after its fail-safe cleanup, and its
remote hold helper has both a PID guard and a 60-second hard timeout. No helper
process is intended to remain running after a flash operation.

The external SPI NOR is not XIP-capable on MSPM0G3507. Native ARM code still
cannot execute directly from it; native overlays would require copying a linked,
position-constrained image into SRAM. Lua source/bytecode modules are the safer
on-demand mechanism and already allow the current full hardware API to fit.
