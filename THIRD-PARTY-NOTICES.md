# Third-party notices

本仓库的代码以 MIT 许可发布（见 [LICENSE](LICENSE)）。以下第三方组件
按各自许可证使用；许可证全文随组件保留或收录在
[LICENSES/](LICENSES/README.md)。

## 仓库内随附的组件

| 组件 | 许可证 | 说明 |
|------|--------|------|
| Lua 5.5.1（`mspm0_lua/third_party/lua`） | MIT | Lua.org, PUC-Rio；许可证文本见 `third_party/lua/LICENSE` |
| littlefs（`mspm0_lua/third_party/littlefs`） | BSD-3-Clause | littlefs authors / Arm Limited；许可证文本见 `third_party/littlefs/LICENSE` |
| TI MSPM0 设备头文件（`mspm0_lua/third_party/mspm0`） | BSD-3-Clause | Texas Instruments；许可证文本见 `third_party/mspm0/LICENSE` |
| GPUI（`gpui_ide/vendor/gpui`，含本地修改） | Apache-2.0 | Zed Industries；`LICENSE-APACHE` 随源码保留。本仓库对其进行了本地修改以适配 IDE，修改内容未单独分发到上游 |
| Tuffy 字体（`gpui_ide/font/Tuffy.ttf`） | 公有领域 | 声明见 `gpui_ide/font/Tuffy-LICENSE.txt` |
| `fxc.exe` / `d3dcompiler_47.dll`（`gpui_ide/tools/fxc`） | Microsoft（专有） | Windows SDK 组件，仅用于 IDE 构建期 shader 编译；不随发布产物分发，请遵守 Microsoft 软件许可条款 |
| `tools/serial_probe_rs` | MIT（本项目代码） | 仅使用 crates.io 上的 MIT/Apache 依赖 |

## Rust crate 依赖（IDE 与工具）

构建解析到的 crate 已逐版本核对 crates.io 声明（约 1000 个版本），绝大多数为
MIT、Apache-2.0 或其双许可。需要注意的类别：

### MPL-2.0（文件级弱 copyleft）

以下 crate 链接进 IDE/工具二进制或构建流程，按 MPL-2.0 第 3 条，其源代码
可从 [crates.io](https://crates.io) 对应版本页面获取，许可证全文见
[LICENSES/MPL-2.0.txt](LICENSES/MPL-2.0.txt)：

- `serialport`（运行时，串口通信）
- `dwrote`（运行时，Windows DirectWrite 绑定，经 zed-font-kit 引入）
- `option-ext`（运行时，经 dirs-sys 引入）
- `cbindgen`（构建期工具，仅编译时使用，不链接进产物）

MPL-2.0 不要求整个作品开源，仅要求这些文件本身以 MPL-2.0 提供且源码可取；
本项目未修改这些 crate。

### 多许可证中选择宽松项

- `self_cell`：Apache-2.0 OR GPL-2.0-only → 按 Apache-2.0 使用
- `unescaper`：MIT OR GPL-3.0-only → 按 MIT 使用
- `r-efi`：MIT OR Apache-2.0 OR LGPL-2.1-or-later → 按 MIT 使用

### 其他宽松许可证

- Unlicense OR MIT / Unlicense（memchr、byteorder、aho-corasick、globset、
  walkdir、same-file、winapi-util、jiff、termcolor 等）→ 按 MIT/公有领域使用
- BSD-2-Clause / BSD-3-Clause（少量）
- Zlib、ISC、CC0-1.0、MIT-0（少量）
- Unicode-3.0（ICU4X 数据包，约 36 个）
- `av-scenechange`：MIT

## 构建期下载、不随仓库分发的组件

| 组件 | 许可证 | 说明 |
|------|--------|------|
| ARM GNU 工具链（`tools/arm-gnu-toolchain`） | GPL-3.0 + GCC Runtime Library Exception | 由 `tools/download_toolchain.py` 下载。固件链接 libgcc/newlib 受 GCC Runtime Library Exception 保护，固件可任意许可 |
| newlib / nano.specs | BSD 系宽松许可 | 工具链自带，仅构建期使用 |
| TI MSPM0 SDK（`tools/mspm0-sdk`） | BSD-3-Clause | 由 `tools/fetch_sdk_zip.py` 下载；下载即表示接受 TI 的 SDK 条款 |
| Zig（用于构建 `luac_mspm0`） | MIT | 由 `tools/build_luac.py` 按固定版本下载并校验 SHA-256 |

## 商标与字体说明

- `config.json` 中引用的 `SimHei.ttf` 是 Windows 系统字体，**不随仓库或发布
  包分发**；IDE 仅在用户系统存在该字体时使用，缺失时回退到内置字体。
- 地猛星、Luckfox、GPUI、SEGGER J-Link 等为各自所有者的商标，本仓库仅作
  事实性引用。
