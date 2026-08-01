# Changelog

本项目的变更记录。格式参考
[Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### Added

- 首次开源发布：MSPM0G3507 Lua 固件（modular / bytecode / source /
  source_full 四种 profile）与 GPUI 原生 Windows IDE。
- 原生模块机制：8 个 4 KiB 运行槽、NMUP v1 事务更新、catalog 身份与
  SHA-256 校验。
- LittleFS 外置 Flash 文件系统：HEX 流式上传、`require` / `runfile`、
  启动文件与可打断循环。
- 自动构建 CI：`build-firmware`（工具链/SDK 自动下载、全部固件产物、
  回归测试、Release 附件）与 `build-ide`（Windows + LLVM-MinGW）。
- 主机侧工具：工具链/SDK 下载、Lua 编译、烧录、协议测试与板控脚本。

### Changed

- 构建脚本改为跨平台（Linux/Windows），机器相关路径与凭据全部改为
  环境变量，并移除旧版 HTML IDE（web_ide）。

## [1.0.6] - 2026-07-30

归档发布基线（`modules.json` catalog `1.0.6`）：modular 核心、
13 个原生模块、完整 API 元数据与测试向量。
