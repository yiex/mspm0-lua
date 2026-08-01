# MSPM0G3507 模块化 Lua 固件 - IDE 发布与运行契约

本文是 `mspm0g3507.lua-modular` `1.0.2` 的稳定上位机契约。机器可读
文件优先级高于本文；同一发布必须整体分发，不允许混用不同版本文件。

## 1. 发布身份与文件

| 字段 | 固定值 |
|---|---|
| firmware/catalog ID | `mspm0g3507.lua-modular` |
| firmware/catalog version | `1.0.2` |
| target | `MSPM0G3507` |
| Core ABI | `7` |
| native module format | `2` |
| NMUP format | `1` |
| Lua ABI | Lua 5.5, `LUA_32BITS` |
| slot | 8 x 4096 B, `0x18000..0x1FFFF` |
| catalog SHA-256 | `c1609433a2bc70d4d991de00454f23f377fd71021f523447a3e00d61d4e2d23c` |

发布单元包括：

- `modules/modules.json`：模块 ID、版本、依赖、冲突、Lua module、资源和集合。
- `build_modules/index.json`：104 个固定地址变体的路径、地址、长度、CRC、SHA-256、build ID。
- `build_modules/<module>/slot<N>/<module>.bin`：13 x 8 个预构建变体。
- `build_modular/mspm0_lua_modular.bin`：只需首次安装或 Core/ABI 升级时烧录。
- `release/catalog_manifest.json`：顶层身份、所有关键文件哈希和 catalog 哈希算法。
- `release/mspm0-lua.api.json`：完整 API 元数据，13 模块、7 个固件全局、84 个函数。
- `release/test-vectors/*`：正确/损坏 NMUP、字段 JSON、完整逐行 transcript。

catalog SHA-256 对 `modules.json`、`index.json` 和 104 个模块 bin 计算。按相对
POSIX 路径的 UTF-8 字节升序排列，每个文件写入以下规范记录，再对全部记录串联
值做 SHA-256：

```text
utf8(path) || NUL || ascii(decimal_length) || NUL || lowercase_sha256 || LF
```

Core/API/顶层 manifest 不参与 catalog 哈希，以避免循环依赖；其各自 SHA-256
记录在顶层 manifest。

## 2. 首次安装与日常更新边界

首次给 MCU 安装 Core 可使用 J-Link、BSL 或生产烧录器。客户日常开发不再需要
BOOT、BSL、复位或调试器。普通 CH340 应用串口完成以下两种独立持久化事务：

1. NMUP 写外置 LittleFS，然后由 Core 写 MCU 内部模块槽。
2. 依赖 `.luac` 和最后的 `main.luac` 写外置 LittleFS。

模块 bin 不会从 SPI Flash 直接执行。它先经三遍校验，然后写入 MCU 内部
`0x18000..0x1FFFF` 固定槽，VM 重建后由 Lua 直接调用槽内原生代码。整个过程
不调用 ROM BSL，UART 会话不重启。

catalog 总大小允许大于 MCU Flash；一次运行只选择最多 8 个模块。IDE 不能为
一次选择重新编译 C，也不能把 13 个模块强行塞入 8 个槽。确定性模块顺序就是
slot 顺序，同一输入和 catalog 必须得到相同 NMUP 字节。

## 3. 连接、停止与身份查询

应用串口固定为 UART0 PA10/PA11、8-N-1、LF 结尾、忽略 CR。每次硬复位恢复
115200。最大输入内容 255 字符；HEX 数据块建议 120 个二进制字节，即 240 个
十六进制字符。

IDE 在运行事务开始时先发送 `!`。紧循环中的停止 hook 只保证消费 `!`，不保证
在 VM 仍忙时处理 `fwinfo`；因此应先等待 `STOP`，然后查询身份。

```text
H -> fwinfo
D -> FW_INFO mspm0g3507.lua-modular 1.0.2
D -> FW_TARGET MSPM0G3507
D -> FW_ABI 7
D -> FW_MODULE_FORMAT 2
D -> FW_NMUP_FORMAT 1
D -> FW_SLOTS 8 4096
D -> FW_CATALOG c1609433a2bc70d4d991de00454f23f377fd71021f523447a3e00d61d4e2d23c
D -> FW_INFO_END
```

