use std::collections::HashMap;

use gpui::Hsla;

use crate::metadata::TargetProfile;
use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Text,
    Keyword,
    Builtin,
    Module,
    String,
    Number,
    Comment,
    Operator,
    Pin,
}

impl TokenKind {
    pub fn color(self, theme: &Theme) -> Hsla {
        match self {
            TokenKind::Text => theme.syn_text,
            TokenKind::Keyword => theme.syn_keyword,
            TokenKind::Builtin => theme.syn_builtin,
            TokenKind::Module => theme.syn_module,
            TokenKind::String => theme.syn_string,
            TokenKind::Number => theme.syn_number,
            TokenKind::Comment => theme.syn_comment,
            TokenKind::Operator => theme.syn_operator,
            TokenKind::Pin => theme.syn_pin,
        }
    }
}

pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

const BUILTINS: &[&str] = &[
    "print",
    "millis",
    "delay_ms",
    "yield",
    "stopped",
    "bytes",
    "byte",
    "runfile",
    "require",
    "assert",
    "collectgarbage",
    "type",
    "tonumber",
    "tostring",
    "pairs",
    "ipairs",
    "next",
    "select",
    "error",
    "pcall",
    "xpcall",
];

/// Modular-firmware fallback completion surface. A loaded API profile takes
/// precedence; this prevents the pre-profile editor from suggesting legacy APIs.
const MODULES: &[&str] = &[
    "gpio", "uart", "adc", "i2c", "spi", "pwm", "tmr", "event", "oled", "iq", "can", "dac",
    "crc", "comp", "rtc", "opa",
];

struct MemberSpec {
    module: &'static str,
    members: &'static [&'static str],
    detail: &'static str,
}

const MEMBERS: &[MemberSpec] = &[
    MemberSpec {
        module: "gpio",
        members: &["mode", "set", "write", "od_write", "get", "read", "toggle", "af", "owner", "policy", "valid", "release"],
        detail: "firmware gpio",
    },
    MemberSpec {
        module: "uart",
        members: &["open", "close", "tx", "rx", "valid"],
        detail: "firmware uart",
    },
    MemberSpec {
        module: "adc",
        members: &["channel", "instance", "read", "read_mv", "release"],
        detail: "firmware adc",
    },
    MemberSpec {
        module: "i2c",
        members: &["write", "read", "write_read", "write_on", "read_on", "write_read_on", "probe_on", "recover", "valid", "bytes"],
        detail: "firmware i2c",
    },
    MemberSpec {
        module: "spi",
        members: &["xfer", "xfer_on", "read_on", "valid"],
        detail: "firmware spi",
    },
    MemberSpec {
        module: "pwm",
        members: &["open", "open_on", "duty", "close", "open_pair", "close_pair", "route"],
        detail: "firmware pwm",
    },
    MemberSpec {
        module: "tmr",
        members: &["start", "every", "ready", "take", "stop", "millis", "delay", "hw_start", "hw_value", "hw_ready", "hw_stop", "capture_open", "capture_ready", "capture_read", "capture_close", "route"],
        detail: "firmware timer",
    },
    MemberSpec {
        module: "event",
        members: &["run", "poll", "stop"],
        detail: "firmware event loop",
    },
    MemberSpec {
        module: "oled",
        members: &["open", "close", "fill", "clear", "text", "number"],
        detail: "bundled SSD1306 OLED Lua library",
    },
    MemberSpec {
        module: "iq",
        members: &[
            "from",
            "from_x10",
            "from_x100",
            "to_x10",
            "to_x100",
            "to_x1000",
            "mul",
            "div",
            "sin_deg",
            "cos_deg",
            "atan2_deg",
        ],
        detail: "firmware Q16.16",
    },
    MemberSpec {
        module: "can",
        members: &["open", "open_on", "close", "send", "recv", "valid"],
        detail: "firmware CAN",
    },
    MemberSpec {
        module: "dac",
        members: &["open", "write", "write_mv", "close"],
        detail: "firmware DAC",
    },
    MemberSpec {
        module: "crc",
        members: &["crc16", "crc32"],
        detail: "firmware CRC",
    },
    MemberSpec {
        module: "comp",
        members: &["open", "read", "close"],
        detail: "firmware comparator",
    },
    MemberSpec {
        module: "rtc",
        members: &["open", "set", "get", "close"],
        detail: "firmware RTC",
    },
    MemberSpec {
        module: "opa",
        members: &["open", "ready", "close"],
        detail: "firmware operational amplifier",
    },
];

