//! Desktop view of a [`rencord_theme::Theme`].
//!
//! Hex lives in `rencord-theme`. This module only copies tokens into the
//! fields the GPUI shell reads.

use rencord_theme::{AppearanceFile, Chrome, ColorMode, ThemeFile};

pub use rencord_theme::{guild_tile, role_color, PresetId as ThemeMode, Theme};

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

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "compact" => Some(Self::Compact),
            "cozy" => Some(Self::Cozy),
            "spacious" => Some(Self::Spacious),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Cozy => "cozy",
            Self::Spacious => "spacious",
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

    fn message_scale(self) -> f32 {
        match self {
            Self::Compact => 0.8,
            Self::Cozy => 1.0,
            Self::Spacious => 1.25,
        }
    }
}

/// Spacing scale in pixels: 4, 8, 12, 16, 24, 32.
pub const SPACE: [f32; 6] = [4.0, 8.0, 12.0, 16.0, 24.0, 32.0];

/// Corner radii: none, control, panel, pill.
pub const RADII: [f32; 4] = [0.0, 6.0, 12.0, 999.0];

/// Type scale in pixels at the default font size of 15.
pub const TYPE_SCALE: [f32; 5] = [11.0, 13.0, 15.0, 18.0, 24.0];

pub const FONT_STEPS: [f32; 5] = [13.0, 14.0, 15.0, 16.0, 17.0];

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

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub light: bool,
    pub ink_chrome: bool,
    pub show_top_rule: bool,
    pub font_ui: &'static str,
    pub font_mono: &'static str,
    pub font_meta: &'static str,
    pub app_bg: u32,
    pub qr_bg: u32,
    pub rail: u32,
    pub sidebar: u32,
    pub chat: u32,
    pub members: u32,
    pub elevated: u32,
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
    pub ping: u32,
    pub text_secondary: u32,
    pub text_danger: u32,
    pub border_strong: u32,
    pub status_online: u32,
    pub status_idle: u32,
    pub status_dnd: u32,
    pub status_offline: u32,
    pub home_bg: u32,
    pub home_bg_active: u32,
    pub home_glyph: u32,
    pub home_glyph_active: u32,
    pub home_border: u32,
    pub home_border_active: u32,
    pub add_bg: u32,
    pub add_border: u32,
    pub add_glyph: u32,
    pub badge_bg: u32,
    pub badge_fg: u32,
    pub voice_bg: u32,
    pub voice_border: u32,
    pub voice_label: u32,
    pub voice_meta: u32,
    pub mention_fg: u32,
    pub mention_bg: u32,
    pub syntax_number: u32,
    pub syntax_keyword: u32,
    pub hover_border: u32,
    pub hover_divider: u32,
    pub overlay: u32,
    pub pill: u32,
    pub positive: u32,
    pub shadow: u32,
    pub hangup_bg: u32,
    pub hangup_fg: u32,
    pub stage: u32,
    pub rail_w: f32,
    pub tile: f32,
    pub tile_radius: f32,
    pub side_w: f32,
    pub member_w: f32,
    pub shell_gap: f32,
    pub rail_pad: f32,
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
    pub code_size: f32,
    pub code_line: f32,
    pub narrow_at: f32,
    pub body_size: f32,
    pub body_line: f32,
    pub border_width: f32,
    pub shadow_y: f32,
    pub shadow_blur: f32,
    pub shadow_spread: f32,
}

