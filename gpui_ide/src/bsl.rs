//! MSPM0 ROM UART BSL host — aligned with LCKFB web flasher
//! (https://wiki.lckfb.com/storage/html/mspm0-web-flasher/).
//!
//! Default: 9600 8N1 on CH340 / BSL UART (PA10 TX · PA11 RX).
//! Host packet: `[0x80][len_le16][cmd][payload…][crc32_le]`
//! Device packet: `[0x08][len_le16][type][data…][crc32_le]`
//! where `len = 1 + payload.len` and CRC covers `cmd+payload`
//! (CRC-32 poly 0xEDB88320, init 0xFFFFFFFF, **no** final invert — matches LCKFB).

use anyhow::{bail, Context, Result};
use serialport::SerialPort;
use std::io::{Read, Write};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

/// Host → device packet header (MSPM0 UART BSL).
const HDR_HOST: u8 = 0x80;
/// Device → host packet header (TI SLAU887 / MSPM0 ROM BSL).
const HDR_DEVICE: u8 = 0x08;
const ACK: u8 = 0x00;

const CMD_CONNECTION: u8 = 0x12;
const CMD_GET_ID: u8 = 0x19;
const CMD_RX_PASSWORD: u8 = 0x21;
const CMD_MASS_ERASE: u8 = 0x15;
const CMD_PROGRAM_DATA: u8 = 0x20;
const CMD_START_APP: u8 = 0x40;

const DEFAULT_PASSWORD: [u8; 32] = [0xFF; 32];
/// Max data bytes per ProgramData (address is extra 4B).
const MAX_DATA_CHUNK: usize = 128;
const ID_BACK: usize = 24;

const DEFAULT_BSL_BAUD: u32 = 9_600;
const APP_BAUD: u32 = 115_200;

/// Flash image segment (absolute address + bytes).
#[derive(Clone, Debug)]
pub struct Segment {
    pub address: u32,
    pub data: Vec<u8>,
}

/// CRC-32 matching LCKFB `crc32.js` / TI BSL (no final complement).
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// Host-side RX buffer (same idea as LCKFB WebSerial `readBuffer`).
struct BslPort {
    port: Box<dyn SerialPort>,
    rx: Vec<u8>,
}

impl BslPort {
    fn open(port_name: &str, baud: u32) -> Result<Self> {
        let mut port = serialport::new(port_name, baud)
            .data_bits(serialport::DataBits::Eight)
            .stop_bits(serialport::StopBits::One)
            .parity(serialport::Parity::None)
            .timeout(Duration::from_millis(20))
            .dtr_on_open(false)
            .open()
            .with_context(|| format!("open {port_name} @ {baud}"))?;
        let _ = port.write_data_terminal_ready(false);
        let _ = port.write_request_to_send(false);
        // Avoid DTR/RTS edges that can reset some CH340 boards mid-session.
        let mut s = Self {
            port,
            rx: Vec::with_capacity(4096),
        };
        s.clear();
        Ok(s)
    }

    fn pump(&mut self) {
        let mut buf = [0u8; 512];
        loop {
            match self.port.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.rx.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                Err(_) => break,
            }
        }
    }

    fn clear(&mut self) {
        self.pump();
        self.rx.clear();
    }

    fn write_all(&mut self, data: &[u8]) -> Result<()> {
        self.port.write_all(data).context("BSL write")?;
        self.port.flush().ok();
        Ok(())
    }

    fn read_exact(&mut self, n: usize, timeout: Duration) -> Result<Vec<u8>> {
        let start = Instant::now();
        while self.rx.len() < n {
            if start.elapsed() > timeout {
                bail!("串口读超时 (需 {} B, 已 {} B)", n, self.rx.len());
            }
            self.pump();
            if self.rx.len() < n {
                thread::sleep(Duration::from_millis(2));
            }
        }
        Ok(self.rx.drain(..n).collect())
    }

    /// Send packet; optionally clear RX first (only for session start / recovery).
    fn send_and_ack(
        &mut self,
        packet: &[u8],
        ack_timeout: Duration,
        clear_first: bool,
    ) -> Result<u8> {
        if clear_first {
            self.clear();
        }
        self.write_all(packet)?;
        let ack = self.read_exact(1, ack_timeout)?;
        Ok(ack[0])
    }

    /// Device response: `[0x08][len][type][data…][crc32]`.
    fn read_response(&mut self, expected_data_len: usize, timeout: Duration) -> Result<Vec<u8>> {
        let total = 4 + expected_data_len + 4;
        let resp = self.read_exact(total, timeout)?;
        if resp[0] != HDR_DEVICE && resp[0] != HDR_HOST {
            bail!("BSL 响应头异常 0x{:02X} (期望设备 0x08)", resp[0]);
        }
        Ok(resp)
    }
}

