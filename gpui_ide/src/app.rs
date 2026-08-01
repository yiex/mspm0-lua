use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    actions, div, prelude::*, px, rgb, size, App, Application, Bounds, Context, CursorStyle,
    Entity, FocusHandle, Focusable, KeyBinding, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, SharedString, Window, WindowBounds, WindowControlArea, WindowHandle, WindowOptions,
};
use parking_lot::Mutex;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::color_wheel::{rgb_to_hsla, ColorPart, ColorWheel};
use crate::compile::{compile_source, ensure_luac_name, find_compiler};
use crate::console::{
    ConsoleCtxSlot, ConsoleView, CopySelection as ConsoleCopy, SelectAll as ConsoleSelectAll,
};
use crate::editor::{
    AcceptCompletion, Backspace, CodeEditor, Copy, Cut, Delete, Down, End, Enter,
    Escape as EditorEscape, Home, Left, Paste, Redo, Right, SelectAll, SelectLeft, SelectRight,
    Tab, Undo, Up,
};
use crate::fontpack;
use crate::line_input::{
    Backspace as LiBackspace, Copy as LiCopy, Cut as LiCut, Delete as LiDelete, End as LiEnd,
    Enter as LiEnter, Escape as LiEscape, Home as LiHome, Left as LiLeft, LineInput,
    Paste as LiPaste, Right as LiRight, SelectAll as LiSelectAll,
};
use crate::metadata::{self, BoardChoice, TargetProfile};
use crate::modular;
use crate::project::{self, ProjectMeta, TreeEntry, TreeKind, TreeSort};
use crate::serial::{is_lfs_error, list_ports, PortChoice, SerialSession};
use crate::settings::{
    AppSettings, TransferMode, APP_SERIAL_BAUD, BSL_SERIAL_BAUD, DEFAULT_CONSOLE_H,
    DEFAULT_SIDEBAR_W, HIDE_CONSOLE_H, HIDE_SIDEBAR_W, MAX_CONSOLE_H, MAX_SIDEBAR_W, MIN_CONSOLE_H,
    MIN_SIDEBAR_W,
};
use crate::snippets;
use crate::telemetry::{
    is_transport_noise, normalize_transport_line, SerialLineCleaner, TelemetryView,
};
use crate::theme::{Theme, ThemeId};

actions!(
    ide,
    [
        Run,
        Stop,
        Connect,
        Disconnect,
        RefreshPorts,
        RefreshFiles,
        ClearLog,
        CopyLog,
        CopyProjectContext,
        DownloadLuac,
        FlashFirmware,
        NewProject,
        OpenProject,
        SaveFile,
        SaveFileAs,
        OpenSource,
        CloseMenu,
        CycleTheme,
        ThemeDark,
        ThemeLight,
        OpenSettings,
        OpenAbout,
        OpenKeys,
    ]
);

/// Top-level menu bar groups (by function).
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuId {
    /// 工程 / 源文件 / 设置
    File,
    /// 磁盘 example/ 下的工程示例
    Example,
    /// Runtime-discovered boards/ files.
    Board,
    /// 编译上传与板端脚本
    Run,
    /// 连接、烧录、Flash
    Device,
    /// Optional workbench panels.
    View,
    Help,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    Settings,
    About,
    Keys,
    /// Confirm LittleFS reformat after SCRIPT_ERR name/fs (etc.).
    FormatFs,
    /// Confirm delete path in `dialog_path`.
    DeleteConfirm,
    /// Required on first use only when more than one board is installed.
    BoardSelect,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TelemetryDock {
    Hidden,
    Right,
    Window,
    Editor,
}

#[derive(Clone, Copy)]
struct TelemetryDrag;

struct TelemetryDragPreview {
    position: Point<Pixels>,
}

impl Render for TelemetryDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let preview_bg: gpui::Hsla = gpui::rgb(0x182333).into();
        div()
            .pl(self.position.x - px(78.))
            .pt(self.position.y - px(16.))
            .child(
                div()
                    .w(px(156.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(0x45a3ff))
                    .bg(preview_bg.opacity(0.94))
                    .text_xs()
                    .text_color(gpui::rgb(0xe7edf5))
                    .shadow_md()
                    .child("数据可视化"),
            )
    }
}

struct TelemetryWindow {
    telemetry: Entity<TelemetryView>,
    owner: gpui::WeakEntity<IdeApp>,
    theme: Theme,
}

impl TelemetryWindow {
    fn new(
        telemetry: Entity<TelemetryView>,
        owner: gpui::WeakEntity<IdeApp>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Self {
        let owner_on_close = owner.clone();
        cx.on_release(move |_, cx| {
            let _ = owner_on_close.update(cx, |ide, cx| {
                if ide.telemetry_dock == TelemetryDock::Window {
                    ide.telemetry_window = None;
                    ide.telemetry_dock = TelemetryDock::Hidden;
                    ide.telemetry_tab_active = false;
                    cx.notify();
                }
            });
        })
        .detach();

        Self {
            telemetry,
            owner,
            theme,
        }
    }

    fn return_to_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.owner.update(cx, |ide, cx| {
            ide.telemetry_window = None;
            ide.show_telemetry_right(cx);
        });
        window.remove_window();
    }
}

impl Render for TelemetryWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme.code)
            .text_color(theme.text)
            .child(
                div()
                    .h(px(38.))
                    .min_h(px(38.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(theme.line)
                    .bg(theme.panel)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("数据可视化"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("telemetry-window-dock-right")
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(theme.line)
                            .bg(theme.btn_bg)
                            .text_xs()
                            .text_color(theme.text)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.menu_hover))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.return_to_editor(window, cx);
                            }))
                            .child("返回编辑器"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(self.telemetry.clone()),
            )
    }
}

/// One open editor document (tab).
#[derive(Clone)]
struct EditorTab {
    path: Option<PathBuf>,
    title: String,
    content: String,
    dirty: bool,
    target_name: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogKind {
    Rx,
    Tx,
    Sys,
    Diag,
    Err,
}

#[derive(Clone)]
struct LogLine {
    kind: LogKind,
    text: SharedString,
}

struct SharedState {
    busy: bool,
    /// True after main upload OK until user stops / script done markers.
    script_running: bool,
    /// Set only by the explicit Stop control; suppresses trailing runtime error markers.
    stop_requested: bool,
    connected: bool,
    port_name: Option<String>,
    files: Vec<(String, u64)>,
    selected_file: Option<String>,
    logs: Vec<LogLine>,
    status: String,
    error: Option<String>,
    session: Option<Arc<SerialSession>>,
    rx_cursor: usize,
    rx_cleaner: SerialLineCleaner,
    log_epoch: u64,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            busy: false,
            script_running: false,
            stop_requested: false,
            connected: false,
            port_name: None,
            files: Vec::new(),
            selected_file: None,
            logs: Vec::new(),
            status: "就绪".into(),
            error: None,
            session: None,
            rx_cursor: 0,
            rx_cleaner: SerialLineCleaner::default(),
            log_epoch: 0,
        }
    }
}

pub struct IdeApp {
    theme: Theme,
    editor: Entity<CodeEditor>,
    console: Entity<ConsoleView>,
    telemetry: Entity<TelemetryView>,
    target_name: String,
    ports: Vec<PortChoice>,
    selected_port_idx: usize,
    compiler: Option<PathBuf>,
    shared: Arc<Mutex<SharedState>>,
    focus_handle: FocusHandle,
    show_ports: bool,
    last_console_epoch: u64,
    /// Open project directory (contains mspm0_lua.json / main.lua).
    project_dir: Option<PathBuf>,
    project_meta: ProjectMeta,
    target_profile: Option<Arc<TargetProfile>>,
    board_choices: Vec<BoardChoice>,
    /// Process-local deployment memory; intentionally never serialized.
    run_cache: Arc<Mutex<SessionRunCache>>,
    /// Current source file path on disk (may be outside project).
    source_path: Option<PathBuf>,
    dirty: bool,
    /// Open editor tabs (multi-file).
    open_tabs: Vec<EditorTab>,
    active_tab: usize,
    open_menu: Option<MenuId>,
    /// Project explorer tree (flat rows with depth).
    project_tree: Vec<TreeEntry>,
    /// runfile() references from current editor buffer.
    source_refs: Vec<String>,
    tree_selected: Option<PathBuf>,
    /// Left project tree visible.
    show_sidebar: bool,
    /// Bottom output panel visible.
    show_console: bool,
    /// On-demand serial visualization location.
    telemetry_dock: TelemetryDock,
    /// Native window used while visualization is detached from the workbench.
    telemetry_window: Option<WindowHandle<TelemetryWindow>>,
    /// Tracks a telemetry drag so release outside the workbench can detach it.
    telemetry_dragging: bool,
    /// Whether the locked visualization tab is the active editor page.
    telemetry_tab_active: bool,
    /// Sidebar width in px (remembered).
    sidebar_width: f32,
    /// Console height in px (remembered).
    console_height: f32,
    /// Width of the optional right-side visualization panel.
    telemetry_width: f32,
    /// Dragging sidebar | editor splitter: (start mouse x, start width).
    drag_sidebar: Option<(f32, f32)>,
    /// Dragging editor — console splitter: (start mouse y, start height).
    drag_console: Option<(f32, f32)>,
    /// Dragging editor | visualization splitter: (start mouse x, start width).
    drag_telemetry: Option<(f32, f32)>,
    settings: AppSettings,
    dialog: Option<DialogKind>,
    /// Project tree context menu: path + window coords.
    tree_ctx: Option<(PathBuf, f32, f32)>,
    /// Context menu shows sort submenu.
    tree_ctx_sort: bool,
    /// Sidebar sort mode.
    tree_sort: TreeSort,
    /// Output panel context menu at window coords (x, y).
    console_ctx: Option<(f32, f32)>,
    console_ctx_slot: ConsoleCtxSlot,
    /// Output search query (empty = no filter highlight).
    console_search: Entity<LineInput>,
    console_search_open: bool,
    /// Custom theme color wheel (Settings).
    color_wheel: Entity<ColorWheel>,
    last_wheel_rev: u64,
    last_wheel_part: ColorPart,
    /// Detail line shown in FormatFs dialog.
    format_fs_detail: String,
    /// Path for DeleteConfirm dialog.
    dialog_path: Option<PathBuf>,
    /// Inline rename / new-file name target in project tree.
    rename_path: Option<PathBuf>,
    /// True when rename started from「新建」(Esc cancels and removes the file).
    rename_is_new: bool,
    /// Focus LineInput once after entering rename.
    rename_focus: bool,
    rename_input: Entity<LineInput>,
}

impl IdeApp {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut settings = AppSettings::load();
        let board_choices = match metadata::discover_boards() {
            Ok(choices) => choices,
            Err(_) => Vec::new(),
        };
        let selected_valid = settings
            .selected_board
            .as_ref()
            .is_some_and(|selected| board_choices.iter().any(|board| &board.id == selected));
        if !selected_valid {
            settings.selected_board = None;
        }
        if board_choices.len() == 1 && settings.selected_board.is_none() {
            settings.set_selected_board(board_choices[0].id.clone());
        }
        let tree_sort = match settings.tree_sort.as_str() {
            "type" => TreeSort::Type,
            "date" => TreeSort::Date,
            "size" => TreeSort::Size,
            _ => TreeSort::Name,
        };
        let saved_telemetry_dock = settings.telemetry_dock.clone();
        let editor = cx.new(|cx| CodeEditor::new(cx, ""));
        let rename_input = cx.new(|cx| LineInput::new(cx, ""));
        let console_search = cx.new(|cx| LineInput::new(cx, ""));
        let color_wheel = cx.new(|cx| ColorWheel::new(cx));
        let console_ctx_slot: ConsoleCtxSlot = Arc::new(Mutex::new(None));
        let slot = console_ctx_slot.clone();
        let console = cx.new(|cx| ConsoleView::with_ctx_slot(slot, cx));
        let telemetry = cx.new(|_| TelemetryView::new());
        let compiler = find_compiler();
        let mut app = Self {
            theme: Theme::resolve(settings.theme, Some(settings.palette(settings.theme))),
            editor,
            console,
            telemetry,
            rename_input,
            target_name: "main.luac".into(),
            ports: list_ports(),
            selected_port_idx: 0,
            compiler,
            shared: Arc::new(Mutex::new(SharedState::default())),
            focus_handle: cx.focus_handle(),
            show_ports: false,
            last_console_epoch: 0,
            project_dir: None,
            project_meta: ProjectMeta::default(),
            target_profile: None,
            board_choices,
            run_cache: Arc::new(Mutex::new(SessionRunCache::default())),
            source_path: None,
            dirty: false,
            open_tabs: Vec::new(),
            active_tab: 0,
            open_menu: None,
            project_tree: Vec::new(),
            source_refs: Vec::new(),
            tree_selected: None,
            show_sidebar: settings.show_sidebar,
            show_console: settings.show_console,
            telemetry_dock: match saved_telemetry_dock.as_str() {
                "hidden" => TelemetryDock::Hidden,
                "editor" => TelemetryDock::Editor,
                "window" | "right" => TelemetryDock::Right,
                _ => TelemetryDock::Right,
            },
            telemetry_window: None,
            telemetry_dragging: false,
            telemetry_tab_active: saved_telemetry_dock == "editor",
            sidebar_width: settings.sidebar_width,
            console_height: settings.console_height,
            telemetry_width: 460.0,
            drag_sidebar: None,
            drag_console: None,
            drag_telemetry: None,
            settings,
            dialog: None,
            tree_ctx: None,
            tree_ctx_sort: false,
            tree_sort,
            console_ctx: None,
            console_ctx_slot,
            console_search,
            console_search_open: false,
            color_wheel,
            last_wheel_rev: 0,
            last_wheel_part: ColorPart::Accent,
            format_fs_detail: String::new(),
            dialog_path: None,
            rename_path: None,
            rename_is_new: false,
            rename_focus: false,
        };
        if app.board_choices.len() > 1 && app.settings.selected_board.is_none() {
            app.dialog = Some(DialogKind::BoardSelect);
        }
        app.pick_default_port();
        // Restore last port if still present.
        if let Some(name) = app.settings.last_port.clone() {
            if let Some(idx) = app.ports.iter().position(|p| p.name == name) {
                app.selected_port_idx = idx;
            }
        }
        let t = app.theme;
        let font = app.settings.editor_font_size;
        app.editor.update(cx, |ed, cx| {
            ed.set_theme(t, cx);
            ed.set_font_px(font, cx);
        });
        app.console.update(cx, |c, cx| c.set_theme(t, cx));
        app.telemetry.update(cx, |scope, cx| scope.set_theme(t, cx));
        app.rename_input.update(cx, |inp, cx| inp.set_theme(t, cx));
        app.console_search
            .update(cx, |inp, cx| inp.set_theme(t, cx));
        app.color_wheel.update(cx, |w, cx| w.set_theme(t, cx));
        app.sync_wheel_from_settings(cx);
        if app.compiler.is_none() {
            app.push_err("内置 Lua 编译器不可用（构建异常）", cx);
        }
        // Empty default tab — examples live in <exe>/example/.
        app.open_tabs.push(EditorTab {
            path: None,
            title: "未命名.lua".into(),
            content: String::new(),
            dirty: false,
            target_name: "main.luac".into(),
        });
        app.active_tab = 0;
        // Load the supported default target even before a project is opened.
        let startup_registry_context =
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let startup_meta = app.project_meta.clone();
        app.configure_target(&startup_registry_context, &startup_meta, cx);
        // Restore last project if present.
        if let Some(dir) = app.settings.last_project.clone() {
            if dir.is_dir() {
                if let Ok(meta) = project::load_project(&dir) {
                    let main = project::resolve_main(&dir, &meta);
                    let src = project::read_source_file(&main).unwrap_or_default();
                    app.project_dir = Some(dir);
                    app.project_meta = meta.clone();
                    if let Some(project_dir) = app.project_dir.clone() {
                        app.configure_target(&project_dir, &meta, cx);
                    }
                    app.target_name = meta.target_luac.clone();
                    app.tree_selected = Some(main.clone());
                    app.open_tabs.clear();
                    app.set_source(src, Some(main), cx);
                    app.refresh_project_tree(cx);
                }
            }
        }
        {
            let mut s = app.shared.lock();
            s.status = "就绪".into();
        }
        cx.on_release(|app, cx| {
            if let Some(handle) = app.telemetry_window.take() {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        })
        .detach();
        app.start_rx_poll(cx);
        if saved_telemetry_dock == "window" {
            app.show_telemetry_window(cx);
        }
        app
    }

    fn refresh_project_tree(&mut self, cx: &mut Context<Self>) {
        if let Some(dir) = self.project_dir.clone() {
            self.project_tree =
                project::list_project_tree(&dir, &self.project_meta, self.tree_sort);
        } else {
            self.project_tree.clear();
        }
        let src = self.editor.read(cx).text();
        self.source_refs = project::find_runfile_refs(&src);
        cx.notify();
    }

    fn configure_target(
        &mut self,
        _project_dir: &std::path::Path,
        _meta: &ProjectMeta,
        cx: &mut Context<Self>,
    ) {
        let Some(board_id) = self.settings.selected_board.clone() else {
            self.target_profile = None;
            self.editor
                .update(cx, |editor, cx| editor.set_target_profile(None, false, cx));
            return;
        };
        match metadata::load_board(&board_id) {
            Ok(profile) => {
                self.push_sys(
                    format!(
                        "目标元数据已加载 · {} · {} · {}",
                        profile.board_id, profile.chip_id, profile.api_id
                    ),
                    cx,
                );
                self.target_profile = Some(profile.clone());
                self.editor.update(cx, |editor, cx| {
                    editor.set_target_profile(Some(profile), true, cx)
                });
            }
            Err(error) => {
                self.target_profile = None;
                self.editor
                    .update(cx, |editor, cx| editor.set_target_profile(None, true, cx));
                self.push_err(format!("目标元数据加载失败: {error:#}"), cx);
            }
        }
    }

    fn select_board(&mut self, board_id: String, cx: &mut Context<Self>) {
        if !self.board_choices.iter().any(|board| board.id == board_id) {
            self.push_err(format!("开发板不存在: {board_id}"), cx);
            return;
        }
        match metadata::load_board(&board_id) {
            Ok(profile) => {
                self.settings.set_selected_board(board_id);
                self.dialog = None;
                self.target_profile = Some(profile.clone());
                self.editor.update(cx, |editor, cx| {
                    editor.set_target_profile(Some(profile.clone()), true, cx)
                });
                self.push_sys(
                    format!(
                        "开发板已切换 · {} · {} · {}",
                        profile.board_id, profile.chip_id, profile.api_id
                    ),
                    cx,
                );
            }
            Err(error) => {
                self.push_err(format!("开发板切换失败，继续使用原配置: {error:#}"), cx);
            }
        }
    }

    fn set_tree_sort(&mut self, sort: TreeSort, cx: &mut Context<Self>) {
        self.tree_sort = sort;
        self.tree_ctx = None;
        self.tree_ctx_sort = false;
        let key = match sort {
            TreeSort::Name => "name",
            TreeSort::Type => "type",
            TreeSort::Date => "date",
            TreeSort::Size => "size",
        };
        self.settings.set_tree_sort(key);
        self.refresh_project_tree(cx);
    }

    fn open_tree_entry(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path.is_dir() {
            self.tree_selected = Some(path);
            cx.notify();
            return;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        // Only open text sources / config.
        if !(name.ends_with(".lua")
            || name.ends_with(".json")
            || name.ends_with(".txt")
            || name.ends_with(".md"))
        {
            self.tree_selected = Some(path);
            return;
        }
        // Already open → switch tab (keep unsaved buffer).
        if let Some(i) = self
            .open_tabs
            .iter()
            .position(|t| t.path.as_ref() == Some(&path))
        {
            self.activate_tab(i, cx);
            return;
        }
        match project::read_source_file(&path) {
            Ok(src) => {
                self.tree_selected = Some(path.clone());
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if stem == "main" {
                        self.target_name = "main.luac".into();
                    } else if name.ends_with(".lua") {
                        self.target_name = format!("{stem}.luac");
                    }
                }
                self.set_source(src, Some(path), cx);
                self.source_refs = project::find_runfile_refs(&self.editor.read(cx).text());
                cx.notify();
            }
            Err(err) => self.push_err(format!("打开失败: {err:#}"), cx),
        }
    }

    fn window_title(&self) -> String {
        let dirty = if self.dirty { " *" } else { "" };
        let file = self
            .source_path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("未命名.lua");
        if let Some(dir) = &self.project_dir {
            let proj = self.project_meta.name.as_str();
            let short = dir.file_name().and_then(|s| s.to_str()).unwrap_or(proj);
            format!("{file}{dirty} — {short}")
        } else {
            format!("{file}{dirty}")
        }
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.open_menu = None;
        cx.notify();
    }

    fn toggle_menu(&mut self, id: MenuId, cx: &mut Context<Self>) {
        self.open_menu = if self.open_menu == Some(id) {
            None
        } else {
            Some(id)
        };
        cx.notify();
    }

    fn show_telemetry_right(&mut self, cx: &mut Context<Self>) {
        self.telemetry_dock = TelemetryDock::Right;
        self.telemetry_dragging = false;
        self.telemetry_tab_active = false;
        self.close_telemetry_window(cx);
        self.settings.set_telemetry_dock("right");
        cx.notify();
    }

    fn dock_telemetry_in_editor(&mut self, cx: &mut Context<Self>) {
        self.flush_active_tab(cx);
        self.telemetry_dock = TelemetryDock::Editor;
        self.telemetry_dragging = false;
        self.telemetry_tab_active = true;
        self.close_telemetry_window(cx);
        self.settings.set_telemetry_dock("editor");
        cx.notify();
    }

