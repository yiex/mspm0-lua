//! HSV color wheel: drag to pick hue + saturation; lightness via strip.

use gpui::{
    canvas, div, fill, hsla, point, prelude::*, px, size, Bounds, Context, CursorStyle,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, SharedString, Window,
};

use crate::theme::Theme;

const WHEEL_SIZE: f32 = 168.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ColorPart {
    #[default]
    Accent,
    Bg,
    Panel,
    Code,
    Text,
}

impl ColorPart {
    pub fn all() -> [ColorPart; 5] {
        [
            ColorPart::Accent,
            ColorPart::Bg,
            ColorPart::Panel,
            ColorPart::Code,
            ColorPart::Text,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            ColorPart::Accent => "强调",
            ColorPart::Bg => "背景",
            ColorPart::Panel => "面板",
            ColorPart::Code => "代码",
            ColorPart::Text => "文字",
        }
    }
}

pub struct ColorWheel {
    pub hue: f32,
    pub sat: f32,
    pub light: f32,
    pub part: ColorPart,
    dragging: bool,
    theme: Theme,
    last_bounds: Option<Bounds<Pixels>>,
    /// Bumped when color changes (parent polls).
    pub revision: u64,
}

impl ColorWheel {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            hue: 0.58,
            sat: 0.55,
            light: 0.55,
            part: ColorPart::Accent,
            dragging: false,
            theme: Theme::default(),
            last_bounds: None,
            revision: 0,
        }
    }

    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    pub fn set_hsla(&mut self, h: f32, s: f32, l: f32, cx: &mut Context<Self>) {
        self.hue = h.clamp(0., 1.);
        self.sat = s.clamp(0., 1.);
        self.light = l.clamp(0.08, 0.95);
        cx.notify();
    }

    pub fn set_part(&mut self, part: ColorPart, cx: &mut Context<Self>) {
        self.part = part;
        cx.notify();
    }

    pub fn current_rgb(&self) -> u32 {
        hsla_to_rgb(self.hue, self.sat, self.light)
    }

    fn apply_pos(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = self.last_bounds else {
            return;
        };
        let side = bounds.size.width.min(bounds.size.height).max(px(1.));
        let cx0 = bounds.left() + side / 2.;
        let cy0 = bounds.top() + side / 2.;
        let dx = f32::from(pos.x - cx0);
        let dy = f32::from(pos.y - cy0);
        let r = (dx * dx + dy * dy).sqrt();
        let max_r = f32::from(side) * 0.5;
        if max_r <= 1. {
            return;
        }
        let mut ang = dy.atan2(dx);
        if ang < 0. {
            ang += std::f32::consts::TAU;
        }
        self.hue = (ang / std::f32::consts::TAU).clamp(0., 1.);
        self.sat = (r / max_r).clamp(0., 1.);
        self.revision = self.revision.wrapping_add(1);
        cx.notify();
    }

    fn on_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        self.dragging = true;
        self.apply_pos(event.position, cx);
    }

    fn on_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.dragging && event.pressed_button == Some(MouseButton::Left) {
            self.apply_pos(event.position, cx);
        }
    }

    fn on_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.dragging {
            self.dragging = false;
            self.revision = self.revision.wrapping_add(1);
            cx.notify();
        }
    }
}

pub fn hsla_to_rgb(h: f32, s: f32, l: f32) -> u32 {
    let c = hsla(h, s, l, 1.0).to_rgb();
    let r = (c.r * 255.0).round() as u32;
    let g = (c.g * 255.0).round() as u32;
    let b = (c.b * 255.0).round() as u32;
    (r << 16) | (g << 8) | b
}