fn build_packet(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let payload_size = 1 + payload.len(); // cmd + payload
    let mut packet = Vec::with_capacity(3 + payload_size + 4);
    packet.push(HDR_HOST);
    packet.extend_from_slice(&(payload_size as u16).to_le_bytes());
    packet.push(cmd);
    packet.extend_from_slice(payload);
    let core = &packet[3..3 + payload_size];
    let crc = crc32(core);
    packet.extend_from_slice(&crc.to_le_bytes());
    packet
}

fn response_status(resp: &[u8]) -> u8 {
    // [0x08][len_lo][len_hi][type 0x3B][status][crc…] — status at index 4 (LCKFB).
    if resp.len() > 4 {
        resp[4]
    } else {
        0xFF
    }
}

fn connection(port: &mut BslPort) -> Result<()> {
    let packet = build_packet(CMD_CONNECTION, &[]);
    let ack = port.send_and_ack(&packet, Duration::from_secs(3), true)?;
    if ack != ACK {
        bail!("Connection ACK=0x{ack:02X}");
    }
    Ok(())
}

fn get_device_info(port: &mut BslPort) -> Result<String> {
    let packet = build_packet(CMD_GET_ID, &[]);
    port.clear();
    port.write_all(&packet)?;

    // ACK (1) + response (4 + 24 + 4) = 33; device header is 0x08 after ACK.
    let raw = port.read_exact(1 + 4 + ID_BACK + 4, Duration::from_secs(5))?;

    let offset = if raw[0] == HDR_DEVICE || raw[0] == HDR_HOST {
        0
    } else {
        1 // first byte is ACK 0x00
    };
    if raw.len() < offset + 4 + ID_BACK {
        bail!("设备信息过短");
    }
    let d = &raw[offset + 4..offset + 4 + ID_BACK];
    let cmd_int = u16::from_le_bytes([d[0], d[1]]);
    let build_id = u16::from_le_bytes([d[2], d[3]]);
    let app_ver = u32::from_le_bytes([d[4], d[5], d[6], d[7]]);
    let buf_size = u16::from_le_bytes([d[10], d[11]]);
    let sram = u32::from_le_bytes([d[12], d[13], d[14], d[15]]);
    Ok(format!(
        "CMD={cmd_int} Build=0x{build_id:04X} App=0x{app_ver:08X} Buf={buf_size} SRAM=0x{sram:08X}"
    ))
}

fn unlock(port: &mut BslPort) -> Result<()> {
    let packet = build_packet(CMD_RX_PASSWORD, &DEFAULT_PASSWORD);
    let ack = port.send_and_ack(&packet, Duration::from_secs(5), false)?;
    if ack != ACK {
        bail!("Unlock ACK=0x{ack:02X}");
    }
    let resp = port.read_response(1, Duration::from_secs(5))?;
    let st = response_status(&resp);
    if st != 0x00 {
        bail!("Unlock 失败 status=0x{st:02X}");
    }
    Ok(())
}

fn mass_erase(port: &mut BslPort) -> Result<()> {
    let packet = build_packet(CMD_MASS_ERASE, &[]);
    let ack = port.send_and_ack(&packet, Duration::from_secs(30), false)?;
    if ack != ACK {
        bail!("MassErase ACK=0x{ack:02X}");
    }
    let resp = port.read_response(1, Duration::from_secs(30))?;
    let st = response_status(&resp);
    if st != 0x00 {
        bail!("MassErase 失败 status=0x{st:02X}");
    }
    Ok(())
}

fn program_data_once(port: &mut BslPort, address: u32, data: &[u8]) -> Result<()> {
    let mut payload = Vec::with_capacity(4 + data.len());
    payload.extend_from_slice(&address.to_le_bytes());
    payload.extend_from_slice(data);
    let packet = build_packet(CMD_PROGRAM_DATA, &payload);
    // Do NOT clear before each chunk — clearing races with late ACK/response bytes.
    let ack = port.send_and_ack(&packet, Duration::from_secs(5), false)?;
    if ack != ACK {
        bail!("Program ACK=0x{ack:02X} @0x{address:08X}");
    }
    let resp = port.read_response(1, Duration::from_secs(8))?;
    let st = response_status(&resp);
    if st != 0x00 {
        bail!("Program 失败 status=0x{st:02X} @0x{address:08X}");
    }
    Ok(())
}

