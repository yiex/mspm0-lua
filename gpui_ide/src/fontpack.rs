//! Static OLED call analysis and on-demand glyph rasterization.
//!
//! The generated Lua module is consumed by the bundled `oled.lua` runtime.
//! Nothing is added to the resident firmware when a project does not use OLED.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const SUPPORTED_SIZES: [u8; 2] = [8, 16];
const MAX_GLYPHS: usize = 192;
const MAX_BITMAP_BYTES: usize = 12 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OledFontPlan {
    pub uses_oled: bool,
    pub glyphs: BTreeMap<u8, BTreeSet<u32>>,
    pub texts: BTreeMap<u8, BTreeMap<String, Vec<u32>>>,
    pub fill_bytes: BTreeSet<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Ident(String),
    String(String),
    Integer(i64),
    Symbol(char),
}

/// Analyze reachable Lua sources. Comments and string contents cannot create
/// false API calls. Aliases such as `local display = require("oled")` work.
pub fn analyze_oled_fonts(source: &str) -> Result<OledFontPlan, String> {
    let tokens = lex(source)?;
    let mut aliases = BTreeSet::from(["oled".to_string()]);
    let mut plan = OledFontPlan::default();

    for i in 0..tokens.len() {
        if matches!(tokens.get(i), Some(Token::Ident(name)) if name == "require") {
            if call_string_argument(&tokens, i + 1).as_deref() == Some("oled") {
                plan.uses_oled = true;
                if i >= 2 && tokens.get(i - 1) == Some(&Token::Symbol('=')) {
                    if let Some(Token::Ident(alias)) = tokens.get(i - 2) {
                        aliases.insert(alias.clone());
                    }
                }
            }
        }
    }

    let mut i = 0usize;
    while i + 3 < tokens.len() {
        let Some(Token::Ident(alias)) = tokens.get(i) else {
            i += 1;
            continue;
        };
        if !aliases.contains(alias) || tokens.get(i + 1) != Some(&Token::Symbol('.')) {
            i += 1;
            continue;
        }
        let Some(Token::Ident(method)) = tokens.get(i + 2) else {
            i += 1;
            continue;
        };
        if tokens.get(i + 3) != Some(&Token::Symbol('(')) {
            i += 1;
            continue;
        }
        plan.uses_oled = true;
        if method == "clear" {
            plan.fill_bytes.insert(0);
            i += 3;
            continue;
        }
        if method == "fill" {
            let (args, end) = split_call_arguments(&tokens, i + 3)?;
            let value = args
                .first()
                .and_then(|arg| static_integer(arg))
                .ok_or_else(|| {
                    format!(
                        "{alias}.fill 的填充值必须是静态整数 0..255；当前精简 Core 无动态字节转换函数"
                    )
                })?;
            if !(0..=u8::MAX as i64).contains(&value) {
                return Err(format!("{alias}.fill 的填充值 {value} 超出 0..255"));
            }
            plan.fill_bytes.insert(value as u8);
            i = end;
            continue;
        }
        if method != "text" && method != "number" {
            i += 3;
            continue;
        }
        let (args, end) = split_call_arguments(&tokens, i + 3)?;
        let size_index = if method == "text" { 3 } else { 4 };
        let Some(size_arg) = args.get(size_index) else {
            return Err(format!(
                "{alias}.{method} 必须显式提供字号（仅支持 8 或 16）"
            ));
        };
        let size = static_integer(size_arg).ok_or_else(|| {
            format!("{alias}.{method} 的字号必须是静态整数 8 或 16，不能使用变量")
        })?;
        if !SUPPORTED_SIZES.contains(&(size as u8)) || !(0..=u8::MAX as i64).contains(&size) {
            return Err(format!("不支持 OLED 字号 {size}；当前仅支持 8 和 16"));
        }
        let size = size as u8;
        let glyphs = plan.glyphs.entry(size).or_default();
        for ch in '0'..='9' {
            glyphs.insert(ch as u32);
        }
        glyphs.insert('.' as u32);
        glyphs.insert('-' as u32);
        glyphs.insert(' ' as u32);
        if method == "text" {
            let text_arg = args
                .get(2)
                .ok_or_else(|| format!("{alias}.text 缺少文本参数"))?;
            let [Token::String(value)] = *text_arg else {
                return Err(format!(
                    "{alias}.text 的文本必须是静态字符串；变量数值请使用 oled.number"
                ));
            };
            let codes = value.chars().map(|ch| ch as u32).collect::<Vec<_>>();
            glyphs.extend(codes.iter().copied());
            plan.texts
                .entry(size)
                .or_default()
                .insert(value.clone(), codes);
        }
        i = end;
    }
    Ok(plan)
}

