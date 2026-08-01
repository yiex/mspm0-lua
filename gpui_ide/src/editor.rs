use std::ops::Range;
use std::sync::Arc;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds, ClipboardItem, Context,
    CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    Focusable, GlobalElementId, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, Pixels, Point, ScrollDelta, ScrollHandle, ScrollWheelEvent,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, Window,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::compile::sanitize_source_text;
use crate::metadata::TargetProfile;
use crate::scrollbar::{v_scrollbar, ScrollbarDrag};
use crate::syntax::{self, CompletionItem};
use crate::theme::Theme;

actions!(
    editor,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Enter,
        Tab,
        Paste,
        Cut,
        Copy,
        Undo,
        Redo,
        Escape,
        AcceptCompletion,
    ]
);

#[derive(Clone)]
struct LineLayoutCache {
    start: usize,
    text: SharedString,
    shaped: ShapedLine,
}

#[derive(Clone)]
struct CompletionEntry {
    label: SharedString,
    insert: SharedString,
    detail: SharedString,
}

#[derive(Clone)]
struct EditSnapshot {
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
}

const EDIT_HISTORY_LIMIT: usize = 512;

fn source_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut start = 0usize;
    text.split('\n').map(move |line| {
        let line_start = start;
        start += line.len() + 1;
        (line_start, line)
    })
}

