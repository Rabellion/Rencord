//! Per-account appearance. This file never round-trips to Discord.

use crate::file::{theme_from_file, ThemeError, ThemeFile};
use crate::preset::PresetId;
use crate::theme::Theme;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppearanceFile {
    pub version: u32,
    pub preset: String,
    #[serde(default)]
    pub custom: Option<ThemeFile>,
    #[serde(default)]
    pub sync_system: bool,
    #[serde(default = "default_light")]
    pub light_preset: String,
    #[serde(default = "default_dark")]
    pub dark_preset: String,
    #[serde(default)]
    pub sync_discord: bool,
    #[serde(default)]
    pub seeded_from_discord: bool,
    #[serde(default)]
    pub high_contrast: bool,
    #[serde(default)]
    pub reduced_motion: bool,
    #[serde(default)]
    pub colorblind_status: bool,
    #[serde(default = "default_density")]
    pub density: String,
    #[serde(default = "default_font")]
    pub font_px: f32,
    #[serde(default = "default_one")]
    pub font_scale: f32,
    #[serde(default = "default_one")]
    pub message_spacing: f32,
    #[serde(default = "default_one")]
    pub radius_scale: f32,
    #[serde(default = "default_one")]
    pub saturation: f32,
}

fn default_light() -> String {
    "light".into()
}
fn default_dark() -> String {
    "onyx-ink".into()
}
fn default_density() -> String {
    "cozy".into()
}
fn default_font() -> f32 {
    15.0
}
fn default_one() -> f32 {
    1.0
}

impl Default for AppearanceFile {
    fn default() -> Self {
        Self {
            version: 1,
            preset: PresetId::OnyxInk.id().into(),
            custom: None,
            sync_system: false,
            light_preset: default_light(),
            dark_preset: default_dark(),
            sync_discord: false,
            seeded_from_discord: false,
            high_contrast: false,
            reduced_motion: false,
            colorblind_status: false,
            density: default_density(),
            font_px: default_font(),
            font_scale: 1.0,
            message_spacing: 1.0,
            radius_scale: 1.0,
            saturation: 1.0,
        }
    }
}

impl AppearanceFile {
    pub fn preset_id(&self) -> PresetId {
        PresetId::parse(&self.preset).unwrap_or(PresetId::OnyxInk)
    }

    pub fn light_id(&self) -> PresetId {
        PresetId::parse(&self.light_preset).unwrap_or(PresetId::Light)
    }

    pub fn dark_id(&self) -> PresetId {
        PresetId::parse(&self.dark_preset).unwrap_or(PresetId::OnyxInk)
    }

    pub fn custom_theme(&self) -> Result<Option<Theme>, ThemeError> {
        self.custom.clone().map(theme_from_file).transpose()
    }
}

pub fn read_appearance(path: &std::path::Path) -> Option<AppearanceFile> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_appearance(path: &std::path::Path, file: &AppearanceFile) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(file).unwrap_or_else(|_| "{}".into());
    std::fs::write(path, text)
}