/// Highlight a single line (no newlines). Offsets are line-local.
pub fn highlight_line(line: &str) -> Vec<Span> {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            spans.push(Span {
                start: i,
                end: bytes.len(),
                kind: TokenKind::Comment,
            });
            break;
        }
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            spans.push(Span {
                start,
                end: i,
                kind: TokenKind::String,
            });
            continue;
        }
        if (bytes[i] == b'P' || bytes[i] == b'p')
            && i + 1 < bytes.len()
            && (bytes[i + 1] == b'A'
                || bytes[i + 1] == b'B'
                || bytes[i + 1] == b'a'
                || bytes[i + 1] == b'b')
        {
            let start = i;
            i += 2;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i - start >= 3 {
                spans.push(Span {
                    start,
                    end: i,
                    kind: TokenKind::Pin,
                });
                continue;
            }
            i = start;
        }
        if bytes[i].is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || bytes[i] == b'.'
                    || bytes[i] == b'x'
                    || bytes[i] == b'X'
                    || (bytes[i] >= b'a' && bytes[i] <= b'f')
                    || (bytes[i] >= b'A' && bytes[i] <= b'F'))
            {
                i += 1;
            }
            spans.push(Span {
                start,
                end: i,
                kind: TokenKind::Number,
            });
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let kind = if KEYWORDS.contains(&word) {
                TokenKind::Keyword
            } else if BUILTINS.contains(&word) {
                TokenKind::Builtin
            } else if MODULES.contains(&word) {
                TokenKind::Module
            } else {
                TokenKind::Text
            };
            spans.push(Span {
                start,
                end: i,
                kind,
            });
            continue;
        }
        // operators / punctuation as operator color for .
        if b"+-*/%^#=<>~:.,;()[]{}".contains(&bytes[i]) {
            let start = i;
            i += 1;
            spans.push(Span {
                start,
                end: i,
                kind: TokenKind::Operator,
            });
            continue;
        }
        i += 1;
    }
    if spans.is_empty() && !line.is_empty() {
        spans.push(Span {
            start: 0,
            end: line.len(),
            kind: TokenKind::Text,
        });
    }
    spans
}

#[derive(Clone)]
pub struct CompletionItem {
    pub label: String,
    pub insert: String,
    pub detail: String,
}

/// Byte start of the identifier under cursor and the prefix text.
pub fn word_prefix_at(text: &str, cursor: usize) -> (usize, String) {
    let cursor = cursor.min(text.len());
    let mut end = cursor;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut start = end;
    let bytes = text.as_bytes();
    while start > 0 {
        let prev = start - 1;
        let b = bytes[prev];
        if b.is_ascii_alphanumeric() || b == b'_' {
            start = prev;
        } else {
            break;
        }
    }
    while start < end && !text.is_char_boundary(start) {
        start += 1;
    }
    (start, text[start..end].to_string())
}

/// If cursor is after `mod.` return (module_name, member_prefix, replace_start).
pub fn member_prefix_at(text: &str, cursor: usize) -> Option<(String, String, usize)> {
    let cursor = cursor.min(text.len());
    let mut end = cursor;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let bytes = text.as_bytes();
    // member prefix
    let mut mem_start = end;
    while mem_start > 0 {
        let b = bytes[mem_start - 1];
        if b.is_ascii_alphanumeric() || b == b'_' {
            mem_start -= 1;
        } else {
            break;
        }
    }
    if mem_start == 0 || bytes[mem_start - 1] != b'.' {
        return None;
    }
    let dot = mem_start - 1;
    let mod_end = dot;
    let mut mod_start = mod_end;
    while mod_start > 0 {
        let b = bytes[mod_start - 1];
        if b.is_ascii_alphanumeric() || b == b'_' {
            mod_start -= 1;
        } else {
            break;
        }
    }
    if mod_start >= mod_end {
        return None;
    }
    let module = text[mod_start..mod_end].to_string();
    let member = text[mem_start..end].to_string();
    Some((module, member, mem_start))
}

pub fn completions_for_prefix(prefix: &str) -> Vec<CompletionItem> {
    completions_at("", prefix.len().min(0), prefix)
}

/// Context-aware completions: pass full buffer + cursor for `mod.` members.
pub fn completions_at(text: &str, cursor: usize, fallback_prefix: &str) -> Vec<CompletionItem> {
    completions_at_with_profile(text, cursor, fallback_prefix, None, true)
}

