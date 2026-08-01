//! Generic serial telemetry parsing and a native GPUI waveform view.
//!
//! This is an independent implementation of common oscilloscope concepts. It
//! does not use VOFA+ code, assets, branding, or private protocol details.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, point, prelude::*, px, Animation, AnimationExt as _, Context, Entity, Hsla,
    IntoElement, PathBuilder, Render, SharedString, Window,
};

use crate::theme::Theme;

const MAX_CHANNELS: usize = 12;
const MAX_SAMPLES: usize = 1_200;
const DEFAULT_WINDOW: usize = 240;

const SERIES_COLORS: [u32; MAX_CHANNELS] = [
    0x45a3ff, 0x31d7a5, 0xffc857, 0xff6b7a, 0xb48cff, 0x54d6e8, 0xff8f4c, 0x93d65c, 0xe66fb4,
    0x77a7ff, 0xc8d36b, 0x6ed0a7,
];

#[derive(Clone)]
struct Sample {
    frame: u64,
    value: f64,
}

#[derive(Clone)]
struct TelemetryChannel {
    name: String,
    values: VecDeque<Sample>,
    visible: bool,
}

#[derive(Clone)]
struct ChannelSnapshot {
    name: String,
    values: Vec<Sample>,
    visible: bool,
    color: Hsla,
    last: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TelemetryMode {
    Waveform,
    Attitude,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlotLayout {
    Overlay,
    Stacked,
}

#[derive(Clone, Copy, Default)]
struct MotionSnapshot {
    roll: Option<f64>,
    pitch: Option<f64>,
    yaw: Option<f64>,
    gyro: [Option<f64>; 3],
    accel: [Option<f64>; 3],
}

impl MotionSnapshot {
    fn has_attitude(self) -> bool {
        self.roll.is_some() || self.pitch.is_some() || self.yaw.is_some()
    }

    fn has_motion(self) -> bool {
        self.has_attitude()
            || self.gyro.iter().any(Option::is_some)
            || self.accel.iter().any(Option::is_some)
    }
}

#[derive(Clone, Copy, Default)]
pub struct TelemetryStats {
    pub frames: u64,
    pub channels: usize,
    pub visible_channels: usize,
    pub rate_hz: f32,
    pub paused: bool,
    pub auto_range: bool,
    pub window: usize,
}

/// Stateful cleanup for arbitrary serial chunks. It keeps incomplete lines out
/// of both the console and the telemetry parser.
#[derive(Default)]
pub struct SerialLineCleaner {
    pending: String,
    expect_clock_after_banner: bool,
    baud_tail: Option<String>,
}

impl SerialLineCleaner {
    pub fn reset(&mut self) {
        self.pending.clear();
        self.expect_clock_after_banner = false;
        self.baud_tail = None;
    }

    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        if chunk.is_empty() {
            return Vec::new();
        }
        self.pending.push_str(chunk);
        self.pending = self.pending.replace("\r\n", "\n").replace('\r', "\n");

        let mut out = Vec::new();
        while let Some(pos) = self.pending.find('\n') {
            let raw: String = self.pending.drain(..=pos).collect();
            if let Some(line) = self.clean_line(raw.trim_end_matches('\n')) {
                out.push(line);
            }
        }

        // Avoid unbounded memory and console spam if firmware writes binary or
        // never emits a line terminator. A valid telemetry frame is tiny.
        if self.pending.len() > 8_192 {
            self.pending.clear();
            self.expect_clock_after_banner = false;
        }
        out
    }

    fn clean_line(&mut self, raw: &str) -> Option<String> {
        let line = strip_terminal_controls(raw);
        let t = line.trim();
        if t.is_empty() {
            return None;
        }
        if let Some(expected) = self.baud_tail.take() {
            if t == expected {
                return None;
            }
        }
        if let Some(target) = normalize_transport_line(t).strip_prefix("BAUD_SWITCH ") {
            if target.bytes().all(|byte| byte.is_ascii_digit()) && target.len() >= 2 {
                self.baud_tail = Some(target[target.len() - 2..].to_string());
            }
        }
        if self.expect_clock_after_banner {
            self.expect_clock_after_banner = false;
            if matches!(t, "32" | "80") {
                return None;
            }
        }
        if t == "Lua" {
            self.expect_clock_after_banner = true;
            return None;
        }
        Some(t.to_string())
    }
}

fn strip_terminal_controls(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    let mut previous_escape = false;
                    for c in chars.by_ref() {
                        if c == '\u{7}' || (previous_escape && c == '\\') {
                            break;
                        }
                        previous_escape = c == '\u{1b}';
                    }
                }
                _ => {}
            }
            continue;
        }
        if ch == '\t' || !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

