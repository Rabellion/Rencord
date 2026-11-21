//! Built-in presets. Onyx Ink stays the default. Discord names are matched
//! where the public client uses them; the palettes are original values.

use crate::color::{contrast_ratio, ensure_contrast, Color};
use crate::theme::{
    Background, BackgroundKind, Chrome, ColorMode, Colors, FontFace, Metrics, Theme,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetId {
    OnyxInk,
    Dark,
    Ash,
    Onyx,
    Amoled,
    Light,
    MidnightBlurple,
    Nord,
    SoftContrast,
    Crisp,
}

impl PresetId {
    pub const ALL: [Self; 10] = [
        Self::OnyxInk,
        Self::Dark,
        Self::Ash,
        Self::Onyx,
        Self::Amoled,
        Self::Light,
        Self::MidnightBlurple,
        Self::Nord,
        Self::SoftContrast,
        Self::Crisp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::OnyxInk => "Onyx Ink",
            Self::Dark => "Dark",
            Self::Ash => "Ash",
            Self::Onyx => "Onyx",
            Self::Amoled => "AMOLED",
            Self::Light => "Light",
            Self::MidnightBlurple => "Midnight Blurple",
            Self::Nord => "Nord",
            Self::SoftContrast => "Soft Contrast",
            Self::Crisp => "Crisp",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::OnyxInk => "onyx-ink",
            Self::Dark => "dark",
            Self::Ash => "ash",
            Self::Onyx => "onyx",
            Self::Amoled => "amoled",
            Self::Light => "light",
            Self::MidnightBlurple => "midnight-blurple",
            Self::Nord => "nord",
            Self::SoftContrast => "soft-contrast",
            Self::Crisp => "crisp",
        }
    }

    pub fn cycle(self) -> Self {
        let index = Self::ALL.iter().position(|item| *item == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "onyx-ink" | "ink" | "default" => Some(Self::OnyxInk),
            "dark" => Some(Self::Dark),
            "ash" => Some(Self::Ash),
            "onyx" => Some(Self::Onyx),
            "amoled" | "amoled-black" => Some(Self::Amoled),
            "light" => Some(Self::Light),
            "midnight" | "midnight-blurple" => Some(Self::MidnightBlurple),
            "nord" => Some(Self::Nord),
            "soft" | "soft-contrast" | "contrast" => Some(Self::SoftContrast),
            "crisp" | "c" => Some(Self::Crisp),
            _ => None,
        }
    }
}

pub fn build_preset(id: PresetId) -> Theme {
    match id {
        PresetId::OnyxInk => onyx_ink(),
        PresetId::Dark => dark(),
        PresetId::Ash => ash(),
        PresetId::Onyx => onyx(),
        PresetId::Amoled => amoled(),
        PresetId::Light => light(),
        PresetId::MidnightBlurple => midnight(),
        PresetId::Nord => nord(),
        PresetId::SoftContrast => soft(),
        PresetId::Crisp => crisp(),
    }
}

fn onyx_ink() -> Theme {
    theme(
        PresetId::OnyxInk,
        ColorMode::Dark,
        Chrome::Ink,
        ink_metrics(false),
        colors(InkSpec {
            bg_base: 0x000000,
            bg_elevated: 0x0a0a14,
            bg_sunken: 0x000000,
            bg_rail: 0x000000,
            bg_sidebar: 0x000000,
            bg_chat: 0x000000,
            bg_input: 0x000000,
            bg_hover: 0x0c1024,
            bg_selected: 0x080b1a,
            text: 0xedeef7,
            muted: 0x9aa0c0,
            faint: 0x7a80a0,
            accent: 0x4f6bff,
            accent_fg: 0x000000,
            link: 0xa8b8ff,
            danger: 0xe05550,
            text_danger: 0xd46a5c,
            line: 0x15152a,
            strong: 0x1c1c38,
            mention_bg: 0x101428,
            mention_fg: 0xa8b8ff,
            online: 0x5abb72,
            idle: 0xd4c05a,
            dnd: 0xf23f43,
            offline: 0x80848e,
            secondary: 0xc5c8de,
            syntax_number: 0xc4a6f0,
            pill: 0x4f6bff,
            shadow: 0x00000000,
        }),
        Background::default(),
    )
}

