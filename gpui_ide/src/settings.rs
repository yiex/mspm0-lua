//! User preferences: stored as `config.json` next to the executable.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::theme::{CustomPalette, ThemeId};

const CONFIG_NAME: &str = "config.json";

/// Default editor font size in pixels.
pub const DEFAULT_EDITOR_FONT: f32 = 14.0;
pub const MIN_EDITOR_FONT: f32 = 10.0;
pub const MAX_EDITOR_FONT: f32 = 28.0;

pub const DEFAULT_SIDEBAR_W: f32 = 240.0;
pub const MIN_SIDEBAR_W: f32 = 120.0;
pub const MAX_SIDEBAR_W: f32 = 480.0;
pub const HIDE_SIDEBAR_W: f32 = 80.0;

pub const DEFAULT_CONSOLE_H: f32 = 200.0;
pub const MIN_CONSOLE_H: f32 = 80.0;
pub const MAX_CONSOLE_H: f32 = 600.0;
pub const HIDE_CONSOLE_H: f32 = 48.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferMode {
    #[default]
    Low,
    High,
}

/// Per-theme color overrides — fully isolated (no shared accent).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemePalettes {
    pub dark: CustomPalette,
    pub light: CustomPalette,
    pub custom: CustomPalette,
}

impl ThemePalettes {
    pub fn get(&self, id: ThemeId) -> &CustomPalette {
        match id {
            ThemeId::Dark => &self.dark,
            ThemeId::Light => &self.light,
            ThemeId::Custom => &self.custom,
        }
    }

    pub fn get_mut(&mut self, id: ThemeId) -> &mut CustomPalette {
        match id {
            ThemeId::Dark => &mut self.dark,
            ThemeId::Light => &mut self.light,
            ThemeId::Custom => &mut self.custom,
        }
    }

    pub fn clear(&mut self, id: ThemeId) {
        *self.get_mut(id) = CustomPalette::default();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: ThemeId,
    /// Isolated palettes for dark / light / custom.
    pub theme_palettes: ThemePalettes,
    /// Legacy fields (migrated into theme_palettes.custom once).
    #[serde(default, skip_serializing)]
    pub accent_rgb: Option<u32>,
    #[serde(default, skip_serializing)]
    pub custom_palette: CustomPalette,
    pub download_dir: Option<PathBuf>,
    pub last_project: Option<PathBuf>,
    pub last_firmware_dir: Option<PathBuf>,
    /// Last selected serial port name (e.g. COM3).
    pub last_port: Option<String>,
    /// Selected file stem in `boards/`.
    pub selected_board: Option<String>,
    /// Low keeps the complete transaction at 115200; high temporarily uses 460800.
    pub transfer_mode: TransferMode,
    /// Font file name in `font/` used for Chinese OLED glyphs.
    pub font_zh: String,
    /// Font file name in `font/` used for Latin OLED glyphs.
    pub font_en: String,
    /// Sidebar sort: "name" | "type"
    pub tree_sort: String,
    /// Whether project sidebar is visible.
    pub show_sidebar: bool,
    /// Whether output console is visible.
    pub show_console: bool,
    /// Show protocol/debug traffic in addition to Lua output and run results.
    pub full_output: bool,
    /// Project sidebar width in px.
    pub sidebar_width: f32,
    /// Output console height in px.
    pub console_height: f32,
    /// Editor font size in px (Ctrl+wheel).
    pub editor_font_size: f32,
    /// Serial data workbench location: hidden | right | editor | window.
    pub telemetry_dock: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeId::Dark,
            theme_palettes: ThemePalettes::default(),
            accent_rgb: None,
            custom_palette: CustomPalette::default(),
            download_dir: None,
            last_project: None,
            last_firmware_dir: None,
            last_port: None,
            selected_board: None,
            transfer_mode: TransferMode::Low,
            font_zh: "SimHei.ttf".into(),
            font_en: "Tuffy.ttf".into(),
            tree_sort: "name".into(),
            show_sidebar: true,
            show_console: true,
            full_output: false,
            sidebar_width: DEFAULT_SIDEBAR_W,
            console_height: DEFAULT_CONSOLE_H,
            editor_font_size: DEFAULT_EDITOR_FONT,
            telemetry_dock: "right".into(),
        }
    }
}

impl AppSettings {
    /// Directory containing the running executable.
    pub fn exe_dir() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// `config.json` beside the .exe (portable).
    pub fn path() -> PathBuf {
        Self::exe_dir().join(CONFIG_NAME)
    }

    /// Legacy path from older builds (`%APPDATA%/LuaIDE/settings.json`).
    fn legacy_path() -> PathBuf {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("LuaIDE").join("settings.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if let Ok(text) = fs::read_to_string(&path) {
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            if let Ok(mut s) = serde_json::from_str::<Self>(text) {
                s.migrate_legacy_palette();
                s.clamp_values();
                return s;
            }
        }
        // One-shot migrate from old APPDATA settings.
        if let Ok(text) = fs::read_to_string(Self::legacy_path()) {
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            if let Ok(mut s) = serde_json::from_str::<Self>(text) {
                s.migrate_legacy_palette();
                s.clamp_values();
                s.save();
                return s;
            }
        }
        Self::default()
    }

