# MSPM0 Lua IDE (GPUI)

基于 [GPUI](https://gpui.rs) 的原生 Windows IDE，面向 MSPM0G3507 上的
`mspm0_lua` 字节码固件。编辑器内源码在本机编译为与固件匹配的
Lua 5.5.1 / `LUA_32BITS` 字节码，再通过 CH340 串口上传并运行。

## 用户视角

主操作只有两步：

1. **连接** — 选择串口名（CH340 自动标注；J-Link CDC 会提示区分），点连接。
2. **运行** — 一键完成：源码 → luac(目标 ABI) → HEX 上传 `main.luac` → 执行。
   快捷键：`F5` / `Ctrl+Enter` 运行，`Esc` 停止。

次要操作：

| 控件 | 作用 |
|------|------|
| 保存模块 | 编译上传为自定义 `.luac`，给 `runfile()` |
| 下载 .luac | 仅本机编译，不连串口 |
| 重跑 main | 不重新编译，快捷键 `r` |
| 外置 Flash | 列表 / 设为启动 / 删除 |
| 示例板 | LED / 可停循环 / UART / I2C / SPI / CAN / 模块 |

## 运行

```text
gpui_ide\start_ide.cmd
```

或直接运行已打包的 `gpui_ide\dist\Lua IDE.exe`。Lua 编译器已内置，也可通过
`MSPM0_LUAC` 指定外部编译器。

## 可移植数据目录

IDE 只从可执行文件旁读取以下目录和文件：

- `chips/mspm0g3507.json` — 芯片可编程引脚及功能复用名
- `boards/LKDMX.json` — 开发板实际引出脚与外置 Flash 标注
- `apis/mspm0g3507_lua.json` — 当前固件完整 API
- `firmware/` — 基础固件、模块 catalog 与模块镜像
- `example/<project>/` — 完整示例工程
- `font/` — 用户自行放入并有使用权的 OLED 字体
- `config.json` — 开发板、串口、传输模式和界面设置记忆

顶部「开发板」菜单自动读取 `boards/*.json`，显示文件内的中文 `name`。仅一个
开发板时自动选中；多个时首次启动需选择，之后可从菜单切换。切换成功后原子重载
API/Board/Chip 三份文件，自动补全和纠错立即使用新目标。

「运行」菜单可全程使用 115200 低速模式，或由固件命令临时切换到 460800 高速
模式。工程运行会从 `main.lua` 递归追踪 `require` 和 `runfile`，编译所有可到达的
Lua 文件；本次 IDE 进程内若未新增原生模块，则保持已部署模块，只上传修改后的
依赖字节码并重新执行入口。IDE 退出后这份增量记忆自动清空。

文件规则详见 `docs/file-rules.md`。

## 构建（Windows）

前置条件：

- Rust stable + `x86_64-pc-windows-gnullvm` 目标（`rustup target add x86_64-pc-windows-gnullvm`）
- LLVM-MinGW（UCRT x86_64），可用 `LLVM_MINGW` 环境变量指定安装根目录
- `tools/fxc/fxc.exe`（已随仓库提供，用于 GPUI shader 编译）

开发构建：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\build.ps1
```

产物：

```text
target\x86_64-pc-windows-gnullvm\debug\mspm0-lua-ide.exe
```

发布构建（含固件打包）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\build_release.ps1
```

## 设计要点

- 原生窗口，不依赖浏览器 Web Serial
- 串口列表可见，明确区分 CH340 / J-Link
- 主路径收敛为「连接 → 运行」，其余降级为次要
- 控制台实时 RX，状态与错误分色
- 内嵌 Lua 5.5.1 / `LUA_32BITS` 编译器，也可用 `MSPM0_LUAC` 指定外部编译器

## 原生串口遥测视图

数据可视化不是常驻面板。需要时可在「视图」菜单选择「在编辑器右侧打开」「作为
独立窗口打开」或「合并为首位标签」，默认入口仍为代码编辑器右侧的独立工具区；
关闭后不占用编辑空间。拖动右侧工具区标题或锁定的可视化标签时，释放到代码区域
会切换为右侧分屏，释放到标签栏会合并为锁定的第一个标签，释放到其余区域或主窗口
之外则会脱离为独立窗口。独立窗口右上角的「返回编辑器」按钮可恢复右侧分屏。
三种形态继续使用同一份实时数据和控制状态，工具页不会进入源码文件列表，也不会
参与保存或关闭文件操作。

工具区提供「曲线/姿态」两种视图，支持最多 12 个通道、暂停、逐通道或批量显隐、
自动/手动量程、纵向缩放和时间窗调整。曲线默认以共享坐标系「叠加」显示全部可见
信号，也可切换为「分轨」，让每路信号使用独立纵向量程，避免不同数量级的数据互相
压缩。串口每行可使用以下任一文本格式：

```text
12.5,20,-3                    # CSV -> CH1 / CH2 / CH3
temperature:24.5,rpm=1200    # 命名通道
{"roll":1.2,"pitch":-0.8}   # JSON 数值对象
raw 2048 mv 1650             # key value
```

姿态视图识别 `roll/pitch/yaw`（也兼容 `heading`）、陀螺仪 `gx/gy/gz` 或
`gyro_x/gyro_y/gyro_z`，以及加速度 `ax/ay/az` 或 `accel_x/accel_y/accel_z`。
姿态角默认单位为度；弧度数据请使用 `roll_rad/pitch_rad/yaw_rad`。例如：

```text
roll:12.5,pitch:-4.2,yaw:86,gx:0.8,gy:-0.2,gz:1.1,ax:0.02,ay:-0.01,az:0.99
```

Lua 示例位于 `example/telemetry`。串口接收会先重组完整行，再去除 ANSI 转义、
不可打印控制字符、固件启动信息、文件列表与 HEX/脚本握手回显；脚本自己的普通
文本和数值帧仍会保留。

## 协议（与固件一致）

- 上传：`<<<HEX name` → 64B hex 块 + `HEX_OK` → `>>>HEX` → `SCRIPT_OK`
- 列表：`ls` → `LS_END`
- 运行 / 停止：`r` / `!`
- 启动 / 删除：`boot name` / `rm name`
- 进 ROM BSL：`bsl`（软复位进 bootloader；也可 PA18 BOOT + 复位）

## UART 烧录固件（ROM BSL）

菜单「目标 → UART 烧录固件」（CH340，与 BSLTX/BSLRX 同网）：

1. 有可运行应用时：先 `bsl` 软进入，再从 **9600** 起 Connection → Unlock →
   MassErase → ProgramData → StartApp
2. 空白片 / 应用挂起：按住 **PA18(BOOT)** 复位进 BSL，用「UART 烧录（已进
   BSL）」项
3. 镜像：`mspm0_lua_bytecode.bin`（默认搜索 `build_bytecode` / kit 目录）

说明：ROM BSL 默认 9600；密码默认 32×`0xFF`。烧录会断开当前 115200 会话。