字段顺序、大小写和十进制格式固定。IDE 遇到任一不一致必须禁止 NMUP 写入并
报告具体字段。旧固件不支持 `fwinfo` 时不会响应；115200 下 1 秒无任何
`FW_INFO`，或 3 秒未收到 `FW_INFO_END`，按“不支持模块化发布协议”处理，不得
猜测兼容。

## 4. 波特率协商

```text
H@115200 -> baud 460800
D@115200 -> BAUD_SWITCH 460800
D@460800 -> BAUD_OK 460800
```

只接受 115200、460800；其他参数响应 `BAUD_ERR`。推荐超时：旧速率
`BAUD_SWITCH` 2 秒，新速率 `BAUD_OK` 2 秒。收到前者后才能切主机速率，收到
后者后才能传 payload。设备在普通重新打开串口时保持当前速率；硬复位才固定
恢复 115200。

若丢失 `BAUD_OK`，关闭端口，先探测 460800，再探测 115200，每次只发
`fwinfo` 并等待完整结束行；确认活动速率后发 `baud 115200`。不得在速率不确定
时继续发送 NMUP。所有正常事务结束前主动降回 115200。

## 5. 状态预检与精确比较

```text
H -> modstatus
D -> MOD_STATUS IDLE|PENDING
D -> MOD_CATALOG <64-lowercase-hex>
D -> MOD_SLOT <slot> <name> <decimal-size> <lowercase-image-crc32>
   或 MOD_SLOT <slot> BAD
D -> MOD_LAYOUT <valid-count> <lowercase-full-slot-region-crc32>
D -> MOD_PENDING none|invalid|<lowercase-bundle-crc32>
D -> MOD_STATUS_END
```

slot 行只为非空槽输出，按 0..7 升序。CRC32 覆盖模块完整镜像，不只是 payload。
IDE 必须比较 `(slot,name,size,crc32)`，不能只比较模块名。相同模块集合但 slot
排列不同即为不同布局。任何 `BAD` 都要求重装，不能调用其中代码。

`MOD_CATALOG` 必须等于 `fwinfo` 和本地 manifest 的 catalog。`MOD_LAYOUT` CRC32
覆盖完整 32 KiB 槽区（包括空槽的 `0xff` 和各槽未使用尾部），用于快速诊断，不能
替代逐槽四元组校验。`MOD_PENDING` 给出持久化 pending 记录；`invalid` 必须按恢复
故障处理，8 位 CRC32 值必须与待恢复 bundle 对应。

若当前精确布局等于计划布局，跳过 NMUP 阶段，直接上传 Lua。此优化避免无意义
擦写和缩短运行时间。状态为 `PENDING` 时先按第 8 节恢复，不能上传新入口。

### 5.1 存储与文件查询

```text
H -> storageinfo
D -> STORAGE external_littlefs
D -> PART W25Q32JVSSIQ
D -> CAPACITY 4194304
D -> PINS SPI1 PB16 PB15 PB14 PB17
D -> STORAGE_END

H -> fileinfo main.luac
D -> FILE main.luac <decimal-size> <lowercase-crc32>
D -> FILE_END
```

`fileinfo` 的 CRC32 覆盖完整文件并流式计算，不要求文件装入 RAM。稳定失败行只有：

| 响应 | 含义 |
|---|---|
| `FILE_ERR INVALID_NAME` | 空文件名、长度越界或包含非法字符 |
| `FILE_ERR FS_NOT_MOUNTED` | LittleFS 未挂载 |
| `FILE_ERR NOT_FOUND` | 文件不存在 |
| `FILE_ERR IO` | 读取或 CRC 扫描失败 |

错误响应不跟 `FILE_END`；主机收到完整 `FILE_ERR ...` 行即结束本次查询。

## 6. NMUP v1 与模块事务

NMUP 是 32 字节小端 header、8 x 32 字节 slot entry、连续 payload。完整字段和
正确数值见 `release/test-vectors/vectors.json`。bundle CRC 字段计算时自身 4 字节
置零；模块完整镜像用 CRC32，格式 2 header 后的 payload 用 CRC-16/MODBUS。

