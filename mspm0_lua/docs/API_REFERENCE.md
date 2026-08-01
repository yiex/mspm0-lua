# MSPM0G3507 Lua 固件 API 参考

默认固件：`bytecode`。目标：TI MSPM0G3507（48 引脚）。  
风格：Lua 编排，C 热路径。近似/可组合接口已合并删除（见文末）。

---

## 1. 运行模型

| 层次 | 职责 |
|---|---|
| 硬件 ISR | 计数、环形缓冲、DMA；不进入 Lua |
| C 驱动 | 引脚/外设 owner、滤波、PID、字库 |
| Lua 主上下文 | `event.run()` 分发回调与协作任务 |

- `task.*` 为协作 coroutine，非 RTOS 线程。
- 控制台 `!` 可打断紧循环；任务内应 `sleep`/`yield`。

---

## 2. 全局

| API | 说明 |
|---|---|
| `print(...)` | UART0 |
| `millis()` | 毫秒（32 位回绕） |
| `delay_ms(ms)` | 可被 `!` 打断；可喂狗 |
| `yield()` | 让出并检查 stop |
| `stopped()` | 是否收到 stop |
| `bytes(...)` / `byte(s,i)` | 二进制串（≤64）/ 取字节 |
| `runfile(name)` | LittleFS 执行 |
| `require(name)` | 加载并缓存模块（`.luac`） |

---

## 3. 系统 `sys`

| API | 说明 |
|---|---|
| `sys.mem()` | 堆统计 |
| `sys.gc([step_kb])` | 全量或增量 GC |
| `sys.resource()` | 外设 owner |

**固定占用**：UART0 PA10/11；Flash PB14–17；SWD PA19/20；HFXT PA5/6；LFXT PA3/4。

| 资源 | 占用 |
|---|---|
| TIMG0 | 1 ms tick（无脚） |
| TIMG6 / TIMG8 | `cap` / `qei` |
| TIMG12 / TIMG7 | `pwm` 独立（最多 2 路，各一 timer） |
| TIMA0 / TIMA1 | `pwm.comp` 互补（最多 2 对） |
| ADC0+DMA0 | `adc.capture` |
| I2C1 | `oled` 或 `i2c.open(1)` |
| UART1..3 | `uart.open` |
| WWDT | `wdt`（启动后至复位） |

---

## 4. 时间 / 事件 / 任务

### `tmr`（≤4）

`tmr.every(ms[,fn])` → id · `tmr.ready(id)` · `tmr.stop(id)`  
回调 `fn(id, hits)`。勿混用回调与 `ready`。

### `event`

`event.run()` · `event.poll()` · `event.stop()`

### `task`（≤4）

`task.spawn(fn)` · `task.sleep(ms)` · `task.yield()` · `task.cancel(id)`  
主循环用 **`event.run()`**（无 `task.run` 别名）。

---

## 5. GPIO / IRQ

### `gpio`

`mode` · `set` · `get` · `toggle` · `af(pin,pf[,inena])` · `owner` · `release`

### `irq`（≤4）

`irq.on(pin[,edge[,fn[,deb_ms]]])` · `off` · `count`  
edge：`"fall"`/`"rise"`/`"both"`。勿先 `gpio.mode` 同一脚。

**板载 LED（PA14）**：用 `gpio` 或 `pwm.open("PA14")`，无独立 `led` 模块。

```lua
gpio.mode("PA14", "out"); gpio.toggle("PA14")
-- 或
pwm.open("PA14", 1000); pwm.duty(0, 40); pwm.close(0)
```

---

## 6. 通信

### `uart`

| API | 说明 |
|---|---|
| `uart.write` / `read` | 控制台 UART0 |
| `uart.open(id[,tx,rx,baud])` | id=1..3 |
| `uart.tx` / `rx` / `close` | |

| id | 默认 TX/RX | 其他 TX | 其他 RX |
|---:|---|---|---|
| 1 | PA17/PA18 | PB6, PA8 | PA9, PB7 |
| 2 | PA23/PA24 | PA21, PB17 | PA22, PB18 |
| 3 | PA26/PA25 | PA14, PB2 | PB3, PA13 |

### `i2c`

`open` · `write` · `writev` · `read` · `write_read` · `close`  
I2C0=PA1/PA0；I2C1=PA15/16 或 PA17/18。

### `spi`

`open(id,sck,pico,poci,cs[,hz])` · `cs` · `xfer` · `close`  
id0=SPI1 共 Flash 总线；id1=SPI0 默认 PA12/14/13 + CS。

---

## 7. 模拟与定时

### `adc`

| API | 说明 |
|---|---|
| `adc.channel(pin)` | pin→ch |
| `adc.read(ch\|pin)` | 12-bit |
| `adc.capture(ch\|pin[,n[,to_ms[,rate]]])` | DMA；返回 LE u16 串 + period_ns |

