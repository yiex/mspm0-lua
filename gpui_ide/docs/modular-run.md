# IDE 自动组合、取模与上传协议

本文对应 MSPM0G3507 模块化 Core 1.0.2 和当前 Lua IDE。正常开发只使用“运行”，
不使用 `full`，也不需要再次进入 BSL 或重启开发板。

## 1. 分发目录

IDE 可执行文件旁的用户数据保持以下结构：

```text
Lua IDE.exe
chips/
  mspm0g3507.json
boards/
  LKDMX.json
apis/
  mspm0g3507_lua.json
firmware/
  release/
    catalog_manifest.json
    lua/oled.lua
  modules/modules.json
  build_modules/
  build_modular/mspm0_lua_modular.bin
example/
  <project>/main.lua
font/
config.json
```

`chips/mspm0g3507.json` 表示芯片，不表示某个 48 引脚封装。它由最大引脚版本的
60 个 GPIO（PA0..PA31、PB0..PB27）生成，内容只有引脚和该引脚可实现的复用功能。
`boards/LKDMX.json` 只列地猛星开发板实际对外引出的 30 个引脚。两者不能合并。

`apis/mspm0g3507_lua.json` 是当前 IDE 的唯一最新 API 表，也用于补全、参数检查和
引脚路由纠错。`firmware/` 同时存基础固件、原生模块镜像和按需 Lua 功能库。

## 2. 开发板选择

IDE 启动时扫描 `boards/*.json`：

- 只有一个开发板时自动选择。
- 多个开发板且没有历史选择时弹出选择框。
- 选择以 JSON 的 `name` 显示，可使用中文。
- 稳定文件 ID（例如 `LKDMX`）写入 `config.json`。
- 后续可从顶部“开发板”菜单切换。

开发板选择会影响引脚补全和资源校验。API 允许但开发板未引出的芯片引脚不会作为
开发板引脚推荐。

## 3. 工程依赖分析

工程入口固定为 `main.lua`。IDE 对 Lua token 做静态分析，忽略注释和普通字符串中的
伪调用，并递归跟踪静态 `require` 和 `runfile`：

```text
main.lua -> A.lua -> B.lua
```

只有从入口可达的文件会编译和上传，顺序为 `B.luac -> A.luac -> main.luac`。
循环引用会被运行时缓存保护；动态 `require(variable)` 无法静态确定时，工程必须在
`mspm0_lua.json` 的 `native_modules` 明确声明原生模块，IDE 不会退回 `full`。

原生 API 调用和静态模块引用共同决定最小原生模块集合。例如：

```lua
local oled = require("oled")
oled.open(1, "PA15", "PA16", 0x3c, 100000)
oled.text(0, 0, "123", 16)
```

该工程只选择原生 `i2c`，不会选择 GPIO、UART 或所谓的 `full`。

## 4. OLED 自动取模

`oled` 不是常驻 Core，也不是额外的内部 Flash 原生槽。它由三个按需部分组成：

| 部分 | 生成位置 | 作用 |
|---|---|---|
| `i2c.bin` | 固件目录 | 唯一写入内部原生模块槽的组件 |
| `oled.luac` | IDE 编译 `firmware/release/lua/oled.lua` | SSD1306、UTF-8、边界和错误处理 |
| `_oled_font.luac` | IDE 每次按工程生成 | 本工程实际使用的各字号字模 |

IDE 识别 `oled.text(x, y, value, size)` 和
`oled.number(x, y, value, decimals, size)`，也识别以下别名形式：

```lua
local display = require("oled")
display.text(0, 0, "温度", 16)
```

当前支持 8 px 和 16 px。字号必须是静态整数，这样 IDE 才能确定字模布局；字号为变量
或使用其他尺寸会在接触设备前报错。每个实际使用的字号都会自动加入：

```text
0 1 2 3 4 5 6 7 8 9 . - 空格
```

因此通过 `oled.number` 显示的变量数值不需要静态解析。`oled.text` 的文本参数必须是
字符串常量，中文、英文和符号按该次调用的字号加入；任意动态文本无法可靠预取字模，
IDE 会要求改用静态文本或 `oled.number`。
中文使用设置中的中文字体，ASCII 使用设置中的英文字体。字体优先从 `font/` 读取；
文件不存在、格式错误、无法栅格化、缺字、字模数超过 192、位图超过 12 KiB，或生成的
LUAC 超过 20 KiB时，运行会明确失败。

旧 `_run.f16`、`_run.fnt` 和示例中的手写数字点阵不属于模块化流程，IDE 不再上传它们。

## 5. OLED API

所有硬件参数都必须显式填写，不提供默认引脚：

```lua
local oled = require("oled")

oled.open(i2c_id, scl_pin, sda_pin, address_7bit, hz)
oled.clear()
oled.fill(byte_value)
oled.text(x, y, text, size)
oled.number(x, y, fixed_integer, decimals, size)
oled.close()
```

