use anyhow::{bail, Context, Result};
use serialport::{SerialPort, SerialPortInfo, SerialPortType};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};

use crate::modular::{Catalog, PlannedSlot};

const NORMAL_BAUD: u32 = 115_200;
const FAST_BAUD: u32 = 460_800;

fn alternate_baud(baud: u32) -> u32 {
    if baud == FAST_BAUD { NORMAL_BAUD } else { FAST_BAUD }
}

#[derive(Clone, Debug)]
pub struct PortChoice {
    pub name: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirmwareInfo {
    pub firmware_id: String,
    pub version: String,
    pub target: String,
    pub core_abi: u16,
    pub module_format: u16,
    pub nmup_format: u16,
    pub slot_count: u8,
    pub slot_size: u32,
    pub catalog_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleSlotStatus {
    pub slot: u8,
    pub name: Option<String>,
    pub size: Option<u32>,
    pub crc32: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleStatus {
    pub pending: bool,
    pub slots: Vec<ModuleSlotStatus>,
}

impl ModuleStatus {
    pub fn matches_plan(&self, planned: &[PlannedSlot]) -> bool {
        if self.pending || self.slots.len() != planned.len() {
            return false;
        }
        self.slots.iter().zip(planned).all(|(actual, expected)| {
            actual.slot == expected.slot
                && actual.name.as_deref() == Some(expected.name.as_str())
                && actual.size == Some(expected.size)
                && actual.crc32 == Some(expected.crc32)
        })
    }

    pub fn has_bad_slot(&self) -> bool {
        self.slots.iter().any(|slot| slot.name.is_none())
    }
}

pub fn list_ports() -> Vec<PortChoice> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(describe_port)
        .collect()
}

fn describe_port(info: SerialPortInfo) -> PortChoice {
    let label = match &info.port_type {
        SerialPortType::UsbPort(usb) => {
            let product = usb.product.clone().unwrap_or_default();
            let mfr = usb.manufacturer.clone().unwrap_or_default();
            if product.to_ascii_lowercase().contains("j-link")
                || mfr.to_ascii_lowercase().contains("segger")
            {
                format!("{} · J-Link (勿选)", info.port_name)
            } else if product.to_ascii_lowercase().contains("ch340")
                || mfr.to_ascii_lowercase().contains("wch")
            {
                format!("{} · CH340", info.port_name)
            } else if !product.is_empty() {
                format!("{} · {product}", info.port_name)
            } else {
                info.port_name.clone()
            }
        }
        _ => info.port_name.clone(),
    };
    PortChoice {
        name: info.port_name,
        label,
    }
}

pub struct SerialSession {
    // Use one OS handle for RX, TX, and line settings. On Windows, changing
    // baud on a `try_clone()` handle does not reliably update the reader
    // handle used by some USB-UART drivers.
    port: Arc<Mutex<Box<dyn SerialPort>>>,
    rx_buffer: Arc<RwLock<String>>,
    stop: Arc<AtomicBool>,
    suppress_rx: Arc<AtomicBool>,
    join: Mutex<Option<thread::JoinHandle<()>>>,
    current_baud: AtomicU32,
    baud_restore_pending: AtomicBool,
}

impl SerialSession {
    pub fn open(port_name: &str, baud: u32) -> Result<Self> {
        let baud = if baud == 0 { NORMAL_BAUD } else { baud };
        let mut port = serialport::new(port_name, baud)
            .data_bits(serialport::DataBits::Eight)
            .stop_bits(serialport::StopBits::One)
            .parity(serialport::Parity::None)
            .timeout(Duration::from_millis(50))
            .dtr_on_open(false)
            .open()
            .with_context(|| format!("open {port_name}"))?;
        // Avoid CH340/DTR quirks that can drop RX after long TX bursts.
        let _ = port.write_data_terminal_ready(false);
        let _ = port.write_request_to_send(false);

        let rx_buffer = Arc::new(RwLock::new(String::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let suppress_rx = Arc::new(AtomicBool::new(false));
        let port = Arc::new(Mutex::new(port));
        let reader = port.clone();
        let rx = rx_buffer.clone();
        let stop_flag = stop.clone();
        let suppress_flag = suppress_rx.clone();
        let join = thread::spawn(move || {
            let mut buf = [0u8; 1024];
            while !stop_flag.load(Ordering::Relaxed) {
                let result = reader.lock().read(&mut buf);
                match result {
                    Ok(0) => thread::sleep(Duration::from_millis(5)),
                    Ok(n) => {
                        if !suppress_flag.load(Ordering::Acquire) {
                            let text = String::from_utf8_lossy(&buf[..n]);
                            rx.write().push_str(&text);
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(_) => {
                        thread::sleep(Duration::from_millis(20));
                    }
                }
            }
        });

        Ok(Self {
            port,
            rx_buffer,
            stop,
            suppress_rx,
            join: Mutex::new(Some(join)),
            current_baud: AtomicU32::new(baud),
            baud_restore_pending: AtomicBool::new(false),
        })
    }

    pub fn clear_rx(&self) {
        self.rx_buffer.write().clear();
    }

    pub fn rx_snapshot(&self) -> String {
        self.rx_buffer.read().clone()
    }

    pub fn drain_new(&self, since: usize) -> (String, usize) {
        let guard = self.rx_buffer.read();
        let text = if since < guard.len() {
            guard[since..].to_string()
        } else {
            String::new()
        };
        (text, guard.len())
    }

    pub fn write_line(&self, line: &str) -> Result<()> {
        let payload = format!("{}\r\n", line.trim_end_matches(['\r', '\n']));
        let mut port = self.port.lock();
        port.write_all(payload.as_bytes()).context("serial write")?;
        port.flush().ok();
        Ok(())
    }

    pub fn rx_mark(&self) -> usize {
        self.rx_buffer.read().len()
    }

    pub fn current_baud(&self) -> u32 {
        self.current_baud.load(Ordering::Relaxed)
    }

    fn set_local_baud(&self, baud: u32) -> Result<()> {
        let mut port = self.port.lock();
        // BAUD_SWITCH is already in the software RX transcript. Discard any
        // stale tail still queued by the Windows USB-UART driver before its
        // DCB changes, otherwise it may reappear as a short user-output line.
        let _ = port.clear(serialport::ClearBuffer::Input);
        port.set_baud_rate(baud)
            .with_context(|| format!("切换主机串口到 {baud}"))?;
        let _ = port.clear(serialport::ClearBuffer::Input);
        self.current_baud.store(baud, Ordering::Relaxed);
        Ok(())
    }

    /// Retry one complete request at the other firmware-supported baud rate.
    /// This does not send a baud-switch command: it recovers when the board
    /// and host had already drifted to different rates after a noisy transfer.
    fn retry_at_alternate_baud<T>(&self, mut request: impl FnMut() -> Result<T>) -> Result<T> {
        let original_baud = self.current_baud();
        match request() {
            Ok(value) => Ok(value),
            Err(first_error) => {
                let fallback_baud = alternate_baud(original_baud);
                self.suppress_rx.store(true, Ordering::Release);
                let switched = self.set_local_baud(fallback_baud);
                thread::sleep(Duration::from_millis(60));
                self.suppress_rx.store(false, Ordering::Release);
                if let Err(switch_error) = switched {
                    bail!(
                        "串口请求失败 @ {original_baud}: {first_error:#}; 切换备用波特率 {fallback_baud} 失败: {switch_error:#}"
                    );
                }
                match request() {
                    Ok(value) => Ok(value),
                    Err(retry_error) => {
                        self.suppress_rx.store(true, Ordering::Release);
                        let restore = self.set_local_baud(original_baud).err();
                        self.suppress_rx.store(false, Ordering::Release);
                        let restore_note = restore
                            .map(|error| format!("; 恢复 {original_baud} 失败: {error:#}"))
                            .unwrap_or_default();
                        bail!(
                            "串口请求失败 @ {original_baud}: {first_error:#}; 备用波特率 {fallback_baud} 重试失败: {retry_error:#}{restore_note}"
                        );
                    }
                }
            }
        }
    }

    fn wait_for_complete_line(
        &self,
        timeout: Duration,
        mark: usize,
        predicate: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            let snapshot = self.rx_snapshot();
            let tail = &snapshot[mark.min(snapshot.len())..];
            if let Some(line) = complete_lines(tail).find(|line| predicate(line)) {
                return Some(line.to_string());
            }
            thread::sleep(Duration::from_millis(15));
        }
        None
    }

    pub fn stop_and_wait(&self) -> Result<()> {
        self.retry_at_alternate_baud(|| self.stop_and_wait_once())
    }

    fn stop_and_wait_once(&self) -> Result<()> {
        for attempt in 0..2 {
            let mark = self.rx_mark();
            self.write_line("!")?;
            if self
                .wait_for_complete_line(Duration::from_secs(3), mark, |line| line == "STOP")
                .is_some()
            {
                return Ok(());
            }
            if attempt == 0 {
                continue;
            }
        }
        bail!("停止脚本超时：未收到完整 STOP");
    }

    pub fn query_and_verify_firmware(&self, catalog: &Catalog) -> Result<FirmwareInfo> {
        self.retry_at_alternate_baud(|| self.query_and_verify_firmware_once(catalog))
    }

    fn query_and_verify_firmware_once(&self, catalog: &Catalog) -> Result<FirmwareInfo> {
        let mark = self.rx_mark();
        self.write_line("fwinfo")?;
        if self
            .wait_for_complete_line(Duration::from_secs(1), mark, |line| {
                line.starts_with("FW_INFO ")
            })
            .is_none()
        {
            bail!("设备不支持模块化发布协议：1 秒内未收到 FW_INFO");
        }
        if self
            .wait_for_complete_line(Duration::from_secs(3), mark, |line| line == "FW_INFO_END")
            .is_none()
        {
            bail!("fwinfo 响应不完整：3 秒内未收到 FW_INFO_END");
        }
        let snapshot = self.rx_snapshot();
        let info = parse_fwinfo(&snapshot[mark.min(snapshot.len())..])?;
        let mut differences = Vec::new();
        if info.firmware_id != catalog.identity.firmware_id {
            differences.push(format!(
                "firmware-id: 设备={}, catalog={}",
                info.firmware_id, catalog.identity.firmware_id
            ));
        }
        if info.version != catalog.identity.firmware_version {
            differences.push(format!(
                "version: 设备={}, catalog={}",
                info.version, catalog.identity.firmware_version
            ));
        }
        if info.target != catalog.identity.target {
            differences.push(format!(
                "target: 设备={}, catalog={}",
                info.target, catalog.identity.target
            ));
        }
        if info.core_abi != catalog.identity.core_abi {
            differences.push(format!(
                "Core ABI: 设备={}, catalog={}",
                info.core_abi, catalog.identity.core_abi
            ));
        }
        if info.module_format != catalog.identity.module_format {
            differences.push(format!(
                "module format: 设备={}, catalog={}",
                info.module_format, catalog.identity.module_format
            ));
        }
        if info.nmup_format != catalog.identity.nmup_format {
            differences.push(format!(
                "NMUP format: 设备={}, catalog={}",
                info.nmup_format, catalog.identity.nmup_format
            ));
        }
        if info.slot_count != catalog.layout.slot_count
            || info.slot_size != catalog.layout.slot_size
        {
            differences.push(format!(
                "slots: 设备={}x{}, catalog={}x{}",
                info.slot_count,
                info.slot_size,
                catalog.layout.slot_count,
                catalog.layout.slot_size
            ));
        }
        if info.catalog_sha256 != catalog.catalog_sha256 {
            differences.push(format!(
                "catalog SHA-256: 设备={}, 本地={}",
                info.catalog_sha256, catalog.catalog_sha256
            ));
        }
        if !differences.is_empty() {
            bail!("设备身份不匹配，禁止 NMUP：{}", differences.join("；"));
        }
        Ok(info)
    }

    pub fn switch_baud(&self, target: u32) -> Result<()> {
        self.retry_at_alternate_baud(|| self.switch_baud_once(target))
    }

    fn switch_baud_once(&self, target: u32) -> Result<()> {
        if target != NORMAL_BAUD && target != FAST_BAUD {
            bail!("固件只支持 115200 和 460800");
        }
        if self.current_baud() == target {
            return Ok(());
        }
        let mark = self.rx_mark();
        self.write_line(&format!("baud {target}"))?;
        let switch_line = format!("BAUD_SWITCH {target}");
        let response = self.wait_for_complete_line(Duration::from_secs(2), mark, |line| {
            line == switch_line || line == "BAUD_ERR"
        });
        match response.as_deref() {
            Some("BAUD_ERR") => bail!("设备拒绝波特率 {target}"),
            Some(_) => {}
            None => bail!("波特率切换超时：未收到 {switch_line}"),
        }
        self.suppress_rx.store(true, Ordering::Release);
        if let Err(error) = self.set_local_baud(target) {
            self.suppress_rx.store(false, Ordering::Release);
            return Err(error);
        }
        let ack = format!("BAUD_OK {target}");
        // The firmware changes its divisor 300 ms after BAUD_SWITCH and emits
        // no unsolicited new-rate bytes. Confirm actively after it settles.
        thread::sleep(Duration::from_millis(380));
        self.suppress_rx.store(false, Ordering::Release);
        for _ in 0..3 {
            let confirm_mark = self.rx_mark();
            self.write_line(&format!("baud {target}"))?;
            if self
                .wait_for_complete_line(Duration::from_millis(700), confirm_mark, |line| {
                    line == ack
                })
                .is_some()
            {
                return Ok(());
            }
        }
        bail!("波特率切换未确认：新速率 {target} 无响应")
    }

    pub fn recover_to_115200(&self, catalog: &Catalog) -> Result<()> {
        let mut errors = Vec::new();
        for baud in [FAST_BAUD, NORMAL_BAUD] {
            if let Err(error) = self.set_local_baud(baud) {
                errors.push(format!("主机切换 {baud}: {error:#}"));
                continue;
            }
            match self.query_and_verify_firmware_once(catalog) {
                Ok(_) if baud == NORMAL_BAUD => return Ok(()),
                Ok(_) => return self.switch_baud(NORMAL_BAUD),
                Err(error) => errors.push(format!("探测 {baud}: {error:#}")),
            }
        }
        bail!("无法恢复已知串口速率：{}", errors.join("；"));
    }

    /// Return to the normal console baud when a launched script terminates.
    /// The RX worker can observe both STOP and SCRIPT_DONE, so only the first
    /// observer is allowed to negotiate the change.
    pub fn restore_115200_after_run(&self) -> Result<bool> {
        if self.current_baud() == NORMAL_BAUD {
            return Ok(false);
        }
        if self
            .baud_restore_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(false);
        }
        let result = self.switch_baud(NORMAL_BAUD);
        self.baud_restore_pending.store(false, Ordering::Release);
        result.map(|_| true)
    }

    pub fn module_status(&self) -> Result<ModuleStatus> {
        self.retry_at_alternate_baud(|| self.module_status_once())
    }

    fn module_status_once(&self) -> Result<ModuleStatus> {
        let mark = self.rx_mark();
        self.write_line("modstatus")?;
        if self
            .wait_for_complete_line(Duration::from_secs(3), mark, |line| {
                line == "MOD_STATUS_END"
            })
            .is_none()
        {
            bail!("modstatus 超时：未收到 MOD_STATUS_END");
        }
        let snapshot = self.rx_snapshot();
        parse_module_status(&snapshot[mark.min(snapshot.len())..])
    }

    pub fn apply_module_bundle(
        &self,
        bundle: &[u8],
        selected_count: usize,
        mut on_progress: impl FnMut(&str),
    ) -> Result<()> {
        self.retry_at_alternate_baud(|| {
            self.apply_module_bundle_once(bundle, selected_count, |message| on_progress(message))
        })
    }

    fn apply_module_bundle_once(
        &self,
        bundle: &[u8],
        selected_count: usize,
        mut on_progress: impl FnMut(&str),
    ) -> Result<()> {
        self.upload_hex_strict_with_progress_once("modules.upd", bundle, |message| {
            on_progress(message)
        })?;
        let mark = self.rx_mark();
        self.write_line("modapply modules.upd")?;
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut saw_ready = false;
        let mut saw_apply = false;
        let mut saw_done = false;
        let mut processed = 0usize;
        while Instant::now() < deadline {
            let snapshot = self.rx_snapshot();
            let tail = &snapshot[mark.min(snapshot.len())..];
            let lines: Vec<_> = complete_lines(tail).collect();
            while processed < lines.len() {
                let line = lines[processed];
                processed += 1;
                if let Some(reason) = line.strip_prefix("MOD_ERR ") {
                    bail!("模块事务失败 MOD_ERR {reason}");
                }
                if let Some(reason) = line.strip_prefix("MOD_BLOCKED ") {
                    bail!("模块事务被阻止 MOD_BLOCKED {reason}");
                }
                if let Some(rest) = line.strip_prefix("MOD_READY ") {
                    let fields: Vec<_> = rest.split_whitespace().collect();
                    if fields.len() != 2
                        || fields[0].parse::<usize>().ok() != Some(selected_count)
                        || fields[1].parse::<usize>().ok() != Some(bundle.len())
                    {
                        bail!("MOD_READY 与本地 bundle 不一致: {line}");
                    }
                    saw_ready = true;
                    on_progress("模块包已校验");
                } else if line == "MOD_APPLY modules.upd" {
                    saw_apply = true;
                    on_progress("正在写入原生模块…");
                } else if let Some(rest) = line.strip_prefix("MOD_ERASE ") {
                    on_progress(&format!("擦除 slot {rest}"));
                } else if let Some(rest) = line.strip_prefix("MOD_WRITE ") {
                    on_progress(&format!("写入 {rest}"));
                } else if line == "MOD_VERIFY" {
                    on_progress("读回校验原生模块…");
                } else if let Some(count) = line.strip_prefix("MOD_DONE ") {
                    if count.parse::<usize>().ok() != Some(selected_count) {
                        bail!("MOD_DONE 模块数量不一致: {line}");
                    }
                    saw_done = true;
                    on_progress("模块写入完成，等待 VM…");
                } else if line == "Idle" && saw_done {
                    if !saw_ready || !saw_apply {
                        bail!("模块事务缺少 MOD_READY/MOD_APPLY，拒绝接受成功");
                    }
                    return Ok(());
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        bail!("模块事务超时：必须同时收到 MOD_DONE {selected_count} 和随后 Idle");
    }

    pub fn wait_for(&self, token: &str, timeout: Duration, mark: usize) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            {
                let guard = self.rx_buffer.read();
                if guard.len() > mark && guard[mark..].contains(token) {
                    return true;
                }
            }
            thread::sleep(Duration::from_millis(15));
        }
        false
    }

    /// Soft wait: also succeed if later evidence proves the device moved on
    /// (e.g. SCRIPT_OK / LED prints after a missed HEX_OK).
    pub fn wait_for_any(&self, tokens: &[&str], timeout: Duration, mark: usize) -> Option<String> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            {
                let guard = self.rx_buffer.read();
                if guard.len() > mark {
                    let slice = &guard[mark..];
                    for t in tokens {
                        if slice.contains(t) {
                            return Some((*t).to_string());
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(15));
        }
        None
    }

    pub fn upload_hex(&self, name: &str, data: &[u8]) -> Result<()> {
        self.upload_hex_with_progress(name, data, |_| {})
    }

    /// Compatibility upload command used by manual uploads. It stops the current script first.
    pub fn upload_hex_with_progress(
        &self,
        name: &str,
        data: &[u8],
        mut on_progress: impl FnMut(&str),
    ) -> Result<()> {
        let _ = self.write_line("!");
        thread::sleep(Duration::from_millis(120));
        self.upload_hex_strict_with_progress(name, data, |message| on_progress(message))
    }

    /// Transactional HEX upload. Every ACK and the exact received byte count are mandatory.
    pub fn upload_hex_strict_with_progress(
        &self,
        name: &str,
        data: &[u8],
        mut on_progress: impl FnMut(&str),
    ) -> Result<()> {
        self.retry_at_alternate_baud(|| {
            self.upload_hex_strict_with_progress_once(name, data, |message| on_progress(message))
        })
    }

    fn upload_hex_strict_with_progress_once(
        &self,
        name: &str,
        data: &[u8],
        mut on_progress: impl FnMut(&str),
    ) -> Result<()> {
        if data.is_empty() {
            bail!("文件为空");
        }
        if !valid_device_filename(name) {
            bail!("设备文件名必须为 1..28 字节的 ASCII 字母/数字/_/./-");
        }
        let total = data.chunks(120).count().max(1);
        on_progress(&format!("开始上传 {name} · 0/{total}"));
        let start_mark = self.rx_mark();
        self.write_line(&format!("<<<HEX {name}"))?;
        let begin = self.wait_for_complete_line(Duration::from_secs(3), start_mark, |line| {
            line == "SCRIPT_BEGIN" || line.starts_with("SCRIPT_ERR")
        });
        match begin.as_deref() {
            Some("SCRIPT_BEGIN") => {}
            Some(error) => bail!("板端拒绝上传 {name}: {error}"),
            None => bail!("设备未返回完整 SCRIPT_BEGIN"),
        }

        for (index, chunk) in data.chunks(120).enumerate() {
            let hex: String = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let before = self.rx_mark();
            self.write_line(&hex)?;
            let ack = self.wait_for_complete_line(Duration::from_secs(3), before, |line| {
                line == "HEX_OK" || line.starts_with("SCRIPT_ERR")
            });
            match ack.as_deref() {
                Some("HEX_OK") => {}
                Some(error) => bail!("第 {} 块被拒绝: {error}", index + 1),
                None => bail!(
                    "第 {} 块 ACK 超时；为避免重复追加数据，本次上传已中止",
                    index + 1
                ),
            }
            on_progress(&format!(
                "上传中… {}/{} · {} B",
                index + 1,
                total,
                data.len()
            ));
        }

        on_progress("校验 SCRIPT_OK…");
        let finish_mark = self.rx_mark();
        self.write_line(">>>HEX")?;
        let seconds = 8u64.max((data.len() as u64 + 4_999) / 5_000);
        let response =
            self.wait_for_complete_line(Duration::from_secs(seconds), finish_mark, |line| {
                line.starts_with("SCRIPT_OK") || line.starts_with("SCRIPT_ERR")
            });
        let line = response.ok_or_else(|| anyhow::anyhow!("设备未返回 SCRIPT_OK"))?;
        if line.starts_with("SCRIPT_ERR") {
            bail!("板端结束上传失败: {line}");
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 2 || fields[0] != "SCRIPT_OK" {
            bail!("无效 SCRIPT_OK 响应: {line}");
        }
        let received = fields[1]
            .parse::<usize>()
            .with_context(|| format!("SCRIPT_OK 长度不是十进制: {line}"))?;
        if received != data.len() {
            bail!(
                "上传长度不一致：本地 {} B，设备确认 {} B",
                data.len(),
                received
            );
        }
        on_progress(&format!("上传完成 · {} B", data.len()));
        Ok(())
    }

    pub fn list_files(&self) -> Result<Vec<(String, u64)>> {
        self.retry_at_alternate_baud(|| self.list_files_once())
    }

    fn list_files_once(&self) -> Result<Vec<(String, u64)>> {
        // Do not clear_rx — preserves running script prints in the console.
        let mark = self.rx_buffer.read().len();
        self.write_line("ls")?;
        if !self.wait_for("LS_END", Duration::from_secs(4), mark) {
            bail!("ls timeout: missing LS_END");
        }
        let text = self.rx_snapshot();
        Ok(parse_file_list(&text[mark.min(text.len())..]))
    }

    pub fn run_main(&self) -> Result<()> {
        self.write_line("r")
    }

    pub fn stop(&self) -> Result<()> {
        self.write_line("!")
    }

    pub fn set_boot(&self, name: &str) -> Result<()> {
        self.retry_at_alternate_baud(|| self.set_boot_once(name))
    }

    fn set_boot_once(&self, name: &str) -> Result<()> {
        self.clear_rx();
        self.write_line(&format!("boot {name}"))?;
        if !self.wait_for("BOOT_OK", Duration::from_secs(8), 0) {
            bail!("设置启动文件失败");
        }
        Ok(())
    }

    pub fn delete_file(&self, name: &str) -> Result<()> {
        self.retry_at_alternate_baud(|| self.delete_file_once(name))
    }

    fn delete_file_once(&self, name: &str) -> Result<()> {
        self.clear_rx();
        self.write_line(&format!("rm {name}"))?;
        if !self.wait_for("RM_", Duration::from_secs(3), 0) {
            bail!("rm timeout: missing RM_OK/RM_ERR");
        }
        Ok(())
    }

    /// True if RX already saw app boot LFS banner.
    pub fn saw_lfs_banner(&self) -> Option<bool> {
        let snap = self.rx_snapshot();
        if snap.contains("LFS OK") {
            Some(true)
        } else if snap.contains("LFS NO") {
            Some(false)
        } else {
            None
        }
    }

    /// Wait briefly for boot noise / LFS banner after open @ app baud.
    pub fn wait_boot_settle(&self, timeout: Duration) -> Option<bool> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if let Some(v) = self.saw_lfs_banner() {
                return Some(v);
            }
            thread::sleep(Duration::from_millis(40));
        }
        self.saw_lfs_banner()
    }

    /// Probe whether board LittleFS is mounted (`board_lfs_ready`).
    /// New firmware includes a readiness token in `storageinfo` without
    /// writing Flash. The legacy file probe remains for older cores.
    /// Returns `Ok(true)` ready, `Ok(false)` needs format, `Err` on comm failure.
    pub fn probe_lfs(&self) -> Result<bool> {
        self.retry_at_alternate_baud(|| self.probe_lfs_once())
    }

    fn probe_lfs_once(&self) -> Result<bool> {
        let mark = self.rx_mark();
        self.write_line("storageinfo")?;
        if let Some(line) = self.wait_for_complete_line(
            Duration::from_millis(700),
            mark,
            |line| line == "FS_READY" || line == "FS_NOT_READY",
        ) {
            return Ok(line == "FS_READY");
        }

        // Compatibility with cores predating the readiness field.
        if let Some(v) = self.saw_lfs_banner() {
            return Ok(v);
        }
        let _ = self.write_line("!");
        thread::sleep(Duration::from_millis(150));
        if let Some(v) = self.saw_lfs_banner() {
            return Ok(v);
        }
        // `ls` always ends with LS_END even if LFS down — use HEX open as real probe.
        let mark = self.rx_buffer.read().len();
        self.write_line("<<<HEX __p.luac")?;
        match self.wait_for_any(
            &["SCRIPT_BEGIN", "SCRIPT_ERR", "HEX_OK"],
            Duration::from_millis(3500),
            mark,
        ) {
            Some(tok) if tok == "SCRIPT_BEGIN" || tok == "HEX_OK" => {
                let before = self.rx_buffer.read().len();
                let _ = self.write_line(">>>HEX");
                let _ = self.wait_for_any(
                    &["SCRIPT_OK", "SCRIPT_ERR", "SCRIPT_DONE"],
                    Duration::from_secs(5),
                    before,
                );
                let _ = self.delete_file_once("__p.luac");
                Ok(true)
            }
            Some(_) => {
                let snap = self.rx_snapshot();
                let tail = &snap[mark.min(snap.len())..];
                if tail.contains("name/fs") || tail.contains("SCRIPT_ERR open") {
                    Ok(false)
                } else if tail.contains("SCRIPT_ERR") {
                    Ok(false)
                } else {
                    Ok(false)
                }
            }
            None => {
                let snap = self.rx_snapshot();
                if snap.contains("LFS OK") {
                    return Ok(true);
                }
                if snap.contains("LFS NO") {
                    return Ok(false);
                }
                // No UART response → app likely not running (still BSL / need RST).
                bail!(
                    "应用无响应（未看到 LFS/SCRIPT_* · 可能未执行 StartApplication 或需手动 RST）"
                );
            }
        }
    }

    /// Reformat LittleFS on SPI flash (`format` console command → `board_lfs_format`).
    /// Returns capacity bytes on success (from `FORMAT_OK <n>`).
    pub fn format_lfs(&self) -> Result<u32> {
        self.retry_at_alternate_baud(|| self.format_lfs_once())
    }

    fn format_lfs_once(&self) -> Result<u32> {
        let _ = self.write_line("!");
        thread::sleep(Duration::from_millis(200));
        // Capture mark after stop settles so we only match this format response.
        let mark = self.rx_buffer.read().len();
        self.write_line("format")?;
        // Full-chip LittleFS format can take a while on SPI NOR.
        match self.wait_for_any(&["FORMAT_OK", "FORMAT_ERR"], Duration::from_secs(60), mark) {
            Some(tok) if tok == "FORMAT_OK" => {
                let snap = self.rx_snapshot();
                let tail = &snap[mark.min(snap.len())..];
                let cap = tail
                    .lines()
                    .find(|l| l.contains("FORMAT_OK"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);
                Ok(cap)
            }
            Some(_) => bail!("板端 FORMAT_ERR（擦写 SPI Flash 失败）"),
            None => {
                // Fallback: token may have arrived slightly before mark if RX raced.
                let snap = self.rx_snapshot();
                if let Some(line) = snap.lines().rev().find(|l| l.contains("FORMAT_OK")) {
                    let cap = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    return Ok(cap);
                }
                if snap.contains("FORMAT_ERR") {
                    bail!("板端 FORMAT_ERR（擦写 SPI Flash 失败）");
                }
                bail!("format 超时（未收到 FORMAT_OK）");
            }
        }
    }
}

fn valid_device_filename(name: &str) -> bool {
    (1..=28).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn complete_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_inclusive('\n').filter_map(|segment| {
        segment
            .strip_suffix('\n')
            .map(|line| line.trim_end_matches('\r'))
    })
}

fn parse_fwinfo(text: &str) -> Result<FirmwareInfo> {
    let lines: Vec<_> = complete_lines(text)
        .filter(|line| line.starts_with("FW_"))
        .take_while(|line| *line != "FW_INFO_END")
        .collect();
    let ended = complete_lines(text).any(|line| line == "FW_INFO_END");
    if !ended || lines.len() != 7 {
        bail!("fwinfo 字段数量或结束行无效");
    }
    let info = split_exact(lines[0], "FW_INFO", 2)?;
    let target = split_exact(lines[1], "FW_TARGET", 1)?;
    let abi = split_exact(lines[2], "FW_ABI", 1)?;
    let module_format = split_exact(lines[3], "FW_MODULE_FORMAT", 1)?;
    let nmup_format = split_exact(lines[4], "FW_NMUP_FORMAT", 1)?;
    let slots = split_exact(lines[5], "FW_SLOTS", 2)?;
    let catalog = split_exact(lines[6], "FW_CATALOG", 1)?;
    if catalog[0].len() != 64
        || catalog[0] != catalog[0].to_ascii_lowercase()
        || !catalog[0].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("FW_CATALOG 不是 lowercase SHA-256");
    }
    Ok(FirmwareInfo {
        firmware_id: info[0].to_string(),
        version: info[1].to_string(),
        target: target[0].to_string(),
        core_abi: parse_decimal(abi[0], "FW_ABI")?,
        module_format: parse_decimal(module_format[0], "FW_MODULE_FORMAT")?,
        nmup_format: parse_decimal(nmup_format[0], "FW_NMUP_FORMAT")?,
        slot_count: parse_decimal(slots[0], "FW_SLOTS count")?,
        slot_size: parse_decimal(slots[1], "FW_SLOTS size")?,
        catalog_sha256: catalog[0].to_string(),
    })
}

fn parse_module_status(text: &str) -> Result<ModuleStatus> {
    let lines: Vec<_> = complete_lines(text).collect();
    let start = lines
        .iter()
        .position(|line| line.starts_with("MOD_STATUS "))
        .ok_or_else(|| anyhow::anyhow!("modstatus 缺少 MOD_STATUS"))?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| (*line == "MOD_STATUS_END").then_some(index))
        .ok_or_else(|| anyhow::anyhow!("modstatus 缺少 MOD_STATUS_END"))?;
    let pending = match lines[start] {
        "MOD_STATUS IDLE" => false,
        "MOD_STATUS PENDING" => true,
        other => bail!("无效模块状态: {other}"),
    };
    let mut slots = Vec::new();
    let mut previous = None;
    for line in &lines[start + 1..end] {
        let Some(rest) = line.strip_prefix("MOD_SLOT ") else {
            continue;
        };
        let fields: Vec<_> = rest.split_whitespace().collect();
        let slot: u8 = fields
            .first()
            .ok_or_else(|| anyhow::anyhow!("MOD_SLOT 缺少 slot"))?
            .parse()
            .with_context(|| format!("MOD_SLOT slot 不是十进制: {line}"))?;
        if slot >= 8 || previous.is_some_and(|value| slot <= value) {
            bail!("MOD_SLOT 必须按 0..7 严格升序且不重复: {line}");
        }
        previous = Some(slot);
        if fields.len() == 2 && fields[1] == "BAD" {
            slots.push(ModuleSlotStatus {
                slot,
                name: None,
                size: None,
                crc32: None,
            });
            continue;
        }
        if fields.len() != 4 {
            bail!("无效 MOD_SLOT 行: {line}");
        }
        let name = fields[1];
        if name.is_empty()
            || name.len() > 7
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            bail!("MOD_SLOT 模块名无效: {line}");
        }
        let size = fields[2]
            .parse::<u32>()
            .with_context(|| format!("MOD_SLOT size 不是十进制: {line}"))?;
        let crc = fields[3];
        if crc.len() != 8
            || crc != crc.to_ascii_lowercase()
            || !crc.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("MOD_SLOT CRC32 必须是 8 位小写十六进制: {line}");
        }
        slots.push(ModuleSlotStatus {
            slot,
            name: Some(name.to_string()),
            size: Some(size),
            crc32: Some(u32::from_str_radix(crc, 16)?),
        });
    }
    Ok(ModuleStatus { pending, slots })
}

fn split_exact<'a>(line: &'a str, prefix: &str, fields: usize) -> Result<Vec<&'a str>> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some(prefix) {
        bail!("fwinfo 字段顺序错误：期望 {prefix}，实际 {line}");
    }
    let values: Vec<_> = parts.collect();
    if values.len() != fields {
        bail!("{prefix} 字段数量错误: {line}");
    }
    Ok(values)
}