通道：PA27=0 … PA22=7。mV 在 Lua 算：`(raw * vdda) // 4095`。

### `pwm`

| API | 说明 |
|---|---|
| `pwm.open([pin[,hz]])` → id | TIMG12：PA14/PB20(CCP0)，PA25/PA31/PB24(CCP1)；TIMG7：PA17/23/28(CCP0)，PA7/18/24、PB19(CCP1) |
| `pwm.duty(id, duty)` / `close(id)` | 最多 2 路并发（每 timer 一路） |
| `pwm.comp(...)` → id | TIMA0/TIMA1 互补；`comp_duty([id,] d)` / `comp_close([id])` |

`pwm.comp(freq?, duty?, dead_ns? [, hi, lo])` 或 `pwm.comp(hi, lo, freq?, duty?, dead_ns?)`。  
TIMA0 默认 PA8/PA22；TIMA1 例：`pwm.comp("PA15","PB6",20000,50,500)`。

### `cap`（TIMG6）

`open([pin[,edge]])` · `period` · `hz_x10` · `ready` · `hits` · `close`  
默认 PA22；另 PA21、PB6/PB2(CCP0)、PB7/PB3(CCP1)。

### `qei`（TIMG8）

`open` · `pos` · `delta` · `dir` · `set` · `active` · `stim` · `close`  
PHA=PA26，PHB=PA27。`stim` 为测试灰码（PA14/PA25）。

---

## 8. 人机

### `oled`（SSD1306）

`open` · `close` · `ready` · `clear` · `cursor` · `print` · `num` ·  
`text` · `font` · `font16` · `glyph` · `cjk` · `has` · `has_cjk` · `wave` · …

### ATK 姿态（无 C `imu.*`）

用通用 **`uart.open/rx`** 在 Lua 解析 ATK 帧。示例：`scripts/hi_uart_atk.lua`、`hi_uart_atk_oled.lua`。  
默认：UART2 PA23 TX / PA24 RX，115200；帧 `55 55 ID LEN DATA SUM`，姿态 ID `0x01`/`0x53`，角为 °×10。

### `btn` / `enc`

`btn.open(pin[,deb,long])` · `scan` · `event` · `down` · `held_ms` · `close`  
`enc.open(a,b)` · `pos` · `delta` · `cps` · `set` · `poll` · `close`

---

## 9. 控制 / 数值

### `iq`（Q16.16，`ONE=65536`）

`from` / `from_x10` / `from_x100` · `to_x10` / `to_x100` / `to_x1000` ·  
`mul` / `div` · `sin_deg` / `cos_deg` / `atan2_deg`

### `pid`（≤4）

`open("pos"|"inc")` · `tune`(增益×100) / `tune_iq` · `limit` · `ilimit` ·  
`step(id,sp,fb[,dt])` · `cascade` · `reset` · `close`  
误差步进：`pid.step(id, err, 0, dt)` 或自行算 sp/fb。

### `filt`（≤4）

`open("lp"|"ma"[,param])` · `config` · `update` · `get` · `reset` · `close`  
限幅：`filt.update(id, util.clamp(x, lo, hi))`。

### `ramp`（≤4）

`open` · `config` · `set` · `jump` · `step` · `get` · `done` · `close`

### `util`

`clamp` · `deadzone` · `map` · `med3` · `slew` · `avg` · `sign`  
abs/min/max 用 Lua：`v<0 and -v or v` 等。

### `crc`

`crc.crc8(data[,init])` · `crc.modbus(data)`

---

## 10. 存储 / 看门狗

### `fs`

`ready` · `exists` · `read` · `write` · `remove` · `capacity`  
新片空白 NOR 启动可自动 format（`LFS FMT`）；否则控制台 `format`。

### `wdt`

`start([ms])` · `feed` · `active` — 启动后至复位不可停。

---

## 11. 错误

参数/冲突 → Lua error（`pcall`）；无数据 → `nil`；部分写 → `false`。

---

## 12. 控制台 / 构建

UART0 PA10/11 115200 · `!` `r` `ls` `rm` `boot` `format` · HEX 上传  
`python tools/build_fw.py`

---

## 13. 已删除 / 合并（相对旧文档）

| 旧 API | 替代 |
|---|---|
| `led.on/off/toggle/pwm` | `gpio` / `pwm.open("PA14")` |
| `task.run` | `event.run` |
| `requirefile` | `require` |
| `imu.*`（整模块） | `uart.*` + Lua 解析（见 `hi_uart_atk.lua`） |
| `adc.mv` | `(adc.read()*vdda)//4095` |
| `filt.update_clamp` | `filt.update` + `util.clamp` |
| `pid.step_err` | `pid.step(id, err, 0, dt)` |
| `util.abs/min/max` | 普通 Lua 表达式 |

引脚表见 `DIMENGXING_PINMUX.md`。