fn crisp() -> Theme {
    theme(
        PresetId::Crisp,
        ColorMode::Dark,
        Chrome::Classic,
        classic_metrics(false),
        colors(InkSpec {
            bg_base: 0x16181d,
            bg_elevated: 0x23272f,
            bg_sunken: 0x16181d,
            bg_rail: 0x1c1f26,
            bg_sidebar: 0x1c1f26,
            bg_chat: 0x1c1f26,
            bg_input: 0x23272f,
            bg_hover: 0x22262e,
            bg_selected: 0x2a2f38,
            text: 0xedf0f8,
            muted: 0x8592ad,
            faint: 0x9aabc4,
            accent: 0x5865f2,
            accent_fg: 0xffffff,
            link: 0x00a8fc,
            danger: 0xf23f43,
            text_danger: 0xf23f43,
            line: 0x2e3440,
            strong: 0x2e3440,
            mention_bg: 0x22263a,
            mention_fg: 0xffffff,
            online: 0x23a559,
            idle: 0xf0b232,
            dnd: 0xf23f43,
            offline: 0x80848e,
            secondary: 0x8592ad,
            syntax_number: 0xedf0f8,
            pill: 0xffffff,
            shadow: 0x00000000,
        }),
        Background::default(),
    )
}

fn dark() -> Theme {
    theme(
        PresetId::Dark,
        ColorMode::Dark,
        Chrome::Classic,
        classic_metrics(false),
        colors(InkSpec {
            bg_base: 0x1e1f22,
            bg_elevated: 0x232428,
            bg_sunken: 0x2b2d31,
            bg_rail: 0x1e1f22,
            bg_sidebar: 0x2b2d31,
            bg_chat: 0x313338,
            bg_input: 0x383a40,
            bg_hover: 0x3e424a,
            bg_selected: 0x404249,
            text: 0xf2f3f5,
            muted: 0xb5bac1,
            faint: 0xb7bec8,
            accent: 0x5865f2,
            accent_fg: 0xffffff,
            link: 0x00a8fc,
            danger: 0xf23f43,
            text_danger: 0xf23f43,
            line: 0x3f4248,
            strong: 0x4e5158,
            mention_bg: 0x3c4270,
            mention_fg: 0xffffff,
            online: 0x23a559,
            idle: 0xf0b232,
            dnd: 0xf23f43,
            offline: 0x80848e,
            secondary: 0xb5bac1,
            syntax_number: 0xf2f3f5,
            pill: 0xffffff,
            shadow: 0x00000000,
        }),
        Background::default(),
    )
}

fn ash() -> Theme {
    let chat = hsl(231.429, 6.542, 20.98);
    let sidebar = hsl(230.0, 6.383, 18.431);
    let input = hsl(232.5, 6.557, 23.922);
    let rail = hsl(230.0, 7.0, 14.5);
    theme(
        PresetId::Ash,
        ColorMode::Dark,
        Chrome::Classic,
        classic_metrics(false),
        colors_from(
            chat,
            sidebar,
            rail,
            input,
            Color::rgb(0xdaddda),
            Color::rgb(0xc5c8ce),
            Color::rgb(0xaeb3bd),
            Color::rgb(0x5865f2),
            Color::rgb(0xffffff),
            hsl(213.934, 85.915, 72.157),
            false,
        ),
        Background::default(),
    )
}