```text
H -> <<<HEX modules.upd
D -> SCRIPT_BEGIN
H -> <hex block>
D -> HEX_OK
...
H -> >>>HEX
D -> SCRIPT_OK <exact-received-byte-count>
H -> modapply modules.upd
D -> MOD_READY <selected-count> <bundle-bytes>
D -> MOD_APPLY modules.upd
D -> MOD_ERASE <slot>
D -> MOD_WRITE <slot> <name>       # 仅非空槽
...
D -> MOD_VERIFY
D -> MOD_DONE <selected-count>
D -> MOD <name>                    # 新 VM 注册日志
D -> Idle
```

`MOD_DONE <count>` 与随后 `Idle` 两者同时出现，是模块阶段唯一成功终点。此时：

- 所有 8 个槽都已按 bundle 完整擦写并读回验证；
- pending 记录和临时 bundle 已清除；
- 旧 `main.luac` 已删除，旧入口不会自动执行；
- 新 VM 已创建，所选模块已注册，可立即上传依赖 LUAC。

`MOD_DONE` 之前掉线/超时都不能进入 Lua 阶段。收到 `MOD_DONE` 但未收到 `Idle`
也不能假定 VM 可用；重连后查询 `fwinfo` 和 `modstatus`。

## 7. Lua 多文件事务

1.0.2 模块化 Core 只接收 `<<<HEX` 字节码/二进制上传，不接收 `<<<LUA` 源码。
旧 `get`、`rm`、`boot` 和帮助入口也不属于模块化发布契约；查询使用 `ls`、
`storageinfo`、`fileinfo`，替换文件使用原子 HEX 上传。

文件名只允许 ASCII 字母、数字、`_`、`.`、`-`，长度 1..28。`require("x")`
读取 `x.luac`。同名文件通过 `.upload.tmp` 完整写入成功后原子替换；传输中断不会
用半文件覆盖旧目标。固件不接收预声明长度或内容哈希，`SCRIPT_OK` 的十进制数是
实际收到的精确字节数；IDE 必须与本地长度比较，本地 SHA-256 用于事务记录。

非入口文件只落盘：

```text
H -> <<<HEX dependency.luac
...
D -> SCRIPT_OK <bytes>
```

IDE 先按依赖拓扑序上传全部非入口 `.luac`，最后上传 `main.luac`。循环依赖若 Lua
运行时可接受，则循环内按规范化相对模块名 UTF-8 升序；缺文件、重复模块名和越界
路径在硬件访问前报错。

```text
H -> <<<HEX main.luac
...
D -> SCRIPT_OK <bytes>
D -> <模块注册和应用 print 日志，可为多行>
D -> SCRIPT_DONE OK|ERR|PENDING
```

只有完整的 `SCRIPT_DONE OK` 是整个“运行”事务成功。`SCRIPT_OK` 只表示文件已
落盘；不能把 `SCRIPT_DONE` 前缀或 `SCRIPT_DONE ERR` 当成功。`PENDING` 表示模块
恢复未完成。模块阶段成功、Lua 阶段失败时不回滚模块，只修正并重试 Lua 文件。

本发布示例 `examples/ide_oled123` 的自动组合图为：

```text
main.lua -> IDE 内置 oled.lua -> IDE 按工程生成 _oled_font.lua
native closure: i2c
upload: _oled_font.luac -> oled.luac -> main.luac
```

OLED 不是内部槽中的原生 `oled` 模块，而是 IDE 按需上传的 Lua 驱动加原生 `i2c`
模块。工程调用 `oled.open(1, "PA15", "PA16", 0x3c, 100000)`，不得省略具体实例、
引脚、地址或速率。IDE 根据 `oled.text/number` 的静态字号自动生成字模；旧手写点阵和
`_run.f16/_run.fnt` 不属于模块化运行流程。

## 8. 掉电恢复、阻塞和错误分类

pending 在修改内部 Flash 前持久化。掉电后普通启动流程为：

```text
D -> MOD_RECOVER START
D -> MOD_APPLY ... / MOD_ERASE ... / MOD_VERIFY
D -> MOD_DONE <count>
D -> MOD <name> ...
D -> MOD_RECOVERED
D -> Idle
```

恢复失败：

```text
D -> MOD_RECOVER START
D -> MOD_ERR <reason>
D -> MOD_BLOCKED <reason>
D -> MOD_RECOVERY_WAIT
```