/// Rasterize the planned glyphs and emit a Lua source module. The source is
/// compiled by the same in-process compiler as project Lua files.
pub fn build_oled_font_module(
    source: &str,
    font_zh: &str,
    font_en: &str,
) -> Result<Option<(String, String)>, String> {
    let plan = analyze_oled_fonts(source)?;
    if !plan.uses_oled {
        return Ok(None);
    }
    let glyph_count: usize = plan.glyphs.values().map(BTreeSet::len).sum();
    if glyph_count > MAX_GLYPHS {
        return Err(format!(
            "OLED 字模共 {glyph_count} 个，超过单次运行上限 {MAX_GLYPHS}；请拆分工程文本"
        ));
    }

    // Store glyphs by page instead of allocating one Lua table per glyph.
    // i2c.bytes() supplies dynamic command/fill bytes without a 32-entry map.
    let mut source_out = String::from("return {\n");
    let mut bitmap_bytes = 0usize;
    let font_zh = (glyph_count > 0)
        .then(|| resolve_font_file(font_zh, "中文"))
        .transpose()?;
    let font_en = (glyph_count > 0)
        .then(|| resolve_font_file(font_en, "英文"))
        .transpose()?;
    for (size, codes) in &plan.glyphs {
        let mut pages = vec![String::new(); (*size / 8) as usize];
        for code in codes {
            let ch =
                char::from_u32(*code).ok_or_else(|| format!("无效 Unicode 码点 U+{code:04X}"))?;
            let font = if ch.is_ascii() {
                font_en
                    .as_ref()
                    .expect("font resolved for non-empty glyphs")
            } else {
                font_zh
                    .as_ref()
                    .expect("font resolved for non-empty glyphs")
            };
            let glyph = raster_glyph(ch, *size, &font.to_string_lossy())?;
            bitmap_bytes += glyph.len();
            if bitmap_bytes > MAX_BITMAP_BYTES {
                return Err(format!(
                    "OLED 字模数据 {bitmap_bytes} B 超过安全上限 {MAX_BITMAP_BYTES} B"
                ));
            }
            for (page_index, page) in glyph.chunks(*size as usize).enumerate() {
                pages[page_index].push_str(&format!(
                    "      [{code}] = {},\n",
                    lua_binary_literal(page)
                ));
            }
        }
        source_out.push_str(&format!(
            "  [{size}] = {{ width = {size}, pages = {}, glyphs = {{\n",
            size / 8
        ));
        for page in pages {
            source_out.push_str("    {\n");
            source_out.push_str(&page);
            source_out.push_str("    },\n");
        }
        source_out.push_str("  }, texts = {\n");
        if let Some(texts) = plan.texts.get(size) {
            for (text, text_codes) in texts {
                source_out.push_str(&format!("    [{}] = {{", lua_string_literal(text)));
                for code in text_codes {
                    source_out.push_str(&format!("{code},"));
                }
                source_out.push_str("},\n");
            }
        }
        source_out.push_str("  } },\n");
    }
    source_out.push_str("}\n");
    let sizes = plan
        .glyphs
        .iter()
        .map(|(size, codes)| format!("{size}px:{}字", codes.len()))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(Some((
        format!("OLED 自动取模 {sizes}，位图 {bitmap_bytes} B -> _oled_font.luac"),
        source_out,
    )))
}

