use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const PROJECT_FILE: &str = "mspm0_lua.json";
pub const DEFAULT_MAIN: &str = "main.lua";
pub const DEFAULT_BOARD_ID: &str = "dimengxing.mspm0g3507.v1";
pub const DEFAULT_BOARD_VERSION: &str = "1.0.0";
pub const DEFAULT_API_ID: &str = "mspm0g3507.lua-modular";
pub const DEFAULT_API_VERSION: &str = "1.0.4";
pub const DEFAULT_SOURCE: &str = r#"-- MSPM0G3507 Lua bytecode firmware
print("HELLO_MSPM0")
gpio.mode("PA14", "out")
for i = 1, 6 do
  gpio.toggle("PA14")
  delay_ms(120)
end
print("DONE")
"#;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    #[serde(default = "default_main")]
    pub main_source: String,
    #[serde(default = "default_target")]
    pub target_luac: String,
    // Missing means the supported default board; an explicit JSON null opts out.
    #[serde(default, skip_serializing)]
    pub target: Option<ProjectTarget>,
    /// Optional path to `release/catalog_manifest.json`, relative to the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_manifest: Option<String>,
    /// Explicit native modules, required when a dynamic `require` cannot be inferred.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_modules: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectTarget {
    pub board: String,
    pub board_version: String,
    pub api: String,
    pub api_version: String,
}

fn default_main() -> String {
    DEFAULT_MAIN.into()
}

fn default_target() -> String {
    "main.luac".into()
}

pub fn default_project_target() -> Option<ProjectTarget> {
    None
}

impl Default for ProjectMeta {
    fn default() -> Self {
        Self {
            name: "untitled".into(),
            main_source: DEFAULT_MAIN.into(),
            target_luac: "main.luac".into(),
            target: default_project_target(),
            catalog_manifest: None,
            native_modules: Vec::new(),
        }
    }
}

pub fn project_path(dir: &Path) -> PathBuf {
    dir.join(PROJECT_FILE)
}

pub fn load_project(dir: &Path) -> Result<ProjectMeta> {
    let path = project_path(dir);
    if path.is_file() {
        let text =
            fs::read_to_string(&path).with_context(|| format!("读取工程 {}", path.display()))?;
        // PowerShell UTF-8 often writes BOM; strip it for serde_json.
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let meta: ProjectMeta =
            serde_json::from_str(text).with_context(|| format!("解析工程 {}", path.display()))?;
        return Ok(meta);
    }
    // Open folder without meta: treat as project if main.lua exists.
    if dir.join(DEFAULT_MAIN).is_file() {
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("project")
            .to_string();
        return Ok(ProjectMeta {
            name,
            ..Default::default()
        });
    }
    bail!("目录中无 {} 或 {}", PROJECT_FILE, DEFAULT_MAIN);
}

pub fn save_project_meta(dir: &Path, meta: &ProjectMeta) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("创建目录 {}", dir.display()))?;
    let path = project_path(dir);
    let text = serde_json::to_string_pretty(meta)?;
    fs::write(&path, text).with_context(|| format!("写入 {}", path.display()))?;
    Ok(())
}

pub fn create_project(dir: &Path, name: &str, initial_source: &str) -> Result<ProjectMeta> {
    if dir.exists() {
        let nonempty = fs::read_dir(dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(true);
        if nonempty && project_path(dir).exists() {
            bail!("工程已存在: {}", dir.display());
        }
    }
    let meta = ProjectMeta {
        name: name.to_string(),
        main_source: DEFAULT_MAIN.into(),
        target_luac: "main.luac".into(),
        target: default_project_target(),
        catalog_manifest: None,
        native_modules: Vec::new(),
    };
    save_project_meta(dir, &meta)?;
    let main = dir.join(&meta.main_source);
    if !main.exists() {
        fs::write(&main, initial_source).with_context(|| format!("写入 {}", main.display()))?;
    }
    Ok(meta)
}

pub fn read_source_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("读取 {}", path.display()))
}

pub fn write_source_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content).with_context(|| format!("写入 {}", path.display()))?;
    Ok(())
}

pub fn resolve_main(dir: &Path, meta: &ProjectMeta) -> PathBuf {
    dir.join(&meta.main_source)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    Root,
    Folder,
    Source,
    Config,
    Binary,
    Other,
}

#[derive(Clone, Debug)]
pub struct TreeEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: TreeKind,
    pub depth: usize,
    pub size: u64,
    /// Modified time as unix secs (0 if unknown).
    pub mtime: u64,
    pub is_main: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TreeSort {
    /// Folders first, then name A–Z.
    #[default]
    Name,
    /// By kind: folders, sources, config, binary, other; then name.
    Type,
    /// Newest first (folders first).
    Date,
    /// Largest first (folders first).
    Size,
}