pub fn completions_at_with_profile(
    text: &str,
    cursor: usize,
    fallback_prefix: &str,
    profile: Option<&TargetProfile>,
    allow_legacy_api: bool,
) -> Vec<CompletionItem> {
    let mut out = Vec::new();

    if let Some(profile) = profile {
        if let Some(call) = call_context_at(text, cursor) {
            let prefix = fallback_prefix.to_ascii_lowercase();
            let quoted = call.current.trim_start().starts_with(['\'', '"']);
            out.extend(
                profile
                    .resource_completions(
                        call.module.as_deref(),
                        &call.function,
                        call.argument_index,
                        &call.prior_arguments,
                    )
                    .into_iter()
                    .filter(|item| item.label.to_ascii_lowercase().starts_with(&prefix))
                    .map(|item| CompletionItem {
                        label: item.label.clone(),
                        insert: if quoted {
                            item.insert
                        } else {
                            format!("'{}'", item.insert)
                        },
                        detail: item.detail,
                    }),
            );
            if !out.is_empty() {
                out.truncate(24);
                return out;
            }
        }
    }

    if let Some((module, member_p, _)) = member_prefix_at(text, cursor) {
        if let Some(profile) = profile {
            out.extend(
                profile
                    .member_completions(&module, &member_p)
                    .into_iter()
                    .map(|item| CompletionItem {
                        label: item.label,
                        insert: item.insert,
                        detail: item.detail,
                    }),
            );
            out.truncate(24);
            return out;
        }
        if !allow_legacy_api {
            return out;
        }
        let mp = member_p.to_ascii_lowercase();
        for spec in MEMBERS {
            if spec.module == module {
                for m in spec.members {
                    if m.to_ascii_lowercase().starts_with(&mp) {
                        out.push(CompletionItem {
                            label: (*m).into(),
                            insert: (*m).into(),
                            detail: spec.detail.into(),
                        });
                    }
                }
            }
        }
        out.truncate(24);
        return out;
    }

    let p = fallback_prefix.to_ascii_lowercase();
    if p.is_empty() {
        return out;
    }

    for k in KEYWORDS {
        if k.starts_with(&p) {
            out.push(CompletionItem {
                label: (*k).into(),
                insert: (*k).into(),
                detail: "keyword".into(),
            });
        }
    }
    if let Some(profile) = profile {
        out.extend(
            profile
                .global_completions(&p)
                .into_iter()
                .map(|item| CompletionItem {
                    label: item.label,
                    insert: item.insert,
                    detail: item.detail,
                }),
        );
        out.truncate(24);
        return out;
    }
    if !allow_legacy_api {
        out.truncate(24);
        return out;
    }
    for b in BUILTINS {
        if b.starts_with(&p) {
            out.push(CompletionItem {
                label: (*b).into(),
                insert: (*b).into(),
                detail: "builtin".into(),
            });
        }
    }
    for m in MODULES {
        if m.starts_with(&p) {
            out.push(CompletionItem {
                label: format!("{m}."),
                insert: format!("{m}."),
                detail: "module".into(),
            });
        }
    }

    out.truncate(24);
    out
}

struct CallContext {
    module: Option<String>,
    function: String,
    argument_index: usize,
    prior_arguments: Vec<String>,
    current: String,
}

fn call_context_at(text: &str, cursor: usize) -> Option<CallContext> {
    let cursor = cursor.min(text.len());
    let bytes = text.as_bytes();
    let mut stack = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0usize;
    while index < cursor {
        let byte = bytes[index];
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active {
                quote = None;
            }
        } else if byte == b'-' && index + 1 < cursor && bytes[index + 1] == b'-' {
            index = text[index..cursor]
                .find('\n')
                .map(|offset| index + offset)
                .unwrap_or(cursor);
            continue;
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'(' => stack.push(index),
                b')' => {
                    stack.pop();
                }
                _ => {}
            }
        }
        index += 1;
    }
    let open = *stack.last()?;
    let mut callee_start = open;
    while callee_start > 0 && bytes[callee_start - 1].is_ascii_whitespace() {
        callee_start -= 1;
    }
    let callee_end = callee_start;
    while callee_start > 0 {
        let byte = bytes[callee_start - 1];
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' {
            callee_start -= 1;
        } else {
            break;
        }
    }
    let callee = text[callee_start..callee_end].trim();
    if callee.is_empty() {
        return None;
    }
    let (module, function) = callee
        .rsplit_once('.')
        .map(|(module, function)| (Some(module.to_string()), function.to_string()))
        .unwrap_or((None, callee.to_string()));
    let arguments = split_lua_arguments(&text[open + 1..cursor]);
    let argument_index = arguments.len().saturating_sub(1);
    let current = arguments.last().cloned().unwrap_or_default();
    let prior_arguments = arguments
        .into_iter()
        .take(argument_index)
        .map(|argument| argument.trim().to_string())
        .collect();
    Some(CallContext {
        module,
        function,
        argument_index,
        prior_arguments,
        current,
    })
}

