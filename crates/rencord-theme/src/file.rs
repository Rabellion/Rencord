//! Theme documents. TOML and JSON share one shape. A theme code is
//! `rencord1.` plus URL-safe base64 of a zlib-compressed JSON document.

use crate::color::Color;
use crate::derive::derive_theme;
use crate::preset::PresetId;
use crate::theme::{Background, BackgroundKind, Chrome, ColorMode, FontFace, Theme};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};

pub const THEME_CODE_PREFIX: &str = "rencord1.";
const MAX_THEME_BYTES: usize = 512 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("{0}")]
    Message(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ThemeFile {
    pub version: u32,
    pub name: String,
    pub id: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub chrome: String,
    #[serde(default)]
    pub derive: bool,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub accent: Option<String>,
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    #[serde(default)]
    pub metrics: MetricFile,
    #[serde(default)]
    pub background: BackgroundFile,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MetricFile {
    #[serde(default)]
    pub font_ui: Option<String>,
    #[serde(default)]
    pub font_mono: Option<String>,
    #[serde(default)]
    pub font_meta: Option<String>,
    #[serde(default)]
    pub font_scale: Option<f32>,
    #[serde(default)]
    pub radius_scale: Option<f32>,
    #[serde(default)]
    pub message_spacing: Option<f32>,
    #[serde(default)]
    pub saturation: Option<f32>,
    #[serde(default)]
    pub gap: Option<f32>,
    #[serde(default)]
    pub rail_width: Option<f32>,
    #[serde(default)]
    pub tile_size: Option<f32>,
    #[serde(default)]
    pub radius_tile: Option<f32>,
    #[serde(default)]
    pub radius_panel: Option<f32>,
    #[serde(default)]
    pub radius_control: Option<f32>,
    #[serde(default)]
    pub message_py: Option<f32>,
    #[serde(default)]
    pub message_gap: Option<f32>,
    #[serde(default)]
    pub shadow_blur: Option<f32>,
    #[serde(default)]
    pub shadow_y: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BackgroundFile {
    #[serde(default = "solid_kind")]
    pub kind: String,
    #[serde(default)]
    pub angle: f32,
    #[serde(default)]
    pub stops: Vec<String>,
    #[serde(default)]
    pub grain: f32,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub blur: f32,
    #[serde(default)]
    pub dim: f32,
}

fn solid_kind() -> String {
    "solid".into()
}

impl Default for BackgroundFile {
    fn default() -> Self {
        Self {
            kind: solid_kind(),
            angle: 0.0,
            stops: Vec::new(),
            grain: 0.0,
            image: String::new(),
            blur: 0.0,
            dim: 0.0,
        }
    }
}

impl Theme {
    pub fn to_file(&self) -> ThemeFile {
        let mut colors = BTreeMap::new();
        for key in crate::theme::Colors::keys() {
            if let Some(color) = self.colors.get(key) {
                colors.insert((*key).to_owned(), color.to_hex());
            }
        }
        let (kind, angle, stops) = match &self.background.kind {
            BackgroundKind::Solid => ("solid".to_owned(), 0.0, Vec::new()),
            BackgroundKind::Gradient { angle, stops } => (
                "gradient".to_owned(),
                *angle,
                stops.iter().map(|color| color.to_hex()).collect(),
            ),
        };
        ThemeFile {
            version: 1,
            name: self.name.clone(),
            id: self.id.clone(),
            mode: self.mode.as_str().into(),
            chrome: self.chrome.as_str().into(),
            derive: self.derived,
            base: self.base.map(|color| color.to_hex()),
            accent: self.accent_seed.map(|color| color.to_hex()),
            colors,
            metrics: MetricFile {
                font_ui: Some(self.metrics.font_ui.label().into()),
                font_mono: Some(self.metrics.font_mono.label().into()),
                font_meta: Some(self.metrics.font_meta.label().into()),
                font_scale: Some(self.metrics.font_scale),
                radius_scale: Some(self.metrics.radius_scale),
                message_spacing: Some(self.metrics.message_spacing),
                saturation: Some(self.metrics.saturation),
                gap: Some(self.metrics.gap),
                rail_width: Some(self.metrics.rail_width),
                tile_size: Some(self.metrics.tile_size),
                radius_tile: Some(self.metrics.radius_tile),
                radius_panel: Some(self.metrics.radius_panel),
                radius_control: Some(self.metrics.radius_control),
                message_py: Some(self.metrics.message_py),
                message_gap: Some(self.metrics.message_gap),
                shadow_blur: Some(self.metrics.shadow_blur),
                shadow_y: Some(self.metrics.shadow_y),
            },
            background: BackgroundFile {
                kind,
                angle,
                stops,
                grain: self.background.grain,
                image: self.background.image_path.clone(),
                blur: self.background.image_blur,
                dim: self.background.image_dim,
            },
        }
    }

    pub fn to_toml(&self) -> Result<String, ThemeError> {
        toml::to_string_pretty(&self.to_file()).map_err(|err| ThemeError::Message(err.to_string()))
    }

    pub fn to_json(&self) -> Result<String, ThemeError> {
        serde_json::to_string_pretty(&self.to_file())
            .map_err(|err| ThemeError::Message(err.to_string()))
    }

    pub fn to_code(&self) -> Result<String, ThemeError> {
        let json = serde_json::to_vec(&self.to_file())
            .map_err(|err| ThemeError::Message(err.to_string()))?;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
        encoder
            .write_all(&json)
            .map_err(|err| ThemeError::Message(err.to_string()))?;
        let bytes = encoder
            .finish()
            .map_err(|err| ThemeError::Message(err.to_string()))?;
        Ok(format!("{THEME_CODE_PREFIX}{}", base64_url(&bytes)))
    }
}

pub fn theme_from_toml(text: &str) -> Result<Theme, ThemeError> {
    let file: ThemeFile =
        toml::from_str(text).map_err(|err| ThemeError::Message(err.to_string()))?;
    theme_from_file(file)
}

pub fn theme_from_json(text: &str) -> Result<Theme, ThemeError> {
    let file: ThemeFile =
        serde_json::from_str(text).map_err(|err| ThemeError::Message(err.to_string()))?;
    theme_from_file(file)
}

pub fn theme_from_code(text: &str) -> Result<Theme, ThemeError> {
    let trimmed = text.trim();
    let payload = trimmed
        .strip_prefix(THEME_CODE_PREFIX)
        .ok_or_else(|| ThemeError::Message("theme code must start with rencord1.".into()))?;
    let bytes = base64_url_decode(payload)?;
    let decoder = flate2::read::ZlibDecoder::new(std::io::Cursor::new(bytes));
    let mut json = Vec::new();
    decoder
        .take(MAX_THEME_BYTES as u64)
        .read_to_end(&mut json)
        .map_err(|err| ThemeError::Message(err.to_string()))?;
    if json.len() >= MAX_THEME_BYTES {
        return Err(ThemeError::Message("theme code is too large".into()));
    }
    let text = String::from_utf8(json).map_err(|err| ThemeError::Message(err.to_string()))?;
    theme_from_json(&text)
}

pub fn theme_from_file(file: ThemeFile) -> Result<Theme, ThemeError> {
    validate_file(&file)?;
    let mode = ColorMode::parse(&file.mode);
    let chrome = Chrome::parse(&file.chrome).unwrap_or(Chrome::Classic);
    let mut theme = if file.derive {
        let base = parse_color(file.base.as_deref().unwrap_or(""), "base")?;
        let accent = parse_color(file.accent.as_deref().unwrap_or(""), "accent")?;
        derive_theme(&file.name, base, accent, mode, chrome)
    } else if let Some(preset) = PresetId::parse(&file.id) {
        Theme::preset(preset)
    } else if file.colors.len() >= crate::theme::Colors::keys().len() {
        let mut seed = Theme::preset(PresetId::OnyxInk);
        seed.preset = None;
        seed.id = file.id.clone();
        seed.name = file.name.clone();
        seed
    } else {
        return Err(ThemeError::Message(
            "theme needs a known preset id, derive = true, or every color token".into(),
        ));
    };
    if !file.name.trim().is_empty() {
        theme.name = file.name.trim().to_owned();
    }
    if !file.id.trim().is_empty() {
        theme.id = file.id.trim().to_owned();
    }
    if let Some(mode) = mode {
        theme.mode = mode;
    }
    theme.chrome = chrome;
    theme.metrics.ink_chrome = chrome == Chrome::Ink;
    for (key, value) in &file.colors {
        let color = parse_color(value, key)?;
        if !theme.colors.set(key, color) {
            return Err(ThemeError::Message(format!("unknown color token {key}")));
        }
    }
    apply_metrics(&mut theme, &file.metrics)?;
    theme.background = background_from_file(&file.background)?;
    if file.derive {
        theme.derived = true;
        theme.base = file.base.as_deref().and_then(Color::parse);
        theme.accent_seed = file.accent.as_deref().and_then(Color::parse);
    }
    Ok(theme)
}

pub fn validate_file(file: &ThemeFile) -> Result<(), ThemeError> {
    if file.version != 1 {
        return Err(ThemeError::Message("version must be 1".into()));
    }
    if file.name.trim().is_empty() || file.name.chars().count() > 64 {
        return Err(ThemeError::Message(
            "name must be 1 to 64 characters".into(),
        ));
    }
    if file.id.trim().is_empty() || file.id.chars().count() > 64 {
        return Err(ThemeError::Message("id must be 1 to 64 characters".into()));
    }
    if !file.mode.is_empty() && ColorMode::parse(&file.mode).is_none() {
        return Err(ThemeError::Message("mode must be light or dark".into()));
    }
    if !file.chrome.is_empty() && Chrome::parse(&file.chrome).is_none() {
        return Err(ThemeError::Message("chrome must be ink or classic".into()));
    }
    if file.derive && (file.base.is_none() || file.accent.is_none()) {
        return Err(ThemeError::Message(
            "derive needs base and accent colors".into(),
        ));
    }
    for (key, value) in &file.colors {
        if crate::theme::Colors::keys()
            .iter()
            .all(|known| *known != key)
        {
            return Err(ThemeError::Message(format!("unknown color token {key}")));
        }
        parse_color(value, key)?;
    }
    check_unit("font_scale", file.metrics.font_scale, 0.5, 2.0)?;
    check_unit("radius_scale", file.metrics.radius_scale, 0.0, 2.0)?;
    check_unit("message_spacing", file.metrics.message_spacing, 0.5, 2.0)?;
    check_unit("saturation", file.metrics.saturation, 0.0, 1.0)?;
    check_unit("gap", file.metrics.gap, 0.0, 32.0)?;
    check_unit("grain", Some(file.background.grain), 0.0, 0.03)?;
    check_unit("blur", Some(file.background.blur), 0.0, 40.0)?;
    check_unit("dim", Some(file.background.dim), 0.0, 1.0)?;
    if file.background.stops.len() > 6 {
        return Err(ThemeError::Message(
            "a gradient can have at most 6 stops".into(),
        ));
    }
    match file.background.kind.as_str() {
        "solid" | "gradient" => {}
        other => {
            return Err(ThemeError::Message(format!(
                "background kind {other} is not solid or gradient"
            )))
        }
    }
    for stop in &file.background.stops {
        parse_color(stop, "background stop")?;
    }
    Ok(())
}

fn apply_metrics(theme: &mut Theme, metrics: &MetricFile) -> Result<(), ThemeError> {
    if let Some(name) = &metrics.font_ui {
        theme.metrics.font_ui = FontFace::parse(name);
    }
    if let Some(name) = &metrics.font_mono {
        theme.metrics.font_mono = FontFace::parse(name);
    }
    if let Some(name) = &metrics.font_meta {
        theme.metrics.font_meta = FontFace::parse(name);
    }
    assign(&mut theme.metrics.font_scale, metrics.font_scale);
    assign(&mut theme.metrics.radius_scale, metrics.radius_scale);
    assign(&mut theme.metrics.message_spacing, metrics.message_spacing);
    assign(&mut theme.metrics.saturation, metrics.saturation);
    assign(&mut theme.metrics.gap, metrics.gap);
    assign(&mut theme.metrics.rail_width, metrics.rail_width);
    assign(&mut theme.metrics.tile_size, metrics.tile_size);
    assign(&mut theme.metrics.radius_tile, metrics.radius_tile);
    assign(&mut theme.metrics.radius_panel, metrics.radius_panel);
    assign(&mut theme.metrics.radius_control, metrics.radius_control);
    assign(&mut theme.metrics.message_py, metrics.message_py);
    assign(&mut theme.metrics.message_gap, metrics.message_gap);
    assign(&mut theme.metrics.shadow_blur, metrics.shadow_blur);
    assign(&mut theme.metrics.shadow_y, metrics.shadow_y);
    Ok(())
}

fn assign(slot: &mut f32, value: Option<f32>) {
    if let Some(value) = value {
        *slot = value;
    }
}

fn background_from_file(file: &BackgroundFile) -> Result<Background, ThemeError> {
    let kind = match file.kind.as_str() {
        "gradient" => {
            let stops = file
                .stops
                .iter()
                .map(|stop| parse_color(stop, "background stop"))
                .collect::<Result<Vec<_>, _>>()?;
            if stops.len() < 2 {
                return Err(ThemeError::Message(
                    "a gradient needs at least two stops".into(),
                ));
            }
            BackgroundKind::Gradient {
                angle: file.angle,
                stops,
            }
        }
        _ => BackgroundKind::Solid,
    };
    Ok(Background {
        kind,
        grain: file.grain.clamp(0.0, 0.03),
        image_path: file.image.clone(),
        image_blur: file.blur.clamp(0.0, 40.0),
        image_dim: file.dim.clamp(0.0, 1.0),
    })
}

fn parse_color(value: &str, label: &str) -> Result<Color, ThemeError> {
    Color::parse(value).ok_or_else(|| ThemeError::Message(format!("{label} is not a hex color")))
}

fn check_unit(label: &str, value: Option<f32>, min: f32, max: f32) -> Result<(), ThemeError> {
    if let Some(value) = value {
        if !(min..=max).contains(&value) {
            return Err(ThemeError::Message(format!(
                "{label} must be between {min} and {max}"
            )));
        }
    }
    Ok(())
}

fn base64_url(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let n = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | bytes[index + 2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        index += 3;
    }
    if index < bytes.len() {
        let remain = bytes.len() - index;
        let mut n = (bytes[index] as u32) << 16;
        if remain == 2 {
            n |= (bytes[index + 1] as u32) << 8;
        }
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if remain == 2 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        }
    }
    out
}

fn base64_url_decode(text: &str) -> Result<Vec<u8>, ThemeError> {
    fn val(ch: u8) -> Result<u8, ThemeError> {
        match ch {
            b'A'..=b'Z' => Ok(ch - b'A'),
            b'a'..=b'z' => Ok(ch - b'a' + 26),
            b'0'..=b'9' => Ok(ch - b'0' + 52),
            b'-' => Ok(62),
            b'_' => Ok(63),
            _ => Err(ThemeError::Message("theme code is not base64".into())),
        }
    }
    let bytes = text.as_bytes();
    if bytes.len() > MAX_THEME_BYTES {
        return Err(ThemeError::Message("theme code is too large".into()));
    }
    let mut out = Vec::new();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        let n = ((val(bytes[index])? as u32) << 18)
            | ((val(bytes[index + 1])? as u32) << 12)
            | ((val(bytes[index + 2])? as u32) << 6)
            | val(bytes[index + 3])? as u32;
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
        out.push(n as u8);
        index += 4;
    }
    let rest = bytes.len() - index;
    if rest == 2 {
        let n = ((val(bytes[index])? as u32) << 18) | ((val(bytes[index + 1])? as u32) << 12);
        out.push((n >> 16) as u8);
    } else if rest == 3 {
        let n = ((val(bytes[index])? as u32) << 18)
            | ((val(bytes[index + 1])? as u32) << 12)
            | ((val(bytes[index + 2])? as u32) << 6);
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
    } else if rest != 0 {
        return Err(ThemeError::Message("theme code is not base64".into()));
    }
    Ok(out)
}

pub fn load_theme_dir(path: &std::path::Path) -> Vec<Theme> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(path) else {
        return found;
    };
    let mut paths: Vec<_> = entries.filter_map(|entry| entry.ok()).collect();
    paths.sort_by_key(|entry| entry.file_name());
    for entry in paths {
        let path = entry.path();
        let Some(name) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let theme = match name {
            "toml" => theme_from_toml(&text).ok(),
            "json" => theme_from_json(&text).ok(),
            _ => None,
        };
        if let Some(theme) = theme {
            found.push(theme);
        }
    }
    found
}

pub fn dir_stamp(path: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut stamp = 0u64;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            stamp = stamp.wrapping_add(meta.len());
            if let Ok(modified) = meta.modified() {
                if let Ok(time) = modified.duration_since(std::time::UNIX_EPOCH) {
                    stamp = stamp.wrapping_add(time.as_secs());
                }
            }
        }
    }
    stamp
}