    /// Move old global accent/custom_palette into theme_palettes.custom only.
    fn migrate_legacy_palette(&mut self) {
        let legacy = !self.custom_palette.is_empty() || self.accent_rgb.is_some();
        if legacy && self.theme_palettes.custom.is_empty() {
            let mut p = self.custom_palette.clone();
            if p.accent.is_none() {
                p.accent = self.accent_rgb;
            }
            self.theme_palettes.custom = p;
        }
        // Drop legacy fields from future saves.
        self.accent_rgb = None;
        self.custom_palette = CustomPalette::default();
    }

    fn clamp_values(&mut self) {
        if !self.editor_font_size.is_finite() {
            self.editor_font_size = DEFAULT_EDITOR_FONT;
        }
        self.editor_font_size = self
            .editor_font_size
            .clamp(MIN_EDITOR_FONT, MAX_EDITOR_FONT);
        if !self.sidebar_width.is_finite() {
            self.sidebar_width = DEFAULT_SIDEBAR_W;
        }
        self.sidebar_width = self.sidebar_width.clamp(MIN_SIDEBAR_W, MAX_SIDEBAR_W);
        if !self.console_height.is_finite() {
            self.console_height = DEFAULT_CONSOLE_H;
        }
        self.console_height = self.console_height.clamp(MIN_CONSOLE_H, MAX_CONSOLE_H);
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, text);
        }
    }

    pub fn set_theme(&mut self, theme: ThemeId) {
        self.theme = theme;
        self.save();
    }

    pub fn palette(&self, id: ThemeId) -> &CustomPalette {
        self.theme_palettes.get(id)
    }

    pub fn current_palette(&self) -> &CustomPalette {
        self.palette(self.theme)
    }

    pub fn patch_palette_part(&mut self, id: ThemeId, part: &str, rgb: u32) {
        let p = self.theme_palettes.get_mut(id);
        match part {
            "accent" => p.accent = Some(rgb),
            "bg" => p.bg = Some(rgb),
            "panel" => p.panel = Some(rgb),
            "code" => p.code = Some(rgb),
            "text" => p.text = Some(rgb),
            _ => {}
        }
        self.save();
    }

    /// Restore stock colors for one theme (Dark/Light/Custom).
    pub fn reset_theme_palette(&mut self, id: ThemeId) {
        self.theme_palettes.clear(id);
        self.save();
    }

    pub fn set_download_dir(&mut self, dir: impl AsRef<Path>) {
        self.download_dir = Some(dir.as_ref().to_path_buf());
        self.save();
    }

    pub fn set_last_project(&mut self, dir: impl AsRef<Path>) {
        self.last_project = Some(dir.as_ref().to_path_buf());
        self.save();
    }

    pub fn set_last_firmware_dir(&mut self, dir: impl AsRef<Path>) {
        self.last_firmware_dir = Some(dir.as_ref().to_path_buf());
        self.save();
    }

    pub fn set_last_port(&mut self, port: impl Into<String>) {
        self.last_port = Some(port.into());
        self.save();
    }

    pub fn set_selected_board(&mut self, board: impl Into<String>) {
        self.selected_board = Some(board.into());
        self.save();
    }

    pub fn set_transfer_mode(&mut self, mode: TransferMode) {
        self.transfer_mode = mode;
        self.save();
    }

    pub fn set_font_zh(&mut self, font: impl Into<String>) {
        self.font_zh = font.into();
        self.save();
    }

    pub fn set_font_en(&mut self, font: impl Into<String>) {
        self.font_en = font.into();
        self.save();
    }

    pub fn set_tree_sort(&mut self, sort: &str) {
        self.tree_sort = sort.to_string();
        self.save();
    }

    pub fn set_show_sidebar(&mut self, show: bool) {
        self.show_sidebar = show;
        self.save();
    }

    pub fn set_show_console(&mut self, show: bool) {
        self.show_console = show;
        self.save();
    }

    pub fn set_full_output(&mut self, full: bool) {
        self.full_output = full;
        self.save();
    }

    pub fn set_sidebar_width(&mut self, w: f32) {
        self.sidebar_width = w.clamp(MIN_SIDEBAR_W, MAX_SIDEBAR_W);
        self.save();
    }

    pub fn set_console_height(&mut self, h: f32) {
        self.console_height = h.clamp(MIN_CONSOLE_H, MAX_CONSOLE_H);
        self.save();
    }

    pub fn set_editor_font_size(&mut self, size: f32) {
        self.editor_font_size = size.clamp(MIN_EDITOR_FONT, MAX_EDITOR_FONT);
        self.save();
    }

    pub fn set_telemetry_dock(&mut self, dock: &str) {
        self.telemetry_dock = dock.to_string();
        self.save();
    }
}

/// ROM BSL UART — fixed 9600 8N1 (not user-configurable).
pub const BSL_SERIAL_BAUD: u32 = 9_600;

/// Script / console UART — fixed by board firmware (`UART_0_BAUD_RATE`).
pub const APP_SERIAL_BAUD: u32 = 115_200;
