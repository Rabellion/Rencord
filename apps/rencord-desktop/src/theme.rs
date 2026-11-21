//! Design tokens for the desktop shell.
//!
//! Colors are sRGB hex. The GPUI view maps them with `rgb`. Durations and
//! easing names are the motion contract; the window uses the durations for
//! overlay fades.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutKind {
    /// Familiar server rail, channel list, chat, member list.
    Discord,
    /// Palette-first, two channel panes, little chrome.
    Keyboard,
    /// One sidebar with favorites and mentions, floating voice card.
    Sidebar,
}

impl LayoutKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Discord => "Discord, perfected",
            Self::Keyboard => "Keyboard-first",
            Self::Sidebar => "Unified sidebar",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "a" | "discord" => Some(Self::Discord),
            "b" | "keyboard" => Some(Self::Keyboard),
            "c" | "sidebar" => Some(Self::Sidebar),
            _ => None,
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Discord => Self::Keyboard,
            Self::Keyboard => Self::Sidebar,
            Self::Sidebar => Self::Discord,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Onyx,
    Dark,
    Ash,
    Light,
}

impl ThemeMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Onyx => "Onyx Ink",
            Self::Dark => "Crisp",
            Self::Ash => "Ash",
            Self::Light => "Light",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Onyx => Self::Dark,
            Self::Dark => Self::Ash,
            Self::Ash => Self::Light,
            Self::Light => Self::Onyx,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "onyx" | "amoled" => Some(Self::Onyx),
            "dark" | "crisp" | "c" => Some(Self::Dark),
            "ash" => Some(Self::Ash),
            "light" => Some(Self::Light),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Compact,
    Cozy,
    Spacious,
}

impl Density {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Cozy => "Cozy",
            Self::Spacious => "Spacious",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Compact => Self::Cozy,
            Self::Cozy => Self::Spacious,
            Self::Spacious => Self::Compact,
        }
    }

    pub fn channel_row_px(self) -> f32 {
        match self {
            Self::Compact => 28.0,
            Self::Cozy => 32.0,
            Self::Spacious => 36.0,
        }
    }

    pub fn server_icon_px(self) -> f32 {
        match self {
            Self::Compact => 40.0,
            Self::Cozy => 48.0,
            Self::Spacious => 54.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub mode: ThemeMode,
    pub app_bg: u32,
    pub rail: u32,
    pub sidebar: u32,
    pub chat: u32,
    pub members: u32,
    pub elevated: u32,
    /// Composer and search fill. Lighter than the chat on Ash, dark on Onyx.
    pub input: u32,
    pub hover: u32,
    pub selected: u32,
    pub text: u32,
    pub muted: u32,
    pub faint: u32,
    pub accent: u32,
    pub accent_text: u32,
    pub danger: u32,
    pub line: u32,
    pub code_bg: u32,
    pub link: u32,
    /// Soft mention wash. Not the harsh yellow Discord uses.
    pub ping: u32,
}

impl Palette {
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Onyx => Self {
                mode,
                app_bg: 0x000000,
                rail: 0x000000,
                sidebar: 0x000000,
                chat: 0x000000,
                members: 0x000000,
                elevated: 0x0a0a14,
                input: 0x000000,
                hover: 0x0c1024,
                selected: 0x080b1a,
                text: 0xedeef7,
                muted: 0x9aa0c0,
                faint: 0x7a80a0,
                accent: 0x4f6bff,
                accent_text: 0x000000,
                danger: 0xe05550,
                line: 0x15152a,
                code_bg: 0x000000,
                link: 0xa8b8ff,
                ping: 0x101428,
            },
            ThemeMode::Ash | ThemeMode::Dark => Self {
                mode,
                app_bg: 0x16181d,
                rail: 0x1c1f26,
                sidebar: 0x1c1f26,
                chat: 0x1c1f26,
                members: 0x1c1f26,
                elevated: 0x23272f,
                input: 0x23272f,
                hover: 0x22262e,
                selected: 0x2a2f38,
                text: 0xedf0f8,
                muted: 0x8592ad,
                faint: 0x576175,
                accent: 0x5865f2,
                accent_text: 0xffffff,
                danger: 0xf23f43,
                line: 0x2e3440,
                code_bg: 0x16181d,
                link: 0x00a8fc,
                ping: 0x22263a,
            },
            ThemeMode::Light => Self {
                mode,
                app_bg: 0xe6e9f0,
                rail: 0xf7f8fb,
                sidebar: 0xf7f8fb,
                chat: 0xf7f8fb,
                members: 0xf7f8fb,
                elevated: 0xffffff,
                input: 0xffffff,
                hover: 0xe8ebf2,
                selected: 0xdfe3ec,
                text: 0x1a1d26,
                muted: 0x4e5870,
                faint: 0x6b7388,
                accent: 0x5865f2,
                accent_text: 0xffffff,
                danger: 0xda373c,
                line: 0xd5dae6,
                code_bg: 0xeef1f6,
                link: 0x0068e0,
                ping: 0xe4e7ff,
            },
        }
    }
}