/// Firmware transport and boot chatter that has no value in the user console.
pub fn is_transport_noise(line: &str) -> bool {
    let t = normalize_transport_line(line.trim());
    if t.is_empty() {
        return true;
    }
    if matches!(
        t,
        "HEX_OK"
            | "SCRIPT_BEGIN"
            | "SCRIPT_OK"
            | "SCRIPT_DONE"
            | "LS"
            | "LS_END"
            | "BOOT_OK"
            | "BOOT_ERR"
            | "RM_OK"
            | "RM_ERR"
            | "FORMAT_OK"
            | "FORMAT_ERR"
            | "GET_END"
            | "GET_ERR"
            | "Run"
            | "Idle"
            | "STOP"
            | "BSL"
            | "JEDEC OK"
            | "JEDEC FAIL"
            | "LFS OK"
            | "LFS NO"
            | "LFS FMT"
            | "main.luac"
            | "main.lua"
            | "builtin"
            | "NO main"
            | "r/f/!/ls/format/get/rm/boot/bsl/HEX"
            | "!"
            | "r"
            | "ls"
    ) {
        return true;
    }
    if t.starts_with("HEX_OK")
        || t.starts_with("SCRIPT_OK")
        || t.starts_with("SCRIPT_BEGIN")
        || t.starts_with("SCRIPT_DONE")
        || t.starts_with("<<<HEX")
        || t.starts_with(">>>HEX")
        || t.starts_with(">>> !")
        || t.starts_with(">>> r")
        || t.starts_with("GET_BEGIN ")
        || t.starts_with("FORMAT_OK ")
        || t.starts_with("F ")
        || t.starts_with("FW_")
        || t.starts_with("MOD_")
        || t.starts_with("MOD ")
        || t.starts_with("BAUD_")
        || t.starts_with("FS_")
        || t.starts_with("STORAGE ")
        || t.starts_with("PART ")
        || t.starts_with("CAPACITY ")
        || t.starts_with("PINS ")
        || t == "STORAGE_END"
        || t.starts_with("FILE ")
        || t.starts_with("FILE_ERR ")
    {
        return true;
    }
    // Upload payload chunks are long hexadecimal-only lines. Requiring 16
    // digits keeps ordinary numeric values usable as scalar telemetry.
    t.len() >= 16 && t.len() <= 128 && t.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A divisor change can leave a few hexadecimal characters before the first
/// complete control line on some USB-UART bridges. Only normalize a tightly
/// bounded prefix before a known protocol marker, leaving Lua output intact.
pub fn normalize_transport_line(line: &str) -> &str {
    for marker in ["BAUD_", "FW_", "MOD_", "FS_", "SCRIPT_", "HEX_"] {
        if let Some(pos) = line.find(marker) {
            if pos <= 4 && line[..pos].bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return &line[pos..];
            }
        }
    }
    line
}

pub struct TelemetryView {
    theme: Theme,
    mode: TelemetryMode,
    plot_layout: PlotLayout,
    channels: Vec<TelemetryChannel>,
    frame: u64,
    paused: bool,
    auto_range: bool,
    manual_range: Option<(f64, f64)>,
    window: usize,
    first_frame_at: Option<Instant>,
    last_frame_at: Option<Instant>,
    filtered_rate_hz: f32,
}

