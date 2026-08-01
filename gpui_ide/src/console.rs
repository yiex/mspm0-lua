use std::ops::Range;
use std::sync::Arc;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, App, Bounds, ClipboardItem, Context,
    CursorStyle, Element, ElementId, Entity, FocusHandle, Focusable, GlobalElementId, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ScrollHandle, ShapedLine, SharedString, Style, TextRun, Window,
};
use parking_lot::Mutex;

use crate::scrollbar::{is_scrolled_to_bottom, v_scrollbar, ScrollbarDrag};
use crate::theme::Theme;

actions!(console, [CopySelection, SelectAll]);

/// Shared with IdeApp so right-click can open a window-level menu.
pub type ConsoleCtxSlot = Arc<Mutex<Option<(f32, f32)>>>;

#[derive(Clone)]
struct LineCache {
    start: usize,
    text: SharedString,
    shaped: ShapedLine,
}

pub struct ConsoleView {
    focus_handle: FocusHandle,
    content: SharedString,
    line_colors: Vec<gpui::Hsla>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    line_layouts: Vec<LineCache>,
    last_bounds: Option<Bounds<Pixels>>,
    line_height: Pixels,
    top_pad: Pixels,
    is_selecting: bool,
    pub scroll_handle: ScrollHandle,
    stick_to_bottom: bool,
    sb_drag: ScrollbarDrag,
    theme: Theme,
    /// Window-coord right-click; parent IdeApp polls and shows menu.
    ctx_slot: ConsoleCtxSlot,
    /// Case-insensitive substring filter for highlight (empty = none).
    search_query: String,
}