/// Spacing scale in pixels: 4, 8, 12, 16, 24, 32.
pub const SPACE: [f32; 6] = [4.0, 8.0, 12.0, 16.0, 24.0, 32.0];

/// Corner radii: none, control, panel, pill.
pub const RADII: [f32; 4] = [0.0, 6.0, 12.0, 999.0];

/// Type scale in pixels at the default font size of 15.
pub const TYPE_SCALE: [f32; 5] = [11.0, 13.0, 15.0, 18.0, 24.0];

pub const FONT_STEPS: [f32; 3] = [13.0, 15.0, 17.0];

/// Settings choice for the HDR-to-SDR curve. Defaults to the BT.2390 EETF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdrCurveChoice {
    Bt2390,
    Hable,
    Aces,
}

impl HdrCurveChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bt2390 => "BT.2390",
            Self::Hable => "Hable",
            Self::Aces => "ACES",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Bt2390 => Self::Hable,
            Self::Hable => Self::Aces,
            Self::Aces => Self::Bt2390,
        }
    }

    pub fn tone_curve(self) -> rencord_hdr::ToneCurve {
        match self {
            Self::Bt2390 => rencord_hdr::ToneCurve::Bt2390,
            Self::Hable => rencord_hdr::ToneCurve::Hable,
            Self::Aces => rencord_hdr::ToneCurve::Aces,
        }
    }
}

/// Motion: hover 80ms, overlay 140ms, layout 180ms. Ease-out cubic.
pub const MOTION_HOVER_MS: u64 = 80;
pub const MOTION_OVERLAY_MS: u64 = 140;
pub const MOTION_LAYOUT_MS: u64 = 180;

pub fn token_summary() -> String {
    format!(
        "space {} radius {} type {} hover {}ms layout {}ms",
        SPACE[1], RADII[1], TYPE_SCALE[2], MOTION_HOVER_MS, MOTION_LAYOUT_MS
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    pub theme: ThemeMode,
    pub density: Density,
    pub font_px: f32,
    pub layout: LayoutKind,
    pub ui_scale: f32,
    pub icon_px: f32,
    pub saturation: f32,
    pub soft_ping: bool,
    pub show_members: bool,
    pub show_sidebar: bool,
    pub split_pane: bool,
    pub pip: bool,
    pub stats_hud: bool,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    /// Offer a 10-bit track when every viewer is a Rencord peer that asked for it.
    pub hdr_send: bool,
    /// Accept a 10-bit track and present it on an HDR display when one is reported.
    pub hdr_receive: bool,
    pub hdr_curve: HdrCurveChoice,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Onyx,
            density: Density::Cozy,
            font_px: 15.0,
            layout: LayoutKind::Discord,
            ui_scale: 1.0,
            icon_px: 48.0,
            saturation: 1.0,
            soft_ping: true,
            show_members: true,
            show_sidebar: true,
            split_pane: false,
            pip: false,
            stats_hud: false,
            input_device: None,
            output_device: None,
            hdr_send: false,
            hdr_receive: false,
            hdr_curve: HdrCurveChoice::Bt2390,
        }
    }
}

impl Preferences {
    pub fn palette(&self) -> Palette {
        let mut palette = Palette::for_mode(self.theme);
        if self.saturation < 0.99 {
            palette = desaturate(palette, self.saturation.clamp(0.5, 1.0));
        }
        palette
    }

    pub fn server_icon_px(&self) -> f32 {
        self.icon_px * self.ui_scale
    }

    pub fn bump_scale(&mut self, direction: i32) {
        let steps = [0.85_f32, 1.0, 1.15, 1.3];
        let index = steps
            .iter()
            .position(|step| (*step - self.ui_scale).abs() < 0.02)
            .unwrap_or(1);
        let next = (index as i32 + direction).clamp(0, steps.len() as i32 - 1) as usize;
        self.ui_scale = steps[next];
    }