    fn show_telemetry_window(&mut self, cx: &mut Context<Self>) {
        if self.telemetry_dock == TelemetryDock::Window {
            if let Some(handle) = self.telemetry_window {
                if handle
                    .update(cx, |_, window, _| window.activate_window())
                    .is_ok()
                {
                    self.open_menu = None;
                    return;
                }
            }
            self.telemetry_window = None;
        }

        if self.telemetry_dock == TelemetryDock::Editor {
            self.flush_active_tab(cx);
        }
        self.telemetry_dock = TelemetryDock::Window;
        self.telemetry_dragging = false;
        self.telemetry_tab_active = false;
        self.open_menu = None;
        self.settings.set_telemetry_dock("window");
        cx.notify();

        let telemetry = self.telemetry.clone();
        let theme = self.theme;
        cx.spawn(
            async move |owner: gpui::WeakEntity<IdeApp>, cx: &mut gpui::AsyncApp| {
                let bounds =
                    match cx.update(|app| Bounds::centered(None, size(px(900.), px(620.)), app)) {
                        Ok(bounds) => bounds,
                        Err(error) => {
                            let _ = owner.update(cx, |ide, cx| {
                                ide.telemetry_dock = TelemetryDock::Hidden;
                                ide.push_err(format!("打开数据可视化窗口失败: {error:#}"), cx);
                                cx.notify();
                            });
                            return;
                        }
                    };
                let window_owner = owner.clone();
                let result = cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: Some(gpui::TitlebarOptions {
                            title: Some("Lua IDE · 数据可视化".into()),
                            appears_transparent: false,
                            traffic_light_position: None,
                        }),
                        window_background: gpui::WindowBackgroundAppearance::Opaque,
                        window_min_size: Some(size(px(600.), px(420.))),
                        ..Default::default()
                    },
                    move |_, app| {
                        app.new(|cx| TelemetryWindow::new(telemetry, window_owner, theme, cx))
                    },
                );

                match result {
                    Ok(handle) => {
                        let _ = owner.update(cx, |ide, cx| {
                            if ide.telemetry_dock == TelemetryDock::Window {
                                ide.telemetry_window = Some(handle);
                                cx.notify();
                            } else {
                                let _ = handle.update(cx, |_, window, _| window.remove_window());
                            }
                        });
                    }
                    Err(error) => {
                        let _ = owner.update(cx, |ide, cx| {
                            ide.telemetry_window = None;
                            ide.telemetry_dock = TelemetryDock::Hidden;
                            ide.push_err(format!("打开数据可视化窗口失败: {error:#}"), cx);
                            cx.notify();
                        });
                    }
                }
            },
        )
        .detach();
    }

    fn close_telemetry_window(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.telemetry_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    fn hide_telemetry(&mut self, cx: &mut Context<Self>) {
        self.telemetry_dock = TelemetryDock::Hidden;
        self.telemetry_dragging = false;
        self.telemetry_tab_active = false;
        self.settings.set_telemetry_dock("hidden");
        self.close_telemetry_window(cx);
        cx.notify();
    }

    fn mark_clean(&mut self) {
        self.dirty = false;
        if let Some(tab) = self.open_tabs.get_mut(self.active_tab) {
            tab.dirty = false;
        }
    }

    fn tab_title_for(path: &Option<PathBuf>) -> String {
        path.as_ref()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("未命名.lua")
            .to_string()
    }

    /// Flush editor buffer into the active tab snapshot.
    fn flush_active_tab(&mut self, cx: &mut Context<Self>) {
        if self.open_tabs.is_empty() {
            return;
        }
        let idx = self.active_tab.min(self.open_tabs.len() - 1);
        let text = self.editor.read(cx).text();
        let tab = &mut self.open_tabs[idx];
        if tab.content != text {
            tab.content = text;
            tab.dirty = true;
            self.dirty = true;
        }
        tab.path = self.source_path.clone();
        tab.title = Self::tab_title_for(&tab.path);
        tab.target_name = self.target_name.clone();
    }

    fn ensure_tab_for_path(&mut self, path: Option<PathBuf>, content: &str, target: &str) -> usize {
        if let Some(ref p) = path {
            if let Some(i) = self
                .open_tabs
                .iter()
                .position(|t| t.path.as_ref() == Some(p))
            {
                return i;
            }
        }
        self.open_tabs.push(EditorTab {
            path: path.clone(),
            title: Self::tab_title_for(&path),
            content: content.to_string(),
            dirty: false,
            target_name: target.to_string(),
        });
        self.open_tabs.len() - 1
    }

    fn activate_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.open_tabs.len() {
            return;
        }
        self.flush_active_tab(cx);
        self.telemetry_tab_active = false;
        self.active_tab = idx;
        let tab = self.open_tabs[idx].clone();
        self.source_path = tab.path.clone();
        self.target_name = tab.target_name.clone();
        self.dirty = tab.dirty;
        self.tree_selected = tab.path.clone();
        self.editor
            .update(cx, |ed, cx| ed.set_text(tab.content.clone(), cx));
        self.source_refs = project::find_runfile_refs(&tab.content);
        cx.notify();
    }

    fn close_tab_at(&mut self, idx: usize, cx: &mut Context<Self>) {
        if self.open_tabs.is_empty() || idx >= self.open_tabs.len() {
            return;
        }
        // Always keep at least one tab.
        if self.open_tabs.len() == 1 {
            self.open_tabs[0] = EditorTab {
                path: None,
                title: "未命名.lua".into(),
                content: String::new(),
                dirty: false,
                target_name: "main.luac".into(),
            };
            self.active_tab = 0;
            self.source_path = None;
            self.dirty = false;
            self.target_name = "main.luac".into();
            self.editor.update(cx, |ed, cx| ed.set_text("", cx));
            self.source_refs.clear();
            cx.notify();
            return;
        }
        self.open_tabs.remove(idx);
        let next = if self.active_tab > idx {
            self.active_tab - 1
        } else if self.active_tab >= self.open_tabs.len() {
            self.open_tabs.len() - 1
        } else {
            self.active_tab
        };
        // Load without flush (removed tab already discarded).
        self.active_tab = next;
        let tab = self.open_tabs[next].clone();
        self.source_path = tab.path.clone();
        self.target_name = tab.target_name.clone();
        self.dirty = tab.dirty;
        self.tree_selected = tab.path.clone();
        self.editor
            .update(cx, |ed, cx| ed.set_text(tab.content.clone(), cx));
        self.source_refs = project::find_runfile_refs(&tab.content);
        cx.notify();
    }

    fn set_source(
        &mut self,
        text: impl Into<SharedString>,
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let text: SharedString = text.into();
        self.telemetry_tab_active = false;
        let content = text.to_string();
        // If switching path while editing another tab, snapshot current first.
        if !self.open_tabs.is_empty() {
            let cur_path = self
                .open_tabs
                .get(self.active_tab)
                .and_then(|t| t.path.clone());
            if cur_path != path {
                self.flush_active_tab(cx);
            }
        }
        let target = self.target_name.clone();
        let idx = self.ensure_tab_for_path(path.clone(), &content, &target);
        self.active_tab = idx;
        if let Some(tab) = self.open_tabs.get_mut(idx) {
            tab.content = content.clone();
            tab.path = path.clone();
            tab.title = Self::tab_title_for(&path);
            tab.target_name = target;
            tab.dirty = false;
        }
        self.editor.update(cx, |ed, cx| ed.set_text(text, cx));
        self.source_path = path;
        self.mark_clean();
        cx.notify();
    }

    fn pick_default_port(&mut self) {
        if let Some(idx) = self.ports.iter().position(|p| {
            let l = p.label.to_ascii_lowercase();
            l.contains("ch340") && !l.contains("j-link")
        }) {
            self.selected_port_idx = idx;
        } else if let Some(idx) = self
            .ports
            .iter()
            .position(|p| !p.label.to_ascii_lowercase().contains("j-link"))
        {
            self.selected_port_idx = idx;
        }
    }

    fn push_sys(&self, text: impl Into<String>, cx: &mut Context<Self>) {
        let mut s = self.shared.lock();
        s.logs.push(LogLine {
            kind: LogKind::Sys,
            text: text.into().into(),
        });
        s.log_epoch += 1;
        cx.notify();
    }

    fn push_err(&self, text: impl Into<String>, cx: &mut Context<Self>) {
        let t = text.into();
        let mut s = self.shared.lock();
        s.error = Some(t.clone());
        s.logs.push(LogLine {
            kind: LogKind::Err,
            text: t.into(),
        });
        s.log_epoch += 1;
        cx.notify();
    }

    fn push_diag(&self, text: impl Into<String>, cx: &mut Context<Self>) {
        let mut s = self.shared.lock();
        s.logs.push(LogLine {
            kind: LogKind::Diag,
            text: text.into().into(),
        });
        s.log_epoch += 1;
        cx.notify();
    }

    fn sync_console(&mut self, cx: &mut Context<Self>) {
        let full_output = self.settings.full_output;
        let (epoch, lines) = {
            let s = self.shared.lock();
            if s.log_epoch == self.last_console_epoch {
                return;
            }
            let theme = self.theme;
            let mut lines: Vec<(String, gpui::Hsla)> = Vec::new();
            for line in s.logs.iter().rev().take(400).rev() {
                let color = match line.kind {
                    LogKind::Tx => theme.blue,
                    LogKind::Rx => theme.green,
                    LogKind::Sys => theme.muted,
                    LogKind::Diag => theme.yellow,
                    LogKind::Err => theme.red,
                };
                for part in line.text.lines() {
                    if let Some(text) = console_display_text(line.kind, part, full_output) {
                        lines.push((text, color));
                    }
                }
            }
            (s.log_epoch, lines)
        };
        self.last_console_epoch = epoch;
        self.console
            .update(cx, |console, cx| console.set_lines(&lines, cx));
    }

    fn start_rx_poll(&self, cx: &mut Context<Self>) {
        let shared = self.shared.clone();
        let ctx_slot = self.console_ctx_slot.clone();
        let telemetry = self.telemetry.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                // Console right-click → open menu on IdeApp (window coords).
                if let Some(pos) = ctx_slot.lock().take() {
                    this.update(cx, |app, cx| {
                        app.console_ctx = Some(pos);
                        app.tree_ctx = None;
                        app.open_menu = None;
                        cx.notify();
                    })
                    .ok();
                }
                let mut changed = false;
                let mut telemetry_lines = Vec::new();
                let mut restore_after_run = None;
                {
                    let mut s = shared.lock();
                    if let Some(session) = s.session.clone() {
                        let (chunk, next) = session.drain_new(s.rx_cursor);
                        if !chunk.is_empty() {
                            s.rx_cursor = next;
                            let parts = s.rx_cleaner.push(&chunk);
                            for part in parts {
                                // Protocol tokens update status, while the raw
                                // line stays available to Complete Output.
                                let terminal = part.contains("SCRIPT_DONE")
                                    || part.contains("LED_BLINK_DONE")
                                    || part.contains("stopped");
                                let stopped_by_user = s.stop_requested
                                    && (terminal
                                        || part.contains("STOP")
                                        || part.contains("LUA stopped"));
                                if stopped_by_user {
                                    s.script_running = false;
                                    s.status = "已停止".into();
                                } else {
                                    if let Some(status) = protocol_status_update(&part) {
                                        s.status = status;
                                        if terminal {
                                            s.script_running = false;
                                        }
                                        if part.contains("SCRIPT_OK") && !s.busy {
                                            // size may appear after SCRIPT_OK
                                            s.script_running = true;
                                        }
                                    }
                                    if terminal {
                                        s.script_running = false;
                                        if s.status.starts_with("运行中") {
                                            s.status = "完成".into();
                                        }
                                    }
                                }
                                if terminal || stopped_by_user {
                                    restore_after_run = Some(session.clone());
                                }
                                if !is_transport_noise(&part) {
                                    telemetry_lines.push(part.clone());
                                }
                                s.logs.push(LogLine {
                                    kind: LogKind::Rx,
                                    text: part.into(),
                                });
                                changed = true;
                            }
                            if s.logs.len() > 600 {
                                let drain = s.logs.len() - 600;
                                s.logs.drain(0..drain);
                            }
                            if changed {
                                s.log_epoch += 1;
                            }
                        }
                    }
                }
                if let Some(session) = restore_after_run {
                    let shared_restore = shared.clone();
                    std::thread::spawn(move || {
                        match session.restore_115200_after_run() {
                            Ok(true) => {
                                let mut state = shared_restore.lock();
                                state.logs.push(LogLine {
                                    kind: LogKind::Sys,
                                    text: "Serial restored to 115200 after run".into(),
                                });
                                state.log_epoch += 1;
                            }
                            Ok(false) => {}
                            Err(error) => {
                                let mut state = shared_restore.lock();
                                state.logs.push(LogLine {
                                    kind: LogKind::Err,
                                    text: format!("Serial restore to 115200 failed: {error:#}").into(),
                                });
                                state.log_epoch += 1;
                            }
                        }
                    });
                }
                if !telemetry_lines.is_empty() {
                    let _ = telemetry.update(cx, |scope, cx| {
                        for line in &telemetry_lines {
                            scope.ingest_line(line, cx);
                        }
                    });
                }
                if changed {
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            }
        })
        .detach();
    }

    fn refresh_ports(&mut self, _: &RefreshPorts, _: &mut Window, cx: &mut Context<Self>) {
        self.ports = list_ports();
        self.pick_default_port();
        self.shared.lock().status = format!("{} 个串口", self.ports.len());
        cx.notify();
    }

    fn connect(&mut self, _: &Connect, _: &mut Window, cx: &mut Context<Self>) {
        if self.shared.lock().connected {
            return;
        }
        let Some(port) = self.ports.get(self.selected_port_idx).cloned() else {
            self.push_err("没有可用串口", cx);
            return;
        };
        if port.label.to_ascii_lowercase().contains("j-link") {
            self.push_err("请选择 CH340（PA10/PA11），不要选 J-Link CDC", cx);
            return;
        }
        match SerialSession::open(&port.name, APP_SERIAL_BAUD) {
            Ok(session) => {
                self.settings.set_last_port(&port.name);
                let session = Arc::new(session);
                {
                    let mut s = self.shared.lock();
                    s.session = Some(session.clone());
                    s.connected = true;
                    s.port_name = Some(port.name.clone());
                    s.status = format!("已连接 {} · {APP_SERIAL_BAUD}", port.name);
                    s.rx_cursor = 0;
                    s.rx_cleaner.reset();
                    s.error = None;
                    s.busy = true;
                }
                self.show_ports = false;
                cx.notify();
                // A previous long-running Lua script owns the console until it
                // receives `!`. Stop it before issuing any connection probes.
                let shared = self.shared.clone();
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(200))
                        .await;
                    let probe = {
                        let sess = session.clone();
                        let handle = std::thread::spawn(move || -> anyhow::Result<bool> {
                            sess.stop_and_wait()?;
                            sess.probe_lfs()
                        });
                        loop {
                            cx.background_executor()
                                .timer(Duration::from_millis(40))
                                .await;
                            this.update(cx, |_, cx| cx.notify()).ok();
                            if handle.is_finished() {
                                break;
                            }
                        }
                        handle
                            .join()
                            .unwrap_or_else(|_| Err(anyhow::anyhow!("探测线程异常")))
                    };
                    this.update(cx, |app, cx| {
                        {
                            let mut s = shared.lock();
                            s.busy = false;
                            match &probe {
                                Ok(true) => {
                                    s.status = format!(
                                        "已连接 {} · Flash 正常",
                                        s.port_name.as_deref().unwrap_or("?")
                                    );
                                }
                                Ok(false) => {
                                    s.status = "已连接 · Flash 需重置".into();
                                    s.logs.push(LogLine {
                                        kind: LogKind::Err,
                                        text: "连接检测：LittleFS 未就绪".into(),
                                    });
                                    s.log_epoch += 1;
                                }
                                Err(err) => {
                                    s.status = format!("已连接 · 探测失败: {err:#}");
                                }
                            }
                        }
                        cx.notify();
                        match probe {
                            Ok(true) => app.spawn_list_files(cx),
                            Ok(false) => app.offer_format_fs(
                                "连接后检测：LittleFS 未挂载，上传脚本前请重置 Flash。",
                                cx,
                            ),
                            Err(_) => app.spawn_list_files(cx),
                        }
                    })
                    .ok();
                })
                .detach();
            }
            Err(err) => self.push_err(format!("连接失败: {err:#}"), cx),
        }
    }

    fn disconnect(&mut self, _: &Disconnect, _: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.shared.lock();
        s.session = None;
        s.connected = false;
        s.script_running = false;
        s.port_name = None;
        s.files.clear();
        s.selected_file = None;
        s.rx_cleaner.reset();
        s.status = "已断开".into();
        cx.notify();
    }

    /// UART BSL flash — device must already be in ROM BSL.
    fn flash_firmware(&mut self, _: &FlashFirmware, _: &mut Window, cx: &mut Context<Self>) {
        if self.shared.lock().busy {
            self.push_err("忙，请稍候", cx);
            return;
        }
        let port_name = {
            let s = self.shared.lock();
            s.port_name.clone().or_else(|| {
                self.ports
                    .get(self.selected_port_idx)
                    .map(|p| p.name.clone())
            })
        };
        let Some(port_name) = port_name else {
            self.push_err("请先选择 CH340 串口", cx);
            return;
        };
        if let Some(p) = self.ports.iter().find(|p| p.name == port_name) {
            if p.label.to_ascii_lowercase().contains("j-link") {
                self.push_err("请选择 CH340，不要选 J-Link", cx);
                return;
            }
        }

        let mut dlg = rfd::FileDialog::new()
            .set_title("选择固件")
            .add_filter("Firmware", &["bin", "hex", "txt"])
            .add_filter("Binary", &["bin"])
            .add_filter("Intel HEX", &["hex"])
            .add_filter("TI-TXT", &["txt"])
            .add_filter("All", &["*"]);
        // Memory first; else <exe>/firmware if present.
        if let Some(dir) = self.settings.last_firmware_dir.as_ref() {
            if dir.is_dir() {
                dlg = dlg.set_directory(dir);
            }
        } else {
            let fw_dir = crate::settings::AppSettings::exe_dir().join("firmware");
            if fw_dir.is_dir() {
                dlg = dlg.set_directory(&fw_dir);
            }
            if let Some(def) = crate::bsl::find_default_firmware() {
                if let Some(name) = def.file_name().and_then(|s| s.to_str()) {
                    dlg = dlg.set_file_name(name);
                }
            }
        }
        let Some(path) = dlg.pick_file() else {
            return;
        };
        if let Some(parent) = path.parent() {
            self.settings.set_last_firmware_dir(parent);
        }

        {
            let mut s = self.shared.lock();
            s.session = None;
            s.connected = false;
            s.script_running = false;
            s.busy = true;
            s.status = format!("烧录 {}…", path.display());
            s.error = None;
            s.logs.push(LogLine {
                kind: LogKind::Sys,
                text: format!("串口烧录 · {} · {}", port_name, path.display()).into(),
            });
            s.log_epoch += 1;
        }
        cx.notify();

        let shared = self.shared.clone();
        let path_disp = path.display().to_string();
        let bsl_baud = BSL_SERIAL_BAUD;
        cx.spawn(async move |this, cx| {
            let done = Arc::new(Mutex::new(None::<Result<(), String>>));
            let done_bg = done.clone();
            let shared_bg = shared.clone();
            let port = port_name.clone();
            let path_c = path.clone();
            std::thread::spawn(move || {
                let r = crate::bsl::flash_bin_file(&port, &path_c, true, bsl_baud, |msg| {
                    let m = msg.to_string();
                    let mut s = shared_bg.lock();
                    s.status = m.clone();
                    if m.starts_with("烧录中") {
                        if let Some(last) = s.logs.last_mut() {
                            if last.kind == LogKind::Sys && last.text.as_ref().starts_with("烧录中")
                            {
                                last.text = m.into();
                                s.log_epoch += 1;
                                return;
                            }
                        }
                    }
                    s.logs.push(LogLine {
                        kind: LogKind::Sys,
                        text: m.into(),
                    });
                    s.log_epoch += 1;
                });
                *done_bg.lock() = Some(r.map_err(|e| format!("{e:#}")));
            });

            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                this.update(cx, |_, cx| cx.notify()).ok();
                if done.lock().is_some() {
                    break;
                }
            }

            let result = done
                .lock()
                .take()
                .unwrap_or_else(|| Err("烧录线程异常".into()));
            match result {
                Err(err) => {
                    this.update(cx, |_, cx| {
                        let mut s = shared.lock();
                        s.busy = false;
                        s.status = "烧录失败".into();
                        s.error = Some(err.clone());
                        s.logs.push(LogLine {
                            kind: LogKind::Err,
                            text: format!("烧录失败: {err}").into(),
                        });
                        s.log_epoch += 1;
                        cx.notify();
                    })
                    .ok();
                }
                Ok(()) => {
                    this.update(cx, |_, cx| {
                        let mut s = shared.lock();
                        s.status = "已发 StartApplication · 等待应用启动…".into();
                        s.logs.push(LogLine {
                            kind: LogKind::Sys,
                            text: format!(
                                "烧录成功 · {path_disp} · 0x40 启动应用 · 重连 {APP_SERIAL_BAUD}"
                            )
                            .into(),
                        });
                        s.log_epoch += 1;
                        cx.notify();
                    })
                    .ok();
                    // BSL thread already slept ~900ms after 0x40; extra settle + open retries.
                    cx.background_executor()
                        .timer(Duration::from_millis(600))
                        .await;

                    let mut open_r: Result<SerialSession, anyhow::Error> =
                        Err(anyhow::anyhow!("未尝试重连"));
                    for attempt in 1..=6 {
                        this.update(cx, |_, cx| {
                            shared.lock().status =
                                format!("重连应用串口 {attempt}/6 @ {APP_SERIAL_BAUD}…");
                            cx.notify();
                        })
                        .ok();
                        let port_re = port_name.clone();
                        let one = {
                            let handle = std::thread::spawn(move || {
                                SerialSession::open(&port_re, APP_SERIAL_BAUD)
                            });
                            loop {
                                cx.background_executor()
                                    .timer(Duration::from_millis(40))
                                    .await;
                                this.update(cx, |_, cx| cx.notify()).ok();
                                if handle.is_finished() {
                                    break;
                                }
                            }
                            handle
                                .join()
                                .unwrap_or_else(|_| Err(anyhow::anyhow!("重连线程异常")))
                        };
                        match one {
                            Ok(s) => {
                                open_r = Ok(s);
                                break;
                            }
                            Err(e) => {
                                open_r = Err(e);
                                cx.background_executor()
                                    .timer(Duration::from_millis(450))
                                    .await;
                            }
                        }
                    }

                    match open_r {
                        Err(err) => {
                            this.update(cx, |_, cx| {
                                let mut s = shared.lock();
                                s.busy = false;
                                s.connected = false;
                                s.session = None;
                                s.status = "烧录完成 · 应用未连上".into();
                                s.logs.push(LogLine {
                                    kind: LogKind::Err,
                                    text: format!(
                                        "重连失败: {err:#} · 已发 StartApplication(0x40)；\
                                         若无启动请手动 RST 后连接（非 BSL 键）"
                                    )
                                    .into(),
                                });
                                s.log_epoch += 1;
                                cx.notify();
                            })
                            .ok();
                        }
                        Ok(session) => {
                            let session = Arc::new(session);
                            this.update(cx, |_, cx| {
                                let mut s = shared.lock();
                                s.session = Some(session.clone());
                                s.connected = true;
                                s.port_name = Some(port_name.clone());
                                s.rx_cursor = 0;
                                s.rx_cleaner.reset();
                                s.status = "检测 Flash (LittleFS)…".into();
                                s.log_epoch += 1;
                                cx.notify();
                            })
                            .ok();
                            // Let the app start, then stop any boot script before probing.
                            let probe = {
                                let sess = session.clone();
                                let handle = std::thread::spawn(move || {
                                    let _ = sess.wait_boot_settle(Duration::from_millis(1500));
                                    sess.stop_and_wait()?;
                                    sess.probe_lfs()
                                });
                                loop {
                                    cx.background_executor()
                                        .timer(Duration::from_millis(40))
                                        .await;
                                    this.update(cx, |_, cx| cx.notify()).ok();
                                    if handle.is_finished() {
                                        break;
                                    }
                                }
                                handle
                                    .join()
                                    .unwrap_or_else(|_| Err(anyhow::anyhow!("探测线程异常")))
                            };
                            this.update(cx, |app, cx| {
                                {
                                    let mut s = shared.lock();
                                    s.busy = false;
                                    match &probe {
                                        Ok(true) => {
                                            s.status = "烧录完成 · Flash 正常".into();
                                            s.logs.push(LogLine {
                                                kind: LogKind::Sys,
                                                text: "应用已启动 · LittleFS 就绪".into(),
                                            });
                                            s.error = None;
                                        }
                                        Ok(false) => {
                                            s.status =
                                                "烧录完成 · Flash 空/未就绪 · 自动初始化…".into();
                                            s.logs.push(LogLine {
                                                kind: LogKind::Sys,
                                                text: "LittleFS 未就绪 · 自动 format".into(),
                                            });
                                        }
                                        Err(err) => {
                                            s.status = "烧录完成 · 应用无响应".into();
                                            s.logs.push(LogLine {
                                                kind: LogKind::Err,
                                                text: format!(
                                                    "{err:#} · 可手动 RST 后连接；\
                                                     目标菜单可「重置 Flash」"
                                                )
                                                .into(),
                                            });
                                        }
                                    }
                                    s.log_epoch += 1;
                                }
                                cx.notify();
                                match &probe {
                                    Ok(true) => app.spawn_list_files(cx),
                                    Ok(false) => {
                                        // Empty / unmounted after mass-erase → auto format.
                                        app.confirm_format_fs(cx);
                                    }
                                    Err(_) => {}
                                }
                            })
                            .ok();
                        }
                    }
                }
            }
        })
        .detach();
    }

    fn spawn_list_files(&self, cx: &mut Context<Self>) {
        let shared = self.shared.clone();
        let session = {
            let s = shared.lock();
            s.session.clone()
        };
        let Some(session) = session else { return };
        if shared.lock().busy {
            return;
        }
        shared.lock().busy = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { session.list_files() })
                .await;
            this.update(cx, |_, cx| {
                let mut s = shared.lock();
                s.busy = false;
                match result {
                    Ok(files) => {
                        let n = files.len();
                        s.files = files;
                        s.logs.push(LogLine {
                            kind: LogKind::Sys,
                            text: format!("Flash 文件 {n} 个").into(),
                        });
                    }
                    Err(err) => {
                        s.logs.push(LogLine {
                            kind: LogKind::Err,
                            text: format!("ls 失败: {err:#}").into(),
                        });
                    }
                }
                s.log_epoch += 1;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn refresh_files(&mut self, _: &RefreshFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.spawn_list_files(cx);
    }

    fn clear_log(&mut self, _: &ClearLog, _: &mut Window, cx: &mut Context<Self>) {
        {
            let mut s = self.shared.lock();
            s.logs.clear();
            s.log_epoch += 1;
        }
        self.console.update(cx, |c, cx| c.clear(cx));
        self.last_console_epoch = self.shared.lock().log_epoch;
        cx.notify();
    }

    fn copy_log(&mut self, _: &CopyLog, _: &mut Window, cx: &mut Context<Self>) {
        let full_output = self.settings.full_output;
        let text = {
            let s = self.shared.lock();
            s.logs
                .iter()
                .flat_map(|line| line.text.lines().map(move |part| (line.kind, part)))
                .filter_map(|(kind, part)| console_display_text(kind, part, full_output))
                .collect::<Vec<_>>()
                .join("\n")
        };
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.shared.lock().status = "已复制".into();
        cx.notify();
    }

    fn copy_project_context(&mut self, _: &CopyProjectContext, _: &mut Window, cx: &mut Context<Self>) {
        self.flush_active_tab(cx);
        let fallback = self.editor.read(cx).text();
        let context = build_project_context(
            self.project_dir.as_deref(),
            self.source_path.as_deref(),
            &fallback,
            self.settings.selected_board.as_deref(),
        );
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(context));
        self.shared.lock().status = "工程上下文已复制".into();
        cx.notify();
    }

    fn send_stop_signal(&self) {
        let session = self.shared.lock().session.clone();
        if let Some(session) = session {
            let _ = session.stop();
        }
    }

    fn stop(&mut self, _: &Stop, _: &mut Window, cx: &mut Context<Self>) {
        let session = self.shared.lock().session.clone();
        if let Some(session) = session {
            if let Err(err) = session.stop() {
                self.push_err(format!("停止失败: {err:#}"), cx);
            } else {
                let mut s = self.shared.lock();
                s.script_running = false;
                s.stop_requested = true;
                s.status = "已停止".into();
                cx.notify();
            }
        }
    }

    fn run(&mut self, _: &Run, window: &mut Window, cx: &mut Context<Self>) {
        // Merged button: if already running after upload OK, act as Stop.
        if self.shared.lock().script_running {
            self.stop(&Stop, window, cx);
            return;
        }
        self.report_analyze(cx);
        self.flush_active_tab(cx);
        self.run_modular_transaction(cx);
    }

    fn new_project(&mut self, _: &NewProject, _: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        // Pick the project folder (create empty folder in dialog if needed).
        let folder = rfd::FileDialog::new()
            .set_title("新建工程 — 选择或创建工程文件夹")
            .pick_folder();
        let Some(dir) = folder else {
            return;
        };
        let proj_name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("MyLuaProject")
            .to_string();
        let initial = project::DEFAULT_SOURCE;
        match project::create_project(&dir, &proj_name, initial) {
            Ok(meta) => {
                let main = project::resolve_main(&dir, &meta);
                let src = project::read_source_file(&main).unwrap_or_else(|_| initial.to_string());
                self.project_dir = Some(dir.clone());
                self.project_meta = meta.clone();
                self.configure_target(&dir, &meta, cx);
                self.target_name = meta.target_luac.clone();
                self.tree_selected = Some(main.clone());
                self.set_source(src, Some(main), cx);
                self.refresh_project_tree(cx);
                self.settings.set_last_project(&dir);
                self.shared.lock().status = format!("工程 · {proj_name}");
                cx.notify();
            }
            Err(err) => self.push_err(format!("新建工程失败: {err:#}"), cx),
        }
    }

    fn open_project(&mut self, _: &OpenProject, _: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        let mut dlg = rfd::FileDialog::new().set_title("打开工程目录");
        if let Some(last) = self.settings.last_project.as_ref() {
            if let Some(parent) = last.parent() {
                dlg = dlg.set_directory(parent);
            }
        }
        let folder = dlg.pick_folder();
        let Some(dir) = folder else {
            return;
        };
        match project::load_project(&dir) {
            Ok(meta) => {
                let main = project::resolve_main(&dir, &meta);
                match project::read_source_file(&main) {
                    Ok(src) => {
                        self.project_dir = Some(dir.clone());
                        self.project_meta = meta.clone();
                        self.configure_target(&dir, &meta, cx);
                        self.target_name = meta.target_luac.clone();
                        self.tree_selected = Some(main.clone());
                        self.set_source(src, Some(main), cx);
                        self.refresh_project_tree(cx);
                        self.settings.set_last_project(&dir);
                        self.shared.lock().status = format!("工程 · {}", meta.name);
                        cx.notify();
                    }
                    Err(err) => self.push_err(format!("读取源文件失败: {err:#}"), cx),
                }
            }
            Err(err) => self.push_err(format!("打开工程失败: {err:#}"), cx),
        }
    }

    fn open_source(&mut self, _: &OpenSource, _: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        let mut dlg = rfd::FileDialog::new()
            .set_title("打开 Lua 源文件")
            .add_filter("Lua", &["lua"])
            .add_filter("All", &["*"]);
        if let Some(dir) = &self.project_dir {
            dlg = dlg.set_directory(dir);
        }
        let Some(path) = dlg.pick_file() else {
            return;
        };
        match project::read_source_file(&path) {
            Ok(src) => {
                // If file is inside current project, keep project; else keep project_dir as-is.
                self.set_source(&src, Some(path.clone()), cx);
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    if name != "main" {
                        self.target_name = format!("{name}.luac");
                    } else {
                        self.target_name = "main.luac".into();
                    }
                }
                self.shared.lock().status = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("已打开")
                    .to_string();
                cx.notify();
            }
            Err(err) => self.push_err(format!("打开失败: {err:#}"), cx),
        }
    }

    fn save_file(&mut self, _: &SaveFile, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        if self.source_path.is_none() {
            self.save_file_as(&SaveFileAs, window, cx);
            return;
        }
        let path = self.source_path.clone().unwrap();
        let text = self.editor.read(cx).text();
        match project::write_source_file(&path, &text) {
            Ok(()) => {
                // Keep project meta in sync if saving project main.
                if let Some(dir) = &self.project_dir {
                    let _ = project::save_project_meta(dir, &self.project_meta);
                }
                self.dirty = false;
                if let Some(tab) = self.open_tabs.get_mut(self.active_tab) {
                    tab.content = text;
                    tab.dirty = false;
                    tab.path = Some(path);
                    tab.title = Self::tab_title_for(&tab.path);
                }
                self.refresh_project_tree(cx);
                self.shared.lock().status = "已保存".into();
                cx.notify();
            }
            Err(err) => self.push_err(format!("保存失败: {err:#}"), cx),
        }
    }

    fn save_file_as(&mut self, _: &SaveFileAs, _: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        let mut dlg = rfd::FileDialog::new()
            .set_title("另存为")
            .add_filter("Lua", &["lua"])
            .set_file_name(
                self.source_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("main.lua"),
            );
        if let Some(dir) = &self.project_dir {
            dlg = dlg.set_directory(dir);
        }
        let Some(path) = dlg.save_file() else {
            return;
        };
        let path = if path.extension().is_none() {
            path.with_extension("lua")
        } else {
            path
        };
        let text = self.editor.read(cx).text();
        match project::write_source_file(&path, &text) {
            Ok(()) => {
                self.source_path = Some(path.clone());
                self.dirty = false;
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    self.target_name = if name == "main" {
                        "main.luac".into()
                    } else {
                        format!("{name}.luac")
                    };
                }
                if let Some(tab) = self.open_tabs.get_mut(self.active_tab) {
                    tab.content = text;
                    tab.dirty = false;
                    tab.path = Some(path.clone());
                    tab.title = Self::tab_title_for(&tab.path);
                    tab.target_name = self.target_name.clone();
                }
                // If no project yet and parent looks usable, create lightweight project meta.
                if self.project_dir.is_none() {
                    if let Some(parent) = path.parent() {
                        let meta = ProjectMeta {
                            name: parent
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("project")
                                .into(),
                            main_source: path
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("main.lua")
                                .into(),
                            target_luac: self.target_name.clone(),
                            ..ProjectMeta::default()
                        };
                        if project::save_project_meta(parent, &meta).is_ok() {
                            self.project_dir = Some(parent.to_path_buf());
                            self.project_meta = meta.clone();
                            self.configure_target(parent, &meta, cx);
                        }
                    }
                }
                self.refresh_project_tree(cx);
                self.shared.lock().status = format!("已另存为 {}", path.display());
                cx.notify();
                cx.notify();
            }
            Err(err) => self.push_err(format!("另存为失败: {err:#}"), cx),
        }
    }

    fn action_close_menu(&mut self, _: &CloseMenu, _: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
    }

    fn download_luac(&mut self, _: &DownloadLuac, _: &mut Window, cx: &mut Context<Self>) {
        let Some(compiler) = self.compiler.clone() else {
            self.push_err("编译器不可用", cx);
            return;
        };
        let source = self.editor.read(cx).text();
        let name = match ensure_luac_name(&self.target_name) {
            Ok(n) => n,
            Err(err) => {
                self.push_err(err.to_string(), cx);
                return;
            }
        };
        let mut dlg = rfd::FileDialog::new()
            .set_title("保存 .luac")
            .set_file_name(&name)
            .add_filter("Lua bytecode", &["luac"])
            .add_filter("All", &["*"]);
        if let Some(dir) = self.settings.download_dir.as_ref() {
            if dir.is_dir() {
                dlg = dlg.set_directory(dir);
            }
        } else if let Some(dir) = self.project_dir.as_ref() {
            dlg = dlg.set_directory(dir);
        }
        let Some(path) = dlg.save_file() else {
            return;
        };
        if let Some(parent) = path.parent() {
            self.settings.set_download_dir(parent);
        }
        match compile_source(&compiler, &source) {
            Ok(bytes) => {
                if let Err(err) = std::fs::write(&path, &bytes) {
                    self.push_err(format!("写入失败: {err}"), cx);
                    return;
                }
                self.shared.lock().status =
                    format!("已保存 {} B · {}", bytes.len(), path.display());
                cx.notify();
            }
            Err(err) => self.push_err(format!("编译失败: {err:#}"), cx),
        }
    }

    fn open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.dialog = Some(DialogKind::Settings);
        cx.notify();
    }

    fn open_about(&mut self, _: &OpenAbout, _: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.dialog = Some(DialogKind::About);
        cx.notify();
    }

    fn open_keys(&mut self, _: &OpenKeys, _: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.dialog = Some(DialogKind::Keys);
        cx.notify();
    }

    fn offer_format_fs(&mut self, detail: impl Into<String>, cx: &mut Context<Self>) {
        self.format_fs_detail = detail.into();
        self.dialog = Some(DialogKind::FormatFs);
        cx.notify();
    }

    fn confirm_format_fs(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        let session = self.shared.lock().session.clone();
        let Some(session) = session else {
            self.push_err("请先连接串口", cx);
            return;
        };
        if self.shared.lock().busy {
            self.push_err("忙，请稍候", cx);
            return;
        }
        {
            let mut s = self.shared.lock();
            s.busy = true;
            s.script_running = false;
            s.status = "重置 Flash (LittleFS)…".into();
        }
        cx.notify();
        let shared = self.shared.clone();
        // Blocking serial work on OS thread (not GPUI executor) — avoids UI stall.
        cx.spawn(async move |this, cx| {
            let result = {
                let handle = std::thread::spawn(move || session.format_lfs());
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(40))
                        .await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                    if handle.is_finished() {
                        break;
                    }
                }
                handle
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("format 线程异常")))
            };
            let ok = matches!(&result, Ok(_));
            this.update(cx, |app, cx| {
                {
                    let mut s = shared.lock();
                    s.busy = false;
                    match &result {
                        Ok(cap) => {
                            s.status = if *cap > 0 {
                                format!("Flash 已重置 · {cap} B")
                            } else {
                                "Flash 已重置".into()
                            };
                            s.logs.push(LogLine {
                                kind: LogKind::Sys,
                                text: if *cap > 0 {
                                    format!("LittleFS format 完成 · 容量 {cap} B · 请重新运行")
                                        .into()
                                } else {
                                    "LittleFS format 完成 · 请重新运行".into()
                                },
                            });
                            s.error = None;
                            s.files.clear();
                        }
                        Err(err) => {
                            s.status = "重置 Flash 失败".into();
                            s.error = Some(format!("{err:#}"));
                            s.logs.push(LogLine {
                                kind: LogKind::Err,
                                text: format!("format 失败: {err:#}").into(),
                            });
                        }
                    }
                    s.log_epoch += 1;
                }
                // Drop shared lock before spawn_list_files (re-locks) — was UI deadlock.
                cx.notify();
                if ok {
                    app.spawn_list_files(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn menu_format_fs(&mut self, cx: &mut Context<Self>) {
        self.open_menu = None;
        if !self.shared.lock().connected {
            self.push_err("请先连接串口", cx);
            return;
        }
        self.offer_format_fs("手动重置：将清空 SPI Flash 上全部脚本文件。", cx);
    }

    fn new_lua_in_project(&mut self, under: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(proj) = self.project_dir.clone() else {
            self.push_err("请先打开工程", cx);
            return;
        };
        let base = match under {
            Some(p) if p.is_dir() => p,
            Some(p) => p.parent().map(|x| x.to_path_buf()).unwrap_or(proj.clone()),
            None => proj.clone(),
        };
        // Unique default name untitled.lua / untitled2.lua …
        let mut n = 0u32;
        let path = loop {
            let name = if n == 0 {
                "untitled.lua".to_string()
            } else {
                format!("untitled{n}.lua")
            };
            let p = base.join(&name);
            if !p.exists() {
                break p;
            }
            n += 1;
            if n > 999 {
                self.push_err("无法生成文件名", cx);
                return;
            }
        };
        if let Err(err) = project::write_source_file(&path, "-- \n") {
            self.push_err(format!("创建失败: {err:#}"), cx);
            return;
        }
        self.tree_ctx = None;
        self.tree_ctx_sort = false;
        self.refresh_project_tree(cx);
        self.tree_selected = Some(path.clone());
        // Inline name edit (same UI as rename).
        self.begin_rename_inner(path, true, cx);
    }

    fn begin_rename(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.begin_rename_inner(path, false, cx);
    }

    fn begin_rename_inner(&mut self, path: PathBuf, is_new: bool, cx: &mut Context<Self>) {
        self.tree_ctx = None;
        self.tree_ctx_sort = false;
        if self.project_dir.as_ref() == Some(&path) {
            self.push_err("不能重命名工程根目录", cx);
            return;
        }
        let old_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();
        self.rename_path = Some(path);
        self.rename_is_new = is_new;
        self.rename_focus = true;
        let t = self.theme;
        self.rename_input.update(cx, |inp, cx| {
            inp.set_theme(t, cx);
            inp.set_text(old_name, cx);
        });
        cx.notify();
    }

    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        let path = self.rename_path.take();
        let was_new = self.rename_is_new;
        self.rename_is_new = false;
        self.rename_focus = false;
        // Undo create if user cancelled naming a brand-new file.
        if was_new {
            if let Some(p) = path {
                let _ = std::fs::remove_file(&p);
                self.refresh_project_tree(cx);
            }
        }
        cx.notify();
    }

    fn confirm_rename(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.rename_path.clone() else {
            cx.notify();
            return;
        };
        let is_new = self.rename_is_new;
        let mut name = self.rename_input.read(cx).text().trim().to_string();
        if name.is_empty() {
            self.push_err("名称不能为空", cx);
            return;
        }
        if name.contains('/') || name.contains('\\') || name.contains(':') || name.contains('*') {
            self.push_err("名称含非法字符", cx);
            return;
        }
        // New file: auto-append .lua if no extension.
        if is_new && !name.contains('.') {
            name.push_str(".lua");
        }
        let Some(parent) = path.parent() else {
            self.push_err("无效路径", cx);
            return;
        };
        let new_path = parent.join(&name);
        if new_path == path {
            self.rename_path = None;
            self.rename_is_new = false;
            self.rename_focus = false;
            if is_new {
                self.open_tree_entry(path, cx);
            }
            cx.notify();
            return;
        }
        if new_path.exists() {
            self.push_err("目标已存在", cx);
            return;
        }
        if let Err(err) = std::fs::rename(&path, &new_path) {
            self.push_err(format!("重命名失败: {err}"), cx);
            return;
        }
        self.rename_path = None;
        self.rename_is_new = false;
        self.rename_focus = false;
        if self.source_path.as_ref() == Some(&path) {
            self.source_path = Some(new_path.clone());
        }
        for tab in self.open_tabs.iter_mut() {
            if tab.path.as_ref() == Some(&path) {
                tab.path = Some(new_path.clone());
                tab.title = Self::tab_title_for(&tab.path);
            } else if let Some(ref tp) = tab.path {
                if tp.starts_with(&path) {
                    if let Ok(rel) = tp.strip_prefix(&path) {
                        tab.path = Some(new_path.join(rel));
                        tab.title = Self::tab_title_for(&tab.path);
                    }
                }
            }
        }
        self.tree_selected = Some(new_path.clone());
        self.refresh_project_tree(cx);
        if is_new {
            self.open_tree_entry(new_path, cx);
            self.shared.lock().status = "已新建".into();
        } else {
            self.shared.lock().status = "已重命名".into();
        }
        cx.notify();
    }

    fn begin_delete(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.tree_ctx = None;
        self.tree_ctx_sort = false;
        if self.project_dir.as_ref() == Some(&path) {
            self.push_err("不能删除工程根目录", cx);
            return;
        }
        self.dialog_path = Some(path);
        self.dialog = Some(DialogKind::DeleteConfirm);
        cx.notify();
    }

    fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.dialog_path.take() else {
            self.dialog = None;
            cx.notify();
            return;
        };
        self.dialog = None;
        if path.is_dir() {
            if let Err(err) = std::fs::remove_dir_all(&path) {
                self.push_err(format!("删除失败: {err}"), cx);
                return;
            }
        } else if let Err(err) = std::fs::remove_file(&path) {
            self.push_err(format!("删除失败: {err}"), cx);
            return;
        }
        let to_close: Vec<usize> = self
            .open_tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                t.path
                    .as_ref()
                    .map(|p| p == &path || p.starts_with(&path))
                    .unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect();
        for i in to_close.into_iter().rev() {
            self.close_tab_at(i, cx);
        }
        if self.source_path.as_ref() == Some(&path)
            || self
                .source_path
                .as_ref()
                .map(|p| p.starts_with(&path))
                .unwrap_or(false)
        {
            self.source_path = self
                .open_tabs
                .get(self.active_tab)
                .and_then(|t| t.path.clone());
        }
        if self.tree_selected.as_ref() == Some(&path) {
            self.tree_selected = None;
        }
        self.refresh_project_tree(cx);
        self.shared.lock().status = "已删除".into();
        cx.notify();
    }

    fn open_in_explorer(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let target = if path.is_dir() {
            path
        } else {
            path.parent().map(|p| p.to_path_buf()).unwrap_or(path)
        };
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("explorer")
                .arg(target.as_os_str())
                .spawn();
        }
        #[cfg(not(windows))]
        {
            let _ = target;
        }
        cx.notify();
    }

    fn run_modular_transaction(&mut self, cx: &mut Context<Self>) {
        self.flush_active_tab(cx);
        if self.target_profile.is_none() {
            self.push_err("目标元数据未加载；请修复 Chip/Board/API 文件后再运行", cx);
            return;
        }
        let Some(compiler) = self.compiler.clone() else {
            self.push_err("编译器不可用", cx);
            return;
        };
        let session = {
            let state = self.shared.lock();
            if state.busy {
                self.push_err("忙，请稍候", cx);
                return;
            }
            state.session.clone()
        };
        let Some(session) = session else {
            self.push_err("请先连接串口", cx);
            return;
        };
        let fallback_source = self.editor.read(cx).text();
        let project_dir = self.project_dir.clone();
        let project_meta = self.project_meta.clone();
        let overlays: Vec<(PathBuf, String)> = self
            .open_tabs
            .iter()
            .filter_map(|tab| {
                tab.path
                    .as_ref()
                    .map(|path| (path.clone(), tab.content.clone()))
            })
            .collect();
        let shared = self.shared.clone();
        let run_cache = self.run_cache.clone();
        let transfer_mode = self.settings.transfer_mode;
        let font_root = metadata::data_root().ok().map(|root| root.join("font"));
        let font_zh = font_root
            .as_ref()
            .map(|root| {
                root.join(&self.settings.font_zh)
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| self.settings.font_zh.clone());
        let font_en = font_root
            .as_ref()
            .map(|root| {
                root.join(&self.settings.font_en)
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| self.settings.font_en.clone());
        let progress = Arc::new(Mutex::new(String::new()));
        {
            let mut state = shared.lock();
            state.busy = true;
            state.script_running = false;
            state.stop_requested = false;
            state.error = None;
            state.status = "校验模块化工程…".into();
        }
        cx.notify();

        let session_for_result = session.clone();
        cx.spawn(async move |this, cx| {
            let progress_thread = progress.clone();
            let handle = std::thread::spawn(move || {
                execute_modular_run(
                    &compiler,
                    session,
                    project_dir.as_deref(),
                    &project_meta,
                    &overlays,
                    &fallback_source,
                    transfer_mode,
                    &run_cache,
                    &font_zh,
                    &font_en,
                    |message| *progress_thread.lock() = message.to_string(),
                )
            });
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let message = progress.lock().clone();
                if !message.is_empty() {
                    shared.lock().status = message;
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
                if handle.is_finished() {
                    break;
                }
            }
            let result = handle
                .join()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("模块化运行线程异常")));
            this.update(cx, |app, cx| {
                let mut state = shared.lock();
                state.busy = false;
                match result {
                    Ok(summary) => {
                        let script_finished = session_for_result.rx_snapshot().contains("SCRIPT_DONE")
                            || session_for_result.rx_snapshot().contains("LUA stopped");
                        state.script_running = !script_finished;
                        state.status = format!(
                            "运行中 · {} Lua · {} B · {}",
                            summary.script_count,
                            summary.lua_bytes,
                            if summary.modules_updated {
                                "原生模块已更新"
                            } else {
                                "原生模块未变化"
                            }
                        );
                        state.logs.push(LogLine {
                            kind: LogKind::Sys,
                            text: format!(
                                "模块化运行已启动 · [{}] · NMUP {} · {} Lua / {} B · 点击停止结束",
                                summary.modules.join(", "),
                                &summary.bundle_sha256[..12],
                                summary.script_count,
                                summary.lua_bytes
                            )
                            .into(),
                        });
                        state.log_epoch += 1;
                        if script_finished {
                            state.status = "Completed".into();
                        }
                    }
                    Err(error) => {
                        state.script_running = false;
                        let message = format!("{error:#}");
                        state.status = format!("运行失败: {message}");
                        state.logs.push(LogLine {
                            kind: LogKind::Err,
                            text: format!("模块化运行失败: {message}").into(),
                        });
                        state.log_epoch += 1;
                        if is_lfs_error(&message) {
                            drop(state);
                            app.offer_format_fs(message, cx);
                            return;
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn compile_and_upload(&mut self, name: &str, wait_done: bool, cx: &mut Context<Self>) {
        let Some(compiler) = self.compiler.clone() else {
            self.push_err("编译器不可用", cx);
            return;
        };
        let session = {
            let s = self.shared.lock();
            if s.busy {
                self.push_err("忙，请稍候", cx);
                return;
            }
            s.session.clone()
        };
        let Some(session) = session else {
            self.push_err("请先连接串口", cx);
            return;
        };
        let source = self.editor.read(cx).text();
        let name = name.to_string();
        let shared = self.shared.clone();
        {
            let mut s = shared.lock();
            s.busy = true;
            s.script_running = false;
            s.status = "编译中…".into();
            s.error = None;
        }
        cx.notify();

        cx.spawn(async move |this, cx| {
            // After ! stop, board prints Idle then re-arms UART; wait before HEX.
            cx.background_executor()
                .timer(Duration::from_millis(350))
                .await;

            // Probe LittleFS before compile/upload — offer format early.
            {
                let sess = session.clone();
                let probe = {
                    let handle = std::thread::spawn(move || sess.probe_lfs());
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(40))
                            .await;
                        this.update(cx, |_, cx| cx.notify()).ok();
                        if handle.is_finished() {
                            break;
                        }
                    }
                    handle
                        .join()
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("探测线程异常")))
                };
                match probe {
                    Ok(true) => {}
                    Ok(false) => {
                        this.update(cx, |app, cx| {
                            let mut s = shared.lock();
                            s.busy = false;
                            s.script_running = false;
                            s.status = "Flash 未就绪".into();
                            s.logs.push(LogLine {
                                kind: LogKind::Err,
                                text: "上传前检测：LittleFS 未挂载".into(),
                            });
                            s.log_epoch += 1;
                            app.offer_format_fs(
                                "上传前检测失败：LittleFS 未就绪（SCRIPT_ERR name/fs）。是否重置 Flash？",
                                cx,
                            );
                        })
                        .ok();
                        return;
                    }
                    Err(err) => {
                        this.update(cx, |app, cx| {
                            let mut s = shared.lock();
                            s.busy = false;
                            s.script_running = false;
                            s.status = format!("Flash 探测失败: {err:#}");
                            s.logs.push(LogLine {
                                kind: LogKind::Err,
                                text: format!("上传前探测失败: {err:#}").into(),
                            });
                            s.log_epoch += 1;
                            app.offer_format_fs(
                                format!("探测异常: {err:#} · 若确认 Flash 异常可重置"),
                                cx,
                            );
                        })
                        .ok();
                        return;
                    }
                }
            }

            let compile_result = cx
                .background_executor()
                .spawn({
                    let compiler = compiler.clone();
                    let source = source.clone();
                    async move { compile_source(&compiler, &source) }
                })
                .await;

            let bytes = match compile_result {
                Ok(b) => b,
                Err(err) => {
                    this.update(cx, |_, cx| {
                        let mut s = shared.lock();
                        s.busy = false;
                        s.script_running = false;
                        s.status = format!("编译失败: {err:#}");
                        s.logs.push(LogLine {
                            kind: LogKind::Err,
                            text: format!("编译失败: {err:#}").into(),
                        });
                        s.log_epoch += 1;
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };

            // IDE fonts: 16x16 for oled.text (CJK+digits via 黑体); optional 6x8 for oled.print.
            let f16 = match fontpack::analyze_and_pack_f16(&source) {
                Ok(v) => v,
                Err(err) => {
                    this.update(cx, |_, cx| {
                        let mut s = shared.lock();
                        s.busy = false;
                        s.script_running = false;
                        s.status = format!("字模错误: {err}");
                        s.logs.push(LogLine {
                            kind: LogKind::Err,
                            text: format!("字模错误: {err}").into(),
                        });
                        s.log_epoch += 1;
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            let (f16_msg, f16_pack) = f16;
            let (font_codes, font_pack) = fontpack::analyze_and_pack(&source);
            let font_n = font_codes.len();

            this.update(cx, |_, cx| {
                let mut s = shared.lock();
                s.status = format!("编译 OK · {} B · 上传中…", bytes.len());
                if !f16_pack.is_empty() {
                    s.logs.push(LogLine {
                        kind: LogKind::Sys,
                        text: f16_msg.clone().into(),
                    });
                    s.log_epoch += 1;
                }
                if font_n > 0 {
                    s.logs.push(LogLine {
                        kind: LogKind::Sys,
                        text: format!("OLED 6x8: {font_n} 字 → _run.fnt").into(),
                    });
                    s.log_epoch += 1;
                }
                cx.notify();
            })
            .ok();

            // Phase 0a: 16x16 pack (_run.f16) — hard fail on upload error
            if !f16_pack.is_empty() {
                let session_f = session.clone();
                let pack = f16_pack;
                let progress_f = Arc::new(Mutex::new(String::new()));
                let shared_f = shared.clone();
                let pf = progress_f.clone();
                let handle_f = std::thread::spawn(move || {
                    let r = session_f.upload_hex_with_progress("_run.f16", &pack, |msg| {
                        *pf.lock() = msg.to_string();
                    });
                    *pf.lock() = "__done__".into();
                    r
                });
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(40))
                        .await;
                    let msg = progress_f.lock().clone();
                    if !msg.is_empty() && msg != "__done__" {
                        shared_f.lock().status = format!("字模16 {msg}");
                        this.update(cx, |_, cx| cx.notify()).ok();
                    }
                    if handle_f.is_finished() {
                        break;
                    }
                }
                if let Err(err) = handle_f
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("_run.f16 线程异常")))
                {
                    let msg = format!("{err:#}");
                    let lfs = is_lfs_error(&msg);
                    this.update(cx, |app, cx| {
                        let mut s = shared.lock();
                        s.busy = false;
                        s.script_running = false;
                        s.status = format!("_run.f16 上传失败: {msg}");
                        s.logs.push(LogLine {
                            kind: LogKind::Err,
                            text: format!("_run.f16 上传失败: {msg}").into(),
                        });
                        s.log_epoch += 1;
                        if lfs {
                            app.offer_format_fs(msg, cx);
                        } else {
                            cx.notify();
                        }
                    })
                    .ok();
                    return;
                }
            }
            // Phase 0b: optional 6x8 (_run.fnt) — soft fail unless LFS broken
            if !font_pack.is_empty() {
                let session_f = session.clone();
                let pack = font_pack;
                let progress_f = Arc::new(Mutex::new(String::new()));
                let shared_f = shared.clone();
                let pf = progress_f.clone();
                let handle_f = std::thread::spawn(move || {
                    let r = session_f.upload_hex_with_progress("_run.fnt", &pack, |msg| {
                        *pf.lock() = msg.to_string();
                    });
                    *pf.lock() = "__done__".into();
                    r
                });
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(40))
                        .await;
                    let msg = progress_f.lock().clone();
                    if !msg.is_empty() && msg != "__done__" {
                        shared_f.lock().status = format!("字模6 {msg}");
                        this.update(cx, |_, cx| cx.notify()).ok();
                    }
                    if handle_f.is_finished() {
                        break;
                    }
                }
                if let Err(err) = handle_f
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("_run.fnt 线程异常")))
                {
                    let msg = format!("{err:#}");
                    let lfs = is_lfs_error(&msg);
                    this.update(cx, |app, cx| {
                        let mut s = shared.lock();
                        s.logs.push(LogLine {
                            kind: LogKind::Err,
                            text: format!("_run.fnt 上传失败(继续): {msg}").into(),
                        });
                        s.log_epoch += 1;
                        if lfs {
                            s.busy = false;
                            s.script_running = false;
                            s.status = format!("_run.fnt 失败: {msg}");
                            app.offer_format_fs(msg, cx);
                        } else {
                            cx.notify();
                        }
                    })
                    .ok();
                    if lfs {
                        return;
                    }
                }
            }

            // Phase 1: upload — success ends at SCRIPT_OK; progress → status only.
            let progress = Arc::new(Mutex::new(String::new()));
            let upload_result = {
                let session = session.clone();
                let name = name.clone();
                let bytes = bytes.clone();
                let progress_bg = progress.clone();
                let shared_ui = shared.clone();
                // Run upload on background thread so we can pump status from this async task.
                let handle = std::thread::spawn(move || {
                    let r = session.upload_hex_with_progress(&name, &bytes, |msg| {
                        *progress_bg.lock() = msg.to_string();
                    });
                    *progress_bg.lock() = "__done__".into();
                    r.map(|_| bytes.len())
                });
                // Poll progress until upload thread finishes.
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(40))
                        .await;
                    let msg = progress.lock().clone();
                    if !msg.is_empty() && msg != "__done__" {
                        shared_ui.lock().status = msg;
                        this.update(cx, |_, cx| cx.notify()).ok();
                    }
                    if handle.is_finished() {
                        break;
                    }
                }
                handle.join().unwrap_or_else(|_| Err(anyhow::anyhow!("上传线程异常")))
            };

            let nbytes = match upload_result {
                Ok(n) => n,
                Err(err) => {
                    let msg = format!("{err:#}");
                    let lfs = is_lfs_error(&msg);
                    this.update(cx, |app, cx| {
                        let mut s = shared.lock();
                        s.busy = false;
                        let snap = s
                            .session
                            .as_ref()
                            .map(|sess| sess.rx_snapshot())
                            .unwrap_or_default();
                        let ran = snap.contains("SCRIPT_OK")
                            || snap.contains("SCRIPT_DONE")
                            || snap.contains("LED_BLINK");
                        if ran && wait_done && name == "main.luac" {
                            s.script_running = true;
                            s.status = format!("{name} 已运行（ACK 不完整）");
                            cx.notify();
                        } else {
                            s.script_running = false;
                            s.status = format!("上传失败: {msg}");
                            s.logs.push(LogLine {
                                kind: LogKind::Err,
                                text: format!("上传失败: {msg}").into(),
                            });
                            s.log_epoch += 1;
                            if lfs {
                                app.offer_format_fs(msg, cx);
                            } else {
                                cx.notify();
                            }
                        }
                    })
                    .ok();
                    return;
                }
            };

            // Upload OK → switch Run button to Stop immediately.
            this.update(cx, |_, cx| {
                let mut s = shared.lock();
                s.busy = false;
                if wait_done && name == "main.luac" {
                    s.script_running = true;
                    s.status = format!("运行中 · {name} · {nbytes} B");
                } else {
                    s.script_running = false;
                    s.status = format!("已上传 {name} · {nbytes} B");
                }
                cx.notify();
            })
            .ok();

            // Phase 2/3: wait script settle + refresh list (best-effort).
            if wait_done && name == "main.luac" {
                let mark = session.rx_snapshot().len().saturating_sub(256);
                let _ = cx
                    .background_executor()
                    .spawn({
                        let session = session.clone();
                        async move {
                            let _ = session.wait_for_any(
                                &["SCRIPT_DONE", "LED_BLINK_DONE", "stopped"],
                                Duration::from_secs(300),
                                mark,
                            );
                        }
                    })
                    .await;
                this.update(cx, |_, cx| {
                    let mut s = shared.lock();
                    if s.script_running {
                        s.script_running = false;
                        s.status = format!("完成 · {name}");
                    }
                    cx.notify();
                })
                .ok();
            }

            let files = cx
                .background_executor()
                .spawn({
                    let session = session.clone();
                    async move { session.list_files().unwrap_or_default() }
                })
                .await;
            if !files.is_empty() {
                this.update(cx, |_, cx| {
                    shared.lock().files = files;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Open a project from `<exe>/example/<name>/`.
    fn open_example_project(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.open_menu = None;
        if !dir.is_dir() {
            self.push_err(format!("示例不存在: {}", dir.display()), cx);
            return;
        }
        let meta = match project::load_project(&dir) {
            Ok(m) => m,
            Err(_) => {
                // Lightweight project if only .lua files.
                let name = dir
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("example")
                    .to_string();
                ProjectMeta {
                    name: name.clone(),
                    main_source: "main.lua".into(),
                    target_luac: "main.luac".into(),
                    ..ProjectMeta::default()
                }
            }
        };
        let main = project::resolve_main(&dir, &meta);
        let src = if main.is_file() {
            project::read_source_file(&main).unwrap_or_default()
        } else {
            // First .lua in folder.
            fs::read_dir(&dir)
                .ok()
                .and_then(|rd| {
                    rd.flatten()
                        .map(|e| e.path())
                        .find(|p| {
                            p.extension()
                                .and_then(|x| x.to_str())
                                .map(|x| x.eq_ignore_ascii_case("lua"))
                                .unwrap_or(false)
                        })
                        .and_then(|p| project::read_source_file(&p).ok())
                })
                .unwrap_or_default()
        };
        let open_path = if main.is_file() {
            main.clone()
        } else {
            dir.join("main.lua")
        };
        self.project_dir = Some(dir.clone());
        self.project_meta = meta.clone();
        self.configure_target(&dir, &meta, cx);
        self.target_name = meta.target_luac.clone();
        self.tree_selected = Some(open_path.clone());
        self.open_tabs.clear();
        self.set_source(src, Some(open_path), cx);
        self.refresh_project_tree(cx);
        // Do not overwrite last_project with example path unless user saves elsewhere.
        self.shared.lock().status = format!("示例 · {}", meta.name);
        cx.notify();
    }

    /// Pre-read buffer: only surface real errors (no tip spam).
    fn report_analyze(&self, cx: &mut Context<Self>) {
        let issues = self.editor.read(cx).analyze();
        for it in issues.iter().take(8) {
            if it.severity == "error" {
                self.push_diag(format!("L{} · {}", it.line, it.message), cx);
            }
        }
    }

    fn menu_item(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        shortcut: impl Into<SharedString>,
        enabled: bool,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let theme = self.theme;
        let id: SharedString = id.into();
        let label: SharedString = label.into();
        let shortcut: SharedString = shortcut.into();
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .min_w(px(220.))
            .px_3()
            .py_1p5()
            .text_sm()
            .text_color(if enabled { theme.text } else { theme.muted })
            .when(enabled, |el| {
                el.cursor_pointer()
                    .hover(|s| s.bg(theme.accent_soft))
                    .on_click(on_click)
            })
            .child(label)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted)
                    .font_family("Cascadia Code")
                    .ml_6()
                    .child(shortcut),
            )
    }

    fn menu_separator(&self) -> impl IntoElement {
        div().h_px().w_full().my_1().bg(self.theme.line)
    }

    fn menu_tab(
        &self,
        id: MenuId,
        title: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.theme;
        let open = self.open_menu == Some(id);
        div()
            .id(SharedString::from(format!("menu-tab-{title}")))
            .h_full()
            .px_3()
            .flex()
            .items_center()
            .text_sm()
            .cursor_pointer()
            .occlude()
            .bg(if open {
                theme.accent_soft
            } else {
                theme.titlebar
            })
            .text_color(if open { theme.blue } else { theme.text })
            .hover(|s| s.bg(theme.menu_hover).text_color(theme.text))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_menu(id, cx);
            }))
            .child(title)
    }

    /// Windows caption button (Zed-style): hit-test via WindowControlArea.
    fn caption_btn(
        id: &'static str,
        icon: &'static str,
        area: WindowControlArea,
        theme: Theme,
        is_close: bool,
    ) -> impl IntoElement {
        let hover_bg = if is_close {
            rgb(0xe81120).into()
        } else {
            theme.panel2
        };
        let hover_fg = if is_close {
            rgb(0xffffff).into()
        } else {
            theme.text
        };
        div()
            .id(id)
            .w(px(46.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .window_control_area(area)
            .font_family("Segoe MDL2 Assets")
            .text_size(px(10.))
            .text_color(theme.muted)
            .hover(|s| s.bg(hover_bg).text_color(hover_fg))
            .child(icon)
    }

    fn rebuild_theme(&mut self, cx: &mut Context<Self>) {
        let id = self.settings.theme;
        let palette = self.settings.palette(id).clone();
        self.theme = Theme::resolve(id, Some(&palette));
        let t = self.theme;
        self.editor.update(cx, |ed, cx| ed.set_theme(t, cx));
        self.console.update(cx, |c, cx| c.set_theme(t, cx));
        self.telemetry
            .update(cx, |scope, cx| scope.set_theme(t, cx));
        if let Some(handle) = self.telemetry_window {
            let _ = handle.update(cx, |view, _, cx| {
                view.theme = t;
                cx.notify();
            });
        }
        self.rename_input.update(cx, |inp, cx| inp.set_theme(t, cx));
        self.console_search
            .update(cx, |inp, cx| inp.set_theme(t, cx));
        self.color_wheel.update(cx, |w, cx| w.set_theme(t, cx));
        cx.notify();
    }

    fn apply_theme(&mut self, id: ThemeId, cx: &mut Context<Self>) {
        self.settings.set_theme(id);
        self.rebuild_theme(cx);
        self.sync_wheel_from_settings(cx);
    }

    fn cycle_theme_mode(&mut self, cx: &mut Context<Self>) {
        self.apply_theme(self.settings.theme.next(), cx);
    }

    fn reset_current_theme(&mut self, cx: &mut Context<Self>) {
        let id = self.settings.theme;
        self.settings.reset_theme_palette(id);
        self.rebuild_theme(cx);
        self.sync_wheel_from_settings(cx);
    }

    fn sync_wheel_from_settings(&mut self, cx: &mut Context<Self>) {
        let part = self.color_wheel.read(cx).part;
        let pal = self.settings.current_palette();
        // Prefer stored override; else show current resolved theme color.
        let rgb_u = match part {
            ColorPart::Accent => pal.accent.unwrap_or_else(|| hsla_to_u32(self.theme.blue)),
            ColorPart::Bg => pal.bg.unwrap_or_else(|| hsla_to_u32(self.theme.bg)),
            ColorPart::Panel => pal.panel.unwrap_or_else(|| hsla_to_u32(self.theme.panel)),
            ColorPart::Code => pal.code.unwrap_or_else(|| hsla_to_u32(self.theme.code)),
            ColorPart::Text => pal.text.unwrap_or_else(|| hsla_to_u32(self.theme.text)),
        };
        let (h, s, l) = rgb_to_hsla(rgb_u);
        self.color_wheel.update(cx, |w, cx| w.set_hsla(h, s, l, cx));
        self.last_wheel_rev = self.color_wheel.read(cx).revision;
        self.last_wheel_part = part;
    }

    fn poll_color_wheel(&mut self, cx: &mut Context<Self>) {
        let (rev, part, rgb_u) = self
            .color_wheel
            .update(cx, |w, _| (w.revision, w.part, w.current_rgb()));
        if part != self.last_wheel_part {
            self.last_wheel_part = part;
            self.sync_wheel_from_settings(cx);
            return;
        }
        if rev == self.last_wheel_rev {
            return;
        }
        self.last_wheel_rev = rev;
        let key = match part {
            ColorPart::Accent => "accent",
            ColorPart::Bg => "bg",
            ColorPart::Panel => "panel",
            ColorPart::Code => "code",
            ColorPart::Text => "text",
        };
        // Write only into the *current* theme's palette — never bleed across themes.
        let id = self.settings.theme;
        self.settings.patch_palette_part(id, key, rgb_u);
        self.rebuild_theme(cx);
    }

    fn persist_layout(&mut self) {
        self.settings.set_show_sidebar(self.show_sidebar);
        self.settings.set_show_console(self.show_console);
        if self.show_sidebar {
            self.settings.set_sidebar_width(self.sidebar_width);
        }
        if self.show_console {
            self.settings.set_console_height(self.console_height);
        }
    }

    fn on_split_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((origin_x, origin_w)) = self.drag_sidebar {
            let x = f32::from(event.position.x);
            let mut w = origin_w + (x - origin_x);
            if w < HIDE_SIDEBAR_W {
                self.show_sidebar = false;
                self.drag_sidebar = None;
                self.persist_layout();
                cx.notify();
                return;
            }
            w = w.clamp(MIN_SIDEBAR_W, MAX_SIDEBAR_W);
            self.sidebar_width = w;
            cx.notify();
        }
        if let Some((origin_y, origin_h)) = self.drag_console {
            let y = f32::from(event.position.y);
            // Dragging the top edge of console: move up → taller.
            let mut h = origin_h + (origin_y - y);
            if h < HIDE_CONSOLE_H {
                self.show_console = false;
                self.drag_console = None;
                self.persist_layout();
                cx.notify();
                return;
            }
            h = h.clamp(MIN_CONSOLE_H, MAX_CONSOLE_H);
            self.console_height = h;
            cx.notify();
        }
        if let Some((origin_x, origin_w)) = self.drag_telemetry {
            let x = f32::from(event.position.x);
            self.telemetry_width = (origin_w + origin_x - x).clamp(340.0, 720.0);
            cx.notify();
        }
    }

    fn on_split_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.drag_sidebar.is_some()
            || self.drag_console.is_some()
            || self.drag_telemetry.is_some()
        {
            self.drag_sidebar = None;
            self.drag_console = None;
            self.drag_telemetry = None;
            self.persist_layout();
            cx.notify();
        }
    }

    fn on_telemetry_drag_out(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.telemetry_dragging {
            self.show_telemetry_window(cx);
        }
    }

    fn cycle_theme(&mut self, _: &CycleTheme, _window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_theme_mode(cx);
    }

    fn theme_dark(&mut self, _: &ThemeDark, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_theme(ThemeId::Dark, cx);
    }

    fn theme_light(&mut self, _: &ThemeLight, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_theme(ThemeId::Light, cx);
    }

    fn render_titlebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let max_icon = if window.is_maximized() {
            "\u{e923}" // restore
        } else {
            "\u{e922}" // maximize
        };
        // Use native non-client hit testing so Windows owns the move/snap loop.
        div()
            .id("titlebar")
            .flex()
            .items_center()
            .w_full()
            .h(px(36.))
            .bg(theme.titlebar)
            .border_b_1()
            .border_color(theme.line)
            .text_color(theme.text)
            .child(
                div()
                    .id("titlebar-menus")
                    .flex()
                    .items_center()
                    .h_full()
                    .pl_1()
                    .gap_0()
                    .bg(theme.titlebar)
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.menu_tab(MenuId::File, "文件", cx))
                    .child(self.menu_tab(MenuId::Example, "示例", cx))
                    .child(self.menu_tab(MenuId::Board, "开发板", cx))
                    .child(self.menu_tab(MenuId::Run, "运行", cx))
                    .child(self.menu_tab(MenuId::Device, "设备", cx))
                    .child(self.menu_tab(MenuId::View, "数据", cx))
                    .child(self.menu_tab(MenuId::Help, "帮助", cx)),
            )
            .child(
                div()
                    .id("titlebar-drag")
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .min_w(px(48.))
                    .bg(theme.titlebar)
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted)
                            .truncate()
                            .child(self.window_title()),
                    ),
            )
            .child(
                div()
                    .id("titlebar-theme")
                    .h_full()
                    .w(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.titlebar)
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .id("theme-cycle")
                            .w(px(32.))
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .cursor_pointer()
                            .font_family("Segoe MDL2 Assets")
                            .text_size(px(14.))
                            .bg(if self.settings.theme == ThemeId::Custom {
                                theme.accent_soft
                            } else {
                                theme.titlebar
                            })
                            .text_color(if self.settings.theme == ThemeId::Custom {
                                theme.blue
                            } else {
                                theme.muted
                            })
                            .hover(|s| s.bg(theme.menu_hover).text_color(theme.text))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cycle_theme_mode(cx);
                            }))
                            .child(self.settings.theme.icon()),
                    ),
            )
            .child(
                div()
                    .id("titlebar-controls")
                    .flex()
                    .items_center()
                    .h_full()
                    .child(Self::caption_btn(
                        "win-min",
                        "\u{e921}",
                        WindowControlArea::Min,
                        theme,
                        false,
                    ))
                    .child(Self::caption_btn(
                        "win-max",
                        max_icon,
                        WindowControlArea::Max,
                        theme,
                        false,
                    ))
                    .child(Self::caption_btn(
                        "win-close",
                        "\u{e8bb}",
                        WindowControlArea::Close,
                        theme,
                        true,
                    )),
            )
    }

    /// Segoe MDL2 glyphs — clean monochrome tree icons.
    fn tree_icon(kind: TreeKind, is_main: bool) -> &'static str {
        if is_main {
            return "\u{e8a5}"; // Document
        }
        match kind {
            TreeKind::Root => "\u{e8b7}",   // FolderOpen
            TreeKind::Folder => "\u{e8b7}", // Folder
            TreeKind::Source => "\u{e8a5}", // Document
            TreeKind::Config => "\u{e713}", // Settings
            TreeKind::Binary => "\u{e8b8}", // HardDrive
            TreeKind::Other => "\u{e8a5}",
        }
    }

    fn render_project_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let has_project = self.project_dir.is_some();
        let tree = self.project_tree.clone();
        let refs = self.source_refs.clone();
        let selected = self.tree_selected.clone();
        let current = self.source_path.clone();
        let rename_path = self.rename_path.clone();
        let proj_name = if has_project {
            self.project_meta.name.clone()
        } else {
            "(无工程)".into()
        };
        let proj_dir_for_blank = self.project_dir.clone();

        let side_w = self.sidebar_width.clamp(MIN_SIDEBAR_W, MAX_SIDEBAR_W);
        div()
            .flex()
            .flex_col()
            .w(px(side_w))
            .h_full()
            .bg(theme.panel)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px_3()
                    .py_1p5()
                    .border_b_1()
                    .border_color(theme.line)
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(proj_name),
                    )
                    .when(has_project, |el| {
                        el.child(
                            div()
                                .id("tree-refresh")
                                .w(px(22.))
                                .h(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .text_xs()
                                .font_family("Segoe MDL2 Assets")
                                .text_color(theme.blue)
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.accent_soft))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_project_tree(cx);
                                }))
                                .child("\u{e72c}"), // Refresh
                        )
                    })
                    .child(
                        div()
                            .id("tree-hide")
                            .w(px(22.))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .text_xs()
                            .text_color(theme.muted)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.accent_soft))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_sidebar = false;
                                this.persist_layout();
                                cx.notify();
                            }))
                            .child("✕"),
                    ),
            )
            .child(
                div()
                    .id("project-tree-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_1()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            // Click blank tree area → exit rename.
                            if this.rename_path.is_some() {
                                this.cancel_rename(cx);
                            }
                        }),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            // Blank area only (row handlers stop_propagation).
                            if this.rename_path.is_some() {
                                this.cancel_rename(cx);
                            }
                            if let Some(dir) = proj_dir_for_blank.clone() {
                                this.tree_ctx = Some((
                                    dir,
                                    f32::from(event.position.x),
                                    f32::from(event.position.y),
                                ));
                                this.tree_ctx_sort = false;
                                this.console_ctx = None;
                                this.open_menu = None;
                                cx.notify();
                            }
                        }),
                    )
                    .children({
                        // Skip Root row — header already shows project name.
                        let rows: Vec<_> = tree
                            .into_iter()
                            .filter(|e| e.kind != TreeKind::Root)
                            .collect();
                        if rows.is_empty() {
                            vec![div()
                                .px_3()
                                .py_3()
                                .text_xs()
                                .text_color(theme.muted)
                                .child(if has_project {
                                    "(空 · 右键新建)"
                                } else {
                                    "打开工程"
                                })
                                .into_any_element()]
                        } else {
                            rows.into_iter()
                                .map(|entry| {
                                    let path = entry.path.clone();
                                    let path_r = path.clone();
                                    let is_sel = selected.as_ref() == Some(&path)
                                        || current.as_ref() == Some(&path);
                                    let is_renaming = rename_path.as_ref() == Some(&path);
                                    // depth 1 → pad 8; depth 2 → pad 22 (no root row).
                                    let depth = entry.depth.saturating_sub(1);
                                    let pad = 8.0 + depth as f32 * 14.0;
                                    let icon = Self::tree_icon(entry.kind, entry.is_main);
                                    let label = entry.name.clone();
                                    let icon_color = if entry.is_main {
                                        theme.yellow
                                    } else if entry.kind == TreeKind::Source {
                                        theme.blue
                                    } else if entry.kind == TreeKind::Folder
                                        || entry.kind == TreeKind::Root
                                    {
                                        theme.muted
                                    } else if entry.kind == TreeKind::Config {
                                        theme.green
                                    } else {
                                        theme.muted
                                    };
                                    let rename_entity = self.rename_input.clone();
                                    div()
                                        .id(SharedString::from(format!("tree-{}", path.display())))
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1()
                                        .pl(px(pad))
                                        .pr_2()
                                        .py_1()
                                        .cursor_pointer()
                                        .bg(if is_sel || is_renaming {
                                            theme.accent_soft
                                        } else {
                                            theme.panel
                                        })
                                        .hover(|s| s.bg(theme.menu_hover))
                                        .when(is_renaming, |el| {
                                            el.on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                        })
                                        .when(!is_renaming, |el| {
                                            el.on_click(cx.listener(move |this, _, _, cx| {
                                                this.tree_ctx = None;
                                                if this.rename_path.is_some() {
                                                    this.cancel_rename(cx);
                                                    return;
                                                }
                                                this.open_tree_entry(path.clone(), cx);
                                            }))
                                        })
                                        .on_mouse_down(
                                            gpui::MouseButton::Right,
                                            cx.listener(
                                                move |this, event: &gpui::MouseDownEvent, _, cx| {
                                                    cx.stop_propagation();
                                                    if this.rename_path.is_some() {
                                                        this.cancel_rename(cx);
                                                    }
                                                    this.tree_ctx = Some((
                                                        path_r.clone(),
                                                        f32::from(event.position.x),
                                                        f32::from(event.position.y),
                                                    ));
                                                    this.tree_ctx_sort = false;
                                                    this.console_ctx = None;
                                                    this.open_menu = None;
                                                    this.tree_selected = Some(path_r.clone());
                                                    cx.notify();
                                                },
                                            ),
                                        )
                                        .child(
                                            div()
                                                .w(px(16.))
                                                .text_xs()
                                                .font_family("Segoe MDL2 Assets")
                                                .text_color(icon_color)
                                                .child(icon),
                                        )
                                        .child(if is_renaming {
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap_1()
                                                .h(px(24.))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .h_full()
                                                        // Keep clicks inside editor from bubbling as "outside".
                                                        .on_mouse_down(
                                                            gpui::MouseButton::Left,
                                                            |_, _, cx| cx.stop_propagation(),
                                                        )
                                                        .child(rename_entity),
                                                )
                                                .child(
                                                    div()
                                                        .id("rename-ok")
                                                        .w(px(20.))
                                                        .h(px(20.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .rounded_sm()
                                                        .text_xs()
                                                        .text_color(theme.green)
                                                        .cursor_pointer()
                                                        .hover(|s| s.bg(theme.accent_soft))
                                                        .on_mouse_down(
                                                            gpui::MouseButton::Left,
                                                            |_, _, cx| cx.stop_propagation(),
                                                        )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.confirm_rename(cx);
                                                        }))
                                                        .child("✓"),
                                                )
                                                .child(
                                                    div()
                                                        .id("rename-cancel")
                                                        .w(px(20.))
                                                        .h(px(20.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .rounded_sm()
                                                        .text_xs()
                                                        .text_color(theme.muted)
                                                        .cursor_pointer()
                                                        .hover(|s| s.bg(theme.accent_soft))
                                                        .on_mouse_down(
                                                            gpui::MouseButton::Left,
                                                            |_, _, cx| cx.stop_propagation(),
                                                        )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.cancel_rename(cx);
                                                        }))
                                                        .child("✕"),
                                                )
                                                .into_any_element()
                                        } else {
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_xs()
                                                .whitespace_nowrap()
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .text_color(theme.text)
                                                .child(label)
                                                .into_any_element()
                                        })
                                        .into_any_element()
                                })
                                .collect()
                        }
                    }),
            )
            .when(!refs.is_empty(), |el| {
                el.child(
                    div()
                        .border_t_1()
                        .border_color(theme.line)
                        .px_3()
                        .py_1p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme.muted)
                                .child("runfile"),
                        )
                        .children(
                            refs.into_iter()
                                .map(|r| {
                                    let label = r.clone();
                                    let label_click = r.clone();
                                    let path_opt = self.project_dir.as_ref().map(|d| d.join(&r));
                                    div()
                                        .id(SharedString::from(format!("ref-{label}")))
                                        .mt_1()
                                        .px_2()
                                        .py_1()
                                        .rounded_sm()
                                        .bg(theme.panel2)
                                        .text_xs()
                                        .font_family("Cascadia Code")
                                        .text_color(theme.blue)
                                        .cursor_pointer()
                                        .hover(|s| s.border_color(theme.blue))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(p) = path_opt.clone() {
                                                if p.exists() {
                                                    this.open_tree_entry(p, cx);
                                                } else {
                                                    this.shared.lock().status =
                                                        format!("不存在: {label_click}");
                                                    cx.notify();
                                                }
                                            }
                                        }))
                                        .child(format!("→ {label}"))
                                        .into_any_element()
                                })
                                .collect::<Vec<_>>(),
                        ),
                )
            })
    }

    fn menu_dropdown_items(&self, id: MenuId, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let entity = cx.entity();
        let mut items: Vec<gpui::AnyElement> = Vec::new();
        match id {
            // 工程与本地文件
            MenuId::File => {
                items.push(
                    self.menu_item("mi-new-proj", "新建工程", "Ctrl+Shift+N", true, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.new_project(&NewProject, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(
                    self.menu_item("mi-open-proj", "打开工程", "Ctrl+Shift+O", true, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_project(&OpenProject, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(self.menu_separator().into_any_element());
                items.push(
                    self.menu_item("mi-open-src", "打开文件", "Ctrl+O", true, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_source(&OpenSource, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(
                    self.menu_item("mi-save", "保存", "Ctrl+S", true, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.save_file(&SaveFile, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(
                    self.menu_item("mi-save-as", "另存为", "Ctrl+Shift+S", true, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.save_file_as(&SaveFileAs, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(self.menu_separator().into_any_element());
                items.push(
                    self.menu_item(
                        "mi-dl-luac",
                        "导出字节码",
                        "",
                        self.compiler.is_some(),
                        {
                            let entity = entity.clone();
                            move |_, window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    this.download_luac(&DownloadLuac, window, cx);
                                });
                            }
                        },
                    )
                    .into_any_element(),
                );
                items.push(self.menu_separator().into_any_element());
                items.push(
                    self.menu_item("mi-settings", "设置", "", true, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_settings(&OpenSettings, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
            }
            // 磁盘 <exe>/example/ 工程
            MenuId::Example => {
                let examples = snippets::list_examples();
                if examples.is_empty() {
                    items.push(
                        self.menu_item("mi-ex-empty", "无示例", "", false, |_, _, _| {})
                            .into_any_element(),
                    );
                    items.push(
                        self.menu_item("mi-ex-open-dir", "打开示例目录", "", true, {
                            let entity = entity.clone();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    let root = snippets::example_root();
                                    let _ = std::fs::create_dir_all(&root);
                                    this.open_in_explorer(root, cx);
                                });
                            }
                        })
                        .into_any_element(),
                    );
                } else {
                    for ex in examples {
                        let path = ex.path.clone();
                        let label = ex.label.clone();
                        let mid = SharedString::from(format!("mi-ex-{}", ex.id));
                        items.push(
                            self.menu_item(mid, label, "", true, {
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.open_example_project(path.clone(), cx);
                                    });
                                }
                            })
                            .into_any_element(),
                        );
                    }
                    items.push(self.menu_separator().into_any_element());
                    items.push(
                        self.menu_item("mi-ex-open-dir", "打开示例目录", "", true, {
                            let entity = entity.clone();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    this.open_in_explorer(snippets::example_root(), cx);
                                });
                            }
                        })
                        .into_any_element(),
                    );
                }
            }
            MenuId::Board => {
                if self.board_choices.is_empty() {
                    items.push(
                        self.menu_item(
                            "mi-board-empty",
                            "未找到开发板文件",
                            "",
                            false,
                            |_, _, _| {},
                        )
                        .into_any_element(),
                    );
                } else {
                    let selected = self.settings.selected_board.clone();
                    for board in &self.board_choices {
                        let id = board.id.clone();
                        let name = if selected.as_deref() == Some(board.id.as_str()) {
                            format!("✓ {}", board.name)
                        } else {
                            board.name.clone()
                        };
                        let detail = board.chip.clone();
                        items.push(
                            self.menu_item(
                                SharedString::from(format!("mi-board-{}", board.id)),
                                name,
                                detail,
                                true,
                                {
                                    let entity = entity.clone();
                                    move |_, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.open_menu = None;
                                            this.select_board(id.clone(), cx);
                                        });
                                    }
                                },
                            )
                            .into_any_element(),
                        );
                    }
                }
            }
            // 编译 / 上传 / 板端脚本
            MenuId::Run => {
                let connected = self.shared.lock().connected;
                let busy = self.shared.lock().busy;
                items.push(
                    self.menu_item("mi-run", "编译并运行", "F5", connected && !busy, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_menu = None;
                                this.run(&Run, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(
                    self.menu_item("mi-stop", "停止", "Esc", connected, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_menu = None;
                                this.stop(&Stop, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(
                    self.menu_item("mi-rerun", "重跑", "", connected && !busy, {
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_menu = None;
                                this.run_main_only(cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(self.menu_separator().into_any_element());
                items.push(
                    self.menu_item("mi-refresh-flash", "刷新文件列表", "", connected, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_menu = None;
                                this.refresh_files(&RefreshFiles, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(self.menu_separator().into_any_element());
                let transfer_mode = self.settings.transfer_mode;
                items.push(
                    self.menu_item(
                        "mi-speed-low",
                        if transfer_mode == TransferMode::Low {
                            "✓ 低速传输（115200）"
                        } else {
                            "低速传输（115200）"
                        },
                        "",
                        !busy,
                        {
                            let entity = entity.clone();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.settings.set_transfer_mode(TransferMode::Low);
                                    this.open_menu = None;
                                    this.push_sys("传输模式已设为低速 115200", cx);
                                });
                            }
                        },
                    )
                    .into_any_element(),
                );
                items.push(
                    self.menu_item(
                        "mi-speed-high",
                        if transfer_mode == TransferMode::High {
                            "✓ 高速传输（命令切换 460800）"
                        } else {
                            "高速传输（命令切换 460800）"
                        },
                        "",
                        !busy,
                        {
                            let entity = entity.clone();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.settings.set_transfer_mode(TransferMode::High);
                                    this.open_menu = None;
                                    this.push_sys("传输模式已设为高速 460800", cx);
                                });
                            }
                        },
                    )
                    .into_any_element(),
                );
            }
            // 连接 / 固件 / Flash
            MenuId::Device => {
                let connected = self.shared.lock().connected;
                let busy = self.shared.lock().busy;
                items.push(
                    self.menu_item(
                        "mi-connect",
                        if connected { "断开" } else { "连接" },
                        "",
                        true,
                        {
                            let entity = entity.clone();
                            move |_, window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    if this.shared.lock().connected {
                                        this.disconnect(&Disconnect, window, cx);
                                    } else {
                                        this.connect(&Connect, window, cx);
                                    }
                                });
                            }
                        },
                    )
                    .into_any_element(),
                );
                items.push(
                    self.menu_item("mi-ports", "选择端口", "", true, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_menu = None;
                                this.refresh_ports(&RefreshPorts, window, cx);
                                this.show_ports = true;
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(self.menu_separator().into_any_element());
                items.push(
                    self.menu_item("mi-flash-fw", "烧录固件", "", !busy, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_menu = None;
                                this.flash_firmware(&FlashFirmware, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(
                    self.menu_item("mi-format-fs", "重置 Flash", "", connected && !busy, {
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.menu_format_fs(cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
            }
            MenuId::View => {
                items.push(
                    self.menu_item(
                        "mi-telemetry-right",
                        if self.telemetry_dock == TelemetryDock::Right {
                            "✓ 在编辑器右侧打开"
                        } else {
                            "在编辑器右侧打开"
                        },
                        "",
                        true,
                        {
                            let entity = entity.clone();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    this.show_telemetry_right(cx);
                                });
                            }
                        },
                    )
                    .into_any_element(),
                );
                items.push(
                    self.menu_item(
                        "mi-telemetry-window",
                        if self.telemetry_dock == TelemetryDock::Window {
                            "✓ 作为独立窗口打开"
                        } else {
                            "作为独立窗口打开"
                        },
                        "",
                        true,
                        {
                            let entity = entity.clone();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.show_telemetry_window(cx);
                                });
                            }
                        },
                    )
                    .into_any_element(),
                );
                items.push(
                    self.menu_item(
                        "mi-telemetry-editor",
                        if self.telemetry_dock == TelemetryDock::Editor {
                            "✓ 合并为首位标签"
                        } else {
                            "合并为首位标签"
                        },
                        "",
                        true,
                        {
                            let entity = entity.clone();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    this.dock_telemetry_in_editor(cx);
                                });
                            }
                        },
                    )
                    .into_any_element(),
                );
                items.push(self.menu_separator().into_any_element());
                items.push(
                    self.menu_item(
                        "mi-telemetry-close",
                        "关闭数据可视化",
                        "",
                        self.telemetry_dock != TelemetryDock::Hidden,
                        {
                            let entity = entity.clone();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    this.hide_telemetry(cx);
                                });
                            }
                        },
                    )
                    .into_any_element(),
                );
            }
            MenuId::Help => {
                items.push(
                    self.menu_item("mi-keys", "快捷键", "", true, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_keys(&OpenKeys, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
                items.push(
                    self.menu_item("mi-about", "关于", "", true, {
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_about(&OpenAbout, window, cx);
                            });
                        }
                    })
                    .into_any_element(),
                );
            }
        }
        items
    }

    /// Dropdown left edge under each menu tab (titlebar pl_1 + cumulative tab widths).
    /// Tab: px_3 each side (12+12) + ~2*char width for 2-char Chinese labels (~28) ≈ 52.
    fn menu_left_offset(id: MenuId) -> f32 {
        const PAD: f32 = 4.0;
        const TAB: f32 = 52.0;
        match id {
            MenuId::File => PAD,
            MenuId::Example => PAD + TAB,
            MenuId::Board => PAD + TAB * 2.0,
            MenuId::Run => PAD + TAB * 3.5,
            MenuId::Device => PAD + TAB * 4.5,
            MenuId::View => PAD + TAB * 5.5,
            MenuId::Help => PAD + TAB * 6.5,
        }
    }

    fn select_file(&mut self, name: String, cx: &mut Context<Self>) {
        self.target_name = name.clone();
        self.shared.lock().selected_file = Some(name);
        cx.notify();
    }

    fn set_boot(&mut self, cx: &mut Context<Self>) {
        let (session, name) = {
            let s = self.shared.lock();
            (s.session.clone(), s.selected_file.clone())
        };
        let Some(session) = session else {
            self.push_err("未连接", cx);
            return;
        };
        let Some(name) = name else {
            self.push_err("请先选择文件", cx);
            return;
        };
        if name == "main.luac" {
            return;
        }
        let shared = self.shared.clone();
        shared.lock().busy = true;
        let name_for_log = name.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    session.set_boot(&name)?;
                    session.list_files()
                })
                .await;
            this.update(cx, |_, cx| {
                let mut s = shared.lock();
                s.busy = false;
                match result {
                    Ok(files) => {
                        s.files = files;
                        s.logs.push(LogLine {
                            kind: LogKind::Sys,
                            text: format!("已设为启动: {name_for_log}").into(),
                        });
                    }
                    Err(err) => {
                        s.logs.push(LogLine {
                            kind: LogKind::Err,
                            text: format!("boot 失败: {err:#}").into(),
                        });
                    }
                }
                s.log_epoch += 1;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let (session, name) = {
            let s = self.shared.lock();
            (s.session.clone(), s.selected_file.clone())
        };
        let Some(session) = session else {
            return;
        };
        let Some(name) = name else {
            return;
        };
        if name == "main.luac" {
            self.push_err("不能删除 main.luac（可覆盖上传）", cx);
            return;
        }
        let shared = self.shared.clone();
        let name_for_log = name.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    session.delete_file(&name)?;
                    session.list_files()
                })
                .await;
            this.update(cx, |_, cx| {
                let mut s = shared.lock();
                match result {
                    Ok(files) => {
                        s.files = files;
                        s.selected_file = None;
                        s.logs.push(LogLine {
                            kind: LogKind::Sys,
                            text: format!("已删除 {name_for_log}").into(),
                        });
                    }
                    Err(err) => {
                        s.logs.push(LogLine {
                            kind: LogKind::Err,
                            text: format!("删除失败: {err:#}").into(),
                        });
                    }
                }
                s.log_epoch += 1;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn run_main_only(&mut self, cx: &mut Context<Self>) {
        let session = self.shared.lock().session.clone();
        if let Some(session) = session {
            if let Err(err) = session.run_main() {
                self.push_err(format!("{err:#}"), cx);
            } else {
                let mut s = self.shared.lock();
                s.script_running = true;
                s.status = "重跑 main…".into();
                cx.notify();
            }
        }
    }

    fn btn(
        &self,
        label: impl Into<SharedString>,
        primary: bool,
        danger: bool,
        enabled: bool,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let theme = self.theme;
        let label: SharedString = label.into();
        let bg = if !enabled {
            theme.panel2
        } else if danger {
            theme.danger
        } else if primary {
            theme.blue
        } else {
            theme.btn_bg
        };
        let border = if !enabled {
            theme.line
        } else if danger {
            theme.danger_border
        } else if primary {
            theme.blue
        } else {
            theme.btn_border
        };
        let fg = if !enabled {
            theme.muted
        } else if primary {
            theme.btn_primary_fg
        } else if danger {
            theme.red
        } else {
            theme.text
        };
        let hover_border = theme.blue;
        let hover_bg = if primary {
            theme.blue
        } else {
            theme.menu_hover
        };
        div()
            .id(label.clone())
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .h(px(28.))
            .px_3()
            .rounded_md()
            .bg(bg)
            .border_1()
            .border_color(border)
            .text_sm()
            .whitespace_nowrap()
            .text_color(fg)
            .when(enabled, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.border_color(hover_border).bg(hover_bg))
                    .on_click(on_click)
            })
            .child(label)
    }

    fn render_dialog(&self, kind: DialogKind, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let title = match kind {
            DialogKind::Settings => "设置",
            DialogKind::About => "关于",
            DialogKind::Keys => "快捷键",
            DialogKind::FormatFs => "重置 Flash？",
            DialogKind::DeleteConfirm => "删除？",
            DialogKind::BoardSelect => "选择开发板",
        };
        let body: gpui::AnyElement = match kind {
            DialogKind::BoardSelect => {
                let mut buttons = Vec::new();
                for board in &self.board_choices {
                    let id = board.id.clone();
                    buttons.push(
                        div()
                            .id(SharedString::from(format!("board-first-{}", board.id)))
                            .flex()
                            .items_center()
                            .px_3()
                            .py_2()
                            .rounded_sm()
                            .border_1()
                            .border_color(theme.line)
                            .bg(theme.btn_bg)
                            .cursor_pointer()
                            .hover(|style| style.bg(theme.menu_hover).border_color(theme.blue))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_board(id.clone(), cx);
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .text_color(theme.text)
                                    .child(board.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted)
                                    .child(board.chip.clone()),
                            )
                            .into_any_element(),
                    );
                }
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted)
                            .child("所选开发板将写入 config.json，之后可从顶部“开发板”菜单切换。"),
                    )
                    .children(buttons)
                    .into_any_element()
            }
            DialogKind::DeleteConfirm => {
                let name = self
                    .dialog_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .to_string();
                let is_dir = self
                    .dialog_path
                    .as_ref()
                    .map(|p| p.is_dir())
                    .unwrap_or(false);
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_sm().text_color(theme.text).child(if is_dir {
                        format!("删除文件夹「{name}」及其内容？")
                    } else {
                        format!("删除「{name}」？")
                    }))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                div()
                                    .id("del-cancel")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.line)
                                    .bg(theme.btn_bg)
                                    .text_xs()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.menu_hover))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.dialog = None;
                                        this.dialog_path = None;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child(
                                div()
                                    .id("del-ok")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.danger_border)
                                    .bg(theme.danger)
                                    .text_xs()
                                    .text_color(theme.red)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.menu_hover))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_delete(cx);
                                    }))
                                    .child("删除"),
                            ),
                    )
                    .into_any_element()
            }
            DialogKind::FormatFs => {
                let detail = self.format_fs_detail.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.text)
                            .child("上传失败，可能是板端 LittleFS 损坏或未挂载。"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted)
                            .font_family("Cascadia Code")
                            .child(detail),
                    )
                    .child(div().text_xs().text_color(theme.yellow).child(
                        "重置将发送 format 命令，重做 SPI Flash 文件系统，所有脚本文件会被清空。",
                    ))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .mt_1()
                            .child(
                                div()
                                    .id("fmt-cancel")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.line)
                                    .bg(theme.btn_bg)
                                    .text_xs()
                                    .text_color(theme.text)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.menu_hover))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.dialog = None;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child(
                                div()
                                    .id("fmt-ok")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.danger_border)
                                    .bg(theme.danger)
                                    .text_xs()
                                    .text_color(theme.red)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.menu_hover))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_format_fs(cx);
                                    }))
                                    .child("重置 Flash"),
                            ),
                    )
                    .into_any_element()
            }
            DialogKind::Settings => {
                let theme_id = self.settings.theme;
                let can_reset =
                    !self.settings.palette(theme_id).is_empty() || theme_id == ThemeId::Custom;
                let mut theme_btns: Vec<gpui::AnyElement> = Vec::new();
                for tid in [ThemeId::Dark, ThemeId::Light, ThemeId::Custom] {
                    let on = tid == theme_id;
                    theme_btns.push(
                        div()
                            .id(SharedString::from(format!("th-{}", tid.label())))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(if on { theme.blue } else { theme.line })
                            .bg(if on { theme.accent_soft } else { theme.panel2 })
                            .text_xs()
                            .text_color(if on { theme.blue } else { theme.text })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_theme(tid, cx);
                            }))
                            .child(tid.label())
                            .into_any_element(),
                    );
                }
                let mut font_names: Vec<String> = metadata::data_root()
                    .ok()
                    .and_then(|root| std::fs::read_dir(root.join("font")).ok())
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|entry| {
                        let path = entry.path();
                        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
                        matches!(extension.as_str(), "ttf" | "otf")
                            .then(|| entry.file_name().to_string_lossy().into_owned())
                    })
                    .collect();
                font_names.sort();
                let mut font_rows = Vec::new();
                for font in font_names {
                    let zh_font = font.clone();
                    let en_font = font.clone();
                    let zh_selected = self.settings.font_zh == font;
                    let en_selected = self.settings.font_en == font;
                    font_rows.push(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(theme.text)
                                    .child(font.clone()),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("font-zh-{font}")))
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(if zh_selected { theme.blue } else { theme.line })
                                    .text_xs()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.settings.set_font_zh(zh_font.clone());
                                        cx.notify();
                                    }))
                                    .child(if zh_selected { "✓ 中文" } else { "中文" }),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("font-en-{font}")))
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(if en_selected { theme.blue } else { theme.line })
                                    .text_xs()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.settings.set_font_en(en_font.clone());
                                        cx.notify();
                                    }))
                                    .child(if en_selected { "✓ 英文" } else { "英文" }),
                            )
                            .into_any_element(),
                    );
                }
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted)
                            .child("主题（三套调色板互不干扰 · 标题栏图标轮换）"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .children(theme_btns)
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id("theme-reset")
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(theme.line)
                                    .bg(theme.btn_bg)
                                    .text_xs()
                                    .text_color(if can_reset { theme.text } else { theme.muted })
                                    .when(can_reset, |el| {
                                        el.cursor_pointer().hover(|s| s.bg(theme.menu_hover))
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reset_current_theme(cx);
                                    }))
                                    .child(match theme_id {
                                        ThemeId::Dark => "恢复深色默认",
                                        ThemeId::Light => "恢复浅色默认",
                                        ThemeId::Custom => "清空自定义",
                                    }),
                            ),
                    )
                    .child(div().text_xs().text_color(theme.muted).child(format!(
                        "当前「{}」调色板 · 拖动仅改本主题",
                        theme_id.label()
                    )))
                    .child(self.color_wheel.clone())
                    .child(div().text_xs().text_color(theme.muted).child(format!(
                        "OLED 字体 · 中文 {} · 英文 {}",
                        self.settings.font_zh, self.settings.font_en
                    )))
                    .children(font_rows)
                    .into_any_element()
            }
            DialogKind::About => div()
                .flex()
                .flex_col()
                .gap_2()
                .text_sm()
                .text_color(theme.text)
                .child("Lua IDE")
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted)
                        .child("Lua 5.5.1 / LUA_32BITS · HEX 脚本 · UART BSL"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted)
                        .child("CH340 · PA10/PA11 · 固件烧录请先进入 BSL"),
                )
                .into_any_element(),
            DialogKind::Keys => div()
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .font_family("Cascadia Code")
                .text_color(theme.text)
                .child("F5 / Ctrl+Enter    运行")
                .child("Esc               停止")
                .child("Ctrl+S            保存")
                .child("Ctrl+O            打开")
                .child("Ctrl+Shift+O      打开工程")
                .child("Ctrl+Shift+N      新建工程")
                .child("Ctrl+T            切换主题")
                .child("Tab               补全")
                .into_any_element(),
        };
        div()
            .id("dialog-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000080))
            .child(
                div()
                    .id("dialog-backdrop-catch")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if this.dialog != Some(DialogKind::BoardSelect) {
                                this.dialog = None;
                            }
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .id("dialog-card")
                    .relative()
                    .w(px(if kind == DialogKind::Settings {
                        520.
                    } else {
                        420.
                    }))
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.line)
                    .bg(theme.panel)
                    .shadow_lg()
                    .occlude()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .px_4()
                            .py_2()
                            .border_b_1()
                            .border_color(theme.line)
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .id("dialog-close")
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .text_xs()
                                    .text_color(theme.muted)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.menu_hover))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if this.dialog != Some(DialogKind::BoardSelect) {
                                            this.dialog = None;
                                        }
                                        cx.notify();
                                    }))
                                    .child("关闭"),
                            ),
                    )
                    .child(div().px_4().py_3().child(body)),
            )
    }
}