fn onyx() -> Theme {
    let chat = hsl(220.0, 7.317, 8.039);
    let members = hsl(240.0, 12.5, 3.137);
    theme(
        PresetId::Onyx,
        ColorMode::Dark,
        Chrome::Classic,
        classic_metrics(false),
        colors_from(
            chat,
            Color::rgb(0x000000),
            Color::rgb(0x000000),
            chat,
            Color::rgb(0xdaddda),
            Color::rgb(0xb7bdb8),
            Color::rgb(0x9aa39c),
            Color::rgb(0x5865f2),
            Color::rgb(0xffffff),
            Color::rgb(0x8eb6ff),
            false,
        )
        .with_members(members),
        Background::default(),
    )
}

fn amoled() -> Theme {
    theme(
        PresetId::Amoled,
        ColorMode::Dark,
        Chrome::Classic,
        classic_metrics(false),
        colors(InkSpec {
            bg_base: 0x000000,
            bg_elevated: 0x0a0a0a,
            bg_sunken: 0x000000,
            bg_rail: 0x000000,
            bg_sidebar: 0x000000,
            bg_chat: 0x000000,
            bg_input: 0x000000,
            bg_hover: 0x141414,
            bg_selected: 0x1a1a1a,
            text: 0xf2f3f5,
            muted: 0xb5bac1,
            faint: 0x9aa0a6,
            accent: 0x5865f2,
            accent_fg: 0xffffff,
            link: 0x8eb6ff,
            danger: 0xf23f43,
            text_danger: 0xf23f43,
            line: 0x1a1a1a,
            strong: 0x262626,
            mention_bg: 0x16162e,
            mention_fg: 0xc9d4ff,
            online: 0x23a559,
            idle: 0xf0b232,
            dnd: 0xf23f43,
            offline: 0x8b8f98,
            secondary: 0xb5bac1,
            syntax_number: 0xf2f3f5,
            pill: 0xffffff,
            shadow: 0x00000000,
        }),
        Background::default(),
    )
}

fn light() -> Theme {
    theme(
        PresetId::Light,
        ColorMode::Light,
        Chrome::Classic,
        classic_metrics(false),
        colors(InkSpec {
            bg_base: 0xe6e9f0,
            bg_elevated: 0xffffff,
            bg_sunken: 0xeef1f6,
            bg_rail: 0xf7f8fb,
            bg_sidebar: 0xf7f8fb,
            bg_chat: 0xf7f8fb,
            bg_input: 0xffffff,
            bg_hover: 0xe8ebf2,
            bg_selected: 0xdfe3ec,
            text: 0x1a1d26,
            muted: 0x4e5870,
            faint: 0x5c657c,
            accent: 0x5865f2,
            accent_fg: 0xffffff,
            link: 0x0068e0,
            danger: 0xda373c,
            text_danger: 0xda373c,
            line: 0xd5dae6,
            strong: 0xc5cad6,
            mention_bg: 0xe4e7ff,
            mention_fg: 0x1d4ed8,
            online: 0x14804a,
            idle: 0x8a5a00,
            dnd: 0xd1242f,
            offline: 0x5c6370,
            secondary: 0x4e5870,
            syntax_number: 0x1a1d26,
            pill: 0x060607,
            shadow: 0x1a1d2624,
        }),
        Background::default(),
    )
}

fn midnight() -> Theme {
    let background = Background {
        kind: BackgroundKind::Gradient {
            angle: 160.0,
            stops: vec![
                Color::rgb(0x0c1024),
                Color::rgb(0x1a1040),
                Color::rgb(0x12183a),
            ],
        },
        grain: 0.012,
        ..Background::default()
    };
    theme(
        PresetId::MidnightBlurple,
        ColorMode::Dark,
        Chrome::Classic,
        classic_metrics(false),
        colors(InkSpec {
            bg_base: 0x0c1024,
            bg_elevated: 0x1c2448,
            bg_sunken: 0x10162e,
            bg_rail: 0x0c1024,
            bg_sidebar: 0x121735,
            bg_chat: 0x141a3a,
            bg_input: 0x1a2144,
            bg_hover: 0x222a52,
            bg_selected: 0x2a3464,
            text: 0xe8eaff,
            muted: 0xb7bde4,
            faint: 0x9aa3d0,
            accent: 0x8b9bff,
            accent_fg: 0x0c1024,
            link: 0xc9d2ff,
            danger: 0xff6b6b,
            text_danger: 0xff8f8f,
            line: 0x2e386c,
            strong: 0x3a4680,
            mention_bg: 0x2a3168,
            mention_fg: 0xe4e9ff,
            online: 0x3dd68c,
            idle: 0xf0c14a,
            dnd: 0xff6b8a,
            offline: 0x8e96b8,
            secondary: 0xb7bde4,
            syntax_number: 0xd2c4ff,
            pill: 0xffffff,
            shadow: 0x00000000,
        }),
        background,
    )
}