/// Flat tree for sidebar (root + depth-1 files/dirs, dirs expanded one level).
pub fn list_project_tree(dir: &Path, meta: &ProjectMeta, sort: TreeSort) -> Vec<TreeEntry> {
    let mut out = Vec::new();
    let root_name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&meta.name)
        .to_string();
    out.push(TreeEntry {
        name: root_name,
        path: dir.to_path_buf(),
        kind: TreeKind::Root,
        depth: 0,
        size: 0,
        mtime: 0,
        is_main: false,
    });

    // name, path, is_dir, size, mtime
    let mut entries: Vec<(String, PathBuf, bool, u64, u64)> = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let meta = e.metadata().ok();
            let is_dir = path.is_dir();
            let size = if is_dir {
                0
            } else {
                meta.as_ref().map(|m| m.len()).unwrap_or(0)
            };
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            entries.push((name, path, is_dir, size, mtime));
        }
    }
    entries.sort_by(|a, b| sort_cmp(sort, a.2, &a.0, a.3, a.4, b.2, &b.0, b.3, b.4, true));

    let main_name = meta.main_source.as_str();
    for (name, path, is_dir, size, mtime) in entries {
        let kind = classify_entry(&name, is_dir);
        let is_main = !is_dir && name == main_name;
        out.push(TreeEntry {
            name: name.clone(),
            path: path.clone(),
            kind,
            depth: 1,
            size,
            mtime,
            is_main,
        });
        if is_dir {
            let mut children: Vec<(String, PathBuf, u64, u64)> = Vec::new();
            if let Ok(rd) = fs::read_dir(&path) {
                for e in rd.flatten() {
                    let cp = e.path();
                    if cp.is_dir() {
                        continue;
                    }
                    let cn = e.file_name().to_string_lossy().to_string();
                    if cn.starts_with('.') {
                        continue;
                    }
                    let meta = e.metadata().ok();
                    let sz = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                    let mt = meta
                        .as_ref()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    children.push((cn, cp, sz, mt));
                }
            }
            children.sort_by(|a, b| {
                sort_cmp(sort, false, &a.0, a.2, a.3, false, &b.0, b.2, b.3, false)
            });
            for (cn, cp, sz, mt) in children.into_iter().take(40) {
                let is_main = cn == main_name;
                out.push(TreeEntry {
                    name: cn.clone(),
                    path: cp,
                    kind: classify_entry(&cn, false),
                    depth: 2,
                    size: sz,
                    mtime: mt,
                    is_main,
                });
            }
        }
    }
    out
}

fn sort_cmp(
    sort: TreeSort,
    a_dir: bool,
    a_name: &str,
    a_size: u64,
    a_mtime: u64,
    b_dir: bool,
    b_name: &str,
    b_size: u64,
    b_mtime: u64,
    folders_first: bool,
) -> std::cmp::Ordering {
    if folders_first {
        match (a_dir, b_dir) {
            (true, false) => return std::cmp::Ordering::Less,
            (false, true) => return std::cmp::Ordering::Greater,
            _ => {}
        }
    }
    match sort {
        TreeSort::Name => a_name
            .to_ascii_lowercase()
            .cmp(&b_name.to_ascii_lowercase()),
        TreeSort::Type => {
            let ka = classify_entry(a_name, a_dir);
            let kb = classify_entry(b_name, b_dir);
            kind_rank(ka).cmp(&kind_rank(kb)).then_with(|| {
                a_name
                    .to_ascii_lowercase()
                    .cmp(&b_name.to_ascii_lowercase())
            })
        }
        TreeSort::Date => b_mtime.cmp(&a_mtime).then_with(|| {
            a_name
                .to_ascii_lowercase()
                .cmp(&b_name.to_ascii_lowercase())
        }),
        TreeSort::Size => b_size.cmp(&a_size).then_with(|| {
            a_name
                .to_ascii_lowercase()
                .cmp(&b_name.to_ascii_lowercase())
        }),
    }
}

fn kind_rank(k: TreeKind) -> u8 {
    match k {
        TreeKind::Root => 0,
        TreeKind::Folder => 1,
        TreeKind::Source => 2,
        TreeKind::Config => 3,
        TreeKind::Binary => 4,
        TreeKind::Other => 5,
    }
}

fn classify_entry(name: &str, is_dir: bool) -> TreeKind {
    if is_dir {
        return TreeKind::Folder;
    }
    let lower = name.to_ascii_lowercase();
    if lower == PROJECT_FILE || lower.ends_with(".json") {
        TreeKind::Config
    } else if lower.ends_with(".lua") {
        TreeKind::Source
    } else if lower.ends_with(".luac") {
        TreeKind::Binary
    } else {
        TreeKind::Other
    }
}

/// Find `runfile('xxx')` / `runfile("xxx")` references in source.
pub fn find_runfile_refs(source: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut rest = source;
    while let Some(idx) = rest.find("runfile") {
        let after = &rest[idx + "runfile".len()..];
        let after = after.trim_start();
        if !after.starts_with('(') {
            rest = &rest[idx + 1..];
            continue;
        }
        let after = after[1..].trim_start();
        let quote = after.chars().next();
        if quote != Some('\'') && quote != Some('"') {
            rest = &rest[idx + 1..];
            continue;
        }
        let q = quote.unwrap();
        let body = &after[1..];
        if let Some(end) = body.find(q) {
            let name = body[..end].to_string();
            if !name.is_empty() && !refs.contains(&name) {
                refs.push(name);
            }
            rest = &body[end..];
        } else {
            break;
        }
    }
    refs
}

#[cfg(test)]
mod target_tests {
    use super::*;

    #[test]
    fn board_selection_is_global_and_legacy_target_is_not_saved() {
        let missing: ProjectMeta = serde_json::from_str(r#"{"name":"current"}"#).unwrap();
        assert!(missing.target.is_none());

        let legacy: ProjectMeta = serde_json::from_str(&format!(
            r#"{{"name":"legacy","target":{{"board":"{}","board_version":"1.0.0","api":"{}","api_version":"1.0.1"}}}}"#,
            DEFAULT_BOARD_ID, DEFAULT_API_ID
        ))
        .unwrap();
        assert!(legacy.target.is_some());
        assert!(!serde_json::to_string(&legacy).unwrap().contains("\"target\""));
    }
}