impl Focusable for IdeApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for IdeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_console(cx);
        let theme = self.theme;
        let shared = self.shared.lock().clone_view();
        let source_bytes = self.editor.read(cx).byte_len();
        let port_label = self
            .ports
            .get(self.selected_port_idx)
            .map(|p| p.label.clone())
            .unwrap_or_else(|| "无端口".into());
        let compiler_ok = self.compiler.is_some();
        let connected = shared.connected;
        let busy = shared.busy;
        let script_running = shared.script_running;
        let show_sidebar = self.show_sidebar;
        let show_console = self.show_console;
        let full_output = self.settings.full_output;
        let telemetry_dock = self.telemetry_dock;
        let telemetry_tab_active = self.telemetry_tab_active;
        let telemetry_w = self.telemetry_width.clamp(340.0, 720.0);
        let console_h = self.console_height.clamp(MIN_CONSOLE_H, MAX_CONSOLE_H);
        let open_menu = self.open_menu;
        let menu_items = open_menu.map(|m| self.menu_dropdown_items(m, cx));
        let menu_left = open_menu.map(Self::menu_left_offset).unwrap_or(0.0);
        // Keep active tab content/dirty in sync while typing.
        if !self.open_tabs.is_empty() && !self.telemetry_tab_active {
            let text = self.editor.read(cx).text();
            let idx = self.active_tab.min(self.open_tabs.len() - 1);
            if self.open_tabs[idx].content != text {
                self.open_tabs[idx].content = text;
                self.open_tabs[idx].dirty = true;
                self.dirty = true;
            }
        }
        // Persist Ctrl+wheel font size.
        {
            let font = self.editor.read(cx).font_px();
            if (font - self.settings.editor_font_size).abs() > 0.05 {
                self.settings.set_editor_font_size(font);
            }
        }
        // Inline rename: Enter / Esc from LineInput; focus once.
        if self.rename_path.is_some() {
            if self.rename_focus {
                self.rename_focus = false;
                self.rename_input.update(cx, |inp, _| inp.focus(window));
            }
            let (submit, cancel) = self
                .rename_input
                .update(cx, |inp, _| (inp.take_submit(), inp.take_cancel()));
            if submit {
                self.confirm_rename(cx);
            } else if cancel {
                self.cancel_rename(cx);
            }
        }
        // Console search query → filter highlight.
        if self.console_search_open {
            let q = self.console_search.read(cx).text();
            self.console.update(cx, |c, cx| c.set_search(&q, cx));
        }
        // Custom palette wheel → live theme.
        if self.dialog == Some(DialogKind::Settings) {
            self.poll_color_wheel(cx);
        }
        let tabs_snapshot: Vec<(usize, String, bool, bool)> = self
            .open_tabs
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let title = t.title.clone();
                let dirty = t.dirty;
                let active = i == self.active_tab;
                (i, title, dirty, active)
            })
            .collect();
        let titlebar = self.render_titlebar(window, cx);

        div()
            .id("ide-root")
            .key_context("IdeApp")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::run))
            .on_action(cx.listener(Self::stop))
            .on_action(cx.listener(Self::connect))
            .on_action(cx.listener(Self::disconnect))
            .on_action(cx.listener(Self::refresh_ports))
            .on_action(cx.listener(Self::refresh_files))
            .on_action(cx.listener(Self::clear_log))
            .on_action(cx.listener(Self::copy_log))
            .on_action(cx.listener(Self::copy_project_context))
            .on_action(cx.listener(Self::download_luac))
            .on_action(cx.listener(Self::new_project))
            .on_action(cx.listener(Self::open_project))
            .on_action(cx.listener(Self::open_source))
            .on_action(cx.listener(Self::save_file))
            .on_action(cx.listener(Self::save_file_as))
            .on_action(cx.listener(Self::action_close_menu))
            .on_action(cx.listener(Self::cycle_theme))
            .on_action(cx.listener(Self::theme_dark))
            .on_action(cx.listener(Self::theme_light))
            .on_mouse_move(cx.listener(Self::on_split_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_split_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_split_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_telemetry_drag_out))
            .on_drop(cx.listener(|this, _: &TelemetryDrag, _, cx| {
                this.show_telemetry_window(cx);
            }))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg)
            .text_color(theme.text)
            .font_family("Segoe UI")
            // immersive client-side titlebar (Zed-style)
            .child(titlebar)
            // compact toolbar
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .h(px(36.))
                    .border_b_1()
                    .border_color(theme.line)
                    .bg(theme.panel)
                    // left: project toggle + connect
                    .child(
                        div()
                            .id("toggle-sidebar")
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .cursor_pointer()
                            .bg(if show_sidebar {
                                theme.accent_soft
                            } else {
                                theme.panel2
                            })
                            .border_1()
                            .border_color(theme.line)
                            .hover(|s| s.border_color(theme.blue))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_sidebar = !this.show_sidebar;
                                if this.show_sidebar && this.sidebar_width < MIN_SIDEBAR_W {
                                    this.sidebar_width = DEFAULT_SIDEBAR_W;
                                }
                                this.persist_layout();
                                cx.notify();
                            }))
                            .child(if show_sidebar {
                                "◂ 工程"
                            } else {
                                "▸ 工程"
                            }),
                    )
                    .child(
                        div()
                            .id("port-picker")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.line)
                            .bg(theme.panel2)
                            .text_xs()
                            .text_color(theme.muted)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_ports = !this.show_ports;
                                this.open_menu = None;
                                cx.notify();
                            }))
                            .child(port_label),
                    )
                    .when(!connected, |el| {
                        el.child(self.btn("连接", true, false, !busy, {
                            let entity = cx.entity();
                            move |_, window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.connect(&Connect, window, cx);
                                });
                            }
                        }))
                    })
                    .when(connected, |el| {
                        el.child(self.btn("断开", false, false, true, {
                            let entity = cx.entity();
                            move |_, window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.disconnect(&Disconnect, window, cx);
                                });
                            }
                        }))
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted)
                            .font_family("Cascadia Code")
                            .child(format!("{} · {source_bytes} B", self.target_name)),
                    )
                    .child(self.btn("保存", false, false, true, {
                        let entity = cx.entity();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.save_file(&SaveFile, window, cx);
                            });
                        }
                    }))
                    .child(self.btn("复制工程上下文", false, false, true, {
                        let entity = cx.entity();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.copy_project_context(&CopyProjectContext, window, cx);
                            });
                        }
                    }))
                    // right: merged run/stop
                    .child(self.btn(
                        if script_running {
                            "■ 停止"
                        } else {
                            "▶ 运行"
                        },
                        !script_running,
                        script_running,
                        connected && !busy && (script_running || compiler_ok),
                        {
                            let entity = cx.entity();
                            move |_, window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.run(&Run, window, cx);
                                });
                            }
                        },
                    )),
            )
            // port list (compact)
            .when(self.show_ports, |el| {
                el.child(
                    div()
                        .id("port-list")
                        .px_2()
                        .py_1()
                        .border_b_1()
                        .border_color(theme.line)
                        .bg(theme.panel2)
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .items_center()
                        .child(div().text_xs().text_color(theme.muted).child("端口"))
                        .children(self.ports.iter().enumerate().map(|(idx, p)| {
                            let selected = idx == self.selected_port_idx;
                            let label = p.label.clone();
                            let is_jlink = label.to_ascii_lowercase().contains("j-link");
                            div()
                                .id(SharedString::from(format!("port-{idx}")))
                                .px_2()
                                .py_1()
                                .rounded_sm()
                                .cursor_pointer()
                                .bg(if selected {
                                    theme.accent_soft
                                } else {
                                    theme.panel
                                })
                                .border_1()
                                .border_color(if selected { theme.blue } else { theme.line })
                                .text_xs()
                                .text_color(if is_jlink { theme.yellow } else { theme.text })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.selected_port_idx = idx;
                                    this.show_ports = false;
                                    if let Some(p) = this.ports.get(idx) {
                                        this.settings.set_last_port(&p.name);
                                    }
                                    cx.notify();
                                }))
                                .child(label)
                        }))
                        .child(self.btn("刷新", false, false, true, {
                            let entity = cx.entity();
                            move |_, window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.refresh_ports(&RefreshPorts, window, cx);
                                });
                            }
                        })),
                )
            })
            // body: sidebar | editor + optional console
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(show_sidebar, |el| {
                        el.child(self.render_project_sidebar(cx)).child({
                            let theme = theme;
                            let entity = cx.entity();
                            div()
                                .id("split-sidebar")
                                .w(px(4.))
                                .h_full()
                                .cursor(CursorStyle::ResizeLeftRight)
                                .bg(theme.line)
                                .hover(|s| s.bg(theme.blue))
                                .on_mouse_down(MouseButton::Left, move |ev, _, app| {
                                    entity.update(app, |this, cx| {
                                        this.drag_sidebar =
                                            Some((f32::from(ev.position.x), this.sidebar_width));
                                        cx.notify();
                                    });
                                })
                        })
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                div()
                                    .id("editor-tabs")
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .h(px(32.))
                                    .min_h(px(32.))
                                    .border_b_1()
                                    .border_color(theme.line)
                                    .bg(theme.panel)
                                    .overflow_x_scroll()
                                    .drag_over::<TelemetryDrag>(move |style, _, _, _| {
                                        style.bg(theme.accent_soft)
                                    })
                                    .on_drop(cx.listener(|this, _: &TelemetryDrag, _, cx| {
                                        cx.stop_propagation();
                                        this.dock_telemetry_in_editor(cx);
                                    }))
                                    .when(telemetry_dock == TelemetryDock::Editor, |el| {
                                        let drag_owner = cx.entity();
                                        el.child(
                                            div()
                                                .id("tab-telemetry-locked")
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .h_full()
                                                .px_2()
                                                .border_r_1()
                                                .border_color(theme.line)
                                                .bg(if telemetry_tab_active {
                                                    theme.code
                                                } else {
                                                    theme.panel
                                                })
                                                .cursor_move()
                                                .hover(|s| s.bg(theme.menu_hover))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.flush_active_tab(cx);
                                                    this.telemetry_tab_active = true;
                                                    cx.notify();
                                                }))
                                                .on_drag(
                                                    TelemetryDrag,
                                                    move |_, position, _, app| {
                                                        drag_owner.update(app, |ide, cx| {
                                                            ide.telemetry_dragging = true;
                                                            cx.notify();
                                                        });
                                                        app.new(|_| TelemetryDragPreview {
                                                            position,
                                                        })
                                                    },
                                                )
                                                .child(
                                                    div()
                                                        .size(px(6.))
                                                        .rounded_full()
                                                        .bg(theme.blue),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .whitespace_nowrap()
                                                        .font_family("Cascadia Code")
                                                        .text_color(if telemetry_tab_active {
                                                            theme.text
                                                        } else {
                                                            theme.muted
                                                        })
                                                        .child("数据可视化"),
                                                )
                                                .child(
                                                    div()
                                                        .font_family("Segoe MDL2 Assets")
                                                        .text_size(px(9.))
                                                        .text_color(theme.muted)
                                                        .child("\u{e72e}"),
                                                ),
                                        )
                                    })
                                    .children(tabs_snapshot.into_iter().map(
                                        |(i, title, dirty, active)| {
                                            let label =
                                                if dirty { format!("● {title}") } else { title };
                                            div()
                                                .id(SharedString::from(format!("tab-{i}")))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap_1()
                                                .h_full()
                                                .px_2()
                                                .border_r_1()
                                                .border_color(theme.line)
                                                .bg(if active { theme.code } else { theme.panel })
                                                .cursor_pointer()
                                                .hover(|s| s.bg(theme.menu_hover))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.activate_tab(i, cx);
                                                }))
                                                .on_mouse_down(
                                                    gpui::MouseButton::Middle,
                                                    cx.listener(move |this, _, _, cx| {
                                                        this.close_tab_at(i, cx);
                                                    }),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .whitespace_nowrap()
                                                        .text_color(if active {
                                                            theme.text
                                                        } else {
                                                            theme.muted
                                                        })
                                                        .font_family("Cascadia Code")
                                                        .child(label),
                                                )
                                                .child(
                                                    div()
                                                        .id(SharedString::from(format!(
                                                            "tab-close-{i}"
                                                        )))
                                                        .w(px(16.))
                                                        .h(px(16.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .rounded_sm()
                                                        .text_xs()
                                                        .text_color(theme.muted)
                                                        .hover(|s| {
                                                            s.bg(theme.accent_soft)
                                                                .text_color(theme.text)
                                                        })
                                                        .on_mouse_down(
                                                            gpui::MouseButton::Left,
                                                            cx.listener(move |this, _, _, cx| {
                                                                cx.stop_propagation();
                                                                this.close_tab_at(i, cx);
                                                            }),
                                                        )
                                                        .child("×"),
                                                )
                                                .into_any_element()
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .flex_1()
                                    .min_h_0()
                                    .min_w_0()
                                    .drag_over::<TelemetryDrag>(move |style, _, _, _| {
                                        style.bg(theme.accent_soft)
                                    })
                                    .on_drop(cx.listener(|this, _: &TelemetryDrag, _, cx| {
                                        cx.stop_propagation();
                                        this.show_telemetry_right(cx);
                                    }))
                                    .when(!telemetry_tab_active, |el| {
                                        el.child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .h_full()
                                                .child(self.editor.clone()),
                                        )
                                    })
                                    .when(telemetry_tab_active, |el| {
                                        el.child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .h_full()
                                                .child(self.telemetry.clone()),
                                        )
                                    })
                                    .when(telemetry_dock == TelemetryDock::Right, |el| {
                                        let entity = cx.entity();
                                        let drag_owner = cx.entity();
                                        let telemetry_entity = self.telemetry.clone();
                                        el.child(
                                            div()
                                                .id("split-telemetry")
                                                .w(px(4.))
                                                .h_full()
                                                .cursor(CursorStyle::ResizeLeftRight)
                                                .bg(theme.line)
                                                .hover(|s| s.bg(theme.blue))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    move |ev, _, app| {
                                                        entity.update(app, |this, cx| {
                                                            this.drag_telemetry = Some((
                                                                f32::from(ev.position.x),
                                                                this.telemetry_width,
                                                            ));
                                                            cx.notify();
                                                        });
                                                    },
                                                ),
                                        )
                                        .child(
                                            div()
                                                .id("telemetry-panel")
                                                .flex()
                                                .flex_col()
                                                .w(px(telemetry_w))
                                                .min_w(px(340.))
                                                .h_full()
                                                .border_l_1()
                                                .border_color(theme.line)
                                                .bg(theme.code)
                                                .child(
                                                    div()
                                                        .id("telemetry-drag-handle")
                                                        .h(px(34.))
                                                        .min_h(px(34.))
                                                        .flex()
                                                        .items_center()
                                                        .px_3()
                                                        .border_b_1()
                                                        .border_color(theme.line)
                                                        .bg(theme.panel)
                                                        .cursor_move()
                                                        .on_drag(
                                                            TelemetryDrag,
                                                            move |_, position, _, app| {
                                                                drag_owner.update(
                                                                    app,
                                                                    |ide, cx| {
                                                                        ide.telemetry_dragging =
                                                                            true;
                                                                        cx.notify();
                                                                    },
                                                                );
                                                                app.new(|_| TelemetryDragPreview {
                                                                    position,
                                                                })
                                                            },
                                                        )
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .font_weight(
                                                                    gpui::FontWeight::MEDIUM,
                                                                )
                                                                .text_color(theme.text)
                                                                .child("数据可视化"),
                                                        )
                                                        .child(div().flex_1())
                                                        .child(
                                                            div()
                                                                .id("telemetry-close")
                                                                .size(px(24.))
                                                                .flex()
                                                                .items_center()
                                                                .justify_center()
                                                                .rounded_sm()
                                                                .text_sm()
                                                                .text_color(theme.muted)
                                                                .cursor_pointer()
                                                                .on_mouse_down(
                                                                    MouseButton::Left,
                                                                    |_, _, cx| {
                                                                        cx.stop_propagation()
                                                                    },
                                                                )
                                                                .hover(|s| {
                                                                    s.bg(theme.menu_hover)
                                                                        .text_color(theme.text)
                                                                })
                                                                .on_click(cx.listener(
                                                                    |this, _, _, cx| {
                                                                        this.hide_telemetry(cx);
                                                                    },
                                                                ))
                                                                .child("×"),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_h_0()
                                                        .child(telemetry_entity),
                                                ),
                                        )
                                    }),
                            )
                            .when(show_console, |el| {
                                let entity = cx.entity();
                                let search_open = self.console_search_open;
                                let search_entity = self.console_search.clone();
                                el.child(
                                    div()
                                        .id("split-console")
                                        .h(px(4.))
                                        .w_full()
                                        .cursor(CursorStyle::ResizeUpDown)
                                        .bg(theme.line)
                                        .hover(|s| s.bg(theme.blue))
                                        .on_mouse_down(MouseButton::Left, move |ev, _, app| {
                                            entity.update(app, |this, cx| {
                                                this.drag_console = Some((
                                                    f32::from(ev.position.y),
                                                    this.console_height,
                                                ));
                                                cx.notify();
                                            });
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .h(px(console_h))
                                        .min_h(px(MIN_CONSOLE_H))
                                        .border_t_1()
                                        .border_color(theme.line)
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .px_2()
                                                .py_1()
                                                .bg(theme.panel)
                                                .border_b_1()
                                                .border_color(theme.line)
                                                .child(
                                                    div()
                                                        .px_2()
                                                        .text_xs()
                                                        .font_weight(gpui::FontWeight::MEDIUM)
                                                        .text_color(theme.text)
                                                        .child("输出"),
                                                )
                                                 .child(div().flex_1())
                                                 .child(self.btn(
                                                     if full_output {
                                                         "完整输出 ✓"
                                                     } else {
                                                         "完整输出"
                                                     },
                                                     false,
                                                     full_output,
                                                     true,
                                                     {
                                                         let entity = cx.entity();
                                                         move |_, _, app| {
                                                             entity.update(app, |this, cx| {
                                                                 this.settings
                                                                     .set_full_output(!full_output);
                                                                 this.shared.lock().log_epoch += 1;
                                                                 cx.notify();
                                                             });
                                                         }
                                                     },
                                                 ))
                                                 .when(search_open, |el| {
                                                    el.child(
                                                        div()
                                                            .id("console-search-box")
                                                            .w(px(160.))
                                                            .h(px(22.))
                                                            .px_1()
                                                            .rounded_sm()
                                                            .border_1()
                                                            .border_color(theme.line)
                                                            .bg(theme.code)
                                                            .child(search_entity),
                                                    )
                                                })
                                                .child(self.btn(
                                                    if search_open { "关闭" } else { "搜索" },
                                                    false,
                                                    false,
                                                    true,
                                                    {
                                                        let entity = cx.entity();
                                                        move |_, window, app| {
                                                            entity.update(app, |this, cx| {
                                                                this.console_search_open =
                                                                    !this.console_search_open;
                                                                if this.console_search_open {
                                                                    this.console_search.update(
                                                                        cx,
                                                                        |inp, cx| {
                                                                            inp.set_text("", cx);
                                                                            inp.focus(window);
                                                                        },
                                                                    );
                                                                } else {
                                                                    this.console.update(
                                                                        cx,
                                                                        |c, cx| {
                                                                            c.set_search("", cx);
                                                                        },
                                                                    );
                                                                }
                                                                cx.notify();
                                                            });
                                                        }
                                                    },
                                                ))
                                                .child(self.btn("复制", false, false, true, {
                                                    let entity = cx.entity();
                                                    move |_, window, cx| {
                                                        entity.update(cx, |this, cx| {
                                                            this.copy_log(&CopyLog, window, cx);
                                                        });
                                                    }
                                                }))
                                                .child(self.btn("清屏", false, false, true, {
                                                    let entity = cx.entity();
                                                    move |_, window, cx| {
                                                        entity.update(cx, |this, cx| {
                                                            this.clear_log(&ClearLog, window, cx);
                                                        });
                                                    }
                                                }))
                                                .child(
                                                    div()
                                                        .id("console-hide")
                                                        .px_1()
                                                        .text_xs()
                                                        .text_color(theme.muted)
                                                        .cursor_pointer()
                                                        .hover(|s| s.text_color(theme.text))
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.show_console = false;
                                                            this.persist_layout();
                                                            cx.notify();
                                                        }))
                                                        .child("✕"),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .id("console-body")
                                                .flex_1()
                                                .min_h_0()
                                                .overflow_hidden()
                                                .child(self.console.clone()),
                                        ),
                                )
                            })
                            .when(!show_console, |el| {
                                el.child(
                                    div()
                                        .id("console-show-bar")
                                        .h(px(22.))
                                        .flex()
                                        .items_center()
                                        .px_2()
                                        .border_t_1()
                                        .border_color(theme.line)
                                        .bg(theme.panel)
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.show_console = true;
                                            if this.console_height < MIN_CONSOLE_H {
                                                this.console_height = DEFAULT_CONSOLE_H;
                                            }
                                            this.persist_layout();
                                            cx.notify();
                                        }))
                                        .child(
                                            div().text_xs().text_color(theme.muted).child("▸ 输出"),
                                        ),
                                )
                            }),
                    ),
            )
            // status bar
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_2()
                    .py_0p5()
                    .border_t_1()
                    .border_color(theme.line)
                    .bg(theme.panel)
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(if script_running {
                                theme.green
                            } else if shared.status.contains("失败") {
                                theme.red
                            } else {
                                theme.muted
                            })
                            .font_family("Cascadia Code")
                            .child(shared.status.clone()),
                    ),
            )
            // floating menu dropdown (above everything, not clipped)
            .when(open_menu.is_some(), |el| {
                let items = menu_items.unwrap_or_default();
                el.child(
                    div()
                        .id("menu-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .child(
                            // click outside to close
                            div()
                                .id("menu-backdrop")
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.open_menu = None;
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .id("menu-popup")
                                .absolute()
                                .top(px(36.))
                                .left(px(menu_left))
                                .min_w(px(248.))
                                .rounded_md()
                                .border_1()
                                .border_color(theme.line)
                                .bg(theme.panel)
                                .shadow_md()
                                .py_1()
                                .occlude()
                                .children(items),
                        ),
                )
            })
            // project tree context menu (window coords) + auto flyout 排序
            .when_some(self.tree_ctx.clone(), |el, (ctx_path, mx, my)| {
                if self.project_dir.is_none() {
                    return el;
                }
                let is_dir = ctx_path.is_dir();
                let is_root = self.project_dir.as_ref() == Some(&ctx_path);
                let blank = is_root;
                let sort_open = self.tree_ctx_sort;
                let cur_sort = self.tree_sort;
                let p_new = ctx_path.clone();
                let p_open = ctx_path.clone();
                let p_ren = ctx_path.clone();
                let p_del = ctx_path.clone();
                let p_exp = ctx_path.clone();
                let theme = theme;
                let entity = cx.entity();
                // Use Entity::update so nested move closures do not borrow/move `cx`.
                let mk = |id: SharedString,
                          label: SharedString,
                          danger: bool,
                          close_sort_on_hover: bool,
                          on: Box<dyn Fn(&mut IdeApp, &mut Context<IdeApp>) + 'static>| {
                    let theme = theme;
                    let ent_move = entity.clone();
                    let ent_down = entity.clone();
                    div()
                        .id(id)
                        .px_3()
                        .py_1p5()
                        .text_xs()
                        .text_color(if danger { theme.red } else { theme.text })
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.menu_hover))
                        .on_mouse_move(move |_, _, app| {
                            ent_move.update(app, |this, cx| {
                                if close_sort_on_hover {
                                    if this.tree_ctx_sort {
                                        this.tree_ctx_sort = false;
                                        cx.notify();
                                    }
                                } else if !this.tree_ctx_sort {
                                    this.tree_ctx_sort = true;
                                    cx.notify();
                                }
                            });
                        })
                        .on_mouse_down(gpui::MouseButton::Left, move |_, _, app| {
                            ent_down.update(app, |this, cx| {
                                this.tree_ctx = None;
                                this.tree_ctx_sort = false;
                                on(this, cx);
                            });
                        })
                        .child(label)
                        .into_any_element()
                };
                let sep = || div().h_px().mx_2().my_1().bg(theme.line).into_any_element();
                let mut items: Vec<gpui::AnyElement> = Vec::new();
                // 排序 flyout 挂在「排序」行上，top 与该行对齐
                let mut sort_items: Vec<gpui::AnyElement> = Vec::new();
                for (id, label, sort) in [
                    ("tree-sort-name", "名称", TreeSort::Name),
                    ("tree-sort-type", "类型", TreeSort::Type),
                    ("tree-sort-date", "日期", TreeSort::Date),
                    ("tree-sort-size", "大小", TreeSort::Size),
                ] {
                    let mark = if cur_sort == sort { "✓ " } else { "  " };
                    sort_items.push(mk(
                        id.into(),
                        format!("{mark}{label}").into(),
                        false,
                        false,
                        Box::new(move |this, cx| this.set_tree_sort(sort, cx)),
                    ));
                }
                let sort_row = {
                    let theme = theme;
                    let sort_items = sort_items;
                    let ent_open = entity.clone();
                    let ent_fly = entity.clone();
                    div()
                        .id("tree-sort")
                        .relative()
                        .px_3()
                        .py_1p5()
                        .text_xs()
                        .text_color(theme.text)
                        .cursor_pointer()
                        .bg(if sort_open {
                            theme.accent_soft
                        } else {
                            theme.panel
                        })
                        .hover(|s| s.bg(theme.menu_hover))
                        .on_mouse_move(move |_, _, app| {
                            ent_open.update(app, |this, cx| {
                                if !this.tree_ctx_sort {
                                    this.tree_ctx_sort = true;
                                    cx.notify();
                                }
                            });
                        })
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_between()
                                .gap_4()
                                .child("排序")
                                .child(div().text_color(theme.muted).child("›")),
                        )
                        .when(sort_open, move |el| {
                            el.child(
                                div()
                                    .id("tree-sort-flyout")
                                    .absolute()
                                    // Align flyout top with this row; sit just past the right edge.
                                    .left(px(146.))
                                    .top(px(0.))
                                    .min_w(px(120.))
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.line)
                                    .bg(theme.panel)
                                    .shadow_md()
                                    .py_1()
                                    .occlude()
                                    .on_mouse_move(move |_, _, app| {
                                        ent_fly.update(app, |this, cx| {
                                            if !this.tree_ctx_sort {
                                                this.tree_ctx_sort = true;
                                                cx.notify();
                                            }
                                        });
                                    })
                                    .children(sort_items),
                            )
                        })
                        .into_any_element()
                };
                if blank {
                    items.push(mk(
                        "tree-new".into(),
                        "新建".into(),
                        false,
                        true,
                        Box::new(move |this, cx| {
                            this.new_lua_in_project(Some(p_new.clone()), cx);
                        }),
                    ));
                    items.push(mk(
                        "tree-refresh".into(),
                        "刷新".into(),
                        false,
                        true,
                        Box::new(|this, cx| this.refresh_project_tree(cx)),
                    ));
                    items.push(sep());
                    items.push(sort_row);
                    items.push(sep());
                    items.push(mk(
                        "tree-explorer".into(),
                        "打开目录".into(),
                        false,
                        true,
                        Box::new(move |this, cx| {
                            this.open_in_explorer(p_exp.clone(), cx);
                        }),
                    ));
                } else {
                    if !is_dir {
                        items.push(mk(
                            "tree-open".into(),
                            "打开".into(),
                            false,
                            true,
                            Box::new(move |this, cx| {
                                this.open_tree_entry(p_open.clone(), cx);
                            }),
                        ));
                    }
                    items.push(mk(
                        "tree-rename".into(),
                        "重命名".into(),
                        false,
                        true,
                        Box::new(move |this, cx| {
                            this.begin_rename(p_ren.clone(), cx);
                        }),
                    ));
                    items.push(mk(
                        "tree-delete".into(),
                        "删除".into(),
                        true,
                        true,
                        Box::new(move |this, cx| {
                            this.begin_delete(p_del.clone(), cx);
                        }),
                    ));
                    items.push(sep());
                    items.push(sort_row);
                    items.push(sep());
                    items.push(mk(
                        "tree-explorer".into(),
                        "打开目录".into(),
                        false,
                        true,
                        Box::new(move |this, cx| {
                            this.open_in_explorer(p_exp.clone(), cx);
                        }),
                    ));
                }
                let menu_w = 148.0_f32;
                el.child(
                    div()
                        .id("tree-ctx-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .child(
                            div()
                                .id("tree-ctx-backdrop")
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.tree_ctx = None;
                                        this.tree_ctx_sort = false;
                                        cx.notify();
                                    }),
                                )
                                .on_mouse_down(
                                    gpui::MouseButton::Right,
                                    cx.listener(|this, _, _, cx| {
                                        this.tree_ctx = None;
                                        this.tree_ctx_sort = false;
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .id("tree-ctx-menu")
                                .absolute()
                                .left(px(mx.max(4.0)))
                                .top(px(my.max(4.0)))
                                .min_w(px(menu_w))
                                .rounded_md()
                                .border_1()
                                .border_color(theme.line)
                                .bg(theme.panel)
                                .shadow_md()
                                .py_1()
                                .occlude()
                                .children(items),
                        ),
                )
            })
            // output context menu (window coords from console right-click)
            .when_some(self.console_ctx, |el, (mx, my)| {
                let theme = theme;
                let has_sel = self.console.read(cx).has_selection();
                el.child(
                    div()
                        .id("console-ctx-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .child(
                            div()
                                .id("console-ctx-backdrop")
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.console_ctx = None;
                                        cx.notify();
                                    }),
                                )
                                .on_mouse_down(
                                    gpui::MouseButton::Right,
                                    cx.listener(|this, _, _, cx| {
                                        this.console_ctx = None;
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .id("console-ctx-menu")
                                .absolute()
                                .left(px(mx.max(4.0)))
                                .top(px(my.max(4.0)))
                                .min_w(px(128.))
                                .rounded_md()
                                .border_1()
                                .border_color(theme.line)
                                .bg(theme.panel)
                                .shadow_md()
                                .py_1()
                                .occlude()
                                .child(
                                    div()
                                        .id("console-ctx-copy")
                                        .px_3()
                                        .py_1p5()
                                        .text_xs()
                                        .text_color(theme.text)
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme.menu_hover))
                                        .on_mouse_down(
                                            gpui::MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                if has_sel {
                                                    this.console.update(cx, |c, cx| {
                                                        c.copy_selection_only(cx);
                                                    });
                                                    this.shared.lock().status = "已复制".into();
                                                } else {
                                                    this.console.update(cx, |c, cx| {
                                                        c.copy_all(cx);
                                                    });
                                                    this.shared.lock().status = "已复制全部".into();
                                                }
                                                this.console_ctx = None;
                                                cx.notify();
                                            }),
                                        )
                                        .child(if has_sel { "复制" } else { "复制全部" }),
                                )
                                .child(
                                    div()
                                        .id("console-ctx-clear")
                                        .px_3()
                                        .py_1p5()
                                        .text_xs()
                                        .text_color(theme.text)
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme.menu_hover))
                                        .on_mouse_down(
                                            gpui::MouseButton::Left,
                                            cx.listener(|this, _, window, cx| {
                                                this.console_ctx = None;
                                                this.clear_log(&ClearLog, window, cx);
                                            }),
                                        )
                                        .child("清屏"),
                                ),
                        ),
                )
            })
            // modal dialogs (settings / about / keys)
            .when_some(self.dialog, |el, kind| {
                el.child(self.render_dialog(kind, cx))
            })
    }
}