此时 Lua 不创建，但 UART、`fwinfo`、`modstatus`、文件上传和新的 `modapply` 可用。
上传新的正确 bundle 并再次 `modapply` 可恢复，不要求复位/BSL。

| reason | 写槽可能已开始 | 重试策略 |
|---|---:|---|
| `header` | 否 | 修正格式/ABI/长度并重新上传 |
| `bundle-crc` | 否 | 重新上传完整 bundle |
| `module` | 否 | 重新选择可信 index 变体并上传 |
| `pending-write` | 否 | 检查/清理 LittleFS 后重试同包 |
| `pending-commit` | 否 | 检查 LittleFS 后重试同包 |
| `pending` | 未知 | pending 文件损坏，上传新包并 `modapply` |
| `flash` | 是 | 保持阻塞，重试正确完整 bundle |
| `verify` | 是 | 保持阻塞，重试；重复失败视为硬件故障 |
| `script-disable` | 槽已验证 | 清理 LittleFS/旧入口后重试完整事务 |
| `pending-clear` | 槽已验证 | 重试同包以完成并清 pending |
| `OOM` | 槽已验证 | VM 创建失败；减少 Lua/Core RAM 使用或复位诊断 |

`MOD_ERR header|bundle-crc|module|pending-write|pending-commit` 出现在 stage 阶段，
不会销毁当前 VM 或修改内部槽，也不跟 `MOD_BLOCKED`。apply/recovery 阶段失败会输出
`MOD_BLOCKED <same-reason>`，禁止 Lua 使用可能部分更新的槽。`r`/`f` 在 pending
时响应 `MOD_BLOCKED pending`。

## 9. 超时、重试与日志边界

推荐 IDE 超时：

| 阶段 | 超时 |
|---|---:|
| `STOP` | 3 s；无响应可再次发 `!` 一次 |
| `fwinfo` 首行/结束 | 1 s / 3 s |
| `modstatus` 结束 | 3 s |
| `SCRIPT_BEGIN` | 3 s |
| 每个 `HEX_OK` | 3 s |
| `SCRIPT_OK` | `max(8 s, bytes/5000)` |
| `MOD_DONE + Idle` | 60 s |
| `SCRIPT_DONE` | 15 s，应用长期运行应由 IDE 的运行监视策略另行定义 |

命令响应均为完整行。模块更新期间只有固定 `MOD_*` 日志；Lua print 日志可能在
`SCRIPT_OK` 与 `SCRIPT_DONE` 之间任意插入，上位机必须按终端标记解析而非固定行数。
超时后先停止发送 payload，重新建立已知速率，再做 `fwinfo`/`modstatus` 设备真值
查询。阶段 1 已经 `MOD_DONE + Idle` 时，只重试阶段 2；否则禁止阶段 2。

## 10. 安全与性能

三重格式/CRC/地址/读回检查防随机损坏，不提供攻击者认证。拥有 UART 控制权的人
可以安装原生机器码。量产产品若对不可信用户暴露接口，应在 `modapply` 前增加签名
验证、反回滚计数和命令认证；当前开发板接口按可信调试通道定义。

460800 是默认调试速率：相比 115200，HEX payload 的理论吞吐提高 4 倍；120 字节
块减少逐块 ACK 开销。模块槽内代码直接执行，无动态链接跳板；API v7 共享除法入口
减少多个模块重复 runtime。模块拆分不带来运行期内存常驻开销，未选择模块占用 0
槽空间；每个已占用槽仅使用固定 32 字节 Core RAM 状态。

## 11. 参考实现和验证命令

```powershell
python tools/build_protocol_fixtures.py
python tools/test_module_catalog.py
python tools/test_module_update_bundle.py
python tools/test_serial_module_update.py
Lua IDE.exe  # 打开 example/oled 后直接运行；IDE 自动生成字模并组合 i2c
```

正确向量：`i2c-only-valid.nmup`、`full-valid.nmup`。损坏向量：
`i2c-only-bundle-crc.nmup`，预期 `MOD_ERR bundle-crc`，当前槽保持不变且仍为
`MOD_STATUS IDLE`。逐行可重放记录为 `verified-transcript.txt`。
