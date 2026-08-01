# 构建指南

本文档描述如何从源码重建全部固件产物以及打包 IDE。

## 前置条件

- Python 3.9+
- ARM GNU 工具链（`arm-none-eabi-gcc`，14.2.rel1 已随脚本验证）
- TI MSPM0 SDK（`tools/mspm0-sdk`，仅需 `source/ti/devices`、
  `source/ti/driverlib` 与 `source/third_party/CMSIS`）

工具链与 SDK 均可由脚本自动下载：

```bash
python tools/download_toolchain.py
python tools/fetch_sdk_zip.py
```

`download_toolchain.py` 会按当前系统选择 Windows 或 Linux 的 ARM GNU
工具链归档；`fetch_sdk_zip.py` 从 TI 官方仓库拉取 SDK 并只解出需要的目录。

## 环境变量

| 变量 | 作用 | 默认 |
|------|------|------|
| `MSPM0_SDK` | TI MSPM0 SDK 根目录 | `mspm0_lua/third_party/mspm0_sdk` |
| `MSPM0_TOOLCHAIN` | ARM GNU 工具链根目录 | `tools/arm-gnu-toolchain` |
| `MSPM0_PROFILE` | 固件 profile：`source` / `source_full` / `bytecode` / `modular` | `bytecode` |
| `JLINK_EXE` | J-Link 可执行文件路径 | 常见安装路径 |
| `LUCKFOX_HOST` / `LUCKFOX_USER` / `LUCKFOX_PASS` | 板控 SSH（可选，用于 hold/reset） | 未设置则跳过 |

## 重建 modular 固件（推荐）

```bash
export MSPM0_PROFILE=modular
export MSPM0_SDK=$PWD/tools/mspm0-sdk

# 1. 构建每个原生模块的全部 slot 变体，并生成 build_modules/index.json
python tools/build_native_module.py

# 2. 构建 modular core（内部会先生成 catalog 身份头）
python tools/build_fw.py

# 3. 生成 release/catalog_manifest.json 与 release/docs
python tools/build_catalog_release.py

# 4. 重新生成 API 元数据（firmware.build_id 必须等于 catalog SHA-256），
#    然后刷新 manifest 里的 API 产物哈希
python tools/build_api_metadata.py
python tools/build_catalog_release.py

# 5. 重新生成 NMUP 协议测试向量
python tools/build_protocol_fixtures.py

# 6. 组合烧录底镜像（核心 + 空功能槽）
python tools/compose_firmware.py --modules --output mspm0_lua/build_composed/firmware_core.bin
```

功能模块按槽位独立部署，不预烧进固件。每个模块会生成
`build_modules/<模块>/slot0..7/<模块>.bin` 共 8 个地址变体，IDE 或
`tools/serial_module_set.py` 在运行时把选中的模块写入空闲槽。`sets` 字段
（`io`、`analog`、`serial`、`full` 等）只是组合快捷方式，可用于本地试验：

```bash
python tools/compose_firmware.py --set full --output /tmp/firmware_full.bin
```

可用集合定义在 `mspm0_lua/modules/modules.json` 的 `sets` 字段：
`core`、`io`、`timed_io`、`analog`、`serial`、`full`、`analog_io`、
`analog_monitor`、`integrity`、`clocked_io`、`demo`。

## 其他 profile

```bash
export MSPM0_PROFILE=bytecode   # 字节码固件（IDE 默认目标）
python tools/build_fw.py

export MSPM0_PROFILE=source     # 精简源码固件
python tools/build_fw.py

export MSPM0_PROFILE=source_full
python tools/build_fw.py
```

## 测试

纯主机侧回归测试（无需硬件）：

```bash
python tools/test_module_catalog.py
python tools/test_module_update_bundle.py
```

需要硬件（串口 + 烧录后的 modular 固件）：

```bash
python tools/test_modular_api_surface.py --port <serial-port>
python tools/test_serial_module_update.py --port <serial-port>
```

## 烧录

```bash
# J-Link SWD 烧录完整镜像（不会触碰锁定 BCR 配置段）
python tools/jlink_flash.py mspm0_lua/build_composed/firmware_core.bin

# 需要上位机 hold/reset 时（如板载 J-Link 连接受 BSL 引脚影响），先设置
# LUCKFOX_HOST / LUCKFOX_USER / LUCKFOX_PASS，再：
python tools/hold_boot_flash.py mspm0_lua/build_modular/mspm0_lua_modular.bin
```

## 构建 IDE（Windows）

见 `gpui_ide/README.md`。开发构建 `build.ps1`，发布构建
`build_release.ps1`（后者会先要求固件产物齐全，再打包进 `dist/`）。

## CI

`.github/workflows/build-firmware.yml` 在 Ubuntu runner 上自动完成工具链/SDK
下载、全部固件构建、测试与产物上传；打 tag 时还会附加为 GitHub Release。
