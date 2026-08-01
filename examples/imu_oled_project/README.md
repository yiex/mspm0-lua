# IMU + OLED 模块化示例

## 文件
- `main.lua` — 主程序（IMU 解析 + 循环）
- `oled_draw.lua` — OLED 显示模块（上传为 `oled_draw.luac`）
- `mspm0_lua.json` — 工程描述

## 接线
- OLED SSD1306: I2C1 **PA15 SCL / PA16 SDA**，0x3C，100 kHz
- 陀螺仪/IMU: UART2 **PA23 TX / PA24 RX**，115200
- 调试串口: CH340 **PA10/PA11**
- 勿接 OLED 到 PA18

## 使用步骤
1. IDE → 打开工程 → 本目录
2. 打开 `oled_draw.lua`，目标名 `oled_draw.luac`，点 **保存模块**
3. 打开 `main.lua`，点 **运行**

详见 `mspm0_lua/docs/IMU_OLED_REALTIME.md`
