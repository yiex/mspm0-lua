use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

#[link(name = "mspm0_luac", kind = "static")]
unsafe extern "C" {
    fn mspm0_luac_compile(
        source: *const u8,
        source_len: usize,
        out: *mut *mut u8,
        out_len: *mut usize,
        errbuf: *mut u8,
        errbuf_len: usize,
    ) -> i32;
    fn mspm0_luac_free(p: *mut u8);
}

/// Always available: compiler is linked into this process.
pub fn find_compiler() -> Option<PathBuf> {
    Some(PathBuf::from("in-process"))
}

pub fn compile_source(_compiler: &Path, source: &str) -> Result<Vec<u8>> {
    let source = add_timer_event_runtime(&normalize_source(source));
    if source.trim().is_empty() {
        bail!("源码为空");
    }
    let mut out_ptr: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 0;
    let mut err = vec![0u8; 2048];
    let rc = unsafe {
        mspm0_luac_compile(
            source.as_ptr(),
            source.len(),
            &mut out_ptr,
            &mut out_len,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    if rc != 0 || out_ptr.is_null() || out_len == 0 {
        let msg = std::ffi::CStr::from_bytes_until_nul(&err)
            .ok()
            .map(|c| c.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("compile failed (code {rc})"));
        bail!("{}", friendly_compile_error(&msg));
    }
    let bytes = unsafe { std::slice::from_raw_parts(out_ptr, out_len).to_vec() };
    unsafe { mspm0_luac_free(out_ptr) };
    if !is_lua_bytecode(&bytes) {
        bail!("编译结果不是有效 Lua 字节码");
    }
    Ok(bytes)
}

fn add_timer_event_runtime(source: &str) -> String {
    if !(source.contains("tmr.every")
        || source.contains("event.run")
        || source.contains("event.poll")
        || source.contains("event.stop"))
    {
        return source.to_string();
    }
    format!("{TIMER_EVENT_PRELUDE}{source}")
}

const TIMER_EVENT_PRELUDE: &str = r#"-- IDE-injected SysTick callback dispatcher.
do
  local start, take, native_stop = tmr.start, tmr.take, tmr.stop
  local callbacks, active = {}, {}
  local event_stop = false
  function tmr.every(ms, fn)
    local id
    for i = 0, 3 do if not active[i] then id = i break end end
    if id == nil then error("tmr:full") end
    if fn ~= nil and type(fn) ~= "function" then error("tmr:callback") end
    start(id, ms)
    active[id], callbacks[id] = true, fn
    return id
  end
  function tmr.stop(id)
    callbacks[id], active[id] = nil, nil
    return native_stop(id)
  end
  event = event or {}
  function event.stop() event_stop = true end
  function event.poll()
    local dispatched = 0
    for i = 0, 3 do
      local fn = callbacks[i]
      if fn then
        local hits = take(i)
        if hits > 0 then fn(i, hits); dispatched = dispatched + 1 end
      end
    end
    return dispatched
  end
  function event.run()
    event_stop = false
    while not event_stop do
      local dispatched = event.poll()
      if next(callbacks) == nil then break end
      if dispatched == 0 then yield() end
    end
  end
end
"#;

pub fn is_lua_bytecode(data: &[u8]) -> bool {
    data.len() >= 4 && data[0] == 0x1b && data[1] == b'L' && data[2] == b'u' && data[3] == b'a'
}

pub fn normalize_source(s: &str) -> String {
    sanitize_source_text(s)
        .replace(['“', '”'], "\"")
        .replace(['‘', '’'], "'")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

pub fn sanitize_source_text(s: &str) -> String {
    s.chars()
        .filter_map(|ch| match ch {
            '\u{feff}'
            | '\u{200b}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'
            | '\u{2066}'..='\u{2069}' => None,
            '\u{00a0}' | '\u{3000}' => Some(' '),
            _ => Some(ch),
        })
        .collect()
}

fn friendly_compile_error(raw: &str) -> String {
    let raw = raw.trim();
    let (line, detail) = raw
        .strip_prefix("editor:")
        .and_then(|rest| rest.split_once(':'))
        .and_then(|(line, detail)| line.parse::<usize>().ok().map(|line| (line, detail.trim())))
        .map(|(line, detail)| (Some(line), detail))
        .unwrap_or((None, raw));

    let detail = if detail.contains("unexpected symbol near '<\\239>'") {
        "检测到 UTF-8 BOM 或不可见字符；请删除该字符后重试".to_string()
    } else if detail.contains("'end' expected near '<eof>'") {
        "代码块未闭合：文件结束前缺少 end".to_string()
    } else if detail.contains("unexpected symbol near '<eof>'") {
        "代码在文件末尾意外中断，请检查括号、引号或 end".to_string()
    } else if detail.contains("unfinished string") {
        format!("字符串未闭合：{detail}")
    } else if detail.contains("unexpected symbol near") {
        format!("存在意外符号：{detail}")
    } else {
        detail.to_string()
    };

    match line {
        Some(line) => format!("第 {line} 行：{detail}"),
        None => detail,
    }
}

pub fn valid_luac_name(name: &str) -> bool {
    let name = name.trim();
    (1..=28).contains(&name.len())
        && name.ends_with(".luac")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub fn ensure_luac_name(raw: &str) -> Result<String> {
    let mut name = raw.trim().to_string();
    if name.is_empty() {
        name = "main.luac".into();
    }
    if !name.ends_with(".luac") {
        name.push_str(".luac");
    }
    if !valid_luac_name(&name) {
        bail!("文件名必须是 1..28 字节的字母/数字/_/./-，并以 .luac 结尾");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{compile_source, friendly_compile_error, normalize_source, sanitize_source_text};

    #[test]
    fn removes_bom_and_rich_text_formatting() {
        let source = "\u{feff}print(\u{2018}HI\u{2019})\u{200b}\r\n";
        assert_eq!(normalize_source(source), "print('HI')\n");
        assert_eq!(sanitize_source_text("a\u{00a0}b\u{3000}c"), "a b c");
    }

    #[test]
    fn formats_lua_errors_with_a_readable_line_number() {
        assert_eq!(
            friendly_compile_error("editor:7: 'end' expected near '<eof>'"),
            "第 7 行：代码块未闭合：文件结束前缺少 end"
        );
    }

    #[test]
    fn compiles_bom_prefixed_oled_script() {
        let source = "\u{feff}print('HI_OLED_START')\n\
local ok, err = pcall(function()\n\
  oled.open()\n\
  oled.cursor(0, 0); oled.print('MSPM0 LUA')\n\
  oled.cursor(0, 2); oled.print('R:'); oled.num(18, 2, -550, 1)\n\
end)\n\
if ok then print('HI_OLED_OK') else print('OLED_SKIP', err) end\n";

        let bytecode = compile_source(Path::new("in-process"), source).unwrap();
        assert!(bytecode.starts_with(b"\x1bLua"));
    }

    #[test]
    fn compiles_timer_callback_source_with_dispatcher() {
        let source = "local n = 0\n\
tmr.every(10, function(id, hits)\n\
  n = n + hits\n\
  if n >= 2 then tmr.stop(id); event.stop() end\n\
end)\n\
event.run()\n";
        let bytecode = compile_source(Path::new("in-process"), source).unwrap();
        assert!(bytecode.starts_with(b"\x1bLua"));
        assert!(bytecode.len() > source.len());
    }

    #[test]
    fn compiles_event_stop_source_with_dispatcher() {
        let source = "event.stop()\n";
        let bytecode = compile_source(Path::new("in-process"), source).unwrap();
        assert!(bytecode.starts_with(b"\x1bLua"));
        assert!(bytecode.len() > source.len());
    }

    #[test]
    fn packaged_examples_are_bom_free_and_compile() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("example");
        for project in std::fs::read_dir(root).unwrap().flatten() {
            let Ok(files) = std::fs::read_dir(project.path()) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("lua") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(!source.starts_with('\u{feff}'), "BOM in {}", path.display());
                compile_source(Path::new("in-process"), &source)
                    .unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
                let stale: Vec<_> = crate::syntax::analyze_source(&source)
                    .into_iter()
                    .filter(|issue| issue.severity == "error")
                    .collect();
                assert!(
                    stale.is_empty(),
                    "stale API in {}: {}",
                    path.display(),
                    stale
                        .iter()
                        .map(|issue| format!("L{} {}", issue.line, issue.message))
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            }
        }
    }

    #[test]
    fn bundled_oled_runtime_compiles() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("mspm0_lua/release/lua/oled.lua");
        let source = std::fs::read_to_string(&path).unwrap();
        compile_source(Path::new("in-process"), &source)
            .unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
    }
}