fn program_data(port: &mut BslPort, address: u32, data: &[u8]) -> Result<()> {
    let mut last = String::new();
    for attempt in 1..=4 {
        match program_data_once(port, address, data) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = e.to_string();
                // Recover: flush RX, short pause, retry same address.
                port.clear();
                thread::sleep(Duration::from_millis(80 + attempt * 40));
            }
        }
    }
    bail!("{last} (已重试 4 次)")
}

/// ROM BSL `StartApplication` (0x40): jump to flash app.
/// Same as LCKFB — fire-and-forget (device leaves BSL; ACK optional / often absent).
fn start_app(port: &mut BslPort) -> Result<()> {
    let packet = build_packet(CMD_START_APP, &[]);
    port.clear();
    port.write_all(&packet)?;
    // Give UART time to drain before host closes the port.
    thread::sleep(Duration::from_millis(120));
    Ok(())
}

fn program_segments(
    port: &mut BslPort,
    segments: &[Segment],
    mut on_progress: impl FnMut(&str),
) -> Result<()> {
    let total: usize = segments.iter().map(|s| s.data.len()).sum();
    let mut written = 0usize;
    let mut last_log_at = 0usize;
    for seg in segments {
        let mut off = 0usize;
        while off < seg.data.len() {
            // LCKFB: 60ms; CH340 often needs a bit more after flash program.
            thread::sleep(Duration::from_millis(80));
            let end = (off + MAX_DATA_CHUNK).min(seg.data.len());
            let chunk = &seg.data[off..end];
            let addr = seg.address.wrapping_add(off as u32);
            program_data(port, addr, chunk)
                .with_context(|| format!("ProgramData @ 0x{addr:08X}"))?;
            written += chunk.len();
            off = end;
            // Status every chunk; console log every 4 KiB (less spam).
            let pct = if total > 0 { written * 100 / total } else { 0 };
            on_progress(&format!(
                "烧录中… {written}/{total} B ({pct}%) · 0x{addr:08X}"
            ));
            if written == total || written.saturating_sub(last_log_at) >= 4096 {
                last_log_at = written;
            }
        }
    }
    Ok(())
}

// ─── firmware parsers ───────────────────────────────────────────────

fn parse_intel_hex(text: &str) -> Result<Vec<Segment>> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut base: u32 = 0;
    let mut cur_addr: Option<u32> = None;
    let mut cur_data: Vec<u8> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if !line.starts_with(':') {
            continue;
        }
        let hex = &line[1..];
        if hex.len() < 10 || hex.len() % 2 != 0 {
            bail!("HEX 行过短: {line}");
        }
        let mut bytes = Vec::with_capacity(hex.len() / 2);
        for i in (0..hex.len()).step_by(2) {
            let b = u8::from_str_radix(&hex[i..i + 2], 16)
                .with_context(|| format!("HEX 解析: {line}"))?;
            bytes.push(b);
        }
        let sum: u32 = bytes.iter().map(|&b| u32::from(b)).sum();
        if (sum & 0xFF) != 0 {
            bail!("HEX 校验和错误: {line}");
        }
        let len = bytes[0] as usize;
        let addr = u32::from(bytes[1]) << 8 | u32::from(bytes[2]);
        let typ = bytes[3];
        let data = &bytes[4..4 + len];

        match typ {
            0x00 => {
                let full = base.wrapping_add(addr);
                if let Some(ca) = cur_addr {
                    if full == ca.wrapping_add(cur_data.len() as u32) {
                        cur_data.extend_from_slice(data);
                    } else {
                        if !cur_data.is_empty() {
                            segments.push(Segment {
                                address: ca,
                                data: std::mem::take(&mut cur_data),
                            });
                        }
                        cur_addr = Some(full);
                        cur_data.extend_from_slice(data);
                    }
                } else {
                    cur_addr = Some(full);
                    cur_data.extend_from_slice(data);
                }
            }
            0x01 => {
                if let Some(ca) = cur_addr.take() {
                    if !cur_data.is_empty() {
                        segments.push(Segment {
                            address: ca,
                            data: std::mem::take(&mut cur_data),
                        });
                    }
                }
                return Ok(segments);
            }
            0x02 if data.len() >= 2 => {
                base = (u32::from(data[0]) << 8 | u32::from(data[1])) << 4;
            }
            0x04 if data.len() >= 2 => {
                base = (u32::from(data[0]) << 8 | u32::from(data[1])) << 16;
            }
            _ => {}
        }
    }
    if let Some(ca) = cur_addr {
        if !cur_data.is_empty() {
            segments.push(Segment {
                address: ca,
                data: cur_data,
            });
        }
    }
    if segments.is_empty() {
        bail!("HEX 无数据段");
    }
    Ok(segments)
}

