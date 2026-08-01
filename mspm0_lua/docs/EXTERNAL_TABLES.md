# 外置 Flash 存表策略

## 结论

**可以、也够用。** W25Q + LittleFS 现报 **4 MiB**（`fs.capacity()`），远大于内部 128 KiB。

| 数据 | 放哪 | 原因 |
|---|---|---|
| IQ sin 0..89°（~180 B） | **片内 C** | 热路径、无延迟、体积可忽略 |
| 全量 IQmath / 大字库 / LUT | **LittleFS** | 内 Flash 仅余 ~3 KiB |
| 配置/标定 | LittleFS | 可现场改，不重烧固件 |

## API

```lua
if fs.ready() then
  fs.write("calib.bin", bytes(...))   -- ≤512 B / 次
  local d = fs.read("calib.bin", 512)
end
```

大文件请用 IDE **HEX 上传** 到 LittleFS，脚本里 `fs.read` 分块用（当前单次读 cap 512 B）。

## 不要做的

- 把热路径每帧从 NOR 读整表（延迟 + 堆）  
- 链入 TI `iqmath.a` 全库（单对象即可 > free）  
- 在 24 KiB Lua 堆里 `require` 巨型字库  

## 与 IQ

角度仍用 **°×10 整数**；进 IQ 用 `iq.from_x10`。需要更密 sin 表时：生成 `.bin` 上传，启动时 `fs.read` 进 **C 静态缓冲**（需新动词时再加，注意 RAM）。
