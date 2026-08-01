# 模块化 OLED 自动取模

模块化 Core 1.0.2 不常驻 OLED 驱动或字体。IDE 在工程使用 OLED 时自动组合：

```text
i2c 原生模块槽 + oled.luac + _oled_font.luac + 工程 LUAC
```

其中只有 `i2c` 写入内部原生模块槽。`oled.luac` 和字模是普通串口动态上传文件，
不需要 BSL、复位或重新烧录 Core。

## 用法

```lua
local oled = require("oled")

oled.open(1, "PA15", "PA16", 0x3c, 100000)
oled.clear()
oled.text(0, 0, "温度", 16)
oled.number(0, 16, 253, 1, 8)
```

`oled.clear()` 自动包含 `0x00`；`oled.fill(0xaa)` 等静态 `0..255` 填充值由 IDE
按需打包。精简 Core 没有动态字节转换函数，所以 `oled.fill(variable)` 会在串口传输前
报错。生成字模按显示页保存为二进制字符串，不展开为逐字节整数表，以适配 21 KiB Lua 堆。

所有硬件参数必须明确给出。示例表示 SSD1306 使用 I2C1、PA15 SCL、PA16 SDA、
7 位地址 0x3C 和 100 kHz。

## 取模规则

- IDE 追踪 `main.lua` 的完整静态依赖闭包。
- 识别 `oled.text(..., 8|16)`、`oled.number(..., 8|16)` 及
  `local alias = require("oled")` 的别名调用。
- 注释和普通字符串中的 `oled.text` 不会触发取模。
- 字符串常量按调用中的字号取模。
- 变量内容无法静态获知，因此每个使用的字号总是包含数字 0..9、小数点、负号和空格；
  动态数值使用 `oled.number`，`oled.text` 只接受可静态取模的字符串常量。
- 中文和 ASCII 分别使用 IDE 设置中选择的中文/英文字体。
- 字号必须是静态的 8 或 16；动态字号会在传输前报错。

生成文件 `_oled_font.luac` 是一个按字号和 Unicode 码点索引的 Lua 数据模块，
`oled.luac` 在运行时进行 UTF-8 解码并直接消费该数据。旧 `_run.f16`、`_run.fnt`、
手写数字点阵和 monolithic `oled.*` 不属于当前模块化路径。

## 上传顺序

```text
_oled_font.luac -> oled.luac -> 其他依赖 LUAC -> main.luac
```

同一 IDE 进程内使用 SHA-256 记录上传成功的内容。后续只修改 Lua 且没有新增原生功能时，
不重复写模块槽；未变化的字模和依赖也跳过。关闭 IDE 后缓存清空，下一次运行重新核验。

## 错误与上限

- I2C 实例/路由/地址/速率错误：`oled:i2c_*`。
- 未调用 `open`：`oled:not_open`。
- UTF-8 非法：`oled:utf8`。
- 缺字号或缺字：`oled:font_size:*` / `oled:glyph:*`。
- 坐标未按 8 像素对齐或越过 128x64：`oled:position` / `oled:text_bounds`。
- IDE 限制单次最多 192 个字模、12 KiB 位图数据、20 KiB 字模 LUAC。
- 字体缺失、格式不支持、无法栅格化或运行库哈希不匹配均在串口操作前硬失败。

完整上位机事务和目录约定见 IDE 分发中的 `docs/modular-run.md`。
