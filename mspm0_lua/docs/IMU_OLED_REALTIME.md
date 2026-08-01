# IMU + OLED realtime reliability notes

经验总结：ATK-601/901（UART）+ SSD1306（I2C）在 MSPM0G3507 **bytecode** 固件上的长稳运行、刷屏卡顿与“可预测”主循环设计。

目标场景：

- 控制台：UART0 PA10/PA11 @ 115200（<serial-port> / CH340）
- IMU：UART2 **PA23 TX / PA24 RX** @ 115200（交叉接线）
- OLED：I2C1 **PA15 SCL / PA16 SDA**，地址 **0x3C**，推荐 **100 kHz**（外置 4.7k 上拉）
- 产品路径：PC `luac` → 字节码上传 LittleFS；板端 **无源码编译器**；Lua 堆默认 **24 KiB**

参考脚本：`scripts/hi_uart_atk_oled.lua`（UART 姿态解析与 OLED 显示）。

---

## 1. 现象与根因对照

| 现象 | 常见根因 | 处理方向 |
|---|---|---|
| 屏完全不亮 / `oled i2c write` | 模组未供电、无共地、无上拉、进 BSL（**PA18**） | 先软/硬扫地址；优先 **PA15/16**，勿用 PA18 |
| 上电后一段时间无 ACK，重上电又好 | OLED 掉电/总线卡死 | 固件 soft flush + 首次 open 可 bus recover |
| 能刷一会儿然后整程序卡死 | I2C 写失败未 `pcall` 崩脚本；或 recover 过重 | 热路径 `pcall`；失败**跳过本帧**，不全量 recover |
| 大角度转时刷屏卡顿 | 一帧刷 R/P/Y 太重，主循环长时间在 I2C | **每 tick 只刷一轴**；I2C 等待中 `board_uart_app_poll()` |
| 角度串口停、屏停 | UART ring 满丢新数据；或主循环饿死 | ring 512 + 满丢旧；循环固定上限 drain |
| `not enough memory` | `require` 大字库 + 多模块同时驻留 24K 堆 | 热路径单文件精简字表；大模块按需、冷路径加载 |
| 运行“有时快有时慢” | 热路径字符串拼接 / `bytes()` 触发 GC | `i2c.writev` + 固定表；慢路径再 `print` |

**结论：** I2C1 与 UART2 **无引脚冲突**；卡顿/卡死主要是 **主循环预算**、**I2C 事务量**、**错误处理** 与 **堆分配** 问题，不是“外设互斥”。

---

## 2. 固件侧约定（bytecode）

### 2.1 UART 应用口（UART1/2/3）

实现：`board/board_uart_app.c`

- **RX 中断 + 环形缓冲**（当前 `APP_UART_RING = 512`），**不是 DMA**
- ISR 只推 ring；Lua `uart.rx(id, timeout, max)` 从 ring 读
- ring 满：**丢最旧字节**，保证新 IMU 帧仍能进缓冲
- `board_uart_app_poll()`：主循环 / I2C busy-wait 时把 HW FIFO 再抽进 ring

```lua
uart.open(2, "PA23", "PA24", 115200)
local chunk = uart.rx(2, 0, 64)  -- timeout 0 = 非阻塞
uart.close(2)
```

### 2.2 I2C1

实现：`board/board_i2c1.c`

- 脚位：`PA15/PA16` 或 `PA17/PA18`（SCL/SDA），PF=4，Hi-Z + 内部上拉作补充（**外置上拉仍建议**）
- 写路径：预填 TX FIFO → `startControllerTransfer` 全长 → 等待控制器和总线都空闲（期间 `board_uart_app_poll()`）
- 空闲判定同时检查 `MSR.BUSY` 与 `MSR.BUSBSY`；不能只看控制器 FSM
- NACK/仲裁失败：关闭 ACTIVE、清 FIFO/传输状态、完整重配 I2C1，并自动重试一次
- 超时/总线忙：在重配前用开漏 GPIO 发 9 个 SCL 脉冲 + STOP；若仍忙再恢复一次
- 成功判定：以 **`MSR.ERROR`** 为准（避免 sticky ACK 位误报）
- 超时有界（约 10–30 ms），禁止无限等
- `board_i2c1_t` 带代次校验；底层 `i2c.open(1, ...)` 抢占后，旧 OLED 句柄不再误报 ready

恢复回归脚本：`scripts/i2c_recovery_test.lua`。它连续制造 NACK、探测
`0x3C`、关闭并重开 I2C1；无屏时也应在有限时间内打印
`I2C_RECOVERY_OK`，不能卡在 connected/busy 状态。

### 2.3 `i2c.writev`（零 Lua 字符串分配）

绑定：`lua_bind/lua_bind.c`

```lua
-- i2c.writev(id, addr7, b0, b1, ...)  栈缓冲，热路径优先
assert(i2c.writev(1, 0x3c, 0x00, 0xae))          -- 单字节命令
assert(i2c.writev(1, 0x3c, 0x40, d0,d1,d2,d3,d4,d5)) -- 6 字节字形数据
```

热路径 **不要** 反复 `bytes(...)` / `..` 拼包，否则 24K 堆下 GC 抖动明显。