fn split_lua_arguments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, byte) in text.bytes().enumerate() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                out.push(text[start..index].to_string());
                start = index + 1;
            }
            _ => {}
        }
    }
    out.push(text[start..].to_string());
    out
}

/// Static pre-read: issues / tips for MSPM0 Lua bytecode style.
#[derive(Clone, Debug)]
pub struct AnalyzeIssue {
    pub severity: &'static str, // error | warn | info
    pub line: usize,            // 1-based
    pub message: String,
}

fn contains_api_reference(line: &str, api: &str) -> bool {
    line.match_indices(api).any(|(index, _)| {
        index == 0
            || line[..index]
                .chars()
                .next_back()
                .map_or(true, |ch| !ch.is_ascii_alphanumeric() && ch != '_')
    })
}

pub fn analyze_source(source: &str) -> Vec<AnalyzeIssue> {
    analyze_source_with_profile(source, None)
}

pub fn analyze_source_with_profile(
    source: &str,
    profile: Option<&TargetProfile>,
) -> Vec<AnalyzeIssue> {
    let mut issues = Vec::new();
    let mut open_while = 0i32;
    let mut has_stopped = false;
    let mut has_yield_or_delay = false;
    let mut uses_string_concat_hot = false;
    let mut line_no = 0usize;
    let mut static_locals = HashMap::new();

    for line in source.lines() {
        line_no += 1;
        let t = line.trim();
        if t.starts_with("--") || t.is_empty() {
            continue;
        }
        if let Some((name, value)) = static_local_assignment(t) {
            static_locals.insert(name, value);
        }
        if t.contains("while ") && t.contains(" do") {
            open_while += 1;
        }
        if t == "end" || t.starts_with("end ") || t.starts_with("end;") {
            open_while = (open_while - 1).max(0);
        }
        if t.contains("stopped()") {
            has_stopped = true;
        }
        if t.contains("yield()") || t.contains("delay_ms(") {
            has_yield_or_delay = true;
        }
        if t.contains("..") && (t.contains("while") || open_while > 0) {
            uses_string_concat_hot = true;
        }
        // Legacy target policy is used only for projects without metadata.
        if profile.is_none() && t.contains("PA18")
            && (t.contains("i2c") || t.contains("oled") || t.contains("SDA") || t.contains("sda"))
        {
            issues.push(AnalyzeIssue {
                severity: "warn",
                line: line_no,
                message: "PA18 作 I2C 有 BSL 风险；OLED 优先 PA15/PA16".into(),
            });
        }
        if t.contains("require(") && (t.contains("font") || t.contains("FONT")) {
            issues.push(AnalyzeIssue {
                severity: "warn",
                line: line_no,
                message: "大字库 require 易 OOM；热路径用 oled.* / 外置 fs 表".into(),
            });
        }
        for (library, message) in [
            ("math.", "当前固件未编入 math 标准库；整数色值用 // 和 %，三角函数用 iq.*"),
            ("string.", "当前固件未编入 string 标准库；字节串用 i2c.bytes/spi.bytes，取字节用 byte"),
            ("table.", "当前固件未编入 table 标准库；不要使用 table.insert/table.concat"),
        ] {
            if t.contains(library) {
                issues.push(AnalyzeIssue {
                    severity: "error",
                    line: line_no,
                    message: message.into(),
                });
            }
        }
        for (removed, replacement) in [
            ("led.", "固件已移除 led.*；请使用 gpio.* 或 pwm.*"),
            ("imu.", "固件已移除 imu.*；请使用 uart.* 解析 ATK 数据"),
            ("adc.mv", "固件已移除 adc.mv；请用 adc.read 后换算 mV"),
            ("requirefile", "固件已移除 requirefile；请使用 require"),
            ("task.run", "固件无 task.run；请使用 event.run"),
        ] {
            if profile.is_none() && contains_api_reference(t, removed) {
                issues.push(AnalyzeIssue {
                    severity: "error",
                    line: line_no,
                    message: replacement.into(),
                });
            }
        }
        // Module names are lowercase in the firmware registry.
        for bad in [
            "GPIO.", "UART.", "ADC.", "I2C.", "SPI.", "PWM.", "TMR.", "EVENT.", "OLED.", "IQ.",
            "CAN.", "DAC.", "CRC.", "COMP.", "RTC.", "OPA.",
        ] {
            if profile.is_none() && t.contains(bad) {
                issues.push(AnalyzeIssue {
                    severity: "error",
                    line: line_no,
                    message: format!("Lua 模块名小写：用 {} 而非 {bad}", bad.to_ascii_lowercase()),
                });
            }
        }
        if let Some(profile) = profile {
            for call in calls_in_line(t) {
                // Only firmware symbols are catalogued. Lua base functions and
                // project-local functions must not be diagnosed as absent APIs.
                let is_catalogued = match call.module.as_deref() {
                    Some(module) => profile.has_module(module),
                    None => profile.has_global(&call.function),
                };
                if !is_catalogued {
                    continue;
                }
                let resolved_arguments = call
                    .arguments
                    .iter()
                    .map(|argument| {
                        static_locals
                            .get(argument)
                            .cloned()
                            .unwrap_or_else(|| argument.clone())
                    })
                    .collect::<Vec<_>>();
                for issue in profile.validate_call(
                    call.module.as_deref(),
                    &call.function,
                    &resolved_arguments,
                ) {
                    issues.push(AnalyzeIssue {
                        severity: "error",
                        line: line_no,
                        message: format!("{} · {}", issue.code, issue.message),
                    });
                }
            }
        }
    }

    if source.contains("while ") && source.contains(" do") && !has_stopped {
        issues.push(AnalyzeIssue {
            severity: "warn",
            line: 1,
            message: "长循环建议 while not stopped() do … 以便 ! 停止".into(),
        });
    }
    if source.contains("while ") && !has_yield_or_delay {
        issues.push(AnalyzeIssue {
            severity: "info",
            line: 1,
            message: "循环内建议 yield()/delay_ms 或依赖 VM stop hook".into(),
        });
    }
    if uses_string_concat_hot {
        issues.push(AnalyzeIssue {
            severity: "warn",
            line: 1,
            message: "热路径避免 .. 拼串；用 iq/整数 + oled.num".into(),
        });
    }
    if source.contains("i2c.write") && source.contains("bytes(") && source.contains("while") {
        issues.push(AnalyzeIssue {
            severity: "info",
            line: 1,
            message: "热路径优先 i2c.writev 或 oled.*（C 帧）".into(),
        });
    }
    if !source.contains("iq.")
        && (source.contains("* 180") || source.contains("/ 32768") || source.contains("math."))
    {
        issues.push(AnalyzeIssue {
            severity: "info",
            line: 1,
            message: "定点运算可用 iq.*（IQ16），避免 soft-float".into(),
        });
    }

    issues.truncate(32);
    issues
}