impl Palette {
    pub fn from_theme(theme: &Theme, font_px: f32, density: Density) -> Self {
        let c = &theme.colors;
        let m = &theme.metrics;
        let scale = m.font_scale.max(0.5);
        let radius = m.radius_scale.max(0.0);
        let spacing = m.message_spacing * density.message_scale();
        let u = |color: rencord_theme::Color| color.to_u32();
        let ink = m.ink_chrome;
        let body = if ink {
            m.body_size * scale
        } else {
            font_px * scale
        };
        Self {
            light: theme.mode == ColorMode::Light,
            ink_chrome: ink,
            show_top_rule: m.show_top_rule,
            font_ui: m.font_ui.family(),
            font_mono: m.font_mono.family(),
            font_meta: m.font_meta.family(),
            app_bg: u(c.bg_base),
            qr_bg: u(c.bg_qr),
            rail: u(c.bg_rail),
            sidebar: u(c.bg_sidebar),
            chat: u(c.bg_chat),
            members: u(c.bg_members),
            elevated: u(c.bg_elevated),
            input: u(c.bg_input),
            hover: u(c.bg_hover),
            selected: u(c.bg_selected),
            text: u(c.text_primary),
            muted: u(c.text_muted),
            faint: u(c.text_faint),
            accent: u(c.accent),
            accent_text: u(c.accent_fg),
            danger: u(c.danger),
            line: u(c.border_hairline),
            code_bg: u(c.bg_sunken),
            link: u(c.text_link),
            ping: u(c.bg_mention),
            text_secondary: u(c.text_secondary),
            text_danger: u(c.text_danger),
            border_strong: u(c.border_strong),
            status_online: u(c.status_online),
            status_idle: u(c.status_idle),
            status_dnd: u(c.status_dnd),
            status_offline: u(c.status_offline),
            home_bg: u(c.bg_home),
            home_bg_active: u(c.bg_home_active),
            home_glyph: u(c.text_home),
            home_glyph_active: u(c.text_home_active),
            home_border: u(c.border_home),
            home_border_active: u(c.border_home_active),
            add_bg: u(c.bg_add),
            add_border: u(c.border_add),
            add_glyph: u(c.text_add),
            badge_bg: u(c.bg_badge),
            badge_fg: u(c.text_badge),
            voice_bg: u(c.bg_voice),
            voice_border: u(c.border_voice),
            voice_label: if ink {
                u(c.text_link)
            } else {
                u(c.status_online)
            },
            voice_meta: u(c.text_voice),
            mention_fg: u(c.text_mention),
            mention_bg: u(c.bg_mention),
            syntax_number: u(c.syntax_number),
            syntax_keyword: u(c.syntax_keyword),
            hover_border: u(c.border_hover),
            hover_divider: u(c.border_strong),
            overlay: c.bg_overlay.to_rgba_u32(),
            pill: u(c.pill),
            positive: u(c.positive),
            shadow: c.shadow.to_rgba_u32(),
            hangup_bg: c.bg_hangup.to_rgba_u32(),
            hangup_fg: u(c.text_hangup),
            stage: u(c.bg_stage),
            rail_w: m.rail_width,
            tile: m.tile_size,
            tile_radius: m.radius_tile * radius,
            side_w: m.sidebar_width,
            member_w: m.member_width,
            shell_gap: m.gap,
            rail_pad: m.rail_pad,
            header_h: m.header_h,
            header_px: m.header_px,
            title_px: m.title_px * scale,
            hash_icon: m.hash_icon,
            composer_h: m.composer_h,
            member_row: m.member_row,
            category_pad: m.category_pad,
            category_px: m.category_px * scale,
            channel_label: m.channel_label * scale,
            channel_icon: m.channel_icon,
            dm_avatar: m.dm_avatar,
            voice_mx: m.voice_mx,
            voice_mt: m.voice_mt,
            voice_px: m.voice_px,
            voice_radius: m.voice_radius * radius,
            dock_pl: m.dock_pl,
            dock_pr: m.dock_pr,
            dock_name: m.dock_name * scale,
            avatar: m.avatar,
            message_py: m.message_py * spacing,
            message_gap: m.message_gap * spacing,
            radius_panel: m.radius_panel * radius,
            radius_code: m.radius_code * radius,
            radius_media: m.radius_media * radius,
            radius_control: m.radius_control * radius,
            code_size: if ink {
                m.code_size * scale
            } else {
                (font_px * scale - 1.0).max(12.0)
            },
            code_line: m.code_line,
            narrow_at: m.narrow_at,
            body_size: body,
            body_line: if ink { m.body_line } else { body + 4.0 },
            border_width: m.border_width,
            shadow_y: m.shadow_y,
            shadow_blur: m.shadow_blur,
            shadow_spread: m.shadow_spread,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    pub theme: ThemeMode,
    pub custom: Option<Theme>,
    pub sync_system: bool,
    pub light_preset: ThemeMode,
    pub dark_preset: ThemeMode,
    pub sync_discord: bool,
    pub seeded_from_discord: bool,
    pub high_contrast: bool,
    pub reduced_motion: bool,
    pub colorblind_status: bool,
    pub density: Density,
    pub font_px: f32,
    pub font_scale: f32,
    pub message_spacing: f32,
    pub radius_scale: f32,
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
            theme: ThemeMode::OnyxInk,
            custom: None,
            sync_system: false,
            light_preset: ThemeMode::Light,
            dark_preset: ThemeMode::OnyxInk,
            sync_discord: false,
            seeded_from_discord: false,
            high_contrast: false,
            reduced_motion: false,
            colorblind_status: false,
            density: Density::Cozy,
            font_px: 15.0,
            font_scale: 1.0,
            message_spacing: 1.0,
            radius_scale: 1.0,
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
    pub fn base_theme(&self) -> Theme {
        if let Some(custom) = &self.custom {
            custom.clone()
        } else {
            Theme::preset(self.theme)
        }
    }

    pub fn resolved(&self, system_dark: Option<bool>) -> Theme {
        let mut theme = if self.sync_system {
            match system_dark {
                Some(true) => Theme::preset(self.dark_preset),
                Some(false) => Theme::preset(self.light_preset),
                None => self.base_theme(),
            }
        } else {
            self.base_theme()
        };
        theme.metrics.font_scale = self.font_scale;
        theme.metrics.message_spacing = self.message_spacing;
        theme.metrics.radius_scale = self.radius_scale;
        if (self.saturation - 1.0).abs() > 0.01 {
            theme = theme.desaturate(self.saturation);
        }
        if self.high_contrast {
            theme = theme.high_contrast();
        }
        if self.colorblind_status {
            theme = theme.colorblind_status();
        }
        theme
    }

    pub fn palette_for(&self, system_dark: Option<bool>) -> Palette {
        Palette::from_theme(&self.resolved(system_dark), self.font_px, self.density)
    }

    pub fn palette(&self) -> Palette {
        self.palette_for(None)
    }

    pub fn theme_label(&self) -> String {
        if self.custom.is_some() {
            self.custom
                .as_ref()
                .map(|theme| theme.name.clone())
                .unwrap_or_else(|| "Custom".into())
        } else {
            self.theme.label().into()
        }
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
            .unwrap_or(2);
        let next = index as i32 + direction;
        let next = next.clamp(0, FONT_STEPS.len() as i32 - 1) as usize;
        self.font_px = FONT_STEPS[next];
    }

    pub fn bump_spacing(&mut self, direction: i32) {
        let steps = [0.75_f32, 1.0, 1.25, 1.5];
        self.message_spacing = step_f32(&steps, self.message_spacing, direction);
    }

    pub fn bump_radius(&mut self, direction: i32) {
        let steps = [0.0_f32, 0.75, 1.0, 1.25, 1.5];
        self.radius_scale = step_f32(&steps, self.radius_scale, direction);
    }

    pub fn bump_saturation(&mut self, direction: i32) {
        let steps = [0.4_f32, 0.7, 0.85, 1.0];
        self.saturation = step_f32(&steps, self.saturation, direction);
    }

    pub fn bump_grain(&mut self, direction: i32) {
        let Some(custom) = self.custom.as_mut() else {
            return;
        };
        let steps = [0.0_f32, 0.01, 0.02, 0.03];
        custom.background.grain = step_f32(&steps, custom.background.grain, direction);
    }

    pub fn to_file(&self) -> AppearanceFile {
        AppearanceFile {
            version: 1,
            preset: self.theme.id().into(),
            custom: self.custom.as_ref().map(Theme::to_file),
            sync_system: self.sync_system,
            light_preset: self.light_preset.id().into(),
            dark_preset: self.dark_preset.id().into(),
            sync_discord: self.sync_discord,
            seeded_from_discord: self.seeded_from_discord,
            high_contrast: self.high_contrast,
            reduced_motion: self.reduced_motion,
            colorblind_status: self.colorblind_status,
            density: self.density.as_str().into(),
            font_px: self.font_px,
            font_scale: self.font_scale,
            message_spacing: self.message_spacing,
            radius_scale: self.radius_scale,
            saturation: self.saturation,
        }
    }

    pub fn apply_file(&mut self, file: AppearanceFile) {
        self.theme = file.preset_id();
        self.custom = file.custom_theme().ok().flatten();
        self.sync_system = file.sync_system;
        self.light_preset = file.light_id();
        self.dark_preset = file.dark_id();
        self.sync_discord = file.sync_discord;
        self.seeded_from_discord = file.seeded_from_discord;
        self.high_contrast = file.high_contrast;
        self.reduced_motion = file.reduced_motion;
        self.colorblind_status = file.colorblind_status;
        if let Some(density) = Density::parse(&file.density) {
            self.density = density;
        }
        self.font_px = file.font_px;
        self.font_scale = file.font_scale;
        self.message_spacing = file.message_spacing;
        self.radius_scale = file.radius_scale;
        self.saturation = file.saturation;
    }

    pub fn ensure_custom(&mut self) -> &mut Theme {
        if self.custom.is_none() {
            let mut theme = Theme::preset(self.theme);
            theme.preset = None;
            theme.id = "custom".into();
            theme.name = "Custom".into();
            theme.chrome = if theme.metrics.ink_chrome {
                Chrome::Ink
            } else {
                Chrome::Classic
            };
            self.custom = Some(theme);
        }
        self.custom.as_mut().expect("custom theme")
    }
}

fn step_f32(steps: &[f32], current: f32, direction: i32) -> f32 {
    let index = steps
        .iter()
        .position(|step| (*step - current).abs() < 0.02)
        .unwrap_or(0);
    let next = (index as i32 + direction).clamp(0, steps.len() as i32 - 1) as usize;
    steps[next]
}

#[allow(dead_code)]
pub fn theme_file_from_path(path: &std::path::Path) -> Option<Theme> {
    let text = std::fs::read_to_string(path).ok()?;
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => rencord_theme::theme_from_json(&text).ok(),
        _ => rencord_theme::theme_from_toml(&text).ok(),
    }
}

#[allow(dead_code)]
pub fn appearance_template() -> ThemeFile {
    Theme::preset(ThemeMode::OnyxInk).to_file()
}
