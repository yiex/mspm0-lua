# 通信与中断 API（MSPM0 Lua）

设计原则（来自 UART 联调）：
- **ISR 只做最短工作**：入环/计数，**禁止在 ISR 里跑 Lua**
- **超时、禁止死等**：时钟/总线等待均有上限
- **状态可观测**：串口日志 + `0x20200100` 邮箱
- **分频随时钟**：`g_cpuclk_hz` / `g_uart_busclk_hz` 在 HFXT 成功后切到 80/40 MHz

## 时间 / 定时中断

| API | 说明 |
|-----|------|
| `millis()` | 自启动毫秒计数（TIMG0 1 ms 中断） |
| `delay_ms(n)` | 基于 `millis` 的等待（中断仍运行） |
| `delay_us(n)` | busy-wait，按 `g_cpuclk_hz` |
| `tmr.every(ms[, fn])` | 无 `fn`：轮询 `tmr.ready`；有 `fn`：`event.run` 调 `fn(id, hits)` |
| `tmr.ready(id)` / `tmr.stop(id)` | ready=读清；**勿与同 id 的回调混用** |
| `event.run()` / `task.run()` | 分发回调和协作任务，空闲时进入 WFI |

硬件：`TIMG0`，BUSCLK 源，load ≈ `bus_hz/1000 - 1`。

## 外部中断（GPIO 边沿）

| API | 说明 |
|-----|------|
| `irq.on(pin, edge, fn, debounce_ms?)` | 注册边沿回调；edge: `"fall"` / `"rise"` / `"both"` |
| `irq.off(pin)` | 关闭 |
| `irq.count(pin)` | 读并清零边沿计数 |

硬件：`GROUP1`（GPIOA/B 共享），最多 4 路。  
ISR 只累加计数，Lua 回调由主上下文中的事件分发器执行，不会从 ISR
重入 Lua VM。`hits` 表示本次分发前积累的边沿数；可选消抖在 ISR 侧完成。

```lua
irq.on("PA25", "both", function(pin, hits)
  print(pin, hits)
end, 20)
event.run()
```

`irq.on()` 会直接配置并占用输入脚，不要先对同一脚调用 `gpio.mode()`。
旧的 `irq.on(pin, edge)` + `irq.count(pin)` 轮询形式仍保留兼容。

## UART（真硬件 + RX 中断环）

| API | 说明 |
|-----|------|
| `uart.write(s)` | 控制台 UART0 TX |
| `uart.read(timeout_ms?, max_bytes?)` | 从 256B 环缓冲取数据，单次最多 64B |

RX：`UART0_IRQHandler` → 环；轮询路径再 drain 兜底。

## SPI（硬件 SPI1，共享 Flash 总线）

| API | 说明 |
|-----|------|
| `spi.open(id, sck, mosi, miso, cs, hz)` | SPI1 固定 PB16/15/14；默认应用 CS=PA18 |
| `spi.xfer(id, data, hold_cs?)` | 全双工，返回等长字符串 |
| `spi.cs(id, 1\|0)` | 手动片选 |
| `spi.close(id)` | |

**注意**：SPI1 与 W25Q 共用时钟/数据线，PB17 保留为 Flash CS；应用设备必须使用另一根 CS（默认 PA18）。驱动每次传输后自动恢复 Flash 速率。

## I2C（硬件 I2C0）

| API | 说明 |
|-----|------|
| `i2c.open(id, scl, sda, hz)` | 默认 PA0/PA1 |
| `i2c.write(id, addr7, data)` | 返回 true/false |
| `i2c.read(id, addr7, n)` | 成功字符串 / nil |
| `i2c.write_read(id, addr7, wdata, rn)` | 写后重复起始再读 |
| `i2c.close(id)` | |

PA1=SCL、PA0=SDA，建议使用外部上拉。

精简固件未链接完整 `string` 库；二进制收发使用 `bytes(0x01, ...)` 组包、`byte(s, i)` 取字节。

## 当前硬件状态

- `pwm.*`：TIMG12 硬件 PWM（PA14）
- `adc.read(ch)`：ADC12 单次采样，通道 0..7
- `i2c.*`：硬件 I2C0（PA0/PA1）
- `spi.*`：硬件 SPI1，与 W25Q 共总线、独立 CS
- `qei.*`：未单独占用 QEI 外设，可用 `irq` 边沿计数近似

## 中断架构示意

```
TIMG0 ZERO IRQ ──► s_millis++ / soft-timer pending++
UART0 RX IRQ   ──► RX ring push
GROUP1 GPIO    ──► per-pin edge count++ (optional debounce)
main Lua       ──► event.run: timer/GPIO callbacks + cooperative tasks
```

死代码已去掉：`board_irq_flags` / 未使用的 soft-timer active 查询。

## 协作任务

`task.spawn(fn)` 创建类似线程的 Lua 协程；任务中用 `task.sleep(ms)` 或
`task.yield()` 主动让出执行权，最后调用 `task.run()`。最多同时 4 个任务。
完整示例见 [EVENT_TASKS.md](EVENT_TASKS.md)。