fn nord() -> Theme {
    theme(
        PresetId::Nord,
        ColorMode::Dark,
        Chrome::Classic,
        classic_metrics(false),
        colors(InkSpec {
            bg_base: 0x2e3440,
            bg_elevated: 0x434c5e,
            bg_sunken: 0x2e3440,
            bg_rail: 0x2e3440,
            bg_sidebar: 0x3b4252,
            bg_chat: 0x2e3440,
            bg_input: 0x3b4252,
            bg_hover: 0x434c5e,
            bg_selected: 0x4c566a,
            text: 0xeceff4,
            muted: 0xd8dee9,
            faint: 0xc2cad6,
            accent: 0x88c0d0,
            accent_fg: 0x1e2430,
            link: 0x8fbcbb,
            danger: 0xbf616a,
            text_danger: 0xbf616a,
            line: 0x434c5e,
            strong: 0x4c566a,
            mention_bg: 0x3b4252,
            mention_fg: 0xeceff4,
            online: 0xa3be8c,
            idle: 0xebcb8b,
            dnd: 0xbf616a,
            offline: 0x9aa3b2,
            secondary: 0xd8dee9,
            syntax_number: 0xb48ead,
            pill: 0xeceff4,
            shadow: 0x00000000,
        }),
        Background::default(),
    )
}

fn soft() -> Theme {
    theme(
        PresetId::SoftContrast,
        ColorMode::Light,
        Chrome::Classic,
        classic_metrics(false),
        colors(InkSpec {
            bg_base: 0xe7e1d6,
            bg_elevated: 0xffffff,
            bg_sunken: 0xefeae2,
            bg_rail: 0xefeae1,
            bg_sidebar: 0xf6f1e8,
            bg_chat: 0xfbf8f3,
            bg_input: 0xffffff,
            bg_hover: 0xe7e0d4,
            bg_selected: 0xddd4c6,
            text: 0x1c1612,
            muted: 0x4a433c,
            faint: 0x5c554c,
            accent: 0x0e5e6b,
            accent_fg: 0xffffff,
            link: 0x0b4f86,
            danger: 0xb4232c,
            text_danger: 0xb4232c,
            line: 0xd3cbbe,
            strong: 0xc4b9a8,
            mention_bg: 0xd9e7ea,
            mention_fg: 0x0b4f6a,
            online: 0x0e7a45,
            idle: 0x8a5a00,
            dnd: 0xb4232c,
            offline: 0x5c564e,
            secondary: 0x4a433c,
            syntax_number: 0x1c1612,
            pill: 0x1c1612,
            shadow: 0x1c161228,
        }),
        Background::default(),
    )
}

struct InkSpec {
    bg_base: u32,
    bg_elevated: u32,
    bg_sunken: u32,
    bg_rail: u32,
    bg_sidebar: u32,
    bg_chat: u32,
    bg_input: u32,
    bg_hover: u32,
    bg_selected: u32,
    text: u32,
    muted: u32,
    faint: u32,
    accent: u32,
    accent_fg: u32,
    link: u32,
    danger: u32,
    text_danger: u32,
    line: u32,
    strong: u32,
    mention_bg: u32,
    mention_fg: u32,
    online: u32,
    idle: u32,
    dnd: u32,
    offline: u32,
    secondary: u32,
    syntax_number: u32,
    pill: u32,
    shadow: u32,
}