impl TelemetryView {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
            mode: TelemetryMode::Waveform,
            plot_layout: PlotLayout::Overlay,
            channels: Vec::new(),
            frame: 0,
            paused: false,
            auto_range: true,
            manual_range: None,
            window: DEFAULT_WINDOW,
            first_frame_at: None,
            last_frame_at: None,
            filtered_rate_hz: 0.0,
        }
    }

    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Returns true when the line is a supported numeric telemetry frame.
    pub fn ingest_line(&mut self, line: &str, cx: &mut Context<Self>) -> bool {
        let Some(frame) = parse_telemetry_frame(line) else {
            return false;
        };
        if frame.is_empty() {
            return false;
        }
        if self.paused {
            return true;
        }

        let now = Instant::now();
        if self.first_frame_at.is_none() {
            self.first_frame_at = Some(now);
        }
        if let Some(previous) = self.last_frame_at {
            let dt = now.duration_since(previous).as_secs_f32();
            if dt > 0.000_1 {
                let instant_rate = 1.0 / dt;
                self.filtered_rate_hz = if self.filtered_rate_hz <= 0.0 {
                    instant_rate
                } else {
                    self.filtered_rate_hz * 0.88 + instant_rate * 0.12
                };
            }
        }
        self.last_frame_at = Some(now);
        self.frame = self.frame.saturating_add(1);

        for (name, value) in frame.into_iter().take(MAX_CHANNELS) {
            let idx = if let Some(idx) = self.channels.iter().position(|ch| ch.name == name) {
                idx
            } else if self.channels.len() < MAX_CHANNELS {
                self.channels.push(TelemetryChannel {
                    name,
                    values: VecDeque::new(),
                    visible: true,
                });
                self.channels.len() - 1
            } else {
                continue;
            };
            let values = &mut self.channels[idx].values;
            values.push_back(Sample {
                frame: self.frame,
                value,
            });
            while values.len() > MAX_SAMPLES {
                values.pop_front();
            }
        }
        cx.notify();
        true
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.channels.clear();
        self.frame = 0;
        self.first_frame_at = None;
        self.last_frame_at = None;
        self.filtered_rate_hz = 0.0;
        self.manual_range = None;
        cx.notify();
    }

    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        self.paused = !self.paused;
        cx.notify();
    }

    pub fn toggle_auto_range(&mut self, cx: &mut Context<Self>) {
        if self.auto_range {
            self.manual_range = Some(self.data_range());
        }
        self.auto_range = !self.auto_range;
        cx.notify();
    }

    pub fn zoom_y(&mut self, zoom_in: bool, cx: &mut Context<Self>) {
        let (min, max) = self.display_range();
        let center = (min + max) * 0.5;
        let factor = if zoom_in { 0.72 } else { 1.4 };
        let half = ((max - min) * 0.5 * factor).max(1e-9);
        self.manual_range = Some((center - half, center + half));
        self.auto_range = false;
        cx.notify();
    }

    pub fn adjust_window(&mut self, wider: bool, cx: &mut Context<Self>) {
        self.window = if wider {
            (self.window * 2).min(MAX_SAMPLES)
        } else {
            (self.window / 2).max(60)
        };
        cx.notify();
    }

    pub fn toggle_channel(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(channel) = self.channels.get_mut(index) {
            channel.visible = !channel.visible;
            cx.notify();
        }
    }

    fn set_mode(&mut self, mode: TelemetryMode, cx: &mut Context<Self>) {
        self.mode = mode;
        cx.notify();
    }

    fn toggle_plot_layout(&mut self, cx: &mut Context<Self>) {
        self.plot_layout = match self.plot_layout {
            PlotLayout::Overlay => PlotLayout::Stacked,
            PlotLayout::Stacked => PlotLayout::Overlay,
        };
        cx.notify();
    }

    fn set_all_channels(&mut self, visible: bool, cx: &mut Context<Self>) {
        for channel in &mut self.channels {
            channel.visible = visible;
        }
        cx.notify();
    }

    pub fn stats(&self) -> TelemetryStats {
        TelemetryStats {
            frames: self.frame,
            channels: self.channels.len(),
            visible_channels: self
                .channels
                .iter()
                .filter(|channel| channel.visible)
                .count(),
            rate_hz: self.filtered_rate_hz,
            paused: self.paused,
            auto_range: self.auto_range,
            window: self.window,
        }
    }

    fn data_range(&self) -> (f64, f64) {
        let floor = self.frame.saturating_sub(self.window as u64);
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for channel in self.channels.iter().filter(|ch| ch.visible) {
            for sample in channel.values.iter().filter(|s| s.frame >= floor) {
                min = min.min(sample.value);
                max = max.max(sample.value);
            }
        }
        if !min.is_finite() || !max.is_finite() {
            return (-1.0, 1.0);
        }
        if (max - min).abs() < 1e-12 {
            let pad = min.abs().max(1.0) * 0.08;
            return (min - pad, max + pad);
        }
        let pad = (max - min) * 0.08;
        (min - pad, max + pad)
    }

    fn display_range(&self) -> (f64, f64) {
        if self.auto_range {
            self.data_range()
        } else {
            self.manual_range.unwrap_or_else(|| self.data_range())
        }
    }

    fn snapshots(&self) -> Vec<ChannelSnapshot> {
        let floor = self.frame.saturating_sub(self.window as u64);
        self.channels
            .iter()
            .enumerate()
            .map(|(index, channel)| {
                let values: Vec<Sample> = channel
                    .values
                    .iter()
                    .filter(|sample| sample.frame >= floor)
                    .cloned()
                    .collect();
                ChannelSnapshot {
                    name: channel.name.clone(),
                    last: values.last().map(|s| s.value).unwrap_or(0.0),
                    values,
                    visible: channel.visible,
                    color: gpui::rgb(SERIES_COLORS[index]).into(),
                }
            })
            .collect()
    }

    fn latest_named(&self, aliases: &[&str]) -> Option<f64> {
        self.channels.iter().find_map(|channel| {
            let name = channel.name.trim().to_ascii_lowercase();
            aliases
                .iter()
                .any(|alias| name == *alias)
                .then(|| channel.values.back().map(|sample| sample.value))
                .flatten()
        })
    }

    fn angle_degrees(&self, degrees: &[&str], radians: &[&str]) -> Option<f64> {
        self.latest_named(degrees)
            .or_else(|| self.latest_named(radians).map(f64::to_degrees))
    }

    fn motion_snapshot(&self) -> MotionSnapshot {
        MotionSnapshot {
            roll: self.angle_degrees(&["roll", "angle_x"], &["roll_rad", "angle_x_rad"]),
            pitch: self.angle_degrees(&["pitch", "angle_y"], &["pitch_rad", "angle_y_rad"]),
            yaw: self.angle_degrees(
                &["yaw", "heading", "angle_z"],
                &["yaw_rad", "heading_rad", "angle_z_rad"],
            ),
            gyro: [
                self.latest_named(&["gx", "gyro_x", "gyrox"]),
                self.latest_named(&["gy", "gyro_y", "gyroy"]),
                self.latest_named(&["gz", "gyro_z", "gyroz"]),
            ],
            accel: [
                self.latest_named(&["ax", "accel_x", "accx"]),
                self.latest_named(&["ay", "accel_y", "accy"]),
                self.latest_named(&["az", "accel_z", "accz"]),
            ],
        }
    }
}