fn line_col_at(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset.min(text.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let col = prefix
        .rsplit_once('\n')
        .map(|(_, tail)| tail.chars().count())
        .unwrap_or_else(|| prefix.chars().count());
    (line, col)
}

fn actionable_completions(items: Vec<CompletionItem>, typed: &str) -> Vec<CompletionItem> {
    items
        .into_iter()
        .filter(|item| item.label != typed && item.insert != typed)
        .collect()
}

pub struct CodeEditor {
    focus_handle: FocusHandle,
    content: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    line_layouts: Vec<LineLayoutCache>,
    last_bounds: Option<Bounds<Pixels>>,
    line_height: Pixels,
    top_pad: Pixels,
    is_selecting: bool,
    completions: Vec<CompletionEntry>,
    completion_index: usize,
    completion_prefix_start: usize,
    scroll_handle: ScrollHandle,
    sb_drag: ScrollbarDrag,
    theme: Theme,
    target_profile: Option<Arc<TargetProfile>>,
    target_locked: bool,
    /// Editor-local context menu position, relative to the code surface.
    context_menu: Option<(f32, f32)>,
    undo_stack: Vec<EditSnapshot>,
    redo_stack: Vec<EditSnapshot>,
    /// Editor font size in px (Ctrl+wheel).
    font_px: f32,
    /// Last painted line-number gutter width (for hit-testing).
    gutter_w: Pixels,
}

impl CodeEditor {
    pub fn new(cx: &mut Context<Self>, initial: impl Into<SharedString>) -> Self {
        let content: SharedString = initial.into();
        let len = content.len();
        let font_px = 14.0_f32;
        Self {
            focus_handle: cx.focus_handle(),
            content,
            selected_range: len..len,
            selection_reversed: false,
            marked_range: None,
            line_layouts: Vec::new(),
            last_bounds: None,
            line_height: px(font_px * 1.55),
            top_pad: px(4.),
            is_selecting: false,
            completions: Vec::new(),
            completion_index: 0,
            completion_prefix_start: 0,
            scroll_handle: ScrollHandle::new(),
            sb_drag: ScrollbarDrag::default(),
            theme: Theme::default(),
            target_profile: None,
            target_locked: false,
            context_menu: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            font_px,
            gutter_w: px(0.),
        }
    }

    fn gutter_width_for(&self, line_count: usize) -> f32 {
        let digits = ((line_count as f32).log10().floor() as usize + 1).max(2);
        // Compact gutter: digit width + small pad (no hard panel).
        self.font_px * 0.55 * digits as f32 + 10.0
    }

    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        self.line_layouts.clear();
        cx.notify();
    }

    pub fn set_target_profile(
        &mut self,
        profile: Option<Arc<TargetProfile>>,
        target_locked: bool,
        cx: &mut Context<Self>,
    ) {
        self.target_profile = profile;
        self.target_locked = target_locked;
        self.clear_completions();
        cx.notify();
    }

    pub fn font_px(&self) -> f32 {
        self.font_px
    }

    pub fn set_font_px(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = size.clamp(10.0, 28.0);
        if (self.font_px - size).abs() < 0.05 {
            return;
        }
        self.font_px = size;
        self.line_height = px(size * 1.55);
        self.line_layouts.clear();
        cx.notify();
    }

    fn line_height_px(&self) -> Pixels {
        px(self.font_px * 1.55)
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll_handle
    }

    pub fn text(&self) -> String {
        self.content.to_string()
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text: SharedString = text.into();
        self.content = sanitize_source_text(&text).into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.line_layouts.clear();
        self.clear_completions();
        self.undo_stack.clear();
        self.redo_stack.clear();
        cx.notify();
    }

    fn current_snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            content: self.content.to_string(),
            selected_range: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
        }
    }

    fn remember_edit(&mut self) {
        self.undo_stack.push(self.current_snapshot());
        if self.undo_stack.len() > EDIT_HISTORY_LIMIT {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn restore_snapshot(&mut self, snapshot: EditSnapshot, cx: &mut Context<Self>) {
        self.content = snapshot.content.into();
        self.selected_range = snapshot.selected_range;
        self.selection_reversed = snapshot.selection_reversed;
        self.marked_range = None;
        self.line_layouts.clear();
        self.clear_completions();
        cx.notify();
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let Some(previous) = self.undo_stack.pop() else {
            return;
        };
        self.redo_stack.push(self.current_snapshot());
        self.restore_snapshot(previous, cx);
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        self.undo_stack.push(self.current_snapshot());
        self.restore_snapshot(next, cx);
    }

    pub fn byte_len(&self) -> usize {
        self.content.len()
    }

    fn clear_completions(&mut self) {
        self.completions.clear();
        self.completion_index = 0;
        self.completion_prefix_start = 0;
    }

    fn refresh_completions(&mut self) {
        if !self.selected_range.is_empty() {
            self.clear_completions();
            return;
        }
        let cursor = self.cursor_offset();
        // Don't complete inside comments on this line.
        let line_start = self.content[..cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let line_prefix = &self.content[line_start..cursor];
        if line_prefix.contains("--") {
            self.clear_completions();
            return;
        }

        // After `mod.` show member list even if member prefix is empty.
        let (start, items) = if let Some((_mod, _mem, mem_start)) =
            syntax::member_prefix_at(&self.content, cursor)
        {
            let items = syntax::completions_at_with_profile(
                &self.content,
                cursor,
                "",
                self.target_profile.as_deref(),
                !self.target_locked,
            );
            (mem_start, items)
        } else {
            let (start, prefix) = syntax::word_prefix_at(&self.content, cursor);
            if prefix.is_empty() {
                self.clear_completions();
                return;
            }
            let items = syntax::completions_at_with_profile(
                &self.content,
                cursor,
                &prefix,
                self.target_profile.as_deref(),
                !self.target_locked,
            );
            (start, items)
        };

        if items.is_empty() {
            self.clear_completions();
            return;
        }

        // A completion popup is useful only while it can change the text.  The
        // providers intentionally use prefix matching, so without this filter
        // a fully typed keyword such as `end` would keep suggesting itself.
        let typed = &self.content[start..cursor];
        let items = actionable_completions(items, typed);
        if items.is_empty() {
            self.clear_completions();
            return;
        }

        self.completion_prefix_start = start;
        self.completion_index = 0;
        self.completions = items
            .into_iter()
            .map(|c: CompletionItem| CompletionEntry {
                label: c.label.into(),
                insert: c.insert.into(),
                detail: c.detail.into(),
            })
            .collect();
    }

    /// Static pre-read / lint for current buffer (MSPM0 Lua style).
    pub fn analyze(&self) -> Vec<syntax::AnalyzeIssue> {
        syntax::analyze_source_with_profile(&self.content, self.target_profile.as_deref())
    }

    fn accept_completion(
        &mut self,
        _: &AcceptCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.completions.is_empty() {
            return;
        }
        let item = self.completions[self.completion_index.min(self.completions.len() - 1)].clone();
        let start = self.completion_prefix_start;
        let end = self.cursor_offset();
        self.selected_range = start..end;
        self.selection_reversed = false;
        self.clear_completions();
        self.replace_text_in_range(None, &item.insert, window, cx);
        // Place cursor usefully: inside first () if present.
        if let Some(open) = item.insert.find('(') {
            if item.insert[open..].contains(')') {
                let cursor = start + open + 1;
                // if insert has quotes right after (, put cursor inside quotes
                let after = item.insert.as_bytes().get(open + 1).copied();
                let cursor = if after == Some(b'\'') || after == Some(b'"') {
                    cursor + 1
                } else {
                    cursor
                };
                self.selected_range = cursor..cursor;
            }
        }
        self.clear_completions();
        cx.notify();
    }

    fn escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        if !self.completions.is_empty() {
            self.clear_completions();
            cx.notify();
        }
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_completions();
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_completions();
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if !self.completions.is_empty() {
            if self.completion_index == 0 {
                self.completion_index = self.completions.len() - 1;
            } else {
                self.completion_index -= 1;
            }
            cx.notify();
            return;
        }
        let offset = self.cursor_offset();
        let (line, col) = self.line_col(offset);
        if line == 0 {
            self.move_to(0, cx);
            return;
        }
        self.move_to(self.offset_at(line - 1, col), cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if !self.completions.is_empty() {
            self.completion_index = (self.completion_index + 1) % self.completions.len();
            cx.notify();
            return;
        }
        let offset = self.cursor_offset();
        let (line, col) = self.line_col(offset);
        let lines = self.line_count();
        if line + 1 >= lines {
            self.move_to(self.content.len(), cx);
            return;
        }
        self.move_to(self.offset_at(line + 1, col), cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_completions();
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_completions();
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_completions();
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    pub fn select_all_pub(&mut self, cx: &mut Context<Self>) {
        self.clear_completions();
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_completions();
        let (line, _) = self.line_col(self.cursor_offset());
        self.move_to(self.offset_at(line, 0), cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_completions();
        let (line, _) = self.line_col(self.cursor_offset());
        let line_text = self.nth_line(line);
        self.move_to(self.offset_at(line, line_text.chars().count()), cx);
    }

    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if !self.completions.is_empty() {
            self.accept_completion(&AcceptCompletion, window, cx);
            return;
        }
        // Auto-indent: copy leading whitespace of current line; extra indent after `{([`.
        let cursor = self.cursor_offset();
        let line_start = self.content[..cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let line_prefix = &self.content[line_start..cursor];
        let indent: String = line_prefix
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let mut insert = format!("\n{indent}");
        let before = self.content[..cursor]
            .chars()
            .rev()
            .find(|c| !c.is_whitespace());
        if matches!(before, Some('{' | '(' | '[')) {
            insert.push_str("  ");
        }
        // If next non-ws is closing brace, put it on its own indented line.
        let after = self.content[cursor..].chars().find(|c| !c.is_whitespace());
        if matches!(before, Some('{')) && matches!(after, Some('}'))
            || matches!(before, Some('(')) && matches!(after, Some(')'))
            || matches!(before, Some('[')) && matches!(after, Some(']'))
        {
            let mid = format!("\n{indent}  ");
            let close_line = format!("\n{indent}");
            self.replace_text_in_range(None, &format!("{mid}{close_line}"), window, cx);
            // Cursor between the two lines (after mid).
            let pos = cursor + mid.len();
            self.selected_range = pos..pos;
            cx.notify();
            return;
        }
        self.replace_text_in_range(None, &insert, window, cx);
    }

    fn tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        if !self.completions.is_empty() {
            self.accept_completion(&AcceptCompletion, window, cx);
            return;
        }
        self.replace_text_in_range(None, "  ", window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let cursor = self.cursor_offset();
            // Delete empty auto-pair: | between () [] {} '' ""
            if cursor > 0 && cursor < self.content.len() {
                let bytes = self.content.as_bytes();
                let left = bytes[cursor - 1] as char;
                let right = bytes[cursor] as char;
                let pair = matches!(
                    (left, right),
                    ('(', ')') | ('[', ']') | ('{', '}') | ('\'', '\'') | ('"', '"')
                );
                if pair {
                    self.selected_range = (cursor - 1)..(cursor + 1);
                    self.selection_reversed = false;
                    self.replace_text_in_range(None, "", window, cx);
                    return;
                }
            }
            let prev = self.previous_boundary(cursor);
            if cursor == prev {
                return;
            }
            self.select_to(prev, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                return;
            }
            self.select_to(next, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(
                None,
                &text.replace("\r\n", "\n").replace('\r', "\n"),
                window,
                cx,
            );
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Scrollbar owns the pointer while dragging; never start a text selection.
        if self.sb_drag.active {
            return;
        }
        window.focus(&self.focus_handle);
        self.clear_completions();
        let idx = self.index_for_mouse_position(event.position);
        if event.click_count >= 2 {
            self.is_selecting = false;
            self.select_word_at(idx, cx);
            return;
        }
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(idx, cx);
        } else {
            self.move_to(idx, cx);
        }
    }

    fn on_context_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.clear_completions();
        if self.selected_range.is_empty() {
            self.move_to(self.index_for_mouse_position(event.position), cx);
        }
        let (x, y) = self
            .last_bounds
            .map(|bounds| {
                (
                    f32::from(event.position.x - bounds.left()),
                    f32::from(event.position.y - bounds.top()),
                )
            })
            .unwrap_or((8.0, 8.0));
        self.context_menu = Some((x.max(4.0), y.max(4.0)));
        cx.notify();
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Ctrl+wheel → zoom font (caller may also persist via font_px()).
        if event.modifiers.control {
            let dy = match event.delta {
                ScrollDelta::Lines(p) => p.y,
                ScrollDelta::Pixels(p) => f32::from(p.y) / 40.0,
            };
            // Wheel up (positive y on Windows often) → larger font.
            let step = if dy > 0.0 {
                1.0
            } else if dy < 0.0 {
                -1.0
            } else {
                0.0
            };
            if step != 0.0 {
                self.set_font_px(self.font_px + step, cx);
                cx.stop_propagation();
            }
            let _ = window;
            return;
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.sb_drag.active {
            self.sb_drag = ScrollbarDrag::default();
            cx.notify();
        }
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        // Continue thumb drag even when cursor leaves the scrollbar hitbox.
        if self.sb_drag.active {
            self.sb_drag
                .apply_move(&self.scroll_handle, event.position.y);
            cx.notify();
            return;
        }
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.content.len());
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
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
        let local_x = (position.x - bounds.left() - self.gutter_w).max(px(0.));
        let local = line
            .shaped
            .closest_index_for_x(local_x)
            .min(line.text.len());
        (line.start + local).min(self.content.len())
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

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn line_count(&self) -> usize {
        source_lines(&self.content).count()
    }

    fn nth_line(&self, line: usize) -> &str {
        self.content.split('\n').nth(line).unwrap_or("")
    }

    fn line_col(&self, offset: usize) -> (usize, usize) {
        line_col_at(&self.content, offset)
    }

    fn offset_at(&self, line: usize, col: usize) -> usize {
        let mut start = 0usize;
        for (i, l) in self.content.split('\n').enumerate() {
            if i == line {
                let mut c = 0usize;
                for (idx, _ch) in l.char_indices() {
                    if c == col {
                        return start + idx;
                    }
                    c += 1;
                }
                return start + l.len();
            }
            start += l.len() + 1;
        }
        self.content.len()
    }

    fn x_for_offset(&self, offset: usize, line_h: Pixels) -> (Pixels, Pixels) {
        let (line_idx, _) = self.line_col(offset);
        let y = line_h * line_idx as f32;
        if let Some(line) = self.line_layouts.get(line_idx) {
            let local = offset.saturating_sub(line.start).min(line.text.len());
            (line.shaped.x_for_index(local), y)
        } else {
            (px(0.), y)
        }
    }

    /// Find matching bracket pair around cursor. Returns (open_byte, close_byte) inclusive open, exclusive-ish close index of closer.
    fn matching_brackets(&self, cursor: usize) -> Option<(usize, usize)> {
        let bytes = self.content.as_bytes();
        if bytes.is_empty() {
            return None;
        }
        let len = bytes.len();
        // Prefer char under cursor, else char before cursor.
        let candidates = [cursor.min(len.saturating_sub(1)), cursor.saturating_sub(1)];
        for &pos in &candidates {
            if pos >= len {
                continue;
            }
            let ch = bytes[pos] as char;
            let (open, close, forward) = match ch {
                '(' => ('(', ')', true),
                ')' => ('(', ')', false),
                '[' => ('[', ']', true),
                ']' => ('[', ']', false),
                '{' => ('{', '}', true),
                '}' => ('{', '}', false),
                _ => continue,
            };
            if forward {
                let mut depth = 0i32;
                let mut i = pos;
                while i < len {
                    let c = bytes[i] as char;
                    // Skip simple string/comment-ish: ignore inside "..." on same scan (lightweight).
                    if c == open {
                        depth += 1;
                    } else if c == close {
                        depth -= 1;
                        if depth == 0 {
                            return Some((pos, i));
                        }
                    }
                    i += 1;
                }
            } else {
                let mut depth = 0i32;
                let mut i = pos as isize;
                while i >= 0 {
                    let c = bytes[i as usize] as char;
                    if c == close {
                        depth += 1;
                    } else if c == open {
                        depth -= 1;
                        if depth == 0 {
                            return Some((i as usize, pos));
                        }
                    }
                    i -= 1;
                }
            }
        }
        None
    }

    fn select_word_at(&mut self, offset: usize, cx: &mut Context<Self>) {
        let bytes = self.content.as_bytes();
        if bytes.is_empty() {
            return;
        }
        let offset = offset.min(bytes.len());
        let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut start = offset.min(bytes.len().saturating_sub(1));
        let mut end = start;
        if start < bytes.len() && is_word(bytes[start]) {
            while start > 0 && is_word(bytes[start - 1]) {
                start -= 1;
            }
            while end < bytes.len() && is_word(bytes[end]) {
                end += 1;
            }
        } else if start < bytes.len() {
            end = (start + 1).min(bytes.len());
        }
        self.selected_range = start..end;
        self.selection_reversed = false;
        cx.notify();
    }
}

impl EntityInputHandler for CodeEditor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sanitized_text = sanitize_source_text(new_text);
        let new_text = sanitized_text.as_str();
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        // Auto-pair brackets / quotes when typing a single opener over empty selection.
        let (insert, cursor_offset) = if range.is_empty() && new_text.chars().count() == 1 {
            let ch = new_text.chars().next().unwrap();
            let closer = match ch {
                '(' => Some(')'),
                '[' => Some(']'),
                '{' => Some('}'),
                '"' => Some('"'),
                '\'' => Some('\''),
                _ => None,
            };
            // Skip pair if next char already is the closer (type-through).
            let next = self.content[range.end..].chars().next();
            if closer.is_some() && next == closer {
                // Just move past existing closer when user types it.
                if matches!(ch, ')' | ']' | '}' | '"' | '\'') {
                    let pos = range.end + ch.len_utf8();
                    self.selected_range = pos..pos;
                    self.marked_range.take();
                    self.refresh_completions();
                    cx.notify();
                    return;
                }
            }
            if let Some(c) = closer {
                // Don't auto-pair quote when next char is identifier (likely closing).
                let skip_quote = matches!(ch, '"' | '\'')
                    && next
                        .map(|n| n.is_alphanumeric() || n == '_')
                        .unwrap_or(false);
                if !skip_quote {
                    (format!("{ch}{c}"), 1usize)
                } else {
                    (new_text.to_string(), new_text.len())
                }
            } else if matches!(ch, ')' | ']' | '}') && next == Some(ch) {
                // Type-through closing bracket.
                let pos = range.end + ch.len_utf8();
                self.selected_range = pos..pos;
                self.marked_range.take();
                cx.notify();
                return;
            } else {
                (new_text.to_string(), new_text.len())
            }
        } else {
            (new_text.to_string(), new_text.len())
        };

        self.remember_edit();
        self.content =
            (self.content[0..range.start].to_owned() + &insert + &self.content[range.end..]).into();
        let cursor = range.start + cursor_offset;
        self.selected_range = cursor..cursor;
        self.marked_range.take();
        self.refresh_completions();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sanitized_text = sanitize_source_text(new_text);
        let new_text = sanitized_text.as_str();
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.remember_edit();
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.refresh_completions();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let line_h = window.line_height();
        let (x0, y0) = self.x_for_offset(range.start, line_h);
        let (x1, y1) = self.x_for_offset(range.end, line_h);
        let left = bounds.left() + self.gutter_w;
        Some(Bounds::from_corners(
            point(left + x0, bounds.top() + y0 + px(4.)),
            point(
                left + x1.max(x0 + px(2.)),
                bounds.top() + y1 + px(4.) + line_h,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.index_for_mouse_position(point)))
    }
}

struct EditorElement {
    input: Entity<CodeEditor>,
}

struct GutterLine {
    shaped: ShapedLine,
}

struct PrepaintState {
    lines: Vec<LineLayoutCache>,
    gutter_lines: Vec<GutterLine>,
    gutter_w: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
    /// Bracket match + current line background quads (drawn under selection).
    highlights: Vec<PaintQuad>,
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

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
        // Content height so parent overflow_y_scroll can scroll long files.
        let lines = self.input.read(cx).line_count().max(8) as f32;
        style.size.height = (line_h * lines + px(24.)).into();
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
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();
        let theme = input.theme;
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_h = window.line_height();
        let empty = content.is_empty();
        let line_count_hint = source_lines(&content).count();
        let gutter_w = px(input.gutter_width_for(line_count_hint));
        let text_left = bounds.left() + gutter_w;

        let mut lines = Vec::new();
        if empty {
            // Keep an actual blank first line so focus and the caret are unambiguous.
            let display: SharedString = " ".into();
            let run = TextRun {
                len: display.len(),
                font: style.font(),
                color: style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped =
                window
                    .text_system()
                    .shape_line(display, font_size, &[run], None);
            lines.push(LineLayoutCache {
                start: 0,
                text: SharedString::from(""),
                shaped,
            });
        } else {
            for (start, raw) in source_lines(&content) {
                let display: SharedString = if raw.is_empty() {
                    SharedString::from(" ")
                } else {
                    SharedString::from(raw.to_string())
                };
                let runs: Vec<TextRun> = if raw.is_empty() {
                    vec![TextRun {
                        len: display.len(),
                        font: style.font(),
                        color: style.color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }]
                } else {
                    let spans = syntax::highlight_line(&raw);
                    let mut runs = Vec::new();
                    let mut covered = 0usize;
                    for span in spans {
                        if span.start > covered {
                            runs.push(TextRun {
                                len: span.start - covered,
                                font: style.font(),
                                color: style.color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            });
                        }
                        if span.end > span.start {
                            runs.push(TextRun {
                                len: span.end - span.start,
                                font: style.font(),
                                color: span.kind.color(&theme),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            });
                        }
                        covered = span.end.max(covered);
                    }
                    if covered < raw.len() {
                        runs.push(TextRun {
                            len: raw.len() - covered,
                            font: style.font(),
                            color: style.color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        });
                    }
                    if runs.is_empty() {
                        runs.push(TextRun {
                            len: display.len(),
                            font: style.font(),
                            color: style.color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        });
                    }
                    // shape_line requires sum(run.len) == text.len()
                    let sum: usize = runs.iter().map(|r| r.len).sum();
                    if sum != display.len() {
                        runs = vec![TextRun {
                            len: display.len(),
                            font: style.font(),
                            color: style.color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }];
                    }
                    runs
                };
                let shaped =
                    window
                        .text_system()
                        .shape_line(display.clone(), font_size, &runs, None);
                lines.push(LineLayoutCache {
                    start,
                    text: SharedString::from(raw.to_string()),
                    shaped,
                });
            }
        }

        let mut selections = Vec::new();
        let mut highlights = Vec::new();
        let mut cursor_quad = None;
        {
            // Current line: very soft background.
            {
                let mut line_idx = 0usize;
                for (i, _) in lines.iter().enumerate() {
                    let end = if i + 1 < lines.len() {
                        lines[i + 1].start
                    } else {
                        content.len() + 1
                    };
                    if cursor < end || i + 1 == lines.len() {
                        line_idx = i;
                        break;
                    }
                }
                // Full-width current line (gutter + text) so numbers don't feel split.
                highlights.push(fill(
                    Bounds::new(
                        point(
                            bounds.left(),
                            bounds.top() + px(4.) + line_h * line_idx as f32,
                        ),
                        size(bounds.size.width.max(px(1.)), line_h),
                    ),
                    theme.line_hl,
                ));
            }

            // Bracket match: thin outline box only (no span fill / no solid yellow).
            if selected_range.is_empty() {
                if let Some((open_i, close_i)) = input.matching_brackets(cursor) {
                    let outline_char = |byte_idx: usize, out: &mut Vec<PaintQuad>| {
                        let mut li = 0usize;
                        for (i, _) in lines.iter().enumerate() {
                            let end = if i + 1 < lines.len() {
                                lines[i + 1].start
                            } else {
                                content.len() + 1
                            };
                            if byte_idx < end || i + 1 == lines.len() {
                                li = i;
                                break;
                            }
                        }
                        let line = &lines[li.min(lines.len() - 1)];
                        let local = byte_idx.saturating_sub(line.start).min(line.text.len());
                        let x0 = line.shaped.x_for_index(local);
                        let x1 = if local < line.text.len() {
                            line.shaped.x_for_index((local + 1).min(line.text.len()))
                        } else {
                            x0 + px(8.)
                        };
                        let left = text_left + x0;
                        let right = text_left + x1.max(x0 + px(6.));
                        let top = bounds.top() + px(4.) + line_h * li as f32;
                        let bottom = top + line_h;
                        let t = px(1.);
                        let c = theme.match_bracket;
                        // top / bottom / left / right edges
                        out.push(fill(
                            Bounds::from_corners(point(left, top), point(right, top + t)),
                            c,
                        ));
                        out.push(fill(
                            Bounds::from_corners(point(left, bottom - t), point(right, bottom)),
                            c,
                        ));
                        out.push(fill(
                            Bounds::from_corners(point(left, top), point(left + t, bottom)),
                            c,
                        ));
                        out.push(fill(
                            Bounds::from_corners(point(right - t, top), point(right, bottom)),
                            c,
                        ));
                    };
                    outline_char(open_i, &mut highlights);
                    outline_char(close_i, &mut highlights);
                }
            }

            if selected_range.is_empty() {
                let mut line_idx = 0usize;
                for (i, _) in lines.iter().enumerate() {
                    let end = if i + 1 < lines.len() {
                        lines[i + 1].start
                    } else {
                        content.len() + 1
                    };
                    if cursor < end || i + 1 == lines.len() {
                        line_idx = i;
                        break;
                    }
                }
                let line = &lines[line_idx.min(lines.len() - 1)];
                let local = cursor.saturating_sub(line.start).min(line.text.len());
                let x = line.shaped.x_for_index(local);
                cursor_quad = Some(fill(
                    Bounds::new(
                        point(
                            text_left + x,
                            bounds.top() + px(4.) + line_h * line_idx as f32,
                        ),
                        size(px(2.), line_h),
                    ),
                    theme.blue,
                ));
            } else {
                for (i, line) in lines.iter().enumerate() {
                    let line_end = if i + 1 < lines.len() {
                        lines[i + 1].start
                    } else {
                        content.len()
                    };
                    let sel_start = selected_range.start.max(line.start);
                    let sel_end = selected_range.end.min(line_end);
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
                                point(text_left + x0, bounds.top() + px(4.) + line_h * i as f32),
                                point(
                                    text_left + x1,
                                    bounds.top() + px(4.) + line_h * (i as f32 + 1.),
                                ),
                            ),
                            theme.selection,
                        ));
                    }
                }
            }
        }

        // Line numbers: soft, same surface as code (no panel/divider).
        let mut active_line = 0usize;
        {
            for (i, _) in lines.iter().enumerate() {
                let end = if i + 1 < lines.len() {
                    lines[i + 1].start
                } else {
                    content.len() + 1
                };
                if cursor < end || i + 1 == lines.len() {
                    active_line = i;
                    break;
                }
            }
        }
        let gutter_font = font_size * 0.88;
        let mut gutter_lines = Vec::with_capacity(lines.len());
        for i in 0..lines.len() {
            let label: SharedString = format!("{}", i + 1).into();
            let run = TextRun {
                len: label.len(),
                font: style.font(),
                color: if i == active_line {
                    theme.gutter_active
                } else {
                    theme.gutter
                },
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window
                .text_system()
                .shape_line(label, gutter_font, &[run], None);
            gutter_lines.push(GutterLine { shaped });
        }

        PrepaintState {
            lines,
            gutter_lines,
            gutter_w,
            cursor: cursor_quad,
            selections,
            highlights,
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
        let focus_handle = self.input.read(cx).focus_handle.clone();
        let gutter_w = prepaint.gutter_w;
        let text_origin = bounds.origin + point(gutter_w, px(0.));
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(
                Bounds::new(
                    text_origin,
                    size(
                        (bounds.size.width - gutter_w).max(px(1.)),
                        bounds.size.height,
                    ),
                ),
                self.input.clone(),
            ),
            cx,
        );
        // No separate gutter panel — numbers sit on the same code surface.
        for h in prepaint.highlights.drain(..) {
            window.paint_quad(h);
        }
        for sel in prepaint.selections.drain(..) {
            window.paint_quad(sel);
        }
        let line_h = window.line_height();
        for (i, gl) in prepaint.gutter_lines.iter().enumerate() {
            let tw = gl.shaped.width;
            let x = (gutter_w - tw - px(6.)).max(px(2.));
            gl.shaped
                .paint(
                    bounds.origin + point(x, px(4.) + line_h * i as f32),
                    line_h,
                    window,
                    cx,
                )
                .ok();
        }
        for (i, line) in prepaint.lines.iter().enumerate() {
            line.shaped
                .paint(
                    text_origin + point(px(0.), px(4.) + line_h * i as f32),
                    line_h,
                    window,
                    cx,
                )
                .ok();
        }
        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }
        let lines = std::mem::take(&mut prepaint.lines);
        self.input.update(cx, |input, _cx| {
            input.line_layouts = lines;
            input.last_bounds = Some(bounds);
            input.line_height = line_h;
            input.top_pad = px(4.);
            input.gutter_w = gutter_w;
        });
    }
}