`oled.clear()` 会自动打包 `0x00`。`oled.fill(byte_value)` 的值必须是静态整数
`0..255`（支持 `0xNN`）；IDE 只把工程实际使用的填充值加入 `_oled_font.luac`。
当前模块化 Core 没有动态字节转换函数，因此 `oled.fill(variable)` 会在上传前明确报错，
不会在 21 KiB Lua 堆中展开 256 项字节表。

约束：

- `i2c_id` 只能为 0 或 1。
- SCL/SDA 必须组成同一 I2C 实例的合法复用路由。
- 7 位地址范围为 `0x08..0x77`。
- 速率范围为 10 kHz..1 MHz；示例明确使用 100 kHz。
- `y` 必须按 8 像素对齐。
- 文本不得越过 128x64 显示边界。
- 缺字、UTF-8 错误、I2C 写失败均抛出稳定的 `oled:*` 错误。

`oled.number` 的 `value` 是定点整数。例如 `value=253, decimals=1` 显示 `25.3`。

## 6. 串口事务

低速模式保持 115200。高速模式先在 115200 完成身份确认，再使用固件命令切换到
460800；事务结束主动切回 115200。CH340 只承担普通应用串口，不进入 BSL。

一次运行的严格顺序：

```text
本地校验 catalog、API、依赖、字体并编译全部 LUAC
-> 发送 ! 并等待 STOP
-> fwinfo / modstatus / storageinfo 校验
-> 可选切换到 460800
-> 原生模块槽不匹配时上传并应用 NMUP
-> 等待 MOD_DONE 和 Idle，再次核验模块布局
-> 上传 _oled_font.luac（工程使用 OLED 文本时）
-> 上传 oled.luac（工程使用 OLED 时）
-> 上传可达依赖 LUAC
-> 最后上传 main.luac 并执行
-> 等待 SCRIPT_DONE OK
-> 最终核验模块布局
-> 切回 115200
```

任何本地校验或编译失败都发生在 Flash 写入之前。原生模块事务没有达到
`MOD_DONE + Idle` 时禁止继续上传 Lua。模块事务已完成但 Lua 阶段失败时不回滚已经
验证成功的模块布局，修正问题后只需重试 Lua 阶段。

## 7. IDE 会话增量规则

会话缓存只存在于 IDE 进程内，关闭 IDE 后清空：

- 第一次运行：核验/部署最小原生模块集合，上传所有可达 LUAC。
- 只修改 Lua：跳过模块写入；未变化的依赖、OLED 运行库和字模 LUAC不重复上传，
  `main.luac` 始终上传以触发本次运行。
- 新增功能：把新原生模块加入本会话已有集合，生成新的槽布局并事务更新。
- 删除 Lua 中的功能引用：同一会话内不主动缩减槽集合，避免反复擦写；重启 IDE 后按
  新工程重新计算最小集合。
- 更换工程或 catalog：缓存隔离，不复用旧哈希。

每个已编译 LUAC 以 SHA-256 判断是否变化。`oled.lua` 的发布哈希固化在 IDE 中，缺失、
版本混用或被修改会在串口操作前拒绝运行。

## 8. 自动纠错与补全

IDE 从当前开发板、芯片和 API 三份 JSON 联合生成补全：

- API 名称和参数数量补全。
- 只推荐当前开发板对外引出的引脚。
- I2C SCL/SDA 按实例和路由成对过滤。
- 不存在的 API、缺少的 Lua 文件、非法文件名和超过 8 个原生槽都在运行前报告。
- OLED 缺字号、动态字号、字体缺失、缺字、越界风险和容量超限分别给出明确错误。

示例不得使用默认引脚，所有 `uart/i2c/spi/can/pwm/adc/oled` 调用均应填写具体实例和
具体引脚。

## 9. 上位机实现要点

第三方上位机必须把 NMUP 和 LUAC 视为同一用户操作的两个阶段，但不能伪装成原子回滚：

- 始终以设备 `fwinfo`、`modstatus`、`fileinfo` 为真值。
- 校验 catalog SHA-256、ABI、模块格式、slot 地址、镜像长度、CRC 和模块名。
- HEX 每块等待完整 `HEX_OK`，超时后不要盲目重发同一块。
- `SCRIPT_OK <bytes>` 只表示文件写入，不能替代 `SCRIPT_DONE OK`。
- 发生超时后依次探测 460800 和 115200，通过完整 `fwinfo` 确认当前速率，最后恢复
  115200。
- 不要在正常项目运行中部署 `full`。它只用于 8 槽边界与恢复测试。

Core 1.0.2 当前 catalog SHA-256：

```text
c1609433a2bc70d4d991de00454f23f377fd71021f523447a3e00d61d4e2d23c
```

本次 OLED 自动取模实现没有修改 Core 或原生 `i2c.bin`，所以已烧录 Core 1.0.2 的板卡
无需重新烧录基础固件。