impl Render for TelemetryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let snapshots = self.snapshots();
        let graph_channels = snapshots.clone();
        let (range_min, range_max) = self.display_range();
        let frame = self.frame;
        let window_size = self.window.max(2) as u64;
        let stats = self.stats();
        let has_data = snapshots.iter().any(|ch| !ch.values.is_empty());
        let mode = self.mode;
        let plot_layout = self.plot_layout;
        let motion = self.motion_snapshot();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.code)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x_3()
                    .gap_y_1()
                    .px_3()
                    .py_1p5()
                    .border_b_1()
                    .border_color(theme.line)
                    .bg(theme.panel2)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .p(px(2.))
                            .gap_1()
                            .rounded_sm()
                            .bg(theme.panel)
                            .child({
                                let entity: Entity<TelemetryView> = cx.entity();
                                div()
                                    .id("telemetry-mode-waveform")
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .text_xs()
                                    .text_color(if mode == TelemetryMode::Waveform {
                                        theme.text
                                    } else {
                                        theme.muted
                                    })
                                    .bg(if mode == TelemetryMode::Waveform {
                                        theme.accent_soft
                                    } else {
                                        theme.panel
                                    })
                                    .cursor_pointer()
                                    .on_click(move |_, _, app| {
                                        entity.update(app, |view, cx| {
                                            view.set_mode(TelemetryMode::Waveform, cx)
                                        });
                                    })
                                    .child("曲线")
                            })
                            .child({
                                let entity: Entity<TelemetryView> = cx.entity();
                                div()
                                    .id("telemetry-mode-attitude")
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .text_xs()
                                    .text_color(if mode == TelemetryMode::Attitude {
                                        theme.text
                                    } else {
                                        theme.muted
                                    })
                                    .bg(if mode == TelemetryMode::Attitude {
                                        theme.accent_soft
                                    } else {
                                        theme.panel
                                    })
                                    .cursor_pointer()
                                    .on_click(move |_, _, app| {
                                        entity.update(app, |view, cx| {
                                            view.set_mode(TelemetryMode::Attitude, cx)
                                        });
                                    })
                                    .child("姿态")
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .size(px(7.))
                                    .rounded_full()
                                    .bg(if stats.paused {
                                        theme.yellow
                                    } else {
                                        theme.green
                                    })
                                    .with_animation(
                                        "telemetry-live-pulse",
                                        Animation::new(Duration::from_millis(1_500)).repeat(),
                                        move |el, delta| {
                                            if stats.paused {
                                                el.opacity(1.0)
                                            } else {
                                                el.opacity(
                                                    0.42 + (delta * std::f32::consts::PI)
                                                        .sin()
                                                        .abs()
                                                        * 0.58,
                                                )
                                            }
                                        },
                                    ),
                            )
                            .text_xs()
                            .text_color(if stats.paused {
                                theme.yellow
                            } else {
                                theme.green
                            })
                            .child(if stats.paused {
                                "已暂停"
                            } else {
                                "实时采集"
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family("Cascadia Code")
                            .text_color(theme.muted)
                            .child(format!("{:.1} fps", stats.rate_hz)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family("Cascadia Code")
                            .text_color(theme.muted)
                            .child(format!(
                                "{} 帧 · {}/{} 通道 · {} 点",
                                stats.frames, stats.visible_channels, stats.channels, stats.window
                            )),
                    )
                    .child(div().flex_1())
                    .when(mode == TelemetryMode::Waveform, |el| {
                        el.children(snapshots.iter().enumerate().map(|(index, channel)| {
                            let entity: Entity<TelemetryView> = cx.entity();
                            div()
                                .id(SharedString::from(format!("telemetry-channel-{index}")))
                                .flex()
                                .items_center()
                                .gap_1()
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .border_1()
                                .border_color(if channel.visible {
                                    channel.color
                                } else {
                                    theme.line
                                })
                                .bg(if channel.visible {
                                    theme.panel
                                } else {
                                    theme.panel2
                                })
                                .cursor_pointer()
                                .opacity(if channel.visible { 1.0 } else { 0.48 })
                                .hover(|s| s.border_color(theme.text))
                                .on_click(move |_, _, app| {
                                    entity.update(app, |view, cx| view.toggle_channel(index, cx));
                                })
                                .child(div().size(px(6.)).rounded_full().bg(channel.color))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.text)
                                        .child(format!("{}  {:.4}", channel.name, channel.last)),
                                )
                        }))
                    }),
            )
            .child(render_telemetry_controls(
                mode,
                plot_layout,
                stats,
                theme,
                cx,
            ))
            .when(mode == TelemetryMode::Waveform, |el| {
                el.child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, _, window, _| {
                                    let left = bounds.left() + px(48.);
                                    let right = bounds.right() - px(14.);
                                    let top = bounds.top() + px(14.);
                                    let bottom = bounds.bottom() - px(24.);
                                    let width = right - left;
                                    let height = bottom - top;
                                    if width <= px(1.) || height <= px(1.) {
                                        return;
                                    }

                                    let visible_channels: Vec<_> = graph_channels
                                        .iter()
                                        .filter(|channel| channel.visible)
                                        .collect();
                                    let mut grid = PathBuilder::stroke(px(1.));
                                    for i in 0..=6 {
                                        let x = left + width * (i as f32 / 6.0);
                                        grid.move_to(point(x, top));
                                        grid.line_to(point(x, bottom));
                                    }
                                    let horizontal_lines = if plot_layout == PlotLayout::Overlay {
                                        4
                                    } else {
                                        visible_channels.len().max(1)
                                    };
                                    for i in 0..=horizontal_lines {
                                        let y = top + height * (i as f32 / horizontal_lines as f32);
                                        grid.move_to(point(left, y));
                                        grid.line_to(point(right, y));
                                    }
                                    if let Ok(path) = grid.build() {
                                        window.paint_path(path, theme.line.opacity(0.38));
                                    }

                                    let frame_start = frame.saturating_sub(window_size - 1);
                                    if plot_layout == PlotLayout::Overlay {
                                        let span = (range_max - range_min).max(1e-12);
                                        for channel in &visible_channels {
                                            if channel.values.len() < 2 {
                                                continue;
                                            }
                                            let build_path = |stroke_width: f32| {
                                                let mut path =
                                                    PathBuilder::stroke(px(stroke_width));
                                                let mut started = false;
                                                for sample in &channel.values {
                                                    let x_ratio =
                                                        sample.frame.saturating_sub(frame_start)
                                                            as f32
                                                            / (window_size - 1) as f32;
                                                    let y_ratio = ((sample.value - range_min)
                                                        / span)
                                                        .clamp(0.0, 1.0)
                                                        as f32;
                                                    let p = point(
                                                        left + width * x_ratio,
                                                        bottom - height * y_ratio,
                                                    );
                                                    if started {
                                                        path.line_to(p);
                                                    } else {
                                                        path.move_to(p);
                                                        started = true;
                                                    }
                                                }
                                                path.build()
                                            };
                                            if let Ok(glow) = build_path(5.0) {
                                                window
                                                    .paint_path(glow, channel.color.opacity(0.12));
                                            }
                                            if let Ok(line) = build_path(1.8) {
                                                window.paint_path(line, channel.color);
                                            }
                                        }
                                    } else if !visible_channels.is_empty() {
                                        let lane_count = visible_channels.len() as f32;
                                        for (lane, channel) in visible_channels.iter().enumerate() {
                                            if channel.values.len() < 2 {
                                                continue;
                                            }
                                            let mut local_min = f64::INFINITY;
                                            let mut local_max = f64::NEG_INFINITY;
                                            for sample in &channel.values {
                                                local_min = local_min.min(sample.value);
                                                local_max = local_max.max(sample.value);
                                            }
                                            if (local_max - local_min).abs() < 1e-12 {
                                                let pad = local_min.abs().max(1.0) * 0.08;
                                                local_min -= pad;
                                                local_max += pad;
                                            }
                                            let local_span = (local_max - local_min).max(1e-12);
                                            let lane_top =
                                                top + height * (lane as f32 / lane_count);
                                            let lane_bottom =
                                                top + height * ((lane + 1) as f32 / lane_count);
                                            let lane_pad = px(4.0);
                                            let lane_height =
                                                (lane_bottom - lane_top - lane_pad * 2.0)
                                                    .max(px(1.0));
                                            let build_path = |stroke_width: f32| {
                                                let mut path =
                                                    PathBuilder::stroke(px(stroke_width));
                                                let mut started = false;
                                                for sample in &channel.values {
                                                    let x_ratio =
                                                        sample.frame.saturating_sub(frame_start)
                                                            as f32
                                                            / (window_size - 1) as f32;
                                                    let y_ratio = ((sample.value - local_min)
                                                        / local_span)
                                                        .clamp(0.0, 1.0)
                                                        as f32;
                                                    let p = point(
                                                        left + width * x_ratio,
                                                        lane_bottom
                                                            - lane_pad
                                                            - lane_height * y_ratio,
                                                    );
                                                    if started {
                                                        path.line_to(p);
                                                    } else {
                                                        path.move_to(p);
                                                        started = true;
                                                    }
                                                }
                                                path.build()
                                            };
                                            if let Ok(glow) = build_path(4.5) {
                                                window
                                                    .paint_path(glow, channel.color.opacity(0.12));
                                            }
                                            if let Ok(line) = build_path(1.6) {
                                                window.paint_path(line, channel.color);
                                            }
                                        }
                                    }
                                },
                            )
                            .size_full(),
                        )
                        .when(plot_layout == PlotLayout::Overlay, |el| {
                            el.child(
                                div()
                                    .absolute()
                                    .left_2()
                                    .top_2()
                                    .text_xs()
                                    .font_family("Cascadia Code")
                                    .text_color(theme.muted)
                                    .child(format_axis(range_max)),
                            )
                        })
                        .when(plot_layout == PlotLayout::Overlay, |el| {
                            el.child(
                                div()
                                    .absolute()
                                    .left_2()
                                    .bottom_4()
                                    .text_xs()
                                    .font_family("Cascadia Code")
                                    .text_color(theme.muted)
                                    .child(format_axis(range_min)),
                            )
                        })
                        .when(plot_layout == PlotLayout::Stacked && has_data, |el| {
                            el.child(
                                div()
                                    .absolute()
                                    .left_2()
                                    .top_2()
                                    .text_xs()
                                    .text_color(theme.muted)
                                    .child("独立量程"),
                            )
                        })
                        .when(!has_data, |el| {
                            el.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .gap_1()
                                            .text_color(theme.muted)
                                            .child(div().text_lg().child("等待串口数值帧"))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_family("Cascadia Code")
                                                    .child("CSV · name:value · JSON · key value"),
                                            ),
                                    ),
                            )
                        }),
                )
            })
            .when(mode == TelemetryMode::Attitude, |el| {
                el.child(render_attitude_view(motion, theme))
            })
    }
}