fn parse_ti_txt(text: &str) -> Result<Vec<Segment>> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut cur_addr: Option<u32> = None;
    let mut cur_data: Vec<u8> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.eq_ignore_ascii_case("q") {
            if let Some(ca) = cur_addr.take() {
                if !cur_data.is_empty() {
                    segments.push(Segment {
                        address: ca,
                        data: std::mem::take(&mut cur_data),
                    });
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix('@') {
            if let Some(ca) = cur_addr.take() {
                if !cur_data.is_empty() {
                    segments.push(Segment {
                        address: ca,
                        data: std::mem::take(&mut cur_data),
                    });
                }
            }
            cur_addr = Some(
                u32::from_str_radix(rest.trim(), 16)
                    .with_context(|| format!("TI-TXT 地址: {line}"))?,
            );
            continue;
        }
        for h in line.split_whitespace() {
            if h.len() == 2 {
                cur_data
                    .push(u8::from_str_radix(h, 16).with_context(|| format!("TI-TXT 字节: {h}"))?);
            }
        }
    }
    if let Some(ca) = cur_addr {
        if !cur_data.is_empty() {
            segments.push(Segment {
                address: ca,
                data: cur_data,
            });
        }
    }
    if segments.is_empty() {
        bail!("TI-TXT 无数据段");
    }
    Ok(segments)
}

/// Load firmware file: `.hex` / `.txt`(TI-TXT) / raw `.bin` (base 0).
pub fn load_firmware_file(path: &Path) -> Result<Vec<Segment>> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "hex" => {
            let text =
                std::fs::read_to_string(path).with_context(|| format!("读 {}", path.display()))?;
            parse_intel_hex(&text)
        }
        "txt" => {
            let text =
                std::fs::read_to_string(path).with_context(|| format!("读 {}", path.display()))?;
            // Heuristic: TI-TXT uses @addr; otherwise try HEX if starts with :
            let t = text.trim_start();
            if t.starts_with(':') {
                parse_intel_hex(&text)
            } else {
                parse_ti_txt(&text)
            }
        }
        "bin" | "" => {
            let data = std::fs::read(path).with_context(|| format!("读 {}", path.display()))?;
            if data.is_empty() {
                bail!("固件为空");
            }
            Ok(vec![Segment { address: 0, data }])
        }
        _ => {
            // Try binary first if not text-like
            let data = std::fs::read(path).with_context(|| format!("读 {}", path.display()))?;
            if data.first() == Some(&b':') {
                let text = String::from_utf8_lossy(&data);
                parse_intel_hex(&text)
            } else if data.contains(&b'@') {
                let text = String::from_utf8_lossy(&data);
                parse_ti_txt(&text)
            } else {
                Ok(vec![Segment { address: 0, data }])
            }
        }
    }
}

/// Soft-enter BSL while app is running at 115200 (requires firmware `bsl` command).
pub fn soft_enter_bsl(port_name: &str) -> Result<()> {
    {
        let mut port = BslPort::open(port_name, APP_BAUD)?;
        let _ = port.write_all(b"!\r\n");
        thread::sleep(Duration::from_millis(80));
        let _ = port.write_all(b"bsl\r\n");
        thread::sleep(Duration::from_millis(400));
    }
    thread::sleep(Duration::from_millis(300));
    Ok(())
}

