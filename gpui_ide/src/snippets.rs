//! On-disk examples: `<exe>/example/<name>/` (each is a project folder).

use std::fs;
use std::path::PathBuf;

use crate::settings::AppSettings;

#[derive(Clone, Debug)]
pub struct ExampleProject {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
}

/// List example projects next to the executable.
pub fn list_examples() -> Vec<ExampleProject> {
    let root = AppSettings::exe_dir().join("example");
    if !root.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&root) {
        for e in rd.flatten() {
            let path = e.path();
            if !path.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            // Prefer folders with project meta or any .lua.
            let has_meta = path.join("mspm0_lua.json").is_file();
            let has_lua = fs::read_dir(&path)
                .map(|rd| {
                    rd.flatten().any(|f| {
                        f.path()
                            .extension()
                            .and_then(|x| x.to_str())
                            .map(|x| x.eq_ignore_ascii_case("lua"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            if !has_meta && !has_lua {
                continue;
            }
            out.push(ExampleProject {
                id: name.clone(),
                label: name,
                path,
            });
        }
    }
    out.sort_by(|a, b| {
        a.label
            .to_ascii_lowercase()
            .cmp(&b.label.to_ascii_lowercase())
    });
    out
}

pub fn example_root() -> PathBuf {
    AppSettings::exe_dir().join("example")
}