fn render_telemetry_controls(
    mode: TelemetryMode,
    plot_layout: PlotLayout,
    stats: TelemetryStats,
    theme: Theme,
    cx: &mut Context<TelemetryView>,
) -> impl IntoElement {
    let button =
        |id: &'static str,
         label: &'static str,
         active: bool,
         on_click: Box<dyn Fn(&mut TelemetryView, &mut Context<TelemetryView>)>| {
            let entity: Entity<TelemetryView> = cx.entity();
            div()
                .id(id)
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .border_1()
                .border_color(if active { theme.blue } else { theme.line })
                .bg(if active {
                    theme.accent_soft
                } else {
                    theme.panel
                })
                .text_xs()
                .text_color(if active { theme.text } else { theme.muted })
                .cursor_pointer()
                .hover(|s| s.border_color(theme.text).text_color(theme.text))
                .on_click(move |_, _, app| {
                    entity.update(app, |view, cx| on_click(view, cx));
                })
                .child(label)
                .into_any_element()
        };

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(theme.line)
        .bg(theme.panel)
        .child(button(
            "telemetry-pause",
            if stats.paused { "继续" } else { "暂停" },
            stats.paused,
            Box::new(|view, cx| view.toggle_pause(cx)),
        ))
        .when(mode == TelemetryMode::Waveform, |el| {
            el.child(button(
                "telemetry-layout",
                if plot_layout == PlotLayout::Overlay {
                    "叠加"
                } else {
                    "分轨"
                },
                plot_layout == PlotLayout::Stacked,
                Box::new(|view, cx| view.toggle_plot_layout(cx)),
            ))
            .child(button(
                "telemetry-auto-range",
                if stats.auto_range {
                    "自动量程"
                } else {
                    "手动量程"
                },
                stats.auto_range,
                Box::new(|view, cx| view.toggle_auto_range(cx)),
            ))
            .child(button(
                "telemetry-y-in",
                "Y+",
                false,
                Box::new(|view, cx| view.zoom_y(true, cx)),
            ))
            .child(button(
                "telemetry-y-out",
                "Y-",
                false,
                Box::new(|view, cx| view.zoom_y(false, cx)),
            ))
            .child(button(
                "telemetry-time-less",
                "时间-",
                false,
                Box::new(|view, cx| view.adjust_window(false, cx)),
            ))
            .child(button(
                "telemetry-time-more",
                "时间+",
                false,
                Box::new(|view, cx| view.adjust_window(true, cx)),
            ))
            .child(button(
                "telemetry-show-all",
                "全显",
                stats.channels > 0 && stats.visible_channels == stats.channels,
                Box::new(|view, cx| view.set_all_channels(true, cx)),
            ))
            .child(button(
                "telemetry-hide-all",
                "全隐",
                stats.channels > 0 && stats.visible_channels == 0,
                Box::new(|view, cx| view.set_all_channels(false, cx)),
            ))
        })
        .child(button(
            "telemetry-clear",
            "清数据",
            false,
            Box::new(|view, cx| view.clear(cx)),
        ))
}

