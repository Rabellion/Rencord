//! Semantic tokens. UI code reads these names and does not carry raw hex.

use crate::color::{best_ink, contrast_ratio, ensure_contrast, Color};
use crate::preset::build_preset;
use serde::{Deserialize, Serialize};

macro_rules! color_bag {
    ($($field:ident => $key:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Colors {
            $(pub $field: Color,)*
        }

        impl Colors {
            pub fn get(self, key: &str) -> Option<Color> {
                match key {
                    $($key => Some(self.$field),)*
                    _ => None,
                }
            }

            pub fn set(&mut self, key: &str, color: Color) -> bool {
                match key {
                    $($key => {
                        self.$field = color;
                        true
                    })*
                    _ => false,
                }
            }

            pub fn keys() -> &'static [&'static str] {
                &[$($key),*]
            }

            pub fn lerp(self, other: Self, t: f32) -> Self {
                Self {
                    $($field: self.$field.lerp(other.$field, t),)*
                }
            }

            pub fn map(mut self, mut f: impl FnMut(Color) -> Color) -> Self {
                $(self.$field = f(self.$field);)*
                self
            }
        }
    };
}

color_bag! {
    bg_base => "bg.base",
    bg_elevated => "bg.elevated",
    bg_sunken => "bg.sunken",
    bg_rail => "bg.rail",
    bg_sidebar => "bg.sidebar",
    bg_chat => "bg.chat",
    bg_members => "bg.members",
    bg_input => "bg.input",
    bg_hover => "bg.hover",
    bg_selected => "bg.selected",
    bg_mention => "bg.mention",
    bg_overlay => "bg.overlay",
    bg_add => "bg.add",
    bg_voice => "bg.voice",
    bg_home => "bg.home",
    bg_home_active => "bg.home.active",
    bg_badge => "bg.badge",
    bg_reaction => "bg.reaction",
    bg_reaction_mine => "bg.reaction.mine",
    bg_hangup => "bg.hangup",
    bg_stage => "bg.stage",
    bg_qr => "bg.qr",
    text_primary => "text.primary",
    text_muted => "text.muted",
    text_faint => "text.faint",
    text_link => "text.link",
    text_secondary => "text.secondary",
    text_mention => "text.mention",
    text_danger => "text.danger",
    text_positive => "text.positive",
    text_home => "text.home",
    text_home_active => "text.home.active",
    text_add => "text.add",
    text_badge => "text.badge",
    text_voice => "text.voice",
    text_hangup => "text.hangup",
    border_hairline => "border.hairline",
    border_strong => "border.strong",
    border_accent => "border.accent",
    border_home => "border.home",
    border_home_active => "border.home.active",
    border_add => "border.add",
    border_voice => "border.voice",
    border_hover => "border.hover",
    accent => "accent",
    accent_fg => "accent.fg",
    accent_on => "accent.on",
    danger => "danger",
    positive => "positive",
    status_online => "status.online",
    status_idle => "status.idle",
    status_dnd => "status.dnd",
    status_offline => "status.offline",
    syntax_keyword => "syntax.keyword",
    syntax_number => "syntax.number",
    pill => "pill",
    shadow => "shadow",
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    Light,
    Dark,
}

impl ColorMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chrome {
    Ink,
    Classic,
}