impl ConsoleView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::with_ctx_slot(Arc::new(Mutex::new(None)), cx)
    }

    pub fn with_ctx_slot(ctx_slot: ConsoleCtxSlot, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::from(""),
            line_colors: Vec::new(),
            selected_range: 0..0,
            selection_reversed: false,
            line_layouts: Vec::new(),
            last_bounds: None,
            line_height: px(18.),
            top_pad: px(4.),
            is_selecting: false,
            scroll_handle: ScrollHandle::new(),
            stick_to_bottom: true,
            sb_drag: ScrollbarDrag::default(),
            theme: Theme::default(),
            ctx_slot,
            search_query: String::new(),
        }
    }

    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        self.line_layouts.clear();
        cx.notify();
    }

    pub fn set_search(&mut self, query: &str, cx: &mut Context<Self>) {
        let q = query.to_string();
        if self.search_query == q {
            return;
        }
        self.search_query = q;
        self.line_layouts.clear();
        cx.notify();
    }

    /// Replace console content with colored lines (one color per logical line).
    pub fn set_lines(&mut self, lines: &[(String, gpui::Hsla)], cx: &mut Context<Self>) {
        let mut rebuilt = String::new();
        let mut line_colors = Vec::new();
        for (i, (line, color)) in lines.iter().enumerate() {
            if i > 0 {
                rebuilt.push('\n');
            }
            let body = line.trim_end_matches(['\r', '\n']);
            rebuilt.push_str(body);
            line_colors.push(*color);
        }
        let was_at_end = self.stick_to_bottom || self.selected_range.is_empty();
        self.content = rebuilt.into();
        self.line_colors = line_colors;
        let end = self.content.len();
        if was_at_end {
            self.selected_range = end..end;
            self.stick_to_bottom = true;
            self.scroll_handle.scroll_to_bottom();
        } else {
            self.selected_range.start = self.selected_range.start.min(end);
            self.selected_range.end = self.selected_range.end.min(end);
        }
        self.line_layouts.clear();
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.content = SharedString::from("");
        self.line_colors.clear();
        self.selected_range = 0..0;
        self.line_layouts.clear();
        self.stick_to_bottom = true;
        cx.notify();
    }

    pub fn text(&self) -> String {
        self.content.to_string()
    }

    fn copy_selection(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.copy_all(cx);
        } else {
            self.copy_selection_only(cx);
        }
        cx.notify();
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        self.stick_to_bottom = false;
        cx.notify();
    }

    /// Copy only the current selection (empty selection → no-op).
    pub fn copy_selection_only(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        let text = self.content[self.selected_range.clone()].to_string();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub fn copy_all(&mut self, cx: &mut Context<Self>) {
        let text = self.content.to_string();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub fn has_selection(&self) -> bool {
        !self.selected_range.is_empty()
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sb_drag.active {
            return;
        }
        window.focus(&self.focus_handle);
        if event.button == MouseButton::Right {
            // Window coordinates — IdeApp polls ctx_slot and draws the menu.
            *self.ctx_slot.lock() =
                Some((f32::from(event.position.x), f32::from(event.position.y)));
            cx.notify();
            return;
        }
        self.is_selecting = true;
        self.stick_to_bottom = false;
        let idx = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(idx, cx);
        } else {
            self.move_to(idx, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.sb_drag.active {
            self.sb_drag = ScrollbarDrag::default();
            self.stick_to_bottom = is_scrolled_to_bottom(&self.scroll_handle);
            cx.notify();
        }
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.sb_drag.active {
            self.sb_drag
                .apply_move(&self.scroll_handle, event.position.y);
            self.stick_to_bottom = is_scrolled_to_bottom(&self.scroll_handle);
            cx.notify();
            return;
        }
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_scroll_wheel(
        &mut self,
        _: &gpui::ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // After GPUI applies the wheel delta, re-evaluate stick-to-bottom.
        let handle = self.scroll_handle.clone();
        cx.defer_in(window, move |this, _window, cx| {
            this.stick_to_bottom = is_scrolled_to_bottom(&handle);
            cx.notify();
        });
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.content.len());
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.content.len());
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        if self.line_layouts.is_empty() {
            return 0;
        }
        let line_h = self.line_height.max(px(1.));
        let y = position.y - bounds.top() - self.top_pad;
        if y < px(0.) {
            return 0;
        }
        let idx = (y / line_h).floor() as usize;
        if idx >= self.line_layouts.len() {
            return self.content.len();
        }
        let line = &self.line_layouts[idx];
        let local_x = (position.x - bounds.left()).max(px(0.));
        let local = line
            .shaped
            .closest_index_for_x(local_x)
            .min(line.text.len());
        (line.start + local).min(self.content.len())
    }

    fn line_count(&self) -> usize {
        if self.content.is_empty() {
            1
        } else {
            self.content.lines().count() + if self.content.ends_with('\n') { 1 } else { 0 }
        }
        .max(1)
    }
}

struct ConsoleElement {
    view: Entity<ConsoleView>,
}

struct Prepaint {
    lines: Vec<LineCache>,
    selections: Vec<PaintQuad>,
    search_hits: Vec<PaintQuad>,
}

impl IntoElement for ConsoleElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ConsoleElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let line_h = window.line_height();
        let lines = self.view.read(cx).line_count().max(4) as f32;
        style.size.height = (line_h * lines + px(16.)).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let view = self.view.read(cx);
        let content = view.content.clone();
        let selected = view.selected_range.clone();
        let theme = view.theme;
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_h = window.line_height();

        let mut lines = Vec::new();
        let mut start = 0usize;
        let raw_lines: Vec<String> = if content.is_empty() {
            vec![String::new()]
        } else {
            content.split('\n').map(|s| s.to_string()).collect()
        };
        let n = raw_lines.len();
        for (i, raw) in raw_lines.into_iter().enumerate() {
            let color = view.line_colors.get(i).copied().unwrap_or(theme.text);
            let display: SharedString = if raw.is_empty() {
                SharedString::from(" ")
            } else {
                SharedString::from(raw.clone())
            };
            let run = TextRun {
                len: display.len(),
                font: style.font(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window
                .text_system()
                .shape_line(display, font_size, &[run], None);
            let raw_len = raw.len();
            lines.push(LineCache {
                start,
                text: SharedString::from(raw),
                shaped,
            });
            start += raw_len;
            if i + 1 < n || content.ends_with('\n') {
                start += 1;
            }
        }

        let mut selections = Vec::new();
        if !selected.is_empty() {
            for (i, line) in lines.iter().enumerate() {
                let line_end = if i + 1 < lines.len() {
                    lines[i + 1].start
                } else {
                    content.len()
                };
                let sel_start = selected.start.max(line.start);
                let sel_end = selected.end.min(line_end);
                if sel_start < sel_end {
                    let a = sel_start.saturating_sub(line.start).min(line.text.len());
                    let b = sel_end
                        .saturating_sub(line.start)
                        .min(line.text.len().max(a));
                    let x0 = line.shaped.x_for_index(a);
                    let x1 = if a == b {
                        x0 + px(4.)
                    } else {
                        line.shaped.x_for_index(b)
                    };
                    selections.push(fill(
                        Bounds::from_corners(
                            point(
                                bounds.left() + x0,
                                bounds.top() + px(4.) + line_h * i as f32,
                            ),
                            point(
                                bounds.left() + x1,
                                bounds.top() + px(4.) + line_h * (i as f32 + 1.),
                            ),
                        ),
                        theme.selection,
                    ));
                }
            }
        }

        let mut search_hits = Vec::new();
        let q = view.search_query.trim();
        if !q.is_empty() {
            let q_lower = q.to_lowercase();
            for (i, line) in lines.iter().enumerate() {
                let text_lower = line.text.to_lowercase();
                let mut from = 0usize;
                while let Some(rel) = text_lower[from..].find(&q_lower) {
                    let start = from + rel;
                    let end = start + q.len().min(line.text.len().saturating_sub(start));
                    if end > start {
                        let x0 = line.shaped.x_for_index(start.min(line.text.len()));
                        let x1 = line.shaped.x_for_index(end.min(line.text.len()));
                        search_hits.push(fill(
                            Bounds::from_corners(
                                point(
                                    bounds.left() + x0,
                                    bounds.top() + px(4.) + line_h * i as f32,
                                ),
                                point(
                                    bounds.left() + x1.max(x0 + px(4.)),
                                    bounds.top() + px(4.) + line_h * (i as f32 + 1.),
                                ),
                            ),
                            theme.accent_soft,
                        ));
                    }
                    from = start + q_lower.len().max(1);
                    if from >= text_lower.len() {
                        break;
                    }
                }
            }
        }

        Prepaint {
            lines,
            selections,
            search_hits,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        for h in prepaint.search_hits.drain(..) {
            window.paint_quad(h);
        }
        for sel in prepaint.selections.drain(..) {
            window.paint_quad(sel);
        }
        let line_h = window.line_height();
        for (i, line) in prepaint.lines.iter().enumerate() {
            line.shaped
                .paint(
                    bounds.origin + point(px(0.), px(4.) + line_h * i as f32),
                    line_h,
                    window,
                    cx,
                )
                .ok();
        }
        let lines = std::mem::take(&mut prepaint.lines);
        self.view.update(cx, |view, _| {
            view.line_layouts = lines;
            view.last_bounds = Some(bounds);
            view.line_height = line_h;
            view.top_pad = px(4.);
        });
    }
}

impl Render for ConsoleView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keep following the latest output unless the user scrolled away / is dragging.
        if self.stick_to_bottom && !self.sb_drag.active {
            self.scroll_handle.scroll_to_bottom();
        }
        let theme = self.theme;
        div()
            .id("console-view")
            .key_context("ConsoleView")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::select_all))
            .size_full()
            .flex()
            .bg(theme.code)
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(
                div()
                    .id("console-scroll")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .cursor(CursorStyle::IBeam)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                    .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
                    .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
                    .text_xs()
                    .font_family("Cascadia Code")
                    .line_height(px(18.))
                    .text_color(theme.green)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .p_2()
                    .child(ConsoleElement { view: cx.entity() }),
            )
            .child({
                let entity = cx.entity();
                let entity2 = entity.clone();
                v_scrollbar(
                    "console-sb",
                    &self.scroll_handle,
                    self.sb_drag.clone(),
                    theme,
                    move |d, cx| {
                        entity.update(cx, |v, cx| {
                            v.is_selecting = false;
                            if d.active {
                                v.stick_to_bottom = false;
                            }
                            v.sb_drag = d;
                            cx.notify();
                        });
                    },
                    move |cx| {
                        entity2.update(cx, |v, cx| {
                            v.stick_to_bottom = is_scrolled_to_bottom(&v.scroll_handle);
                            cx.notify();
                        });
                    },
                )
            })
    }
}

impl Focusable for ConsoleView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