fn static_local_assignment(line: &str) -> Option<(String, String)> {
    let declaration = line.strip_prefix("local ")?;
    let (name, value) = declaration.split_once('=')?;
    let name = name.trim();
    if name.is_empty()
        || !name
            .bytes()
            .enumerate()
            .all(|(index, byte)| byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit()))
    {
        return None;
    }
    let value = value.trim().trim_end_matches(';').trim();
    let quoted = value.len() >= 2
        && matches!(value.as_bytes()[0], b'\'' | b'"')
        && value.as_bytes()[0] == *value.as_bytes().last()?;
    let numeric = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
        || value.parse::<i64>().is_ok();
    (quoted || numeric || matches!(value, "true" | "false" | "nil"))
        .then(|| (name.to_string(), value.to_string()))
}

struct ParsedCall {
    module: Option<String>,
    function: String,
    arguments: Vec<String>,
}

fn calls_in_line(line: &str) -> Vec<ParsedCall> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if !(bytes[index].is_ascii_alphabetic() || bytes[index] == b'_') {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric()
                || bytes[index] == b'_'
                || bytes[index] == b'.')
        {
            index += 1;
        }
        let callee = &line[start..index];
        let mut open = index;
        while open < bytes.len() && bytes[open].is_ascii_whitespace() {
            open += 1;
        }
        if open >= bytes.len() || bytes[open] != b'(' {
            continue;
        }
        let mut depth = 1usize;
        let mut cursor = open + 1;
        let mut quote = None;
        let mut escaped = false;
        while cursor < bytes.len() && depth > 0 {
            let byte = bytes[cursor];
            if let Some(active) = quote {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == active {
                    quote = None;
                }
            } else {
                match byte {
                    b'\'' | b'"' => quote = Some(byte),
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
            }
            cursor += 1;
        }
        if depth != 0 {
            continue;
        }
        let (module, function) = callee
            .rsplit_once('.')
            .map(|(module, function)| (Some(module.to_string()), function.to_string()))
            .unwrap_or((None, callee.to_string()));
        let body = &line[open + 1..cursor - 1];
        let arguments = if body.trim().is_empty() {
            Vec::new()
        } else {
            split_lua_arguments(body)
                .into_iter()
                .map(|argument| argument.trim().to_string())
                .collect()
        };
        out.push(ParsedCall {
            module,
            function,
            arguments,
        });
        index = cursor;
    }
    out
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::metadata::{load_board, load_profile_files};

    use super::{analyze_source, analyze_source_with_profile, completions_at, completions_at_with_profile};

    #[test]
    fn fallback_completion_surface_matches_modular_firmware() {
        let oled = completions_at("oled.", 5, "");
        assert!(oled.iter().any(|item| item.label == "number"));
        assert!(!oled.iter().any(|item| item.label == "font16"));

        let can = completions_at("can.", 4, "");
        assert!(can.iter().any(|item| item.label == "open_on"));
        assert!(completions_at("sys.", 4, "").is_empty());
        assert!(completions_at("led.", 4, "").is_empty());
        assert!(completions_at("imu.", 4, "").is_empty());
    }

    #[test]
    fn analyzer_rejects_removed_firmware_apis() {
        let issues = analyze_source("led.toggle()\nprint(adc.mv(0))\n");
        assert!(issues.iter().any(|issue| issue.message.contains("led.*")));
        assert!(issues.iter().any(|issue| issue.message.contains("adc.mv")));
    }

    #[test]
    fn analyzer_rejects_omitted_standard_libraries() {
        let issues = analyze_source("local n = math.floor(1.5)\nstring.format('%d', n)\n");
        assert!(issues.iter().any(|issue| issue.message.contains("math 标准库")));
        assert!(issues.iter().any(|issue| issue.message.contains("string 标准库")));
    }

    #[test]
    fn metadata_completion_and_diagnostics_share_the_pin_solver() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/metadata-standard/examples");
        let profile = load_profile_files(
            &root.join("mspm0g3507-lqfp48.chip.json"),
            &root.join("launchpad-mspm0g3507.board.json"),
            &root.join("mspm0-lua.api.json"),
        )
        .unwrap();
        let source = "i2c.open(1, 'P";
        let items = completions_at_with_profile(source, source.len(), "P", Some(&profile), false);
        assert_eq!(items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), ["PA15"]);

        let issues = analyze_source_with_profile(
            "i2c.open(1, 'PA16', 'PA15', 100000)",
            Some(&profile),
        );
        assert!(issues.iter().any(|issue| issue.message.contains("PIN003")));

        let constants = analyze_source_with_profile(
            "local BUS = 1\nlocal SCL = 'PA15'\nlocal SDA = 'PA16'\nlocal HZ = 100000\ni2c.open(BUS, SCL, SDA, HZ)",
            Some(&profile),
        );
        assert!(constants.is_empty(), "unexpected issues for static locals");
    }

    #[test]
    fn production_examples_have_no_profile_errors() {
        let profile = load_board("LKDMX").unwrap();
        let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("example");
        for directory in fs::read_dir(examples).unwrap() {
            let directory = directory.unwrap();
            if !directory.file_type().unwrap().is_dir() {
                continue;
            }
            for file in fs::read_dir(directory.path()).unwrap() {
                let file = file.unwrap();
                if file.path().extension().is_some_and(|extension| extension == "lua") {
                    let source = fs::read_to_string(file.path()).unwrap();
                    let errors: Vec<_> = analyze_source_with_profile(&source, Some(&profile))
                        .into_iter()
                        .filter(|issue| issue.severity == "error")
                        .collect();
                    assert!(errors.is_empty(), "{}: {errors:?}", file.path().display());
                }
            }
        }
    }
}