impl Chrome {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ink" => Some(Self::Ink),
            "classic" => Some(Self::Classic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ink => "ink",
            Self::Classic => "classic",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFace {
    Inter,
    Figtree,
    JetBrainsMono,
}

impl FontFace {
    pub fn family(self) -> &'static str {
        match self {
            Self::Inter => "Inter",
            Self::Figtree => "Figtree",
            Self::JetBrainsMono => "JetBrains Mono",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "figtree" => Self::Figtree,
            "jetbrains mono" | "jetbrainsmono" | "geist mono" | "geistmono" => Self::JetBrainsMono,
            _ => Self::Inter,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Inter => "Inter",
            Self::Figtree => "Figtree",
            Self::JetBrainsMono => "JetBrains Mono",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Inter => Self::Figtree,
            Self::Figtree => Self::JetBrainsMono,
            Self::JetBrainsMono => Self::Inter,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub font_ui: FontFace,
    pub font_mono: FontFace,
    pub font_meta: FontFace,
    pub font_scale: f32,
    pub radius_scale: f32,
    pub message_spacing: f32,
    pub saturation: f32,
    pub gap: f32,
    pub rail_width: f32,
    pub rail_pad: f32,
    pub tile_size: f32,
    pub radius_tile: f32,
    pub sidebar_width: f32,
    pub member_width: f32,
    pub header_h: f32,
    pub header_px: f32,
    pub title_px: f32,
    pub hash_icon: f32,
    pub composer_h: f32,
    pub member_row: f32,
    pub category_pad: f32,
    pub category_px: f32,
    pub channel_label: f32,
    pub channel_icon: f32,
    pub dm_avatar: f32,
    pub voice_mx: f32,
    pub voice_mt: f32,
    pub voice_px: f32,
    pub voice_radius: f32,
    pub dock_pl: f32,
    pub dock_pr: f32,
    pub dock_name: f32,
    pub avatar: f32,
    pub message_py: f32,
    pub message_gap: f32,
    pub radius_panel: f32,
    pub radius_code: f32,
    pub radius_media: f32,
    pub radius_control: f32,
    pub radius_avatar: f32,
    pub code_size: f32,
    pub code_line: f32,
    pub narrow_at: f32,
    pub body_size: f32,
    pub body_line: f32,
    pub border_width: f32,
    pub shadow_y: f32,
    pub shadow_blur: f32,
    pub shadow_spread: f32,
    pub show_top_rule: bool,
    pub ink_chrome: bool,
}

impl Metrics {
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let n = |a: f32, b: f32| a + (b - a) * t;
        let face = |a: FontFace, b: FontFace| if t >= 0.5 { b } else { a };
        let flag = |a: bool, b: bool| if t >= 0.5 { b } else { a };
        Self {
            font_ui: face(self.font_ui, other.font_ui),
            font_mono: face(self.font_mono, other.font_mono),
            font_meta: face(self.font_meta, other.font_meta),
            font_scale: n(self.font_scale, other.font_scale),
            radius_scale: n(self.radius_scale, other.radius_scale),
            message_spacing: n(self.message_spacing, other.message_spacing),
            saturation: n(self.saturation, other.saturation),
            gap: n(self.gap, other.gap),
            rail_width: n(self.rail_width, other.rail_width),
            rail_pad: n(self.rail_pad, other.rail_pad),
            tile_size: n(self.tile_size, other.tile_size),
            radius_tile: n(self.radius_tile, other.radius_tile),
            sidebar_width: n(self.sidebar_width, other.sidebar_width),
            member_width: n(self.member_width, other.member_width),
            header_h: n(self.header_h, other.header_h),
            header_px: n(self.header_px, other.header_px),
            title_px: n(self.title_px, other.title_px),
            hash_icon: n(self.hash_icon, other.hash_icon),
            composer_h: n(self.composer_h, other.composer_h),
            member_row: n(self.member_row, other.member_row),
            category_pad: n(self.category_pad, other.category_pad),
            category_px: n(self.category_px, other.category_px),
            channel_label: n(self.channel_label, other.channel_label),
            channel_icon: n(self.channel_icon, other.channel_icon),
            dm_avatar: n(self.dm_avatar, other.dm_avatar),
            voice_mx: n(self.voice_mx, other.voice_mx),
            voice_mt: n(self.voice_mt, other.voice_mt),
            voice_px: n(self.voice_px, other.voice_px),
            voice_radius: n(self.voice_radius, other.voice_radius),
            dock_pl: n(self.dock_pl, other.dock_pl),
            dock_pr: n(self.dock_pr, other.dock_pr),
            dock_name: n(self.dock_name, other.dock_name),
            avatar: n(self.avatar, other.avatar),
            message_py: n(self.message_py, other.message_py),
            message_gap: n(self.message_gap, other.message_gap),
            radius_panel: n(self.radius_panel, other.radius_panel),
            radius_code: n(self.radius_code, other.radius_code),
            radius_media: n(self.radius_media, other.radius_media),
            radius_control: n(self.radius_control, other.radius_control),
            radius_avatar: n(self.radius_avatar, other.radius_avatar),
            code_size: n(self.code_size, other.code_size),
            code_line: n(self.code_line, other.code_line),
            narrow_at: n(self.narrow_at, other.narrow_at),
            body_size: n(self.body_size, other.body_size),
            body_line: n(self.body_line, other.body_line),
            border_width: n(self.border_width, other.border_width),
            shadow_y: n(self.shadow_y, other.shadow_y),
            shadow_blur: n(self.shadow_blur, other.shadow_blur),
            shadow_spread: n(self.shadow_spread, other.shadow_spread),
            show_top_rule: flag(self.show_top_rule, other.show_top_rule),
            ink_chrome: flag(self.ink_chrome, other.ink_chrome),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Background {
    pub kind: BackgroundKind,
    pub grain: f32,
    pub image_path: String,
    pub image_blur: f32,
    pub image_dim: f32,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            kind: BackgroundKind::Solid,
            grain: 0.0,
            image_path: String::new(),
            image_blur: 0.0,
            image_dim: 0.0,
        }
    }
}

impl Background {
    pub fn grain(self) -> f32 {
        self.grain.clamp(0.0, 0.03)
    }

    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let kind = if t >= 0.5 {
            other.kind.clone()
        } else {
            self.kind.clone()
        };
        Self {
            kind,
            grain: self.grain + (other.grain - self.grain) * t,
            image_path: if t >= 0.5 {
                other.image_path.clone()
            } else {
                self.image_path.clone()
            },
            image_blur: self.image_blur + (other.image_blur - self.image_blur) * t,
            image_dim: self.image_dim + (other.image_dim - self.image_dim) * t,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundKind {
    Solid,
    Gradient { angle: f32, stops: Vec<Color> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub mode: ColorMode,
    pub chrome: Chrome,
    pub preset: Option<crate::PresetId>,
    pub colors: Colors,
    pub metrics: Metrics,
    pub background: Background,
    pub base: Option<Color>,
    pub accent_seed: Option<Color>,
    pub derived: bool,
}

impl Theme {
    pub fn preset(id: crate::PresetId) -> Self {
        build_preset(id)
    }

    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let chosen = if t >= 0.5 { other } else { self };
        Self {
            id: chosen.id.clone(),
            name: chosen.name.clone(),
            mode: chosen.mode,
            chrome: chosen.chrome,
            preset: chosen.preset,
            colors: self.colors.lerp(other.colors, t),
            metrics: self.metrics.lerp(other.metrics, t),
            background: self.background.lerp(&other.background, t),
            base: chosen.base,
            accent_seed: chosen.accent_seed,
            derived: chosen.derived,
        }
    }

    pub fn signature(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut feed = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100_0000_01b3);
            }
        };
        feed(self.id.as_bytes());
        feed(self.mode.as_str().as_bytes());
        feed(self.chrome.as_str().as_bytes());
        for key in Colors::keys() {
            if let Some(color) = self.colors.get(key) {
                feed(&color.to_rgba_u32().to_le_bytes());
            }
        }
        feed(&self.background.grain.to_le_bytes());
        feed(&self.metrics.font_scale.to_le_bytes());
        feed(&self.metrics.radius_scale.to_le_bytes());
        feed(&self.metrics.message_spacing.to_le_bytes());
        feed(&self.metrics.saturation.to_le_bytes());
        feed(self.background.image_path.as_bytes());
        hash
    }

    pub fn high_contrast(mut self) -> Self {
        let chat = self.colors.bg_chat;
        let input = self.colors.bg_input;
        let text = self.colors.text_primary;
        self.colors.text_muted = ensure_contrast(self.colors.text_muted.mix(text, 0.45), chat, 7.0);
        self.colors.text_faint =
            ensure_contrast(self.colors.text_faint.mix(text, 0.35), input, 4.5);
        self.colors.text_link = ensure_contrast(self.colors.text_link, chat, 7.0);
        self.colors.text_secondary =
            ensure_contrast(self.colors.text_secondary.mix(text, 0.3), chat, 4.5);
        self.colors.border_hairline = self.colors.border_hairline.mix(text, 0.38);
        self.colors.border_strong = self.colors.border_strong.mix(text, 0.28);
        self.colors.accent_fg = best_ink(self.colors.accent);
        self.colors.accent_fg = ensure_contrast(self.colors.accent_fg, self.colors.accent, 4.5);
        self.colors.accent_on = crate::preset::on_accent(self.colors.accent, self.colors.accent_fg);
        for field in [
            &mut self.colors.status_online,
            &mut self.colors.status_idle,
            &mut self.colors.status_dnd,
            &mut self.colors.status_offline,
        ] {
            *field = ensure_contrast(*field, chat, 3.0);
        }
        self.metrics.border_width = (self.metrics.border_width + 0.5).min(2.0);
        self
    }

    pub fn colorblind_status(mut self) -> Self {
        let chat = self.colors.bg_chat;
        // Okabe–Ito hues, then lifted until the dot clears the surface.
        self.colors.status_online = ensure_contrast(Color::rgb(0x00_72_B2), chat, 3.0);
        self.colors.status_idle = ensure_contrast(Color::rgb(0xE6_9F_00), chat, 3.0);
        self.colors.status_dnd = ensure_contrast(Color::rgb(0xCC_79_A7), chat, 3.0);
        self.colors.status_offline = ensure_contrast(Color::rgb(0x6E_6E_6E), chat, 3.0);
        self.colors.positive = self.colors.status_online;
        self.colors.text_positive = self.colors.status_online;
        self
    }

    pub fn desaturate(mut self, saturation: f32) -> Self {
        let scale = saturation.clamp(0.0, 1.0);
        if scale >= 0.999 {
            return self;
        }
        let chat = self.colors.bg_chat;
        let input = self.colors.bg_input;
        self.colors = self.colors.map(|color| color.with_chroma_scale(scale));
        self.colors.text_primary = ensure_contrast(self.colors.text_primary, chat, 4.5);
        self.colors.text_muted = ensure_contrast(self.colors.text_muted, chat, 4.5);
        self.colors.text_muted = ensure_contrast(self.colors.text_muted, input, 4.5);
        self.colors.text_faint = ensure_contrast(self.colors.text_faint, input, 4.5);
        self.colors.text_faint = ensure_contrast(self.colors.text_faint, chat, 4.5);
        self.colors.text_link = ensure_contrast(self.colors.text_link, chat, 4.5);
        self.colors.text_secondary = ensure_contrast(self.colors.text_secondary, chat, 4.5);
        self.colors.text_mention =
            ensure_contrast(self.colors.text_mention, self.colors.bg_mention, 4.5);
        self.colors.accent_fg =
            ensure_contrast(best_ink(self.colors.accent), self.colors.accent, 4.5);
        self.colors.accent_on = crate::preset::on_accent(self.colors.accent, self.colors.accent_fg);
        self.metrics.saturation = scale;
        self
    }
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContrastWarning {
    pub pair: &'static str,
    pub ratio: f32,
    pub minimum: f32,
}

pub fn contrast_warnings(theme: &Theme) -> Vec<ContrastWarning> {
    let c = &theme.colors;
    let mut out = Vec::new();
    let mut check = |pair: &'static str, fg: Color, bg: Color, minimum: f32| {
        let ratio = contrast_ratio(fg, bg);
        if ratio + 0.02 < minimum {
            out.push(ContrastWarning {
                pair,
                ratio,
                minimum,
            });
        }
    };
    check("text on chat", c.text_primary, c.bg_chat, 4.5);
    check("text on sidebar", c.text_primary, c.bg_sidebar, 4.5);
    check("text on input", c.text_primary, c.bg_input, 4.5);
    check("text on elevated", c.text_primary, c.bg_elevated, 4.5);
    check("muted on chat", c.text_muted, c.bg_chat, 4.5);
    check("muted on input", c.text_muted, c.bg_input, 4.5);
    check("faint on input", c.text_faint, c.bg_input, 4.5);
    check("link on chat", c.text_link, c.bg_chat, 4.5);
    check("accent text", c.accent_fg, c.accent, 4.5);
    check("badge text", c.text_badge, c.bg_badge, 4.5);
    check("mention", c.text_mention, c.bg_mention, 4.5);
    check("secondary on chat", c.text_secondary, c.bg_chat, 4.5);
    check("online", c.status_online, c.bg_chat, 3.0);
    check("idle", c.status_idle, c.bg_chat, 3.0);
    check("dnd", c.status_dnd, c.bg_chat, 3.0);
    check("offline", c.status_offline, c.bg_chat, 3.0);
    out
}

pub fn warnings_text(theme: &Theme) -> String {
    let warnings = contrast_warnings(theme);
    if warnings.is_empty() {
        return "Contrast holds at WCAG AA.".into();
    }
    warnings
        .iter()
        .map(|warning| {
            format!(
                "{} is {:.2}:1 (needs {:.1}:1)",
                warning.pair, warning.ratio, warning.minimum
            )
        })
        .collect::<Vec<_>>()
        .join(" · ")
}