fn parse_decimal<T>(value: &str, field: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    if value.len() > 1 && value.starts_with('0') || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("{field} 必须是规范十进制: {value}");
    }
    value.parse().with_context(|| format!("解析 {field}"))
}

/// Board rejected upload due to invalid name or LittleFS not mounted.
pub fn is_lfs_error(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    e.contains("name/fs")
        || e.contains("script_err open")
        || e.contains("script_err name")
        || e.contains("format_err")
}

impl Drop for SerialSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join.lock().take() {
            let _ = handle.join();
        }
    }
}

fn parse_file_list(text: &str) -> Vec<(String, u64)> {
    let mut files = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        // Accept both "F name size" and "F  name  size"
        let rest = if let Some(r) = line.strip_prefix("F ") {
            r
        } else if let Some(r) = line.strip_prefix('F') {
            r.trim_start()
        } else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        if let (Some(name), Some(size)) = (parts.next(), parts.next()) {
            if name.ends_with(".luac") {
                if let Ok(n) = size.parse::<u64>() {
                    files.push((name.to_string(), n));
                }
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{alternate_baud, complete_lines, parse_fwinfo, parse_module_status, FAST_BAUD, NORMAL_BAUD};

    #[test]
    fn alternate_baud_swaps_between_the_only_supported_rates() {
        assert_eq!(alternate_baud(NORMAL_BAUD), FAST_BAUD);
        assert_eq!(alternate_baud(FAST_BAUD), NORMAL_BAUD);
    }

    #[test]
    fn baud_handshake_waits_for_complete_fragmented_lines() {
        let mut transcript = "BAUD_SWITCH 460".to_string();
        assert!(complete_lines(&transcript).next().is_none());

        transcript.push_str("800\r\n");
        assert!(complete_lines(&transcript).any(|line| line == "BAUD_SWITCH 460800"));

        transcript.push_str("BAUD_OK 460");
        assert!(!complete_lines(&transcript).any(|line| line == "BAUD_OK 460800"));

        transcript.push_str("800\n");
        assert!(complete_lines(&transcript).any(|line| line == "BAUD_OK 460800"));
    }

    #[test]
    fn parses_exact_firmware_identity() {
        let info = parse_fwinfo(
            "FW_INFO mspm0g3507.lua-modular 1.0.0\r\n\
FW_TARGET MSPM0G3507\n\
FW_ABI 7\n\
FW_MODULE_FORMAT 2\n\
FW_NMUP_FORMAT 1\n\
FW_SLOTS 8 4096\n\
FW_CATALOG 3ca3fe0faae57debe0094019185b527595586b0c34593c541c226be7614c3f8c\n\
FW_INFO_END\n",
        )
        .unwrap();
        assert_eq!(info.core_abi, 7);
        assert_eq!(info.slot_count, 8);
        assert_eq!(info.slot_size, 4096);
    }

    #[test]
    fn parses_and_compares_module_slots() {
        let status = parse_module_status(
            "MOD_STATUS IDLE\n\
MOD_CATALOG c1609433a2bc70d4d991de00454f23f377fd71021f523447a3e00d61d4e2d23c\n\
MOD_SLOT 0 i2c 3352 18c593d8\n\
MOD_LAYOUT 1 cce9db62\n\
MOD_PENDING none\n\
MOD_STATUS_END\n",
        )
        .unwrap();
        assert!(!status.pending);
        assert_eq!(status.slots[0].name.as_deref(), Some("i2c"));
        assert_eq!(status.slots[0].crc32, Some(0x18c5_93d8));
    }

    #[test]
    fn rejects_noncanonical_crc_and_incomplete_fwinfo() {
        assert!(parse_module_status(
            "MOD_STATUS IDLE\nMOD_SLOT 0 i2c 3352 18C593D8\nMOD_STATUS_END\n"
        )
        .is_err());
        assert!(parse_fwinfo("FW_INFO x 1.0.0\nFW_INFO_END\n").is_err());
    }

    #[test]
    fn parses_hardware_verified_transcript_boundaries() {
        let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
            return;
        };
        let path = root.join("mspm0_lua/release/test-vectors/verified-transcript.txt");
        let Ok(transcript) = std::fs::read_to_string(path) else {
            return;
        };
        let device_lines = transcript
            .lines()
            .filter_map(|line| {
                line.starts_with("D@")
                    .then(|| line.split_once(' ').map(|(_, payload)| payload))
                    .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let info = parse_fwinfo(&device_lines).unwrap();
        assert_eq!(info.firmware_id, "mspm0g3507.lua-modular");
        let status = parse_module_status(&device_lines).unwrap();
        assert_eq!(status.slots.len(), 8);
        assert_eq!(status.slots[0].name.as_deref(), Some("gpio"));
        assert_eq!(status.slots[7].name.as_deref(), Some("can"));
    }
}