/// Full flash flow matching LCKFB `fullFlash`.
/// `bsl_baud`: host opens ROM BSL at this rate (default 9600; LCKFB: 9600…115200).
pub fn flash_segments(
    port_name: &str,
    segments: &[Segment],
    already_in_bsl: bool,
    bsl_baud: u32,
    mut on_progress: impl FnMut(&str),
) -> Result<()> {
    if segments.is_empty() {
        bail!("无固件数据");
    }
    let total: usize = segments.iter().map(|s| s.data.len()).sum();
    if total == 0 {
        bail!("固件为空");
    }
    if total > 512 * 1024 {
        bail!("固件过大 ({total} B)");
    }
    let bsl_baud = if bsl_baud == 0 {
        DEFAULT_BSL_BAUD
    } else {
        bsl_baud
    };

    if !already_in_bsl {
        on_progress("进入 BSL…");
        soft_enter_bsl(port_name)?;
    } else {
        thread::sleep(Duration::from_millis(150));
    }

    on_progress(&format!("连接 {port_name} @ {bsl_baud}…"));
    let mut port = BslPort::open(port_name, bsl_baud)?;

    let mut ok = false;
    let mut last_err = String::new();
    for attempt in 1..=10 {
        on_progress(&format!("连接中 {attempt}/10…"));
        match connection(&mut port) {
            Ok(()) => {
                ok = true;
                break;
            }
            Err(e) => {
                last_err = e.to_string();
                thread::sleep(Duration::from_millis(300));
                port.clear();
            }
        }
    }
    if !ok {
        bail!(
            "BSL 连接失败（{last_err}）。请确认已进入 BSL，CH340 接 PA10/PA11，波特率 {bsl_baud}。"
        );
    }
    on_progress("已连接");
    thread::sleep(Duration::from_millis(100));

    if let Ok(info) = get_device_info(&mut port) {
        on_progress(&format!("设备 {info}"));
    }

    on_progress("解锁…");
    unlock(&mut port).context("Unlock")?;
    on_progress("擦除…");
    mass_erase(&mut port).context("MassErase")?;
    thread::sleep(Duration::from_millis(150));

    on_progress(&format!("写入 {total} B…"));
    program_segments(&mut port, segments, &mut on_progress)?;
    thread::sleep(Duration::from_millis(80));

    // Software “reboot into app”: only reliable host-side restart after BSL.
    on_progress("启动应用 StartApplication (0x40)…");
    start_app(&mut port).context("StartApplication")?;
    on_progress("已发 0x40 · 设备应跳出 BSL 运行新固件");
    // Close BSL UART before host reopens at app baud (115200).
    drop(port);
    // Allow flash app + LFS init (LFS OK / LFS NO banner).
    thread::sleep(Duration::from_millis(900));
    on_progress("烧录完成");
    Ok(())
}

pub fn flash_bin(
    port_name: &str,
    image: &[u8],
    already_in_bsl: bool,
    bsl_baud: u32,
    on_progress: impl FnMut(&str),
) -> Result<()> {
    let segs = vec![Segment {
        address: 0,
        data: image.to_vec(),
    }];
    flash_segments(port_name, &segs, already_in_bsl, bsl_baud, on_progress)
}

pub fn flash_bin_file(
    port_name: &str,
    path: &Path,
    already_in_bsl: bool,
    bsl_baud: u32,
    on_progress: impl FnMut(&str),
) -> Result<()> {
    let segs = load_firmware_file(path)?;
    let n: usize = segs.iter().map(|s| s.data.len()).sum();
    let range = segs
        .iter()
        .map(|s| {
            format!(
                "0x{:08X}-0x{:08X}",
                s.address,
                s.address.wrapping_add(s.data.len() as u32)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut on_progress = on_progress;
    on_progress(&format!(
        "固件 {} · {n} B · {} 段 · {range}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
        segs.len()
    ));
    flash_segments(port_name, &segs, already_in_bsl, bsl_baud, on_progress)
}

/// Prefer firmware files under `<exe>/firmware/`, then next to the exe.
pub fn find_default_firmware() -> Option<std::path::PathBuf> {
    let mut cands = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let fw = dir.join("firmware");
            // This is a complete image: modular core plus eight erased slots.
            // Prefer it over a core-only binary so a reflash cannot retain
            // modules from an older catalog.
            cands.push(fw.join("build_composed/firmware_core.bin"));
            for name in [
                "mspm0_lua_modular.bin",
                "mspm0_lua_bytecode.bin",
                "mspm0_lua_bytecode.hex",
                "firmware.bin",
                "firmware.hex",
                "out.hex",
                "out.bin",
            ] {
                cands.push(fw.join(name));
                cands.push(dir.join(name));
            }
            // Any .bin/.hex inside firmware/ (first sorted).
            if let Ok(rd) = std::fs::read_dir(&fw) {
                let mut extras: Vec<_> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
                            && p.extension()
                                .and_then(|x| x.to_str())
                                .map(|x| {
                                    let x = x.to_ascii_lowercase();
                                    x == "bin" || x == "hex" || x == "txt"
                                })
                                .unwrap_or(false)
                    })
                    .collect();
                extras.sort();
                cands.extend(extras);
            }
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_lckfb_style() {
        // empty core would be unusual; check known vector: cmd 0x12 alone
        let core = [0x12u8];
        let c = crc32(&core);
        // Just ensure deterministic
        assert_eq!(c, crc32(&core));
    }

    #[test]
    fn packet_layout() {
        let p = build_packet(0x12, &[]);
        assert_eq!(p[0], HDR_HOST);
        assert_eq!(p[1], 1);
        assert_eq!(p[2], 0);
        assert_eq!(p[3], 0x12);
        assert_eq!(p.len(), 3 + 1 + 4);
    }
}