### 2.4 PA18 / BSL

复位时若 PA18 被上拉，可能进 BSL（PC 落在 `0x0100xxxx`）。OLED **默认不要接 PA18**。

---

## 3. 主循环模板（可预测）

原则：**每圈工作量有上界**；外设失败不杀脚本；分配只在冷路径。

```lua
uart.open(2, "PA23", "PA24", 115200)
-- oled init once (cold)
local tid = tmr.every(20)
local axis = 0
local dirty = {1, 1, 1}  -- R/P/Y

while not stopped() do
  -- 1) 有界 drain UART
  local n = 0
  local c = uart.rx(2, 0, 64)
  while c and n < 12 do
    push(c); parse()          -- parse 内也要 steps 上限
    n = n + 1
    c = uart.rx(2, 0, 64)
  end

  -- 2) 轻量刷新：一 tick 最多一轴
  if tmr.ready(tid) then
    if dirty[axis + 1] == 1 then
      local ok = pcall(function() paint_axis(axis) end)
      if ok then dirty[axis + 1] = 0 end
      -- 失败：跳过本帧，禁止热路径 reopen/清屏
    end
    axis = (axis + 1) % 3
  end

  -- 3) 日志 ≤ 1 Hz（允许少量字符串）
  yield()
end
```

解析 IMU 帧（`55 55 ID LEN DATA SUM`）时：

- 姿态常用 **ID=1**（部分固件兼容 `0x53`）
- 定点：`ATT_x10 = raw * 1800 // 32768`，显示一位小数即可

---

## 4. OLED 刷屏预算

SSD1306 **页寻址**（init 中 `0x20, 0x02`）才能 `set_cursor` 局部更新。

| 做法 | 事务量（量级） | 评价 |
|---|---|---|
| 每字 `set_cursor` + 写 6 字节 | 每字 4+ 次 I2C | 差 |
| 一行 6 字全刷 × 3 行 / 20ms | 一帧数百次 | 大角度必卡 |
| **每 20ms 只刷一行（一轴）** | 约 9 次 I2C/轴 | 推荐 |
| 数值未变不刷 | 0 | 必须 |

命令字节：每条命令建议 `0x00 + cmd` 单独事务；数据 `0x40 + pixels`。  
不要把多条命令误当成 `0x00, c1, c2, c3` 单次当数据发出。

---

## 5. 内存与 GC（24 KiB Lua 堆）

- 堆：`main.c` 中 `LUA_HEAP_SIZE = 24 * 1024`
- BSS 已含 UART ring ×3 ×512；再增大 ring 会挤栈/堆边界
- **`require` 大字库**（全 ASCII 6x8）+ 驱动模块 + main 同时驻留 → 易 OOM
- 推荐：
  - 实时 UI：**单文件** + 仅数字/符号字模表
  - 菜单/日志：冷路径再 `require`
  - 热路径：整数表 + `writev`，禁止字符串拼包

调试时可看串口：`oe`（连续刷失败）、`sk`（跳过帧）。稳定后应接近 0 且不爬升。

---

## 6. 硬件检查清单

1. OLED **3V3 / GND 与 MCU 共地**，模组供电可靠（勿只靠松动杜邦线）
2. **PA15=SCL、PA16=SDA** + **4.7k 上拉到 3V3**
3. IMU UART **交叉**：模组 TX→PA24，模组 RX→PA23
4. 怀疑总线时：先 GPIO 输入上拉读空闲应为 1/1；再 soft I2C 扫 `0x08..0x77`（期望 HIT `0x3C`）
5. 勿把 OLED 挂在 **PA18**

---

## 7. 构建 / 烧录 / 上传

```text
python tools/build_fw.py
python tools/hold_boot_flash.py mspm0_lua/build_bytecode\mspm0_lua_bytecode.bin

python tools/compile_lua.py mspm0_lua/scripts\hi_uart_atk_oled.lua mspm0_lua/build_bytecode\hi_uart_atk_oled.luac
# 上传为 main.luac（IDE 或 upload_script.py）；运行中先 ! 再 <<<HEX
```

<serial-port> 被 IDE 占用时本地串口会 `PermissionError`，先断开 IDE。

---

## 8. 以后写脚本的硬规则

1. **主循环预算**：收数 / 解析 / 刷屏每项有 `rounds` / `steps` 上限 + `yield`
2. **失败可恢复**：`pcall`；I2C/传感器失败跳过帧，不 `error` 杀 VM
3. **热路径零分配**：无 `..`、`bytes()` 热调、`require` 大表
4. **刷屏节流**：脏标记 + 分轴/分行；勿全屏 clear 在循环里
5. **外设隔离**：ISR 只 ring/计数；Lua 协作 poll；I2C 等待中继续收 UART
6. **堆意识**：24K 总预算；模块化可以，但实时路径要瘦

相关文档：

- `BYTECODE_PERIPHERALS.md` — 外设 API 与引脚表  
- `LUA_MODULES.md` — `require` / LittleFS 模块  
- `DIMENGXING_PINMUX.md` — 板级 pinmux  
- `FLASH_BUDGET.md` — Flash/RAM 预算  