struct ModularRunSummary {
    modules: Vec<String>,
    modules_updated: bool,
    bundle_sha256: String,
    script_count: usize,
    lua_bytes: usize,
}

#[derive(Clone, Default)]
struct SessionRunCache {
    initialized: bool,
    scope: String,
    catalog_sha256: String,
    modules: BTreeSet<String>,
    script_hashes: HashMap<String, String>,
}

struct CompiledScript {
    source_path: Option<PathBuf>,
    upload_name: String,
    bytes: Vec<u8>,
    entry: bool,
}

fn execute_modular_run(
    compiler: &Path,
    session: Arc<SerialSession>,
    project_dir: Option<&Path>,
    meta: &ProjectMeta,
    overlays: &[(PathBuf, String)],
    fallback_source: &str,
    transfer_mode: TransferMode,
    run_cache: &Arc<Mutex<SessionRunCache>>,
    font_zh: &str,
    font_en: &str,
    mut progress: impl FnMut(&str),
) -> anyhow::Result<ModularRunSummary> {
    progress("验证 catalog、API 与模块变体…");
    let mut prepared = modular::prepare_run(project_dir, meta, overlays, fallback_source)?;
    let scope = project_dir
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<single-buffer>".into());
    let cache_before = run_cache.lock().clone();
    let same_session_target = cache_before.initialized
        && cache_before.scope == scope
        && cache_before.catalog_sha256 == prepared.catalog.catalog_sha256;
    if same_session_target {
        let requested: BTreeSet<String> = cache_before
            .modules
            .iter()
            .cloned()
            .chain(prepared.deployment.modules.iter().cloned())
            .collect();
        prepared.deployment = prepared.catalog.plan_modules(&requested)?;
    }
    let planned_modules: BTreeSet<String> = prepared.deployment.modules.iter().cloned().collect();
    let module_label = if prepared.deployment.modules.is_empty() {
        "core".to_string()
    } else {
        prepared.deployment.modules.join(", ")
    };
    progress(&format!(
        "模块计划 [{}] · NMUP {} B · 编译 Lua…",
        module_label,
        prepared.deployment.bundle.len()
    ));

    let font_module = fontpack::build_oled_font_module(&prepared.combined_source, font_zh, font_en)
        .map_err(|error| anyhow::anyhow!("OLED 自动取模失败: {error}"))?;
    let mut compiled =
        Vec::with_capacity(prepared.scripts.len() + usize::from(font_module.is_some()));
    if let Some((message, source)) = font_module {
        progress(&message);
        let bytes = compile_source(compiler, &source)
            .map_err(|error| anyhow::anyhow!("编译 _oled_font.luac 失败: {error:#}"))?;
        if bytes.len() > 20 * 1024 {
            anyhow::bail!("_oled_font.luac 为 {} B，超过 20 KiB 安全上限", bytes.len());
        }
        compiled.push(CompiledScript {
            source_path: None,
            upload_name: "_oled_font.luac".into(),
            bytes,
            entry: false,
        });
    }
    for script in &prepared.scripts {
        progress(&format!("编译 {}…", script.upload_name));
        let bytes = compile_source(compiler, &script.source)
            .map_err(|error| anyhow::anyhow!("编译 {} 失败: {error:#}", script.upload_name))?;
        compiled.push(CompiledScript {
            source_path: script.source_path.clone(),
            upload_name: script.upload_name.clone(),
            bytes,
            entry: script.entry,
        });
    }
    let lua_bytes = compiled.iter().map(|script| script.bytes.len()).sum();
    let compiled_hashes: HashMap<String, String> = compiled
        .iter()
        .map(|script| (script.upload_name.clone(), sha256_bytes(&script.bytes)))
        .collect();
    let mut module_phase_complete = false;
    let transaction = (|| -> anyhow::Result<ModularRunSummary> {
        progress("停止当前脚本…");
        session.stop_and_wait()?;
        if session.current_baud() != APP_SERIAL_BAUD {
            progress("恢复串口到 115200…");
            session.recover_to_115200(&prepared.catalog)?;
            session.stop_and_wait()?;
        }

        progress("核验固件身份…");
        session.query_and_verify_firmware(&prepared.catalog)?;
        progress("读取当前模块槽…");
        let before = session.module_status()?;
        progress("探测 LittleFS…");
        if !session.probe_lfs()? {
            anyhow::bail!("SCRIPT_ERR name/fs：LittleFS 未挂载");
        }

        if transfer_mode == TransferMode::High {
            progress("串口升速到 460800…");
            session.switch_baud(460_800)?;
        } else {
            progress("低速模式：保持 115200…");
        }

        let modules_updated = before.pending
            || before.has_bad_slot()
            || !before.matches_plan(&prepared.deployment.slots);
        if modules_updated {
            progress(&format!("部署原生模块 [{}]…", module_label));
            session.apply_module_bundle(
                &prepared.deployment.bundle,
                prepared.deployment.modules.len(),
                |message| progress(message),
            )?;
            module_phase_complete = true;
            progress("后检固件身份与模块槽…");
            session.query_and_verify_firmware(&prepared.catalog)?;
            let after = session.module_status()?;
            if !after.matches_plan(&prepared.deployment.slots) {
                anyhow::bail!("模块事务虽已返回成功，但 modstatus 后检与计划布局不一致");
            }
        }
        module_phase_complete = true;

        // Upload dependencies first. Uploading the entry script starts Lua immediately,
        // so no command may be issued after its successful SCRIPT_OK acknowledgement.
        for script in compiled.iter().filter(|script| !script.entry) {
            let changed = !same_session_target
                || cache_before.script_hashes.get(&script.upload_name)
                    != compiled_hashes.get(&script.upload_name);
            if !changed {
                progress(&format!("跳过未修改的 {}", script.upload_name));
                continue;
            }
            progress(&format!("上传 {}…", script.upload_name));
            session.upload_hex_strict_with_progress(
                &script.upload_name,
                &script.bytes,
                |message| progress(message),
            )?;
        }

        progress("启动前核验模块槽…");
        let final_status = session.module_status()?;
        if !final_status.matches_plan(&prepared.deployment.slots) {
            anyhow::bail!("启动前 modstatus 与计划模块布局不一致");
        }
        // Keep the negotiated high baud for main.luac itself. A long-running
        // entry script cannot safely be interrupted merely to switch back.
        for script in compiled.iter().filter(|script| script.entry) {
            progress(&format!("启动 {}…", script.upload_name));
            session.upload_hex_strict_with_progress(
                &script.upload_name,
                &script.bytes,
                |message| progress(message),
            )?;
        }
        progress("Lua 已启动；点击停止结束运行");
        write_modular_run_state(project_dir, &prepared, &compiled, "complete", None)?;
        *run_cache.lock() = SessionRunCache {
            initialized: true,
            scope,
            catalog_sha256: prepared.catalog.catalog_sha256.clone(),
            modules: planned_modules,
            script_hashes: compiled_hashes,
        };
        Ok(ModularRunSummary {
            modules: prepared.deployment.modules.clone(),
            modules_updated,
            bundle_sha256: prepared.deployment.bundle_sha256.clone(),
            script_count: compiled.len(),
            lua_bytes,
        })
    })();

    match transaction {
        Ok(summary) => Ok(summary),
        Err(error) => {
            let phase = if module_phase_complete {
                "原生模块阶段已成功；不会回滚模块，只需重试 Lua 阶段"
            } else {
                "原生模块阶段未达到 MOD_DONE + Idle；已禁止 Lua 阶段"
            };
            let _ = session.stop_and_wait();
            let recovery = if session.current_baud() != APP_SERIAL_BAUD {
                session
                    .recover_to_115200(&prepared.catalog)
                    .err()
                    .map(|recovery_error| format!("；恢复 115200 失败: {recovery_error:#}"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let message = format!("{phase}: {error:#}{recovery}");
            let record_error = write_modular_run_state(
                project_dir,
                &prepared,
                &compiled,
                if module_phase_complete {
                    "modules_complete_lua_failed"
                } else {
                    "modules_incomplete"
                },
                Some(&message),
            )
            .err()
            .map(|record_error| format!("；写入事务记录失败: {record_error:#}"))
            .unwrap_or_default();
            Err(anyhow::anyhow!("{message}{record_error}"))
        }
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn write_modular_run_state(
    project_dir: Option<&Path>,
    prepared: &modular::PreparedRun,
    compiled: &[CompiledScript],
    phase: &str,
    error: Option<&str>,
) -> anyhow::Result<()> {
    let Some(project_dir) = project_dir else {
        return Ok(());
    };
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let slots: Vec<_> = prepared
        .deployment
        .slots
        .iter()
        .map(|slot| {
            serde_json::json!({
                "slot": slot.slot,
                "name": slot.name,
                "size": slot.size,
                "crc32": format!("{:08x}", slot.crc32),
            })
        })
        .collect();
    let scripts: Vec<_> = compiled
        .iter()
        .map(|script| {
            serde_json::json!({
                "name": script.upload_name,
                "source": script.source_path.as_ref().map(|path| path.display().to_string()),
                "length": script.bytes.len(),
                "sha256": format!("{:x}", Sha256::digest(&script.bytes)),
                "entry": script.entry,
            })
        })
        .collect();
    let state = serde_json::json!({
        "schema": 1,
        "updated_unix_ms": timestamp_ms,
        "phase": phase,
        "error": error,
        "catalog": {
            "id": prepared.catalog.identity.id,
            "version": prepared.catalog.identity.version,
            "target": prepared.catalog.identity.target,
            "core_abi": prepared.catalog.identity.core_abi,
            "module_format": prepared.catalog.identity.module_format,
            "nmup_format": prepared.catalog.identity.nmup_format,
            "sha256": prepared.catalog.catalog_sha256,
        },
        "modules": prepared.deployment.modules,
        "slots": slots,
        "bundle": {
            "length": prepared.deployment.bundle.len(),
            "sha256": prepared.deployment.bundle_sha256,
        },
        "lua": scripts,
    });
    let path = project_dir.join(".mspm0-run-state.json");
    let bytes = serde_json::to_vec_pretty(&state)?;
    fs::write(&path, bytes)
        .map_err(|error| anyhow::anyhow!("写入模块化运行记录 {}: {error}", path.display()))
}

struct SharedView {
    busy: bool,
    script_running: bool,
    connected: bool,
    port_name: Option<String>,
    files: Vec<(String, u64)>,
    selected_file: Option<String>,
    status: String,
}

impl SharedState {
    fn clone_view(&self) -> SharedView {
        SharedView {
            busy: self.busy,
            script_running: self.script_running,
            connected: self.connected,
            port_name: self.port_name.clone(),
            files: self.files.clone(),
            selected_file: self.selected_file.clone(),
            status: self.status.clone(),
        }
    }
}

fn badge(text: &str, on: bool, theme: &Theme) -> impl IntoElement {
    div()
        .px_2()
        .py_1()
        .rounded_full()
        .border_1()
        .border_color(if on { theme.green } else { theme.line })
        .bg(theme.panel2)
        .text_xs()
        .text_color(if on { theme.green } else { theme.muted })
        .child(text.to_string())
}

fn console_display_text(kind: LogKind, line: &str, full_output: bool) -> Option<String> {
    let text = line.trim();
    if text.is_empty() {
        return None;
    }
    if full_output {
        return Some(text.to_string());
    }
    match kind {
        LogKind::Tx => None,
        LogKind::Diag => None,
        LogKind::Sys => {
            if text.starts_with("模块化运行已启动") {
                Some("运行中".to_string())
            } else if text.starts_with("模块化运行成功") {
                Some("运行完成".to_string())
            } else {
                None
            }
        }
        LogKind::Rx if is_transport_noise(text) => None,
        LogKind::Rx => Some(concise_runtime_line(text)),
        LogKind::Err => Some(concise_error_line(text)),
    }
}

fn concise_runtime_line(text: &str) -> String {
    for prefix in ["LUA run err:", "LUA load err:"] {
        if let Some(detail) = text.strip_prefix(prefix) {
            let detail = detail.trim().trim_start_matches("?:?:").trim();
            return if detail.is_empty() {
                "Lua 运行错误".into()
            } else {
                format!("Lua 错误：{detail}")
            };
        }
    }
    text.to_string()
}

fn concise_error_line(text: &str) -> String {
    if text.contains("not enough memory") {
        return "运行失败：Lua 内存不足".into();
    }
    if text.contains("SCRIPT_DONE ERR") {
        return "运行失败：Lua 脚本执行出错".into();
    }
    if text.contains("BAUD_") || text.contains("波特率") || text.contains("恢复 115200") {
        return "运行失败：串口高速切换失败".into();
    }
    if text.contains("LittleFS") || text.contains("name/fs") {
        return "运行失败：Flash 文件系统未就绪".into();
    }
    const LIMIT: usize = 200;
    if text.chars().count() <= LIMIT {
        return text.to_string();
    }
    let mut short = text.chars().take(LIMIT).collect::<String>();
    short.push('…');
    short
}

/// Map protocol chatter to a compact status-bar string.
fn protocol_status_update(line: &str) -> Option<String> {
    let t = normalize_transport_line(line.trim());
    if t == "HEX_OK" || t.starts_with("HEX_OK") {
        return Some("上传中…".into());
    }
    if t.starts_with("SCRIPT_OK") {
        // e.g. "SCRIPT_OK 275"
        let size = t.split_whitespace().nth(1).unwrap_or("");
        if size.is_empty() {
            return Some("上传完成 · 运行中".into());
        }
        return Some(format!("上传完成 · {size} B · 运行中"));
    }
    if t == "SCRIPT_BEGIN" || t.starts_with("SCRIPT_BEGIN") {
        return Some("开始上传…".into());
    }
    if t == "SCRIPT_DONE OK" || t.contains("LED_BLINK_DONE") {
        return Some("完成".into());
    }
    if t == "SCRIPT_DONE ERR" {
        return Some("脚本执行失败".into());
    }
    if t == "SCRIPT_DONE PENDING" {
        return Some("模块恢复未完成".into());
    }
    if t == "STOP" || t == "stopped" || t.starts_with("stopped") {
        return Some("已停止".into());
    }
    None
}

#[cfg(test)]
mod console_output_tests {
    use super::{
        append_operating_rules, append_api_summary, append_board_capability_summary,
        console_display_text, LogKind,
    };
    use serde_json::{json, Value};
    use std::path::Path;

    #[test]
    fn concise_output_keeps_lua_print_and_hides_protocol() {
        assert_eq!(
            console_display_text(LogKind::Rx, "hello 123", false).as_deref(),
            Some("hello 123")
        );
        assert!(console_display_text(LogKind::Rx, "MOD_SLOT 0 i2c 3352 18c593d8", false)
            .is_none());
        assert!(console_display_text(LogKind::Sys, "Flash 文件 3 个", false).is_none());
    }

    #[test]
    fn concise_output_reports_run_result_and_short_error() {
        assert_eq!(
            console_display_text(
                LogKind::Sys,
                "模块化运行已启动 · [gpio, tmr] · 点击停止结束",
                false
            )
            .as_deref(),
            Some("运行中")
        );
        assert_eq!(
            console_display_text(
                LogKind::Sys,
                "模块化运行成功 · [i2c] · NMUP abcdef",
                false
            )
            .as_deref(),
            Some("运行完成")
        );
        assert_eq!(
            console_display_text(
                LogKind::Rx,
                "LUA run err: ?:?: attempt to call a nil value",
                false
            )
            .as_deref(),
            Some("Lua 错误：attempt to call a nil value")
        );
        assert_eq!(
            console_display_text(LogKind::Err, "脚本执行失败：SCRIPT_DONE ERR", false)
                .as_deref(),
            Some("运行失败：Lua 脚本执行出错")
        );
    }

    #[test]
    fn full_output_preserves_protocol_verbatim() {
        assert_eq!(
            console_display_text(LogKind::Rx, "FW_INFO_END", true).as_deref(),
            Some("FW_INFO_END")
        );
    }

    #[test]
    fn context_api_summary_includes_call_contracts() {
        let api = json!({
            "id": "test.lua",
            "version": "1.2.3",
            "firmware": { "version": "2.0.0", "lua_version": "5.5.0" },
            "globals": [],
            "modules": [{
                "name": "i2c",
                "description": "I2C bus",
                "extensions": { "mspm0.native_module": {
                    "id": "i2c", "minimum_version": "1.0.1",
                    "dependencies": [], "conflicts": []
                }},
                "functions": [{
                    "name": "write",
                    "aliases": ["send"],
                    "overloads": [{
                        "params": [
                            { "name": "scl", "type": "pin", "resource": {
                                "kind": "pin", "scope": "board.exposed",
                                "capability": { "class": "i2c", "signal": "SCL" }
                            }},
                            { "name": "address", "type": "integer", "optional": true,
                              "default": 60, "value_constraints": { "minimum": 0, "maximum": 127 }}
                        ],
                        "returns": [{ "name": "ok", "type": "boolean" }],
                        "effects": [{ "kind": "write", "target": "bus", "lifetime": "call" }]
                    }],
                    "extensions": { "mspm0.blocking": true, "mspm0.errors": ["i2c:nack"] }
                }]
            }]
        });
        let mut summary = String::new();
        append_api_summary(&mut summary, &api);

        assert!(summary.contains("API: test.lua v1.2.3; firmware v2.0.0; Lua 5.5.0"));
        assert!(summary.contains("native module: i2c >= 1.0.1"));
        assert!(summary.contains(
            "i2c.write(scl: pin {board.exposed; class=i2c; signal=SCL}, [address: integer [0..127] = 60]) -> ok: boolean"
        ));
        assert!(summary.contains("aliases: send; blocking; effects: write bus (call); errors: i2c:nack"));
    }

    #[test]
    fn context_real_api_keeps_gpio_pin_and_mode_requirements() {
        let api: Value = serde_json::from_str(include_str!("../apis/mspm0g3507_lua.json"))
            .expect("bundled API metadata must be valid JSON");
        let mut summary = String::new();
        append_api_summary(&mut summary, &api);

        assert!(summary.contains("gpio.mode(pin: pin {board.exposed; class=gpio}"));
        assert!(summary.contains("[mode: string one of {\"out\", \"od\", \"analog\", \"in\", \"in_pu\", \"in_pd\"} = \"in\"]"));
        assert!(summary.contains("errors: gpio:pin, gpio:busy, gpio:mode"));
        assert!(summary.contains("i2c."));
        assert!(summary.contains("### I2C quick reference"));
        assert!(summary.contains("i2c.write_on(id: integer [0..1]"));
    }

    #[test]
    fn context_exposes_board_i2c1_routing_before_full_api_list() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let board: Value = serde_json::from_str(include_str!("../boards/LKDMX.json"))
            .expect("bundled board metadata must be valid JSON");
        let mut summary = String::new();
        append_operating_rules(&mut summary);
        append_board_capability_summary(&mut summary, root, "mspm0g3507", &board);

        assert!(summary.contains("no standard `math`, `string`, or `table` library"));
        let i2c1 = summary
            .lines()
            .find(|line| line.starts_with("- I2C1:"))
            .expect("I2C1 routing must be present");
        assert!(i2c1.contains("PB2"));
        assert!(i2c1.contains("PB3"));
        let spi0 = summary
            .lines()
            .find(|line| line.starts_with("- SPI0:"))
            .expect("SPI0 routing must be present");
        assert!(spi0.contains("PA12"));
        assert!(spi0.contains("PA14"));
        assert!(spi0.contains("PA13"));
        assert!(summary.contains("CS is a distinct exposed GPIO"));
    }
}

pub fn run() {
    Application::new().run(|cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("backspace", Backspace, Some("CodeEditor")),
            KeyBinding::new("delete", Delete, Some("CodeEditor")),
            KeyBinding::new("left", Left, Some("CodeEditor")),
            KeyBinding::new("right", Right, Some("CodeEditor")),
            KeyBinding::new("up", Up, Some("CodeEditor")),
            KeyBinding::new("down", Down, Some("CodeEditor")),
            KeyBinding::new("shift-left", SelectLeft, Some("CodeEditor")),
            KeyBinding::new("shift-right", SelectRight, Some("CodeEditor")),
            KeyBinding::new("ctrl-a", SelectAll, Some("CodeEditor")),
            KeyBinding::new("ctrl-v", Paste, Some("CodeEditor")),
            KeyBinding::new("ctrl-c", Copy, Some("CodeEditor")),
            KeyBinding::new("ctrl-x", Cut, Some("CodeEditor")),
            KeyBinding::new("ctrl-z", Undo, Some("CodeEditor")),
            KeyBinding::new("ctrl-y", Redo, Some("CodeEditor")),
            KeyBinding::new("ctrl-shift-z", Redo, Some("CodeEditor")),
            KeyBinding::new("home", Home, Some("CodeEditor")),
            KeyBinding::new("end", End, Some("CodeEditor")),
            KeyBinding::new("enter", Enter, Some("CodeEditor")),
            KeyBinding::new("tab", Tab, Some("CodeEditor")),
            KeyBinding::new("escape", EditorEscape, Some("CodeEditor")),
            KeyBinding::new("ctrl-space", AcceptCompletion, Some("CodeEditor")),
            KeyBinding::new("f5", Run, Some("IdeApp")),
            KeyBinding::new("escape", Stop, Some("IdeApp")),
            KeyBinding::new("ctrl-enter", Run, Some("IdeApp")),
            KeyBinding::new("ctrl-shift-c", CopyLog, Some("IdeApp")),
    KeyBinding::new("ctrl-shift-i", CopyProjectContext, Some("IdeApp")),
            KeyBinding::new("ctrl-s", SaveFile, Some("IdeApp")),
            KeyBinding::new("ctrl-shift-s", SaveFileAs, Some("IdeApp")),
            KeyBinding::new("ctrl-o", OpenSource, Some("IdeApp")),
            KeyBinding::new("ctrl-shift-o", OpenProject, Some("IdeApp")),
            KeyBinding::new("ctrl-shift-n", NewProject, Some("IdeApp")),
            KeyBinding::new("ctrl-t", CycleTheme, Some("IdeApp")),
            KeyBinding::new("ctrl-c", ConsoleCopy, Some("ConsoleView")),
            KeyBinding::new("ctrl-a", ConsoleSelectAll, Some("ConsoleView")),
            KeyBinding::new("backspace", LiBackspace, Some("LineInput")),
            KeyBinding::new("delete", LiDelete, Some("LineInput")),
            KeyBinding::new("left", LiLeft, Some("LineInput")),
            KeyBinding::new("right", LiRight, Some("LineInput")),
            KeyBinding::new("home", LiHome, Some("LineInput")),
            KeyBinding::new("end", LiEnd, Some("LineInput")),
            KeyBinding::new("enter", LiEnter, Some("LineInput")),
            KeyBinding::new("escape", LiEscape, Some("LineInput")),
            KeyBinding::new("ctrl-a", LiSelectAll, Some("LineInput")),
            KeyBinding::new("ctrl-v", LiPaste, Some("LineInput")),
            KeyBinding::new("ctrl-c", LiCopy, Some("LineInput")),
            KeyBinding::new("ctrl-x", LiCut, Some("LineInput")),
        ]);

        let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // Immersive titlebar: hide system chrome, app draws bar +
                // WindowControlArea hit-test for drag / min / max / close.
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Lua IDE".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                window_background: gpui::WindowBackgroundAppearance::Opaque,
                ..Default::default()
            },
            |window, cx| cx.new(|cx| IdeApp::new(window, cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}

fn build_project_context(
    project_dir: Option<&Path>,
    source_path: Option<&Path>,
    fallback_source: &str,
    selected_board: Option<&str>,
) -> String {
    let mut out = String::from("# Lua IDE context\n");
    let root = AppSettings::exe_dir();
    let mut api_chip = None;
    let mut board_metadata = None;
    if let Some(board_id) = selected_board {
        let board_path = root.join("boards").join(format!("{board_id}.json"));
        match fs::read_to_string(&board_path) {
            Ok(text) => {
                board_metadata = serde_json::from_str::<Value>(&text).ok();
                api_chip = board_metadata
                    .as_ref()
                    .and_then(|board| board.get("chip").and_then(Value::as_str).map(str::to_owned));
                out.push_str("\n## Board\n");
                out.push_str(&text);
                out.push('\n');
            }
            Err(error) => out.push_str(&format!("\n## Board\nUnavailable: {error}\n")),
        }
    }

    let api_chip = api_chip.unwrap_or_else(|| "mspm0g3507".to_string());
    append_operating_rules(&mut out);
    if let Some(board) = board_metadata.as_ref() {
        append_board_capability_summary(&mut out, &root, &api_chip, board);
    }
    let api_path = root.join("apis").join(format!("{api_chip}_lua.json"));
    out.push_str("\n## Available API\n");
    match fs::read_to_string(&api_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    {
        Some(api) => append_api_summary(&mut out, &api),
        None => out.push_str("Unavailable\n"),
    }

    out.push_str("\n## Workspace files\n");
    if let Some(dir) = project_dir {
        let mut files = Vec::new();
        collect_workspace_files(dir, dir, &mut files);
        files.sort();
        for path in files {
            let relative = path.strip_prefix(dir).unwrap_or(&path);
            append_workspace_file(&mut out, relative, &path);
        }
    } else {
        let name = source_path
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("untitled.lua");
        out.push_str(&format!("\n--- {name}\n{fallback_source}\n"));
    }
    out
}

fn append_operating_rules(out: &mut String) {
    out.push_str("\n## Operating rules\n");
    out.push_str("- The Board routing and API signatures below are authoritative. Do not invent functions, parameters, pin pairs, or module IDs.\n");
    out.push_str("- A firmware API absent from this context is unsupported. State uncertainty instead of substituting a guessed API.\n");
    out.push_str("- This firmware has no standard `math`, `string`, or `table` library. For integer color bytes use `value // 256` and `value % 256`; use `iq.*` for trigonometry. Do not use string.char/string.byte/string.format: use i2c.bytes/spi.bytes (1..3 bytes), byte(data, index), and multi-argument print.\n");
    out.push_str("- I2C addresses are 7-bit values: MPU6050 uses 0x68 or 0x69, never 0xD0/0xD1. Diagnose with i2c.valid then i2c.probe_on before register reads.\n");
    out.push_str("- For spi.xfer_on, SCK/PICO/POCI must be a same-instance route listed below and pass spi.valid. CS is a distinct exposed GPIO; DC and RST must also be distinct GPIOs, never reused as SPI bus pins. Use spi.bytes for SPI payload bytes, not i2c.bytes.\n");
}

fn append_board_capability_summary(out: &mut String, root: &Path, chip_name: &str, board: &Value) {
    let exposed = board
        .get("pins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let chip_path = root.join("chips").join(format!("{chip_name}.json"));
    let Ok(chip_text) = fs::read_to_string(chip_path) else {
        return;
    };
    let Ok(chip) = serde_json::from_str::<Value>(&chip_text) else {
        return;
    };
    let Some(chip_pins) = chip.get("pins").and_then(Value::as_object) else {
        return;
    };

    let mut i2c_signals: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for pin in exposed {
        let Some(functions) = chip_pins.get(pin).and_then(Value::as_array) else {
            continue;
        };
        for function in functions.iter().filter_map(Value::as_str) {
            let Some((instance, signal)) = function.split_once('_') else {
                continue;
            };
            if !instance.starts_with("I2C") || !matches!(signal, "SCL" | "SDA") {
                continue;
            }
            i2c_signals
                .entry(instance.to_string())
                .or_default()
                .entry(signal.to_string())
                .or_default()
                .insert(pin.to_string());
        }
    }
    out.push_str("\n## Board peripheral routing\n");
    if !i2c_signals.is_empty() {
        out.push_str("Use the same I2C instance for SCL and SDA; only the exposed pins below are valid for this board.\n");
        for (instance, signals) in i2c_signals {
            let scl = signals
                .get("SCL")
                .map(|pins| pins.iter().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_else(|| "none".into());
            let sda = signals
                .get("SDA")
                .map(|pins| pins.iter().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_else(|| "none".into());
            out.push_str(&format!("- {instance}: SCL = {scl}; SDA = {sda}\n"));
        }
    }

    let mut spi_signals: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for pin in board
        .get("pins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let Some(functions) = chip_pins.get(pin).and_then(Value::as_array) else {
            continue;
        };
        for function in functions.iter().filter_map(Value::as_str) {
            let Some((instance, signal)) = function.split_once('_') else {
                continue;
            };
            let signal = match signal {
                "SCLK" => "SCK",
                "PICO" => "PICO",
                "POCI" => "POCI",
                _ => continue,
            };
            if !instance.starts_with("SPI") {
                continue;
            }
            spi_signals
                .entry(instance.to_string())
                .or_default()
                .entry(signal.to_string())
                .or_default()
                .insert(pin.to_string());
        }
    }
    if !spi_signals.is_empty() {
        out.push_str("SPI requires one matching instance for SCK/PICO/POCI. CS/DC/RST are distinct exposed GPIOs.\n");
        for (instance, signals) in spi_signals {
            let sck = signals
                .get("SCK")
                .map(|pins| pins.iter().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_else(|| "none".into());
            let pico = signals
                .get("PICO")
                .map(|pins| pins.iter().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_else(|| "none".into());
            let poci = signals
                .get("POCI")
                .map(|pins| pins.iter().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_else(|| "none".into());
            out.push_str(&format!("- {instance}: SCK = {sck}; PICO = {pico}; POCI = {poci}\n"));
        }
    }
}

fn append_api_summary(out: &mut String, api: &Value) {
    let id = api.get("id").and_then(Value::as_str).unwrap_or("unknown");
    let version = api.get("version").and_then(Value::as_str).unwrap_or("unknown");
    let firmware = api.get("firmware").unwrap_or(&Value::Null);
    let firmware_version = firmware
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let lua_version = firmware
        .get("lua_version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    out.push_str(&format!(
        "API: {id} v{version}; firmware v{firmware_version}; Lua {lua_version}\n"
    ));
    out.push_str(
        "Notation: [param] is optional; = is the default; pin{...} restricts the pin to the exposed board pins with the listed capability.\n",
    );
    append_i2c_quick_reference(out, api);

    out.push_str("\n### Global functions\n");
    for function in api
        .get("globals")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        append_api_function(out, "", function);
    }

    out.push_str("\n### Modules\n");
    for module in api
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = module.get("name").and_then(Value::as_str) else {
            continue;
        };
        out.push_str(&format!("\n#### {name}"));
        if let Some(description) = module.get("description").and_then(Value::as_str) {
            out.push_str(&format!(" - {description}"));
        }
        out.push('\n');
        append_module_requirements(out, module);
        for function in module
            .get("functions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            append_api_function(out, &format!("{name}."), function);
        }
    }
}

fn append_i2c_quick_reference(out: &mut String, api: &Value) {
    let Some(module) = api
        .get("modules")
        .and_then(Value::as_array)
        .and_then(|modules| modules.iter().find(|module| module.get("name").and_then(Value::as_str) == Some("i2c")))
    else {
        return;
    };
    let wanted = ["valid", "probe_on", "write_on", "read_on", "write_read_on", "bytes"];
    out.push_str("\n### I2C quick reference\n");
    for name in wanted {
        let Some(function) = module
            .get("functions")
            .and_then(Value::as_array)
            .and_then(|functions| functions.iter().find(|function| function.get("name").and_then(Value::as_str) == Some(name)))
        else {
            continue;
        };
        let Some(overload) = function
            .get("overloads")
            .and_then(Value::as_array)
            .and_then(|overloads| overloads.first())
        else {
            continue;
        };
        let params = overload
            .get("params")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(format_api_param)
            .collect::<Vec<_>>();
        out.push_str(&format!("- i2c.{name}({})", params.join(", ")));
        let returns = format_api_returns(overload.get("returns"));
        if !returns.is_empty() {
            out.push_str(&format!(" -> {returns}"));
        }
        out.push('\n');
    }
}

fn append_module_requirements(out: &mut String, module: &Value) {
    let extensions = module.get("extensions").unwrap_or(&Value::Null);
    if let Some(native) = extensions.get("mspm0.native_module") {
        let id = native.get("id").and_then(Value::as_str).unwrap_or("unknown");
        let minimum = native
            .get("minimum_version")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let dependencies = json_string_list(native.get("dependencies"));
        let conflicts = json_string_list(native.get("conflicts"));
        out.push_str(&format!("  native module: {id} >= {minimum}"));
        if !dependencies.is_empty() {
            out.push_str(&format!("; depends on {}", dependencies.join(", ")));
        }
        if !conflicts.is_empty() {
            out.push_str(&format!("; conflicts with {}", conflicts.join(", ")));
        }
        out.push('\n');
    }
    if let Some(injected) = extensions.get("mspm0.compiler_injected") {
        let runtime = injected
            .get("runtime")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let required = json_string_list(injected.get("required_native_modules"));
        let triggers = json_string_list(injected.get("trigger_symbols"));
        out.push_str(&format!("  compiler injected: {runtime}"));
        if !required.is_empty() {
            out.push_str(&format!("; native modules: {}", required.join(", ")));
        }
        if !triggers.is_empty() {
            out.push_str(&format!("; enabled by: {}", triggers.join(", ")));
        }
        out.push('\n');
    }
}

fn append_api_function(out: &mut String, prefix: &str, function: &Value) {
    let Some(name) = function.get("name").and_then(Value::as_str) else {
        return;
    };
    let qualified_name = format!("{prefix}{name}");
    let aliases = json_string_list(function.get("aliases"));
    let function_extensions = function.get("extensions").unwrap_or(&Value::Null);
    let blocking = function_extensions.get("mspm0.blocking").and_then(Value::as_bool);
    let errors = json_string_list(function_extensions.get("mspm0.errors"));
    let overloads = function
        .get("overloads")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    for (index, overload) in overloads.iter().enumerate() {
        let params = overload
            .get("params")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(format_api_param)
            .collect::<Vec<_>>();
        out.push_str(&format!("- {qualified_name}({})", params.join(", ")));
        let returns = format_api_returns(overload.get("returns"));
        if !returns.is_empty() {
            out.push_str(&format!(" -> {returns}"));
        }
        if overloads.len() > 1 {
            out.push_str(&format!(" [overload {}]", index + 1));
        }
        if !aliases.is_empty() {
            out.push_str(&format!("; aliases: {}", aliases.join(", ")));
        }
        if let Some(is_blocking) = blocking {
            out.push_str(if is_blocking { "; blocking" } else { "; nonblocking" });
        }
        let effects = format_api_effects(overload.get("effects"));
        if !effects.is_empty() {
            out.push_str(&format!("; effects: {effects}"));
        }
        if !errors.is_empty() {
            out.push_str(&format!("; errors: {}", errors.join(", ")));
        }
        out.push('\n');
    }
}

fn format_api_param(param: &Value) -> String {
    let name = param.get("name").and_then(Value::as_str).unwrap_or("value");
    let ty = param.get("type").and_then(Value::as_str).unwrap_or("any");
    let mut text = format!("{name}: {ty}");
    if param.get("variadic").and_then(Value::as_bool).unwrap_or(false) {
        text.push_str("...");
    }
    let constraints = format_api_constraints(param.get("value_constraints"));
    if !constraints.is_empty() {
        text.push_str(&format!(" {constraints}"));
    }
    let resource = format_api_resource(param.get("resource"));
    if !resource.is_empty() {
        text.push_str(&format!(" {{{resource}}}"));
    }
    if let Some(default) = param.get("default") {
        text.push_str(&format!(" = {default}"));
    }
    if param.get("optional").and_then(Value::as_bool).unwrap_or(false) {
        text = format!("[{text}]");
    }
    text
}

fn format_api_constraints(constraints: Option<&Value>) -> String {
    let Some(constraints) = constraints else {
        return String::new();
    };
    let minimum = constraints.get("minimum");
    let maximum = constraints.get("maximum");
    let mut parts = Vec::new();
    if minimum.is_some() || maximum.is_some() {
        let min = minimum.map(Value::to_string).unwrap_or_else(|| "-inf".into());
        let max = maximum.map(Value::to_string).unwrap_or_else(|| "+inf".into());
        parts.push(format!("[{min}..{max}]"));
    }
    let allowed = constraints
        .get("allowed_values")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if !allowed.is_empty() {
        parts.push(format!(
            "one of {{{}}}",
            allowed.iter().map(Value::to_string).collect::<Vec<_>>().join(", ")
        ));
    }
    if let Some(max_length) = constraints.get("max_length") {
        parts.push(format!("max length {max_length}"));
    }
    parts.join(", ")
}

fn format_api_resource(resource: Option<&Value>) -> String {
    let Some(resource) = resource else {
        return String::new();
    };
    let mut parts = Vec::new();
    if let Some(scope) = resource.get("scope").and_then(Value::as_str) {
        parts.push(scope.to_string());
    }
    if let Some(capability) = resource.get("capability").and_then(Value::as_object) {
        let mut entries = capability
            .iter()
            .map(|(key, value)| format!("{key}={}", format_api_capability_value(value)))
            .collect::<Vec<_>>();
        entries.sort();
        parts.extend(entries);
    }
    if parts.is_empty() {
        resource
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("resource")
            .to_string()
    } else {
        parts.join("; ")
    }
}

fn format_api_capability_value(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn format_api_returns(returns: Option<&Value>) -> String {
    returns
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| {
            let ty = value.get("type").and_then(Value::as_str).unwrap_or("any");
            match value.get("name").and_then(Value::as_str) {
                Some(name) => format!("{name}: {ty}"),
                None => ty.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_api_effects(effects: Option<&Value>) -> String {
    effects
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|effect| {
            let kind = effect.get("kind").and_then(Value::as_str).unwrap_or("effect");
            let target = effect.get("target").and_then(Value::as_str).unwrap_or("resource");
            let lifetime = effect.get("lifetime").and_then(Value::as_str);
            let exclusive = effect.get("exclusive").and_then(Value::as_bool).unwrap_or(false);
            let mut text = format!("{kind} {target}");
            if let Some(lifetime) = lifetime {
                text.push_str(&format!(" ({lifetime})"));
            }
            if exclusive {
                text.push_str(" exclusive");
            }
            text
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn json_string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn collect_workspace_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_workspace_files(root, &path, out);
        } else if path.is_file() && path.strip_prefix(root).is_ok() {
            out.push(path);
        }
    }
}

fn append_workspace_file(out: &mut String, relative: &Path, path: &Path) {
    let name = relative.to_string_lossy().replace('\\', "/");
    match fs::read_to_string(path) {
        Ok(text) => out.push_str(&format!("\n--- {name}\n{text}\n")),
        Err(_) => {
            let size = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            out.push_str(&format!("\n--- {name}\n[binary file omitted: {size} bytes]\n"));
        }
    }
}

fn hsla_to_u32(c: gpui::Hsla) -> u32 {
    let r = c.to_rgb();
    let ri = (r.r * 255.0).round() as u32;
    let gi = (r.g * 255.0).round() as u32;
    let bi = (r.b * 255.0).round() as u32;
    (ri << 16) | (gi << 8) | bi
}