fn lua_string_literal(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\x{:02x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn lua_binary_literal(value: &[u8]) -> String {
    let mut out = String::with_capacity(2 + value.len() * 4);
    out.push('"');
    for byte in value {
        out.push_str(&format!("\\x{byte:02x}"));
    }
    out.push('"');
    out
}

fn resolve_font_file(path: &str, label: &str) -> Result<std::path::PathBuf, String> {
    let requested = Path::new(path);
    let path = if requested.is_file() {
        requested.to_path_buf()
    } else if let Some(name) = requested.file_name() {
        let system = Path::new(r"C:\Windows\Fonts").join(name);
        if system.is_file() {
            system
        } else {
            requested.to_path_buf()
        }
    } else {
        requested.to_path_buf()
    };
    if !path.is_file() {
        return Err(format!("{label}字体不存在: {}", path.display()));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "ttf" | "ttc" | "otf") {
        return Err(format!("{label}字体格式不支持: {}", path.display()));
    }
    Ok(path)
}

// The source-upload path for obsolete monolithic firmware still references
// these symbols. It must not silently upload packs that modular OLED cannot use.
pub fn analyze_and_pack_f16(source: &str) -> Result<(String, Vec<u8>), String> {
    if analyze_oled_fonts(source)?.uses_oled {
        return Err("OLED 自动取模仅支持模块化运行；请使用工程运行命令".into());
    }
    Ok(("未使用 OLED".into(), Vec::new()))
}

pub fn analyze_and_pack(_source: &str) -> (BTreeSet<u8>, Vec<u8>) {
    (BTreeSet::new(), Vec::new())
}

fn call_string_argument(tokens: &[Token], mut index: usize) -> Option<String> {
    if tokens.get(index) == Some(&Token::Symbol('(')) {
        index += 1;
    }
    match tokens.get(index) {
        Some(Token::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn static_integer(tokens: &[Token]) -> Option<i64> {
    match tokens {
        [Token::Integer(value)] => Some(*value),
        [Token::Symbol('-'), Token::Integer(value)] => Some(-*value),
        _ => None,
    }
}

fn split_call_arguments<'a>(
    tokens: &'a [Token],
    open: usize,
) -> Result<(Vec<&'a [Token]>, usize), String> {
    let mut args = Vec::new();
    let mut depth = 0i32;
    let mut start = open + 1;
    let mut index = open + 1;
    while index < tokens.len() {
        match tokens[index] {
            Token::Symbol('(') | Token::Symbol('{') | Token::Symbol('[') => depth += 1,
            Token::Symbol(')') if depth == 0 => {
                args.push(&tokens[start..index]);
                return Ok((args, index + 1));
            }
            Token::Symbol(')') | Token::Symbol('}') | Token::Symbol(']') => depth -= 1,
            Token::Symbol(',') if depth == 0 => {
                args.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    Err("OLED 调用缺少右括号".into())
}

fn lex(source: &str) -> Result<Vec<Token>, String> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else if bytes[i] == b'-' && bytes.get(i + 1) == Some(&b'-') {
            i += 2;
            if bytes.get(i) == Some(&b'[') && bytes.get(i + 1) == Some(&b'[') {
                i += 2;
                while i + 1 < bytes.len() && &bytes[i..i + 2] != b"]]" {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            } else {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
        } else if matches!(bytes[i], b'\'' | b'"') {
            let quote = bytes[i];
            i += 1;
            let mut value = String::new();
            let mut closed = false;
            while i < bytes.len() {
                if bytes[i] == quote {
                    i += 1;
                    closed = true;
                    break;
                }
                if bytes[i] == b'\\' {
                    i += 1;
                    let escaped = *bytes.get(i).ok_or("Lua 字符串转义不完整")?;
                    value.push(match escaped {
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'\\' => '\\',
                        b'\'' => '\'',
                        b'"' => '"',
                        other => other as char,
                    });
                    i += 1;
                } else {
                    let ch = source[i..]
                        .chars()
                        .next()
                        .ok_or("Lua 字符串不是有效 UTF-8")?;
                    value.push(ch);
                    i += ch.len_utf8();
                }
            }
            if !closed {
                return Err("Lua 字符串缺少结束引号".into());
            }
            out.push(Token::String(value));
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push(Token::Ident(source[start..i].to_string()));
        } else if bytes[i] == b'0' && matches!(bytes.get(i + 1), Some(b'x' | b'X')) {
            let start = i + 2;
            i = start;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            if i == start {
                return Err("OLED 十六进制整数缺少数字".into());
            }
            let value = i64::from_str_radix(&source[start..i], 16)
                .map_err(|_| "OLED 十六进制整数无效")?;
            out.push(Token::Integer(value));
        } else if bytes[i].is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let value = source[start..i]
                .parse::<i64>()
                .map_err(|_| "OLED 字号整数无效")?;
            out.push(Token::Integer(value));
        } else {
            out.push(Token::Symbol(bytes[i] as char));
            i += 1;
        }
    }
    Ok(out)
}

#[cfg(windows)]
fn raster_glyph(ch: char, size: u8, font_path: &str) -> Result<Vec<u8>, String> {
    use std::mem::zeroed;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateCompatibleDC(hdc: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        fn CreateCompatibleBitmap(
            hdc: *mut core::ffi::c_void,
            cx: i32,
            cy: i32,
        ) -> *mut core::ffi::c_void;
        fn SelectObject(
            hdc: *mut core::ffi::c_void,
            h: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void;
        fn DeleteDC(hdc: *mut core::ffi::c_void) -> i32;
        fn DeleteObject(ho: *mut core::ffi::c_void) -> i32;
        fn CreateFontW(
            c_height: i32,
            c_width: i32,
            escapement: i32,
            orientation: i32,
            weight: i32,
            italic: u32,
            underline: u32,
            strikeout: u32,
            charset: u32,
            out_precision: u32,
            clip_precision: u32,
            quality: u32,
            pitch_family: u32,
            face: *const u16,
        ) -> *mut core::ffi::c_void;
        fn SetBkMode(hdc: *mut core::ffi::c_void, mode: i32) -> i32;
        fn SetTextColor(hdc: *mut core::ffi::c_void, color: u32) -> u32;
        fn PatBlt(hdc: *mut core::ffi::c_void, x: i32, y: i32, w: i32, h: i32, rop: u32) -> i32;
        fn TextOutW(
            hdc: *mut core::ffi::c_void,
            x: i32,
            y: i32,
            text: *const u16,
            count: i32,
        ) -> i32;
        fn GetDIBits(
            hdc: *mut core::ffi::c_void,
            bitmap: *mut core::ffi::c_void,
            start: u32,
            lines: u32,
            bits: *mut u8,
            info: *mut BitmapInfo,
            usage: u32,
        ) -> i32;
        fn AddFontResourceExW(
            name: *const u16,
            flags: u32,
            reserved: *mut core::ffi::c_void,
        ) -> i32;
    }
    #[repr(C)]
    struct BitmapInfoHeader {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        image_size: u32,
        xppm: i32,
        yppm: i32,
        colors_used: u32,
        colors_important: u32,
    }
    #[repr(C)]
    struct RgbQuad {
        blue: u8,
        green: u8,
        red: u8,
        reserved: u8,
    }
    #[repr(C)]
    struct BitmapInfo {
        header: BitmapInfoHeader,
        colors: [RgbQuad; 2],
    }

    let path = Path::new(font_path);
    let mut db = fontdb::Database::new();
    db.load_font_file(path)
        .map_err(|error| format!("读取字体 {} 失败: {error}", path.display()))?;
    let family = db
        .faces()
        .next()
        .and_then(|face| face.families.first())
        .map(|(name, _)| name.clone())
        .ok_or_else(|| format!("字体不包含可用字族: {}", path.display()))?;
    unsafe {
        let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if AddFontResourceExW(wide_path.as_ptr(), 0x10, null_mut()) == 0 {
            return Err(format!("无法加载字体: {}", path.display()));
        }
        let wide_family: Vec<u16> = family.encode_utf16().chain(Some(0)).collect();
        let hdc = CreateCompatibleDC(null_mut());
        let bitmap = CreateCompatibleBitmap(hdc, size as i32, size as i32);
        if hdc.is_null() || bitmap.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !hdc.is_null() {
                DeleteDC(hdc);
            }
            return Err("Windows GDI 无法创建 OLED 字模画布".into());
        }
        let old_bitmap = SelectObject(hdc, bitmap);
        let font = CreateFontW(
            -(size as i32),
            0,
            0,
            0,
            700,
            0,
            0,
            0,
            1,
            0,
            0,
            3,
            0,
            wide_family.as_ptr(),
        );
        if font.is_null() {
            SelectObject(hdc, old_bitmap);
            DeleteObject(bitmap);
            DeleteDC(hdc);
            return Err(format!("无法创建字体实例: {}", path.display()));
        }
        let old_font = SelectObject(hdc, font);
        SetBkMode(hdc, 1);
        SetTextColor(hdc, 0x00ff_ffff);
        PatBlt(hdc, 0, 0, size as i32, size as i32, 0x0000_0042);
        let utf16: Vec<u16> = ch.encode_utf16(&mut [0u16; 2]).to_vec();
        if TextOutW(hdc, 0, 0, utf16.as_ptr(), utf16.len() as i32) == 0 {
            SelectObject(hdc, old_font);
            SelectObject(hdc, old_bitmap);
            DeleteObject(font);
            DeleteObject(bitmap);
            DeleteDC(hdc);
            return Err(format!("字体无法栅格化 U+{:04X}", ch as u32));
        }
        let mut info: BitmapInfo = zeroed();
        info.header.size = std::mem::size_of::<BitmapInfoHeader>() as u32;
        info.header.width = size as i32;
        info.header.height = -(size as i32);
        info.header.planes = 1;
        info.header.bit_count = 32;
        let mut pixels = vec![0u8; size as usize * size as usize * 4];
        let ok = GetDIBits(
            hdc,
            bitmap,
            0,
            size as u32,
            pixels.as_mut_ptr(),
            &mut info,
            0,
        );
        SelectObject(hdc, old_font);
        SelectObject(hdc, old_bitmap);
        DeleteObject(font);
        DeleteObject(bitmap);
        DeleteDC(hdc);
        if ok == 0 {
            return Err(format!("读取字模 U+{:04X} 失败", ch as u32));
        }

        let width = size as usize;
        let pages = width / 8;
        let mut out = vec![0u8; width * pages];
        for page in 0..pages {
            for x in 0..width {
                let mut column = 0u8;
                for bit in 0..8 {
                    let y = page * 8 + bit;
                    let offset = (y * width + x) * 4;
                    let luminance = (u16::from(pixels[offset])
                        + u16::from(pixels[offset + 1])
                        + u16::from(pixels[offset + 2]))
                        / 3;
                    if luminance > 96 {
                        column |= 1 << bit;
                    }
                }
                out[page * width + x] = column;
            }
        }
        if ch != ' ' && out.iter().all(|byte| *byte == 0) {
            return Err(format!("所选字体缺少字符 U+{:04X} ({ch})", ch as u32));
        }
        Ok(out)
    }
}

#[cfg(not(windows))]
fn raster_glyph(_ch: char, _size: u8, _font_path: &str) -> Result<Vec<u8>, String> {
    Err("OLED 自动取模当前需要 Windows GDI".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_literals_per_size_and_always_adds_numeric_glyphs() {
        let source = r#"
local display = require("oled")
display.text(0, 0, "温度", 16)
display.number(0, 16, value, 1, 8)
"#;
        let plan = analyze_oled_fonts(source).unwrap();
        assert!(plan.uses_oled);
        assert!(plan.glyphs[&16].contains(&('温' as u32)));
        assert!(plan.glyphs[&16].contains(&('9' as u32)));
        assert!(plan.glyphs[&8].contains(&('.' as u32)));
        assert!(!plan.glyphs[&8].contains(&('温' as u32)));
        assert_eq!(plan.texts[&16]["温度"], ['温' as u32, '度' as u32]);
    }

    #[test]
    fn ignores_comments_and_plain_string_mentions() {
        let plan =
            analyze_oled_fonts("-- oled.text(0,0,'假',16)\nlocal s = \"oled.text(0,0,'x',8)\"")
                .unwrap();
        assert!(!plan.uses_oled);
        assert!(plan.glyphs.is_empty());
    }

    #[test]
    fn rejects_dynamic_or_unsupported_sizes() {
        assert!(analyze_oled_fonts("oled.text(0, 0, 'x', size)").is_err());
        assert!(analyze_oled_fonts("oled.text(0, 0, 'x', 12)").is_err());
        assert!(analyze_oled_fonts("oled.text(0, 0, value, 8)").is_err());
    }

    #[test]
    fn packs_only_static_fill_values() {
        let plan = analyze_oled_fonts(
            "local d=require('oled'); d.clear(); d.fill(0xaa); d.fill(17)",
        )
        .unwrap();
        assert_eq!(plan.fill_bytes, BTreeSet::from([0, 17, 0xaa]));
        assert!(analyze_oled_fonts("oled.fill(value)").is_err());
        assert!(analyze_oled_fonts("oled.fill(256)").is_err());
    }

    #[test]
    fn rasterizes_with_the_selected_local_font() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let font = root.join("font/Tuffy.ttf");
        let (_message, source) = build_oled_font_module(
            "local oled=require('oled'); oled.text(0,0,'ABC',8)",
            &font.to_string_lossy(),
            &font.to_string_lossy(),
        )
        .unwrap()
        .unwrap();
        assert!(source.contains("[65] ="));
        assert!(source.contains("[8] ="));
        assert!(source.contains("\\x"));
        assert!(!source.contains("[255] ="));
        assert!(!source.contains("{0,0,0,"));
        let bytecode = crate::compile::compile_source(Path::new("in-process"), &source).unwrap();
        assert!(bytecode.len() < 20 * 1024);
    }

    #[test]
    fn missing_selected_font_is_a_hard_error() {
        let error = build_oled_font_module(
            "oled.text(0,0,'A',8)",
            "Z:/missing/chinese.ttf",
            "Z:/missing/english.ttf",
        )
        .unwrap_err();
        assert!(error.contains("字体不存在"));
    }
}