fn render_attitude_view(motion: MotionSnapshot, theme: Theme) -> impl IntoElement {
    let roll = motion.roll.unwrap_or(0.0) as f32;
    let pitch = motion.pitch.unwrap_or(0.0) as f32;
    let yaw = motion.yaw.unwrap_or(0.0) as f32;
    let has_motion = motion.has_motion();
    let has_attitude = motion.has_attitude();
    let gyro_magnitude = vector_magnitude(motion.gyro);
    let accel_magnitude = vector_magnitude(motion.accel);

    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(theme.code)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h(px(210.))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let width = f32::from(bounds.size.width);
                            let height = f32::from(bounds.size.height);
                            if width < 40.0 || height < 40.0 {
                                return;
                            }
                            let center = point(
                                bounds.left() + bounds.size.width * 0.5,
                                bounds.top() + bounds.size.height * 0.48,
                            );
                            let radius = width.min(height) * 0.34;

                            let mut rings = PathBuilder::stroke(px(1.));
                            for ring_scale in [1.0_f32, 0.72, 0.45] {
                                for i in 0..=64 {
                                    let a = std::f32::consts::TAU * i as f32 / 64.0;
                                    let p = point(
                                        center.x + px(a.cos() * radius * ring_scale),
                                        center.y + px(a.sin() * radius * ring_scale),
                                    );
                                    if i == 0 {
                                        rings.move_to(p);
                                    } else {
                                        rings.line_to(p);
                                    }
                                }
                            }
                            rings.move_to(point(center.x - px(radius), center.y));
                            rings.line_to(point(center.x + px(radius), center.y));
                            rings.move_to(point(center.x, center.y - px(radius)));
                            rings.line_to(point(center.x, center.y + px(radius)));
                            if let Ok(path) = rings.build() {
                                window.paint_path(path, theme.line.opacity(0.48));
                            }

                            let roll_radians = -roll.to_radians();
                            let pitch_shift = (pitch / 90.0).clamp(-0.7, 0.7) * radius;
                            let tangent = point(
                                px(roll_radians.cos() * radius),
                                px(roll_radians.sin() * radius),
                            );
                            let normal = point(
                                px(-roll_radians.sin() * pitch_shift),
                                px(roll_radians.cos() * pitch_shift),
                            );
                            let mut horizon = PathBuilder::stroke(px(2.));
                            horizon.move_to(point(
                                center.x - tangent.x + normal.x,
                                center.y - tangent.y + normal.y,
                            ));
                            horizon.line_to(point(
                                center.x + tangent.x + normal.x,
                                center.y + tangent.y + normal.y,
                            ));
                            if let Ok(path) = horizon.build() {
                                window.paint_path(path, theme.blue.opacity(0.86));
                            }

                            let yaw_radians = (yaw - 90.0).to_radians();
                            let mut heading = PathBuilder::stroke(px(2.4));
                            heading.move_to(point(
                                center.x + px(yaw_radians.cos() * radius * 0.82),
                                center.y + px(yaw_radians.sin() * radius * 0.82),
                            ));
                            heading.line_to(point(
                                center.x + px(yaw_radians.cos() * radius),
                                center.y + px(yaw_radians.sin() * radius),
                            ));
                            if let Ok(path) = heading.build() {
                                window.paint_path(path, theme.yellow);
                            }

                            let cube_scale = radius * 0.42;
                            let vertices = [
                                [-1.0, -1.0, -1.0],
                                [1.0, -1.0, -1.0],
                                [1.0, 1.0, -1.0],
                                [-1.0, 1.0, -1.0],
                                [-1.0, -1.0, 1.0],
                                [1.0, -1.0, 1.0],
                                [1.0, 1.0, 1.0],
                                [-1.0, 1.0, 1.0],
                            ];
                            let projected: Vec<_> = vertices
                                .into_iter()
                                .map(|vertex| {
                                    let [x, y, z] = rotate_3d(vertex, roll, pitch, yaw);
                                    let perspective = 1.25 / (3.4 - z * 0.34);
                                    point(
                                        center.x + px(x * cube_scale * perspective),
                                        center.y + px(y * cube_scale * perspective),
                                    )
                                })
                                .collect();
                            let edges = [
                                (0, 1),
                                (1, 2),
                                (2, 3),
                                (3, 0),
                                (4, 5),
                                (5, 6),
                                (6, 7),
                                (7, 4),
                                (0, 4),
                                (1, 5),
                                (2, 6),
                                (3, 7),
                            ];
                            for (stroke, color) in [
                                (7.0, theme.blue.opacity(0.10)),
                                (2.0, theme.blue.opacity(0.92)),
                            ] {
                                let mut cube = PathBuilder::stroke(px(stroke));
                                for (from, to) in edges {
                                    cube.move_to(projected[from]);
                                    cube.line_to(projected[to]);
                                }
                                if let Ok(path) = cube.build() {
                                    window.paint_path(path, color);
                                }
                            }
                        },
                    )
                    .size_full(),
                )
                .child(
                    div()
                        .absolute()
                        .top_3()
                        .left_3()
                        .text_xs()
                        .font_family("Cascadia Code")
                        .text_color(theme.muted)
                        .child(if has_attitude {
                            "姿态角 · deg"
                        } else {
                            "等待姿态角"
                        }),
                ),
        )
        .child(
            div()
                .grid()
                .grid_cols(3)
                .border_t_1()
                .border_b_1()
                .border_color(theme.line)
                .children([
                    motion_value("ROLL", motion.roll, theme.blue, theme),
                    motion_value("PITCH", motion.pitch, theme.green, theme),
                    motion_value("YAW", motion.yaw, theme.yellow, theme),
                ]),
        )
        .child(
            div()
                .px_3()
                .py_2()
                .flex()
                .flex_col()
                .gap_2()
                .child(sensor_row(
                    "GYRO",
                    motion.gyro,
                    gyro_magnitude,
                    "deg/s",
                    theme,
                ))
                .child(sensor_row(
                    "ACCEL",
                    motion.accel,
                    accel_magnitude,
                    "g",
                    theme,
                )),
        )
        .when(!has_motion, |el| {
            el.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.code.opacity(0.82))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .text_color(theme.muted)
                            .child(div().text_lg().child("等待姿态数据"))
                            .child(
                                div()
                                    .text_xs()
                                    .font_family("Cascadia Code")
                                    .child("roll/pitch/yaw · gx/gy/gz · ax/ay/az"),
                            ),
                    ),
            )
        })
}

