//! Design tokens for the desktop shell.
//!
//! Colors are sRGB hex. The GPUI view maps them with `rgb`. Durations and
//! easing names are the motion contract; the window uses the durations for
//! overlay fades.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Amoled,
    Dark,
    Light,
}

impl ThemeMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Amoled => "AMOLED",
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Amoled => Self::Dark,
            Self::Dark => Self::Light,
            Self::Light => Self::Amoled,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "amoled" => Some(Self::Amoled),
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Compact,
    Cozy,
}

impl Density {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Cozy => "Cozy",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Compact => Self::Cozy,
            Self::Cozy => Self::Compact,
        }
    }

    /// Uniform virtual-row height. One height keeps the list on the fast path.
    pub fn row_px(self) -> f32 {
        match self {
            Self::Compact => 44.0,
            Self::Cozy => 68.0,
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
}

impl Palette {
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Amoled => Self {
                mode,
                app_bg: 0x000000,
                rail: 0x000000,
                sidebar: 0x050505,
                chat: 0x000000,
                members: 0x050505,
                elevated: 0x101010,
                hover: 0x161616,
                selected: 0x1c1c1c,
                text: 0xf2f3f5,
                muted: 0xa0a2a8,
                faint: 0x6d6f76,
                accent: 0x5b8cff,
                accent_text: 0xffffff,
                danger: 0xed4245,
                line: 0x1a1a1a,
                code_bg: 0x121212,
                link: 0x7aa2ff,
            },
            ThemeMode::Dark => Self {
                mode,
                app_bg: 0x111214,
                rail: 0x0c0d0f,
                sidebar: 0x17181b,
                chat: 0x1a1b1e,
                members: 0x15161a,
                elevated: 0x232428,
                hover: 0x2b2d31,
                selected: 0x35373c,
                text: 0xf2f3f5,
                muted: 0xb5bac1,
                faint: 0x80848e,
                accent: 0x5865f2,
                accent_text: 0xffffff,
                danger: 0xed4245,
                line: 0x2e3035,
                code_bg: 0x111214,
                link: 0x00a8fc,
            },
            ThemeMode::Light => Self {
                mode,
                app_bg: 0xf2f3f5,
                rail: 0xe3e5e8,
                sidebar: 0xffffff,
                chat: 0xffffff,
                members: 0xf7f7f8,
                elevated: 0xffffff,
                hover: 0xeaebee,
                selected: 0xe0e1e6,
                text: 0x1e1f22,
                muted: 0x4e5058,
                faint: 0x6d6f78,
                accent: 0x5865f2,
                accent_text: 0xffffff,
                danger: 0xda373c,
                line: 0xe1e2e4,
                code_bg: 0xf2f3f5,
                link: 0x0068e0,
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
    pub input_device: Option<String>,
    pub output_device: Option<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            density: Density::Cozy,
            font_px: 15.0,
            input_device: None,
            output_device: None,
        }
    }
}

impl Preferences {
    pub fn palette(&self) -> Palette {
        Palette::for_mode(self.theme)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amoled_is_true_black_and_themes_cycle() {
        let amoled = Palette::for_mode(ThemeMode::Amoled);
        assert_eq!(amoled.app_bg, 0x000000);
        assert_eq!(amoled.chat, 0x000000);
        assert_ne!(Palette::for_mode(ThemeMode::Dark).chat, 0x000000);
        assert_ne!(Palette::for_mode(ThemeMode::Light).text, amoled.text);
        assert_eq!(ThemeMode::Amoled.cycle().cycle().cycle(), ThemeMode::Amoled);
    }
}
