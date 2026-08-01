# MSPM0 Lua — 固件与 IDE

![Build firmware](https://img.shields.io/github/actions/workflow/status/yiex/mspm0-lua/build-firmware.yml?branch=main&label=firmware%20build)
![Build IDE](https://img.shields.io/github/actions/workflow/status/yiex/mspm0-lua/build-ide.yml?branch=main&label=ide%20build)
![License](https://img.shields.io/github/license/yiex/mspm0-lua)

在 TI MSPM0G3507（Cortex-M0+，128 KB Flash / 32 KB SRAM）上运行 Lua 的开源
固件项目，配套一个原生 Windows IDE。脚本在电脑上编译为与固件匹配的
Lua 5.5.1 / `LUA_32BITS` 字节码，通过 CH340 串口上传到外置 SPI Flash 的
LittleFS，并由固件执行。

## 仓库结构

| 目录 | 内容 |
|------|------|
| `mspm0_lua/` | 固件源码：应用、board 驱动、Lua 绑定、原生模块、链接脚本与文档 |
| `gpui_ide/` | 基于 GPUI 的原生 Windows IDE 源码 |
| `tools/` | 构建、烧录、测试与部署脚本 |
| `examples/` | 示例工程（IMU + OLED 等） |
| `board_ctrl/` | 上位机板控/恢复工具 |

## 快速开始（构建固件）

1. 准备工具链与 SDK（可自动下载）：

   ```bash
   python tools/download_toolchain.py   # 下载 ARM GNU 工具链
   python tools/fetch_sdk_zip.py        # 下载 TI MSPM0 SDK 到 tools/mspm0-sdk
   ```

2. 构建 modular 固件及全部原生模块：

   ```bash
   export MSPM0_PROFILE=modular
   export MSPM0_SDK=$PWD/tools/mspm0-sdk
   python tools/build_native_module.py
   python tools/build_fw.py
   python tools/build_catalog_release.py
   python tools/build_protocol_fixtures.py
   ```

3. 组合烧录底镜像（核心 + 空功能槽）：

   ```bash
   python tools/compose_firmware.py --modules --output mspm0_lua/build_composed/firmware_core.bin
   ```

构建全部产出在 `mspm0_lua/build*` 目录，详见 [docs/BUILD.md](docs/BUILD.md)。
GitHub Actions 已配置为每次推送自动构建固件并上传产物（见
`.github/workflows/build-firmware.yml`）。

功能模块按**功能槽**独立部署：每个模块会为 8 个槽位分别生成
`build_modules/<模块>/slot0..7/<模块>.bin`，由 IDE 或
`tools/serial_module_set.py` 在运行时写入空闲槽位，而不是预烧进固件。
`firmware_core.bin` 是唯一推荐的整片烧录镜像（功能槽全空，不会残留旧模块）。

IDE 的构建与使用见 [gpui_ide/README.md](gpui_ide/README.md)；Windows 上的
IDE 构建也在 CI 中自动执行（`.github/workflows/build-ide.yml`）。

## 目标板（地猛星 MSPM0G3507）

| 功能 | 引脚 |
|------|------|
| 控制台 UART0 | PA10 TX / PA11 RX，115200 8N1（可协商 460800） |
| LED | PA14 |
| OLED I2C1 | PA15 SCL / PA16 SDA，0x3C，100 kHz |
| IMU UART2 | PA23 TX / PA24 RX |
| 外置 Flash SPI1 | PB14–17 + LittleFS（W25Q 等 SPI NOR） |
| BSL | PA18 BOOT + 复位（ROM BSL 默认 9600） |

## Lua 脚本示例

- `mspm0_lua/scripts/hi_led.lua` — LED 冒烟
- `mspm0_lua/scripts/hi_oled.lua` — SSD1306 OLED
- `mspm0_lua/scripts/hi_imu_oled.lua` — IMU + OLED 实时刷屏
- `mspm0_lua/scripts/sdk_demos/` — 对应 TI SDK 例程的 Lua 移植
- `examples/imu_oled_project/` — 完整示例工程

## 文档

- `mspm0_lua/docs/API_REFERENCE.md` — Lua API 参考
- `mspm0_lua/docs/HOST_INTEGRATION.md` — 串口协议与模块部署
- `mspm0_lua/docs/NATIVE_MODULES.md` — 原生模块机制
- `mspm0_lua/docs/FLASH_BUDGET.md` — Flash/堆预算
- `gpui_ide/README.md` — IDE 构建与使用

## 参与贡献

构建与测试方法见 [docs/BUILD.md](docs/BUILD.md)。
变更记录见 [CHANGELOG.md](CHANGELOG.md)。

## License

项目代码以 MIT 许可发布（见 [LICENSE](LICENSE)）。第三方组件许可见
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