/// Label on a filled accent control. Discord uses white on blurple; Onyx Ink
/// keeps black for `accent.fg` (banners and badges) and white here.
pub(crate) fn on_accent(accent: Color, fallback: Color) -> Color {
    let white = Color::rgb(0xffffff);
    if contrast_ratio(white, accent) >= 3.0 {
        white
    } else {
        fallback
    }
}

fn colors(spec: InkSpec) -> Colors {
    let c = |hex| Color::rgb(hex);
    let accent = c(spec.accent);
    let text = c(spec.text);
    let line = c(spec.line);
    let strong = c(spec.strong);
    let danger = c(spec.danger);
    let online = c(spec.online);
    let onyx_ink = spec.accent == 0x4f6bff && spec.bg_chat == 0x000000 && spec.link == 0xa8b8ff;
    let lightish = spec.text == 0x1a1d26 || spec.text == 0x1c1612;
    let (home_bg, home_active, home_glyph, home_glyph_active, home_border, home_border_active) =
        if onyx_ink {
            (c(0x000000), c(0x000000), accent, accent, line, line)
        } else if lightish {
            (
                c(spec.bg_rail),
                accent,
                text,
                c(spec.accent_fg),
                line,
                accent,
            )
        } else {
            (c(0x2a2e38), accent, text, c(0xffffff), c(0x2a2e38), accent)
        };
    let (add_bg, add_border, add_glyph) = if onyx_ink {
        (c(0x000000), strong, c(spec.muted))
    } else if lightish {
        (c(0xffffff), c(0x14804a), c(0x14804a))
    } else {
        (c(spec.bg_rail), online, online)
    };
    let (badge_bg, badge_fg) = if onyx_ink {
        (accent, c(spec.accent_fg))
    } else {
        (c(0xb91c1c), c(0xffffff))
    };
    let (voice_bg, voice_border, _voice_label, voice_meta) = if onyx_ink {
        (c(spec.bg_chat), line, c(spec.link), c(spec.secondary))
    } else {
        (
            c(spec.bg_elevated),
            c(spec.bg_elevated),
            online,
            c(spec.muted),
        )
    };
    Colors {
        bg_base: c(spec.bg_base),
        bg_elevated: c(spec.bg_elevated),
        bg_sunken: c(spec.bg_sunken),
        bg_rail: c(spec.bg_rail),
        bg_sidebar: c(spec.bg_sidebar),
        bg_chat: c(spec.bg_chat),
        bg_members: c(spec.bg_sidebar),
        bg_input: c(spec.bg_input),
        bg_hover: c(spec.bg_hover),
        bg_selected: c(spec.bg_selected),
        bg_mention: c(spec.mention_bg),
        bg_overlay: Color::rgba(0x0000_0099),
        bg_add: add_bg,
        bg_voice: voice_bg,
        bg_home: home_bg,
        bg_home_active: home_active,
        bg_badge: badge_bg,
        bg_reaction: c(spec.bg_elevated),
        bg_reaction_mine: c(spec.mention_bg),
        bg_hangup: danger.with_alpha(0x29),
        bg_stage: Color::rgb(0x000000),
        bg_qr: Color::rgb(0xffffff),
        text_primary: text,
        text_muted: c(spec.muted),
        text_faint: c(spec.faint),
        text_link: c(spec.link),
        text_secondary: c(spec.secondary),
        text_mention: c(spec.mention_fg),
        text_danger: c(spec.text_danger),
        text_positive: online,
        text_home: home_glyph,
        text_home_active: home_glyph_active,
        text_add: add_glyph,
        text_badge: badge_fg,
        text_voice: voice_meta,
        text_hangup: danger,
        border_hairline: line,
        border_strong: strong,
        border_accent: accent,
        border_home: home_border,
        border_home_active: home_border_active,
        border_add: add_border,
        border_voice: voice_border,
        border_hover: if onyx_ink { strong } else { line },
        accent,
        accent_fg: c(spec.accent_fg),
        accent_on: on_accent(accent, c(spec.accent_fg)),
        danger,
        positive: online,
        status_online: online,
        status_idle: c(spec.idle),
        status_dnd: c(spec.dnd),
        status_offline: c(spec.offline),
        syntax_keyword: c(spec.link),
        syntax_number: c(spec.syntax_number),
        pill: c(spec.pill),
        shadow: Color::rgba(spec.shadow),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn colors_from_public(
    chat: Color,
    sidebar: Color,
    rail: Color,
    input: Color,
    text: Color,
    muted: Color,
    faint: Color,
    accent: Color,
    accent_fg: Color,
    link: Color,
    light: bool,
) -> Colors {
    colors_from(
        chat, sidebar, rail, input, text, muted, faint, accent, accent_fg, link, light,
    )
}

#[allow(clippy::too_many_arguments)]
fn colors_from(
    chat: Color,
    sidebar: Color,
    rail: Color,
    input: Color,
    text: Color,
    muted: Color,
    faint: Color,
    accent: Color,
    accent_fg: Color,
    link: Color,
    light: bool,
) -> Colors {
    let hover = chat.shift_lightness(if light { -0.03 } else { 0.04 });
    let selected = chat.shift_lightness(if light { -0.05 } else { 0.07 });
    let elevated = chat.shift_lightness(if light { 0.02 } else { 0.05 });
    let line = chat.mix(text, if light { 0.16 } else { 0.18 });
    let mention_bg = chat.mix(accent, 0.22);
    let mention_fg = ensure_contrast(
        if light { accent } else { Color::rgb(0xFFFFFF) },
        mention_bg,
        4.5,
    );
    let online = if light {
        Color::rgb(0x14804a)
    } else {
        Color::rgb(0x23a559)
    };
    colors(InkSpec {
        bg_base: rail.to_u32(),
        bg_elevated: elevated.to_u32(),
        bg_sunken: sidebar.to_u32(),
        bg_rail: rail.to_u32(),
        bg_sidebar: sidebar.to_u32(),
        bg_chat: chat.to_u32(),
        bg_input: input.to_u32(),
        bg_hover: hover.to_u32(),
        bg_selected: selected.to_u32(),
        text: text.to_u32(),
        muted: muted.to_u32(),
        faint: faint.to_u32(),
        accent: accent.to_u32(),
        accent_fg: accent_fg.to_u32(),
        link: link.to_u32(),
        danger: if light { 0xda373c } else { 0xf23f43 },
        text_danger: if light { 0xda373c } else { 0xf23f43 },
        line: line.to_u32(),
        strong: line.mix(text, 0.15).to_u32(),
        mention_bg: mention_bg.to_u32(),
        mention_fg: mention_fg.to_u32(),
        online: online.to_u32(),
        idle: if light { 0x8a5a00 } else { 0xf0b232 },
        dnd: if light { 0xd1242f } else { 0xf23f43 },
        offline: if light { 0x5c6370 } else { 0x8b8f98 },
        secondary: muted.to_u32(),
        syntax_number: text.to_u32(),
        pill: if light { 0x060607 } else { 0xffffff },
        shadow: if light { 0x1a1d2624 } else { 0x00000000 },
    })
}

trait Members {
    fn with_members(self, members: Color) -> Self;
}

impl Members for Colors {
    fn with_members(mut self, members: Color) -> Self {
        self.bg_members = members;
        self
    }
}

fn theme(
    id: PresetId,
    mode: ColorMode,
    chrome: Chrome,
    metrics: Metrics,
    colors: Colors,
    background: Background,
) -> Theme {
    Theme {
        id: id.id().into(),
        name: id.label().into(),
        mode,
        chrome,
        preset: Some(id),
        colors,
        metrics,
        background,
        base: None,
        accent_seed: None,
        derived: false,
    }
}

pub fn ink_metrics(shadow: bool) -> Metrics {
    Metrics {
        font_ui: FontFace::Inter,
        font_mono: FontFace::JetBrainsMono,
        font_meta: FontFace::JetBrainsMono,
        font_scale: 1.0,
        radius_scale: 1.0,
        message_spacing: 1.0,
        saturation: 1.0,
        gap: 0.0,
        rail_width: 72.0,
        rail_pad: 10.0,
        tile_size: 44.0,
        radius_tile: 8.0,
        sidebar_width: 240.0,
        member_width: 240.0,
        header_h: 42.0,
        header_px: 14.0,
        title_px: 15.0,
        hash_icon: 16.0,
        composer_h: 38.0,
        member_row: 38.0,
        category_pad: 8.0,
        category_px: 11.0,
        channel_label: 14.0,
        channel_icon: 15.0,
        dm_avatar: 20.0,
        voice_mx: 8.0,
        voice_mt: 6.0,
        voice_px: 10.0,
        voice_radius: 6.0,
        dock_pl: 10.0,
        dock_pr: 6.0,
        dock_name: 13.0,
        avatar: 32.0,
        message_py: 4.0,
        message_gap: 10.0,
        radius_panel: 0.0,
        radius_code: 6.0,
        radius_media: 6.0,
        radius_control: 6.0,
        radius_avatar: 999.0,
        code_size: 12.5,
        code_line: 16.0,
        narrow_at: 900.0,
        body_size: 14.0,
        body_line: 20.0,
        border_width: 1.0,
        shadow_y: if shadow { 8.0 } else { 0.0 },
        shadow_blur: if shadow { 18.0 } else { 0.0 },
        shadow_spread: 0.0,
        show_top_rule: false,
        ink_chrome: true,
    }
}

pub fn classic_metrics(shadow: bool) -> Metrics {
    Metrics {
        font_ui: FontFace::Figtree,
        font_mono: FontFace::JetBrainsMono,
        font_meta: FontFace::Figtree,
        font_scale: 1.0,
        radius_scale: 1.0,
        message_spacing: 1.0,
        saturation: 1.0,
        gap: 0.0,
        rail_width: 72.0,
        rail_pad: 12.0,
        tile_size: 48.0,
        radius_tile: 12.0,
        sidebar_width: 240.0,
        member_width: 240.0,
        header_h: 48.0,
        header_px: 12.0,
        title_px: 16.0,
        hash_icon: 20.0,
        composer_h: 47.0,
        member_row: 44.0,
        category_pad: 14.0,
        category_px: 12.0,
        channel_label: 16.0,
        channel_icon: 20.0,
        dm_avatar: 24.0,
        voice_mx: 0.0,
        voice_mt: 0.0,
        voice_px: 8.0,
        voice_radius: 0.0,
        dock_pl: 8.0,
        dock_pr: 8.0,
        dock_name: 14.0,
        avatar: 40.0,
        message_py: 8.0,
        message_gap: 16.0,
        radius_panel: 0.0,
        radius_code: 12.0,
        radius_media: 12.0,
        radius_control: 6.0,
        radius_avatar: 999.0,
        code_size: 13.0,
        code_line: 22.0,
        narrow_at: 1100.0,
        body_size: 16.0,
        body_line: 20.0,
        border_width: 1.0,
        shadow_y: if shadow { 10.0 } else { 0.0 },
        shadow_blur: if shadow { 22.0 } else { 0.0 },
        shadow_spread: 0.0,
        show_top_rule: false,
        ink_chrome: false,
    }
}

fn hsl(h: f32, s: f32, l: f32) -> Color {
    let s = (s / 100.0).clamp(0.0, 1.0);
    let l = (l / 100.0).clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - ((h_prime % 2.0) - 1.0).abs());
    let (r, g, b) = match h_prime as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    Color::from_bytes(
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
        255,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuildTile {
    pub fill: u32,
    pub border: u32,
    pub letter: u32,
}

pub fn guild_tile(name: &str, active: bool, theme: &Theme) -> GuildTile {
    if theme.colors.bg_rail.relative_luminance() > 0.45 {
        let fill = theme.colors.bg_elevated;
        let border = if active {
            theme.colors.accent
        } else {
            theme.colors.border_hairline
        };
        let letter = ensure_contrast(theme.colors.text_primary, fill, 4.5);
        return GuildTile {
            fill: fill.to_u32(),
            border: border.to_u32(),
            letter: letter.to_u32(),
        };
    }
    if theme.metrics.ink_chrome {
        let fill = if name == "Analytical Engines" {
            0x0a0a16
        } else {
            theme.colors.bg_rail.to_u32()
        };
        let border = if active {
            theme.colors.accent.to_u32()
        } else {
            theme.colors.border_hairline.to_u32()
        };
        let letter = if active {
            theme.colors.text_primary.to_u32()
        } else {
            theme.colors.text_secondary.to_u32()
        };
        return GuildTile {
            fill,
            border,
            letter,
        };
    }
    let fill = match name {
        "Analytical Engines" => 0x3d4a6b,
        "Difference Club" => 0x3a4150,
        _ => hashed_tile(name),
    };
    let letter = ensure_contrast(theme.colors.text_primary, Color::rgb(fill), 4.5).to_u32();
    GuildTile {
        fill,
        border: fill,
        letter,
    }
}

fn hashed_tile(name: &str) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    for byte in name.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let r = 0x30 + (hash & 0x1f);
    let g = 0x34 + ((hash >> 8) & 0x1f);
    let b = 0x40 + ((hash >> 16) & 0x2f);
    (r << 16) | (g << 8) | b
}

pub fn role_color(name: &str, theme: &Theme) -> Color {
    let chat = theme.colors.bg_chat;
    let light = chat.relative_luminance() > 0.45;
    let seed = if light {
        match name {
            "Ada" => 0x1d4ed8,
            "Charles" => 0x15803d,
            "Lovelace" => 0xc2410c,
            _ => hashed_role(name, true),
        }
    } else if theme.metrics.ink_chrome {
        match name {
            "Ada" => 0xa8b0e0,
            "Charles" => 0x7ecca0,
            "Lovelace" => 0xc4a6f0,
            _ => hashed_role(name, false),
        }
    } else {
        match name {
            "Ada" => 0xa8b0d4,
            "Charles" => 0x8fb89a,
            "Lovelace" => 0xc4a48a,
            _ => hashed_role(name, false),
        }
    };
    let color = ensure_contrast(Color::rgb(seed), chat, 4.5);
    if contrast_ratio(color, chat) < 4.5 {
        ensure_contrast(theme.colors.text_primary, chat, 4.5)
    } else {
        color
    }
}

fn hashed_role(name: &str, light: bool) -> u32 {
    let mut hash = 0u32;
    for byte in name.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(byte));
    }
    let hue = (hash % 360) as f32;
    let color = if light {
        hsl(hue, 55.0, 32.0)
    } else {
        hsl(hue, 35.0, 72.0)
    };
    color.to_u32()
}

/// Read-only map from Discord's synced appearance strings. Rencord never
/// writes appearance back to Discord.
pub fn map_discord_appearance(theme: Option<&str>, client_theme: Option<&str>) -> Option<PresetId> {
    let client = client_theme.unwrap_or("").trim().to_ascii_lowercase();
    let theme = theme.unwrap_or("").trim().to_ascii_lowercase();
    let mapped = match client.as_str() {
        "light" => Some(PresetId::Light),
        "ash" => Some(PresetId::Ash),
        "onyx" => Some(PresetId::Onyx),
        "amoled" | "darker" => Some(PresetId::Amoled),
        "midnight" => Some(PresetId::MidnightBlurple),
        "dark" => Some(PresetId::Dark),
        _ => None,
    };
    if mapped.is_some() {
        return mapped;
    }
    match theme.as_str() {
        "light" => Some(PresetId::Light),
        "dark" => Some(PresetId::Dark),
        _ => None,
    }
}