fn motion_value(
    label: &'static str,
    value: Option<f64>,
    color: Hsla,
    theme: Theme,
) -> gpui::AnyElement {
    div()
        .px_2()
        .py_2()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .border_r_1()
        .border_color(theme.line)
        .child(div().text_xs().text_color(theme.muted).child(label))
        .child(
            div()
                .text_sm()
                .font_family("Cascadia Code")
                .text_color(color)
                .child(format_optional(value, "°")),
        )
        .into_any_element()
}

fn sensor_row(
    label: &'static str,
    values: [Option<f64>; 3],
    magnitude: Option<f64>,
    unit: &'static str,
    theme: Theme,
) -> impl IntoElement {
    let axis_colors: [Hsla; 3] = [
        gpui::rgb(0xff6b7a).into(),
        gpui::rgb(0x31d7a5).into(),
        gpui::rgb(0x45a3ff).into(),
    ];
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(48.))
                .text_xs()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.muted)
                .child(label),
        )
        .children(values.into_iter().enumerate().map(|(index, value)| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(axis_colors[index])
                        .child(["X", "Y", "Z"][index]),
                )
                .child(
                    div()
                        .text_xs()
                        .font_family("Cascadia Code")
                        .text_color(theme.text)
                        .truncate()
                        .child(format_optional(value, "")),
                )
        }))
        .child(
            div()
                .text_xs()
                .font_family("Cascadia Code")
                .text_color(theme.muted)
                .child(match magnitude {
                    Some(value) => format!("|v| {value:.2} {unit}"),
                    None => format!("-- {unit}"),
                }),
        )
}

fn format_optional(value: Option<f64>, suffix: &str) -> String {
    value
        .map(|value| format!("{value:.2}{suffix}"))
        .unwrap_or_else(|| "--".into())
}

fn vector_magnitude(values: [Option<f64>; 3]) -> Option<f64> {
    let [Some(x), Some(y), Some(z)] = values else {
        return None;
    };
    Some((x * x + y * y + z * z).sqrt())
}

fn rotate_3d([x, y, z]: [f32; 3], roll: f32, pitch: f32, yaw: f32) -> [f32; 3] {
    let (sr, cr) = roll.to_radians().sin_cos();
    let (sp, cp) = pitch.to_radians().sin_cos();
    let (sy, cy) = yaw.to_radians().sin_cos();

    let (x1, y1, z1) = (x, y * cr - z * sr, y * sr + z * cr);
    let (x2, y2, z2) = (x1 * cp + z1 * sp, y1, -x1 * sp + z1 * cp);
    [x2 * cy - y2 * sy, x2 * sy + y2 * cy, z2]
}

fn format_axis(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 100_000.0 || (magnitude > 0.0 && magnitude < 0.001) {
        format!("{value:.2e}")
    } else if magnitude >= 100.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.3}")
    }
}

fn parse_number(raw: &str) -> Option<f64> {
    let value = raw
        .trim()
        .trim_matches(|c: char| matches!(c, ',' | ';' | '[' | ']' | '(' | ')' | '{' | '}'))
        .parse::<f64>()
        .ok()?;
    value.is_finite().then_some(value)
}