impl Render for CodeEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let completions = self.completions.clone();
        let completion_index = self.completion_index;
        let show_comp = !completions.is_empty();
        let context_menu = self.context_menu;

        // Root keeps move/up so scrollbar drag continues outside the thumb hitbox.
        // Text selection mouse_down lives only on the content pane (not the scrollbar).
        div()
            .id("code-editor")
            .key_context("CodeEditor")
            .track_focus(&self.focus_handle(cx))
            .relative()
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(Self::accept_completion))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.code)
            .text_size(px(self.font_px))
            .text_color(theme.text)
            .font_family("Cascadia Code")
            .line_height(self.line_height_px())
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        div()
                            .id("editor-scroll")
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .h_full()
                            .pt_1()
                            .pb_2()
                            .pr_2()
                            .cursor(CursorStyle::IBeam)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_context_menu))
                            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll_handle)
                            .child(EditorElement { input: cx.entity() }),
                    )
                    .child({
                        let entity = cx.entity();
                        let entity2 = entity.clone();
                        v_scrollbar(
                            "editor-sb",
                            &self.scroll_handle,
                            self.sb_drag.clone(),
                            theme,
                            move |d, cx| {
                                entity.update(cx, |ed, cx| {
                                    ed.is_selecting = false;
                                    ed.sb_drag = d;
                                    cx.notify();
                                });
                            },
                            move |cx| {
                                entity2.update(cx, |_, cx| cx.notify());
                            },
                        )
                    }),
            )
            .when(show_comp, |el| {
                el.child(
                    div()
                        .w_full()
                        .max_h(px(160.))
                        .mt_1()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.blue)
                        .bg(theme.panel)
                        .overflow_hidden()
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .border_b_1()
                                .border_color(theme.line)
                                .text_xs()
                                .text_color(theme.muted)
                                .child("补全 · Tab/Enter 确认 · ↑↓ 选择 · Esc 关闭"),
                        )
                        .children(completions.into_iter().enumerate().map(|(i, item)| {
                            let selected = i == completion_index;
                            let insert = item.insert.clone();
                            div()
                                .id(SharedString::from(format!("comp-{i}")))
                                .flex()
                                .items_center()
                                .justify_between()
                                .px_2()
                                .py_1()
                                .bg(if selected {
                                    theme.accent_soft
                                } else {
                                    theme.panel
                                })
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.completion_index = i;
                                    // inject selected insert
                                    let start = this.completion_prefix_start;
                                    let end = this.cursor_offset();
                                    this.selected_range = start..end;
                                    this.selection_reversed = false;
                                    let text = insert.to_string();
                                    this.clear_completions();
                                    this.replace_text_in_range(None, &text, window, cx);
                                    if let Some(open) = text.find('(') {
                                        let cursor = start + open + 1;
                                        let after = text.as_bytes().get(open + 1).copied();
                                        let cursor = if after == Some(b'\'') || after == Some(b'"')
                                        {
                                            cursor + 1
                                        } else {
                                            cursor
                                        };
                                        this.selected_range = cursor..cursor;
                                    }
                                    this.clear_completions();
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_family("Cascadia Code")
                                        .text_color(if selected { theme.blue } else { theme.text })
                                        .child(item.label),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted)
                                        .font_family("Cascadia Code")
                                        .child(item.detail),
                                )
                        })),
                )
            })
            .when_some(context_menu, |el, (x, y)| {
                el.child(
                    div()
                        .id("editor-context-dismiss")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                            this.context_menu = None;
                            cx.notify();
                        }))
                        .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| {
                            this.context_menu = None;
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id("editor-context-menu")
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .min_w(px(132.))
                        .rounded_sm()
                        .border_1()
                        .border_color(theme.line)
                        .bg(theme.panel)
                        .shadow_md()
                        .py_1()
                        .occlude()
                        .child(editor_context_item("editor-context-copy", "复制", cx.listener(|this, _, window, cx| {
                            this.copy(&Copy, window, cx);
                            this.context_menu = None;
                            cx.notify();
                        })))
                        .child(editor_context_item("editor-context-undo", "撤销", cx.listener(|this, _, window, cx| {
                            this.undo(&Undo, window, cx);
                            this.context_menu = None;
                            cx.notify();
                        })))
                        .child(editor_context_item("editor-context-redo", "重做", cx.listener(|this, _, window, cx| {
                            this.redo(&Redo, window, cx);
                            this.context_menu = None;
                            cx.notify();
                        })))
                        .child(editor_context_item("editor-context-cut", "剪切", cx.listener(|this, _, window, cx| {
                            this.cut(&Cut, window, cx);
                            this.context_menu = None;
                            cx.notify();
                        })))
                        .child(editor_context_item("editor-context-paste", "粘贴", cx.listener(|this, _, window, cx| {
                            this.paste(&Paste, window, cx);
                            this.context_menu = None;
                            cx.notify();
                        })))
                        .child(editor_context_item("editor-context-select-all", "全选", cx.listener(|this, _, window, cx| {
                            this.select_all(&SelectAll, window, cx);
                            this.context_menu = None;
                            cx.notify();
                        }))),
                )
            })
    }
}