    pub fn bump_icon(&mut self, direction: i32) {
        let steps = [36.0_f32, 40.0, 48.0, 54.0];
        let index = steps
            .iter()
            .position(|step| (*step - self.icon_px).abs() < 0.5)
            .unwrap_or(2);
        let next = (index as i32 + direction).clamp(0, steps.len() as i32 - 1) as usize;
        self.icon_px = steps[next];
    }

    pub fn bump_font(&mut self, direction: i32) {
        let index = FONT_STEPS
            .iter()
            .position(|step| (*step - self.font_px).abs() < 0.1)
            .unwrap_or(1);
        let next = index as i32 + direction;
        let next = next.clamp(0, FONT_STEPS.len() as i32 - 1) as usize;
        self.font_px = FONT_STEPS[next];
    }
}

fn desaturate(palette: Palette, saturation: f32) -> Palette {
    let mut palette = palette;
    let fields = [
        &mut palette.app_bg,
        &mut palette.rail,
        &mut palette.sidebar,
        &mut palette.chat,
        &mut palette.members,
        &mut palette.elevated,
        &mut palette.input,
        &mut palette.hover,
        &mut palette.selected,
        &mut palette.text,
        &mut palette.muted,
        &mut palette.faint,
        &mut palette.accent,
        &mut palette.link,
        &mut palette.ping,
        &mut palette.code_bg,
        &mut palette.line,
    ];
    for field in fields {
        *field = mix_gray(*field, saturation);
    }
    palette
}

fn mix_gray(hex: u32, saturation: f32) -> u32 {
    let r = (hex >> 16) & 0xff;
    let g = (hex >> 8) & 0xff;
    let b = hex & 0xff;
    let gray = (r + g + b) / 3;
    let mix = |channel: u32| -> u32 {
        (gray as f32 + (channel as f32 - gray as f32) * saturation).round() as u32
    };
    (mix(r) << 16) | (mix(g) << 8) | mix(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(hex: u32, index: u8) -> f32 {
        let value = ((hex >> (index * 8)) & 0xff) as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(hex: u32) -> f32 {
        // Stored as 0xRRGGBB.
        let r = channel(hex, 2);
        let g = channel(hex, 1);
        let b = channel(hex, 0);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    pub fn contrast_ratio(foreground: u32, background: u32) -> f32 {
        let lighter = luminance(foreground).max(luminance(background));
        let darker = luminance(foreground).min(luminance(background));
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn amoled_is_true_black_and_themes_cycle() {
        let onyx = Palette::for_mode(ThemeMode::Onyx);
        assert_eq!(onyx.app_bg, 0x000000);
        assert_eq!(onyx.chat, 0x000000);
        assert_eq!(onyx.accent, 0x4f6bff);
        assert_eq!(onyx.accent_text, 0x000000);
        assert_eq!(onyx.link, 0xa8b8ff);
        assert_eq!(Palette::for_mode(ThemeMode::Dark).app_bg, 0x16181d);
        assert_eq!(Palette::for_mode(ThemeMode::Ash).chat, 0x1c1f26);
        assert_ne!(Palette::for_mode(ThemeMode::Light).text, onyx.text);
        assert_eq!(
            ThemeMode::Onyx.cycle().cycle().cycle().cycle(),
            ThemeMode::Onyx
        );
    }

    #[test]
    fn body_text_meets_wcag_aa_on_every_theme() {
        for mode in [
            ThemeMode::Onyx,
            ThemeMode::Dark,
            ThemeMode::Ash,
            ThemeMode::Light,
        ] {
            let palette = Palette::for_mode(mode);
            let text = contrast_ratio(palette.text, palette.chat);
            let muted = contrast_ratio(palette.muted, palette.chat);
            let field = contrast_ratio(palette.text, palette.input);
            let placeholder = contrast_ratio(palette.muted, palette.input);
            let accent = contrast_ratio(palette.accent_text, palette.accent);
            assert!(text >= 4.5, "{mode:?} text {text}");
            assert!(muted >= 4.5, "{mode:?} muted {muted}");
            assert!(field >= 4.5, "{mode:?} field {field}");
            assert!(placeholder >= 4.5, "{mode:?} placeholder {placeholder}");
            assert!(accent >= 4.5, "{mode:?} accent {accent}");
        }
    }
}