pub fn parse_telemetry_frame(line: &str) -> Option<Vec<(String, f64)>> {
    let t = line.trim();
    if t.is_empty() {
        return None;
    }

    if t.starts_with('{') && t.ends_with('}') {
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(t) {
            let values: Vec<(String, f64)> = map
                .into_iter()
                .filter_map(|(name, value)| {
                    value.as_f64().filter(|v| v.is_finite()).map(|v| (name, v))
                })
                .take(MAX_CHANNELS)
                .collect();
            if !values.is_empty() {
                return Some(values);
            }
        }
    }

    let fields: Vec<&str> = t
        .split(|c| matches!(c, ',' | ';' | '\t'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let mut named = Vec::new();
    for field in &fields {
        if let Some((name, raw)) = field.split_once(':').or_else(|| field.split_once('=')) {
            if let Some(value) = parse_number(raw) {
                let name = name.trim();
                if !name.is_empty() {
                    named.push((name.to_string(), value));
                }
            }
        }
    }
    if !named.is_empty() && named.len() == fields.len() {
        named.truncate(MAX_CHANNELS);
        return Some(named);
    }

    if fields.len() >= 2 {
        let numeric: Vec<f64> = fields
            .iter()
            .filter_map(|field| parse_number(field))
            .collect();
        if numeric.len() == fields.len() {
            return Some(
                numeric
                    .into_iter()
                    .take(MAX_CHANNELS)
                    .enumerate()
                    .map(|(i, value)| (format!("CH{}", i + 1), value))
                    .collect(),
            );
        }
    }

    let words: Vec<&str> = t.split_whitespace().collect();
    let mut pairs = Vec::new();
    for i in 1..words.len() {
        if let Some(value) = parse_number(words[i]) {
            if parse_number(words[i - 1]).is_none() {
                let name = words[i - 1]
                    .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
                    .to_string();
                if !name.is_empty() {
                    pairs.push((name, value));
                }
            }
        }
    }
    if !pairs.is_empty() {
        pairs.truncate(MAX_CHANNELS);
        return Some(pairs);
    }

    parse_number(t).map(|value| vec![("CH1".into(), value)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_text_frames() {
        assert_eq!(parse_telemetry_frame("1,2.5,-3").unwrap().len(), 3);
        assert_eq!(
            parse_telemetry_frame("temp:24.5,rpm=1200").unwrap()[0].0,
            "temp"
        );
        assert_eq!(parse_telemetry_frame("raw 123 mv 456").unwrap().len(), 2);
        assert_eq!(
            parse_telemetry_frame(r#"{"x":1.5,"y":-2}"#).unwrap().len(),
            2
        );
        assert!(parse_telemetry_frame("hello world").is_none());
    }

    #[test]
    fn parses_complete_imu_frame_without_truncating_axes() {
        let frame =
            parse_telemetry_frame("roll:1,pitch:2,yaw:3,gx:4,gy:5,gz:6,ax:7,ay:8,az:9,temp:24")
                .unwrap();
        assert_eq!(frame.len(), 10);
        assert_eq!(frame[8], ("az".into(), 9.0));
    }

    #[test]
    fn recognizes_attitude_radians_and_motion_aliases() {
        let channel = |name: &str, value: f64| TelemetryChannel {
            name: name.into(),
            values: VecDeque::from([Sample { frame: 1, value }]),
            visible: true,
        };
        let mut view = TelemetryView::new();
        view.channels = vec![
            channel("roll_rad", std::f64::consts::FRAC_PI_2),
            channel("pitch", -12.5),
            channel("heading", 270.0),
            channel("gyro_x", 1.25),
            channel("gy", -2.5),
            channel("accz", 0.98),
        ];
        let motion = view.motion_snapshot();
        assert_eq!(motion.roll, Some(90.0));
        assert_eq!(motion.pitch, Some(-12.5));
        assert_eq!(motion.yaw, Some(270.0));
        assert_eq!(motion.gyro[0], Some(1.25));
        assert_eq!(motion.gyro[1], Some(-2.5));
        assert_eq!(motion.accel[2], Some(0.98));
    }

    #[test]
    fn reports_visible_channels_for_multi_signal_views() {
        let channel = |name: &str, visible: bool| TelemetryChannel {
            name: name.into(),
            values: VecDeque::from([Sample {
                frame: 1,
                value: 1.0,
            }]),
            visible,
        };
        let mut view = TelemetryView::new();
        view.channels = vec![
            channel("temperature", true),
            channel("rpm", false),
            channel("voltage", true),
        ];
        let stats = view.stats();
        assert_eq!(stats.channels, 3);
        assert_eq!(stats.visible_channels, 2);
        assert!(matches!(view.plot_layout, PlotLayout::Overlay));
    }

    #[test]
    fn cleaner_reassembles_lines_and_drops_banner_noise() {
        let mut cleaner = SerialLineCleaner::default();
        assert!(cleaner.push("Lua\r\n8").is_empty());
        assert!(cleaner.push("0\r\n").is_empty());
        assert_eq!(cleaner.push("temp:\u{1b}[31m2").len(), 0);
        assert_eq!(cleaner.push("4.5\u{1b}[0m\r\n"), vec!["temp:24.5"]);
    }

    #[test]
    fn cleaner_drops_terminal_controls_and_oversized_unterminated_noise() {
        let mut cleaner = SerialLineCleaner::default();
        assert_eq!(
            cleaner.push("\u{1b}]0;device title\u{7}rpm:\u{0}1200\r\n"),
            vec!["rpm:1200"]
        );
        assert!(cleaner.push(&"x".repeat(8_193)).is_empty());
        assert_eq!(cleaner.push("temp:25\n"), vec!["temp:25"]);
    }

    #[test]
    fn cleaner_drops_only_the_immediate_baud_tail_fragment() {
        let mut cleaner = SerialLineCleaner::default();
        assert_eq!(
            cleaner.push("BAUD_SWITCH 115200\r\n00\r\nBAUD_OK 115200\r\n"),
            vec!["BAUD_SWITCH 115200", "BAUD_OK 115200"]
        );
        assert_eq!(cleaner.push("00\r\n"), vec!["00"]);
    }

    #[test]
    fn classifies_transport_without_hiding_short_scalar_data() {
        assert!(is_transport_noise("SCRIPT_OK 120"));
        assert!(is_transport_noise("F main.luac 120"));
        assert!(is_transport_noise("FW_INFO_END"));
        assert!(is_transport_noise("MOD_SLOT 0 i2c 3352 18c593d8"));
        assert!(is_transport_noise("BAUD_OK 460800"));
        assert!(is_transport_noise("0BAUD_OK 460800"));
        assert!(is_transport_noise("FS_READY"));
        assert!(!is_transport_noise("42"));
        assert!(!is_transport_noise("temp:42"));
    }
}