fn editor_context_item(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1p5()
        .text_xs()
        .cursor_pointer()
        .hover(|style| style.bg(gpui::rgb(0x26384c)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
        .child(label)
}

impl Focusable for CodeEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{actionable_completions, line_col_at, source_lines};
    use crate::syntax::CompletionItem;

    #[test]
    fn exact_completion_does_not_keep_popup_open() {
        let items = actionable_completions(
            vec![
                CompletionItem {
                    label: "end".into(),
                    insert: "end".into(),
                    detail: "keyword".into(),
                },
                CompletionItem {
                    label: "elseif".into(),
                    insert: "elseif".into(),
                    detail: "keyword".into(),
                },
            ],
            "end",
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "elseif");
    }

    #[test]
    fn trailing_newline_creates_exactly_one_empty_line() {
        let source = "1\n2\n3\n4\n5\n6\n7\n";
        let lines: Vec<_> = source_lines(source).collect();

        assert_eq!(lines.len(), 8);
        assert_eq!(lines[7], (source.len(), ""));
        assert_eq!(line_col_at(source, source.len()), (7, 0));
    }

    #[test]
    fn typing_on_trailing_empty_line_keeps_its_line_number() {
        let mut source = String::from("1\n2\n3\n4\n5\n6\n7\n");
        source.push('x');

        assert_eq!(source_lines(&source).count(), 8);
        assert_eq!(line_col_at(&source, source.len()), (7, 1));
    }
}
