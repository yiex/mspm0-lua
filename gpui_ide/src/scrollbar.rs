use gpui::{
    div, prelude::*, px, App, CursorStyle, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollHandle, SharedString, Window,
};

use crate::theme::Theme;

/// State for dragging the vertical scrollbar thumb.
#[derive(Clone, Default)]
pub struct ScrollbarDrag {
    pub active: bool,
    /// Mouse Y at drag start (window coords).
    pub start_y: Pixels,
    /// Scroll offset.y at drag start.
    pub start_off: Pixels,
}

impl ScrollbarDrag {
    /// Apply a mouse-move while this drag is active (may leave the thumb hitbox).
    pub fn apply_move(&self, handle: &ScrollHandle, mouse_y: Pixels) {
        if !self.active {
            return;
        }
        let b = handle.bounds();
        let max_y = handle.max_offset().height.max(px(1.));
        let view_h = b.size.height.max(px(1.));
        let content_h = view_h + max_y;
        let ratio = (view_h / content_h).clamp(0.08, 1.0);
        let thumb_h = (view_h * ratio).max(px(24.));
        let travel = (view_h - thumb_h).max(px(1.));
        let dy = mouse_y - self.start_y;
        let delta_off = -(dy / travel) * max_y;
        let new_y = (self.start_off + delta_off).clamp(-max_y, px(0.));
        handle.set_offset(Point {
            x: px(0.),
            y: new_y,
        });
    }
}

/// Whether the scroll view is pinned to the bottom (within a small threshold).
pub fn is_scrolled_to_bottom(handle: &ScrollHandle) -> bool {
    let max_y = handle.max_offset().height;
    if max_y <= px(2.) {
        return true;
    }
    let off = -handle.offset().y;
    (max_y - off) <= px(8.)
}

/// Visible vertical scrollbar (track + thumb) bound to a ScrollHandle.
///
/// While `drag.active`, move events on this bar update scroll. Parent views
/// should also call `ScrollbarDrag::apply_move` on their own mouse_move so
/// dragging continues when the cursor leaves the bar into the content pane.
pub fn v_scrollbar(
    id: impl Into<SharedString>,
    handle: &ScrollHandle,
    drag: ScrollbarDrag,
    theme: Theme,
    set_drag: impl Fn(ScrollbarDrag, &mut App) + 'static,
    on_scroll: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    let id: SharedString = id.into();
    let handle = handle.clone();
    let max = handle.max_offset();
    let bounds = handle.bounds();
    let view_h = bounds.size.height.max(px(1.));
    let max_y = max.height.max(px(0.));
    let content_h = view_h + max_y;
    let needs = max_y > px(1.);
    let ratio = if content_h > px(0.) {
        (view_h / content_h).clamp(0.08, 1.0)
    } else {
        1.0
    };
    let track_h = view_h.max(px(40.));
    let thumb_h = (track_h * ratio).max(px(24.));
    let travel = (track_h - thumb_h).max(px(1.));
    let off_y = -handle.offset().y;
    let frac = if max_y > px(0.) {
        (off_y / max_y).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb_top = travel * frac;

    let handle_track = handle.clone();
    let handle_thumb = handle.clone();
    let handle_move = handle.clone();
    let set_drag = std::rc::Rc::new(set_drag);
    let on_scroll = std::rc::Rc::new(on_scroll);
    let drag_for_move = drag.clone();

    div()
        .id(id.clone())
        .w(px(12.))
        .h_full()
        .flex_shrink_0()
        .bg(theme.scrollbar)
        .border_l_1()
        .border_color(theme.line)
        // Keep parent text-select hitboxes from claiming clicks on the bar.
        .occlude()
        .cursor(CursorStyle::Arrow)
        // While dragging over the bar itself (parent is occluded / not hovered).
        .on_mouse_move({
            let on_scroll = on_scroll.clone();
            move |ev: &MouseMoveEvent, _window: &mut Window, cx: &mut App| {
                if drag_for_move.active {
                    drag_for_move.apply_move(&handle_move, ev.position.y);
                    on_scroll(cx);
                }
            }
        })
        .on_mouse_up(MouseButton::Left, {
            let set_drag = set_drag.clone();
            let was_dragging = drag.active;
            move |_ev: &MouseUpEvent, _window: &mut Window, cx: &mut App| {
                if was_dragging {
                    set_drag(ScrollbarDrag::default(), cx);
                }
            }
        })
        .when(needs, |el| {
            el.child(
                div()
                    .id(SharedString::from(format!("{id}-track")))
                    .relative()
                    .w_full()
                    .h_full()
                    .cursor(CursorStyle::Arrow)
                    .on_mouse_down(MouseButton::Left, {
                        let handle = handle_track;
                        let on_scroll = on_scroll.clone();
                        move |ev: &MouseDownEvent, _window: &mut Window, cx: &mut App| {
                            cx.stop_propagation();
                            let b = handle.bounds();
                            let local_y = (ev.position.y - b.top()).max(px(0.));
                            let max_y = handle.max_offset().height.max(px(1.));
                            let view_h = b.size.height.max(px(1.));
                            let content_h = view_h + max_y;
                            let ratio = (view_h / content_h).clamp(0.08, 1.0);
                            let thumb_h = (view_h * ratio).max(px(24.));
                            let travel = (view_h - thumb_h).max(px(1.));
                            let y = (local_y - thumb_h * 0.5).clamp(px(0.), travel);
                            let frac = y / travel;
                            let new_off = -max_y * frac;
                            handle.set_offset(Point {
                                x: px(0.),
                                y: new_off,
                            });
                            on_scroll(cx);
                        }
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("{id}-thumb")))
                            .absolute()
                            .left(px(2.))
                            .top(thumb_top)
                            .w(px(8.))
                            .h(thumb_h)
                            .rounded_sm()
                            .bg(theme.line)
                            .hover(|s| s.bg(theme.muted))
                            .cursor(CursorStyle::Arrow)
                            .on_mouse_down(MouseButton::Left, {
                                let handle = handle_thumb;
                                let set_drag = set_drag.clone();
                                move |ev: &MouseDownEvent, _window: &mut Window, cx: &mut App| {
                                    cx.stop_propagation();
                                    set_drag(
                                        ScrollbarDrag {
                                            active: true,
                                            start_y: ev.position.y,
                                            start_off: handle.offset().y,
                                        },
                                        cx,
                                    );
                                }
                            }),
                    ),
            )
        })
}
