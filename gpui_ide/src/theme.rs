use gpui::{hsla, rgb, Hsla};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeId {
    /// Graphite dark (`dim` in old config maps here).
    #[default]
    #[serde(alias = "dim")]
    Dark,
    /// Soft paper light.
    Light,
    /// User palette (accent + optional surface overrides).
    Custom,
}

impl ThemeId {
    /// Dark → Light → Custom → Dark
    pub fn next(self) -> Self {
        match self {
            ThemeId::Dark => ThemeId::Light,
            ThemeId::Light => ThemeId::Custom,
            ThemeId::Custom => ThemeId::Dark,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeId::Dark => "深色",
            ThemeId::Light => "浅色",
            ThemeId::Custom => "自定义",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            ThemeId::Dark => "\u{e708}",   // moon
            ThemeId::Light => "\u{e706}",  // sun
            ThemeId::Custom => "\u{e790}", // color
        }
    }
}

/// Optional per-surface colors (0xRRGGBB) for Custom theme.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomPalette {
    pub accent: Option<u32>,
    pub bg: Option<u32>,
    pub panel: Option<u32>,
    pub code: Option<u32>,
    pub text: Option<u32>,
}

impl CustomPalette {
    pub fn is_empty(&self) -> bool {
        self.accent.is_none()
            && self.bg.is_none()
            && self.panel.is_none()
            && self.code.is_none()
            && self.text.is_none()
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub id: ThemeId,
    pub bg: Hsla,
    pub panel: Hsla,
    pub panel2: Hsla,
    pub line: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub blue: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub red: Hsla,
    pub code: Hsla,
    pub accent_soft: Hsla,
    pub titlebar: Hsla,
    pub menu_hover: Hsla,
    pub scrollbar: Hsla,
    pub danger: Hsla,
    pub danger_border: Hsla,
    pub selection: Hsla,
    pub line_hl: Hsla,
    pub match_bracket: Hsla,
    pub btn_bg: Hsla,
    pub btn_border: Hsla,
    pub btn_primary_fg: Hsla,
    pub gutter: Hsla,
    pub gutter_active: Hsla,
    pub syn_text: Hsla,
    pub syn_keyword: Hsla,
    pub syn_builtin: Hsla,
    pub syn_module: Hsla,
    pub syn_string: Hsla,
    pub syn_number: Hsla,
    pub syn_comment: Hsla,
    pub syn_operator: Hsla,
    pub syn_pin: Hsla,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0., 1.);
    hsla(
        a.h + (b.h - a.h) * t,
        a.s + (b.s - a.s) * t,
        a.l + (b.l - a.l) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn with_alpha(c: Hsla, a: f32) -> Hsla {
    Hsla {
        h: c.h,
        s: c.s,
        l: c.l,
        a: a.clamp(0., 1.),
    }
}

fn comment_tone(text: Hsla, code: Hsla) -> Hsla {
    let toward_code = if code.l < 0.5 { 0.45 } else { 0.38 };
    let mut comment = mix(text, code, toward_code);
    comment.s = 0.04;
    comment
}

fn from_rgb(u: u32) -> Hsla {
    rgb(u).into()
}

impl Theme {
    pub fn from_id(id: ThemeId) -> Self {
        Self::resolve(id, None)
    }

    /// Pure base for id (no user overrides).
    pub fn base(id: ThemeId) -> Self {
        match id {
            ThemeId::Light => Self::light(),
            ThemeId::Dark => Self::dark(),
            ThemeId::Custom => {
                let mut t = Self::dark();
                t.id = ThemeId::Custom;
                t
            }
        }
    }

    /// Build theme for `id` using only that theme's palette (fully isolated).
    pub fn resolve(id: ThemeId, palette: Option<&CustomPalette>) -> Self {
        let mut t = Self::base(id);
        let Some(p) = palette else {
            return t;
        };
        // Empty palette → stock theme (Dark/Light defaults).
        if p.is_empty() && id != ThemeId::Custom {
            return t;
        }
        if let Some(a) = p.accent {
            t = t.with_accent(a);
        } else if id == ThemeId::Custom {
            // Custom with no accent still needs a stable accent.
            t = t.with_accent(0x6aa3e8);
        }
        if let Some(bg) = p.bg {
            t = t.with_bg(bg);
        }
        if let Some(panel) = p.panel {
            t = t.with_panel(panel);
        }
        if let Some(code) = p.code {
            t = t.with_code(code);
        }
        if let Some(text) = p.text {
            t = t.with_text(text);
        }
        t
    }

    pub fn with_accent(mut self, accent_rgb: u32) -> Self {
        let accent = from_rgb(accent_rgb);
        let dark = matches!(self.id, ThemeId::Dark | ThemeId::Custom) || self.bg.l < 0.5;
        self.blue = accent;
        if dark {
            self.accent_soft = mix(self.panel, accent, 0.28);
            self.selection = mix(self.code, accent, 0.42);
            self.btn_primary_fg = from_rgb(0xffffff);
        } else {
            self.accent_soft = mix(from_rgb(0xffffff), accent, 0.18);
            self.selection = mix(from_rgb(0xffffff), accent, 0.32);
            self.btn_primary_fg = if accent.l < 0.55 {
                from_rgb(0xffffff)
            } else {
                from_rgb(0x1a1d23)
            };
        }
        self
    }

    pub fn with_bg(mut self, rgb_u: u32) -> Self {
        let bg = from_rgb(rgb_u);
        self.bg = bg;
        self.scrollbar = bg;
        self.line_hl = mix(bg, self.panel, 0.35);
        self
    }

    pub fn with_panel(mut self, rgb_u: u32) -> Self {
        let panel = from_rgb(rgb_u);
        self.panel = panel;
        self.titlebar = panel;
        self.panel2 = mix(panel, self.text, 0.08);
        self.menu_hover = self.panel2;
        self.btn_bg = self.panel2;
        self.line = mix(panel, self.text, 0.18);
        self.btn_border = mix(panel, self.text, 0.28);
        self.match_bracket = mix(panel, self.text, 0.35);
        self.accent_soft = mix(panel, self.blue, 0.28);
        self
    }

    pub fn with_code(mut self, rgb_u: u32) -> Self {
        let code = from_rgb(rgb_u);
        self.code = code;
        self.line_hl = mix(code, self.panel, 0.25);
        self.selection = mix(code, self.blue, 0.42);
        self.syn_comment = comment_tone(self.text, code);
        self
    }

    pub fn with_text(mut self, rgb_u: u32) -> Self {
        let text = from_rgb(rgb_u);
        self.text = text;
        self.syn_text = text;
        self.muted = mix(text, self.bg, 0.45);
        self.gutter = with_alpha(self.muted, 0.42);
        self.gutter_active = with_alpha(self.muted, 0.78);
        self.syn_comment = comment_tone(text, self.code);
        self
    }

    pub fn dark() -> Self {
        let muted: Hsla = from_rgb(0x8b929e);
        Self {
            id: ThemeId::Dark,
            bg: from_rgb(0x1a1d23),
            panel: from_rgb(0x22262e),
            panel2: from_rgb(0x2c313a),
            line: from_rgb(0x3d4450),
            text: from_rgb(0xc8cdd5),
            muted,
            blue: from_rgb(0x6aa3e8),
            green: from_rgb(0x7cbc8a),
            yellow: from_rgb(0xd4b56a),
            red: from_rgb(0xd66b6b),
            code: from_rgb(0x16191f),
            accent_soft: from_rgb(0x2a3a52),
            titlebar: from_rgb(0x22262e),
            menu_hover: from_rgb(0x2c313a),
            scrollbar: from_rgb(0x1a1d23),
            danger: from_rgb(0x3a2428),
            danger_border: from_rgb(0xa05050),
            selection: from_rgb(0x2e436e),
            line_hl: from_rgb(0x1c2028),
            match_bracket: from_rgb(0x4a5568),
            btn_bg: from_rgb(0x2c313a),
            btn_border: from_rgb(0x4a5260),
            btn_primary_fg: from_rgb(0xffffff),
            gutter: with_alpha(muted, 0.42),
            gutter_active: with_alpha(muted, 0.78),
            syn_text: from_rgb(0xc8cdd5),
            syn_keyword: from_rgb(0xc792ea),
            syn_builtin: from_rgb(0x82aaff),
            syn_module: from_rgb(0x7fdbca),
            syn_string: from_rgb(0xc3e88d),
            syn_number: from_rgb(0xf78c6c),
            syn_comment: from_rgb(0x7b818a),
            syn_operator: from_rgb(0x89ddff),
            syn_pin: from_rgb(0xffcb6b),
        }
    }

    pub fn light() -> Self {
        let muted: Hsla = from_rgb(0x656d76);
        Self {
            id: ThemeId::Light,
            bg: from_rgb(0xe4e6e9),
            panel: from_rgb(0xeff1f3),
            panel2: from_rgb(0xe2e5e9),
            line: from_rgb(0xcfd4da),
            text: from_rgb(0x24292f),
            muted,
            blue: from_rgb(0x0969da),
            green: from_rgb(0x1a7f37),
            yellow: from_rgb(0x9a6700),
            red: from_rgb(0xcf222e),
            code: from_rgb(0xf6f8fa),
            accent_soft: from_rgb(0xddf0ff),
            titlebar: from_rgb(0xeaecef),
            menu_hover: from_rgb(0xe0e4e8),
            scrollbar: from_rgb(0xe4e6e9),
            danger: from_rgb(0xffebe9),
            danger_border: from_rgb(0xcf222e),
            selection: from_rgb(0xb6d8f2),
            line_hl: from_rgb(0xeaeef2),
            match_bracket: from_rgb(0xa8b0b8),
            btn_bg: from_rgb(0xf0f2f4),
            btn_border: from_rgb(0xc0c6cc),
            btn_primary_fg: from_rgb(0xffffff),
            gutter: with_alpha(muted, 0.38),
            gutter_active: with_alpha(muted, 0.72),
            syn_text: from_rgb(0x24292f),
            syn_keyword: from_rgb(0x8250df),
            syn_builtin: from_rgb(0x0550ae),
            syn_module: from_rgb(0x116329),
            syn_string: from_rgb(0xa40e26),
            syn_number: from_rgb(0x0550ae),
            syn_comment: from_rgb(0x707780),
            syn_operator: from_rgb(0x24292f),
            syn_pin: from_rgb(0x9a6700),
        }
    }
}