pub fn rgb_to_hsla(rgb_u: u32) -> (f32, f32, f32) {
    let r = ((rgb_u >> 16) & 0xff) as f32 / 255.0;
    let g = ((rgb_u >> 8) & 0xff) as f32 / 255.0;
    let b = (rgb_u & 0xff) as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    let d = max - min;
    if d < 1e-6 {
        return (0., 0., l);
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < 1e-6 {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if (max - g).abs() < 1e-6 {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h.rem_euclid(1.), s.clamp(0., 1.), l.clamp(0., 1.))
}

impl Render for ColorWheel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let part = self.part;
        let light = self.light;
        let hue = self.hue;
        let sat = self.sat;
        let preview = hsla(hue, sat, light, 1.0);
        let hex = format!("#{:06X}", self.current_rgb());
        let entity = cx.entity();

        div()
            .id("color-wheel-root")
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .children(ColorPart::all().into_iter().map(|p| {
                        let on = p == part;
                        div()
                            .id(SharedString::from(format!("cw-part-{}", p.label())))
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
                                this.set_part(p, cx);
                            }))
                            .child(p.label())
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .id("wheel-pad")
                            .w(px(WHEEL_SIZE))
                            .h(px(WHEEL_SIZE))
                            .cursor(CursorStyle::PointingHand)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
                            .on_mouse_move(cx.listener(Self::on_move))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
                            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_up))
                            .child({
                                let entity = entity.clone();
                                canvas(
                                    move |bounds, _, cx| {
                                        entity.update(cx, |this, _| {
                                            this.last_bounds = Some(bounds);
                                        });
                                    },
                                    move |bounds, (), window, _cx| {
                                        let side = bounds.size.width.min(bounds.size.height);
                                        let origin = bounds.origin;
                                        let n = 36i32;
                                        let cell = side / n as f32;
                                        let cx0 = origin.x + side / 2.;
                                        let cy0 = origin.y + side / 2.;
                                        let max_r = f32::from(side) * 0.5;
                                        for iy in 0..n {
                                            for ix in 0..n {
                                                let px_x = origin.x + cell * ix as f32 + cell / 2.;
                                                let px_y = origin.y + cell * iy as f32 + cell / 2.;
                                                let dx = f32::from(px_x - cx0);
                                                let dy = f32::from(px_y - cy0);
                                                let r = (dx * dx + dy * dy).sqrt();
                                                if r > max_r {
                                                    continue;
                                                }
                                                let mut ang = dy.atan2(dx);
                                                if ang < 0. {
                                                    ang += std::f32::consts::TAU;
                                                }
                                                let h = ang / std::f32::consts::TAU;
                                                let s = (r / max_r).clamp(0., 1.);
                                                window.paint_quad(fill(
                                                    Bounds::new(
                                                        point(
                                                            origin.x + cell * ix as f32,
                                                            origin.y + cell * iy as f32,
                                                        ),
                                                        size(cell + px(0.5), cell + px(0.5)),
                                                    ),
                                                    hsla(h, s, light, 1.0),
                                                ));
                                            }
                                        }
                                        let ang = hue * std::f32::consts::TAU;
                                        let rr = sat * max_r;
                                        let mx = cx0 + px(rr * ang.cos());
                                        let my = cy0 + px(rr * ang.sin());
                                        let ring = hsla(
                                            0.,
                                            0.,
                                            if light > 0.5 { 0.12 } else { 0.95 },
                                            1.0,
                                        );
                                        window.paint_quad(fill(
                                            Bounds::new(
                                                point(mx - px(5.), my - px(5.)),
                                                size(px(10.), px(10.)),
                                            ),
                                            ring,
                                        ));
                                        window.paint_quad(fill(
                                            Bounds::new(
                                                point(mx - px(3.), my - px(3.)),
                                                size(px(6.), px(6.)),
                                            ),
                                            hsla(hue, sat, light, 1.0),
                                        ));
                                    },
                                )
                                .size_full()
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .w(px(100.))
                            .child(div().text_xs().text_color(theme.muted).child("明度"))
                            .child({
                                let mut bands = Vec::new();
                                for i in 0..12 {
                                    let l = 0.08 + (i as f32 / 11.0) * 0.84;
                                    let on = (light - l).abs() < 0.045;
                                    bands.push(
                                        div()
                                            .id(SharedString::from(format!("light-{i}")))
                                            .h(px(12.))
                                            .w_full()
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(if on { theme.blue } else { theme.line })
                                            .bg(hsla(hue, sat, l, 1.0))
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.light = l;
                                                this.revision = this.revision.wrapping_add(1);
                                                cx.notify();
                                            }))
                                            .into_any_element(),
                                    );
                                }
                                div().flex().flex_col().gap(px(2.)).children(bands)
                            })
                            .child(
                                div()
                                    .mt_1()
                                    .w_full()
                                    .h(px(28.))
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.line)
                                    .bg(preview),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_family("Cascadia Code")
                                    .text_color(theme.muted)
                                    .child(hex),
                            ),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted)
                    .child("拖动色盘 · 点明度条 · 切换部位分别上色"),
            )
    }
}
