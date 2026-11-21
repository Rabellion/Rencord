//! Local settings and the upload history. Both live in files the desktop picks.

use crate::plan::QualityPreference;
use crate::provider::{LitterboxTime, ProviderKind};
use crate::split::ArchiveFormat;
use crate::UploadError;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultAction {
    Ask,
    Compress,
    External,
    Split,
}

impl DefaultAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Compress => "Compress",
            Self::External => "Link",
            Self::Split => "Split",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Ask => Self::Compress,
            Self::Compress => Self::External,
            Self::External => Self::Split,
            Self::Split => Self::Ask,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadSettings {
    pub default_action: DefaultAction,
    pub provider: ProviderKind,
    pub quality: QualityPreference,
    pub archive: ArchiveFormat,
    pub litterbox: LitterboxTime,
    pub zero_x_hours: Option<u32>,
    /// Presigned PUT URL, or an `s3 ...` line (it holds the host's secret key).
    /// Empty means Custom is not configured. Never written to the settings file:
    /// it is read back only so an old file can still be loaded.
    #[serde(default, skip_serializing)]
    pub custom_spec: String,
}

impl Default for UploadSettings {
    fn default() -> Self {
        Self {
            default_action: DefaultAction::Ask,
            provider: ProviderKind::Litterbox,
            quality: QualityPreference::Balanced,
            archive: ArchiveFormat::Zip,
            litterbox: LitterboxTime::H24,
            zero_x_hours: Some(24),
            custom_spec: String::new(),
        }
    }
}

impl UploadSettings {
    pub fn expiry_label(&self) -> String {
        match self.provider {
            ProviderKind::Litterbox => format!("expires {}", self.litterbox.as_api()),
            ProviderKind::Catbox => "kept until Catbox removes it".to_owned(),
            ProviderKind::ZeroX => match self.zero_x_hours {
                Some(hours) => format!("expires in {hours} h"),
                None => "no expiry field".to_owned(),
            },
            ProviderKind::Custom => "your endpoint".to_owned(),
        }
    }

    pub fn cycle_expiry(&mut self) {
        match self.provider {
            ProviderKind::Litterbox => self.litterbox = self.litterbox.cycle(),
            ProviderKind::ZeroX => {
                self.zero_x_hours = match self.zero_x_hours {
                    Some(24) => Some(72),
                    Some(72) => Some(168),
                    Some(_) => None,
                    None => Some(24),
                };
            }
            ProviderKind::Catbox | ProviderKind::Custom => {}
        }
    }
}

pub fn load_settings(path: &Path) -> UploadSettings {
    let Ok(text) = fs::read_to_string(path) else {
        return UploadSettings::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save_settings(path: &Path, settings: &UploadSettings) -> Result<(), UploadError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    restrict_private(&file)?;
    let body = serde_json::to_string_pretty(settings)
        .map_err(|err| UploadError::message(format!("settings JSON: {err}")))?;
    file.write_all(body.as_bytes())?;
    Ok(())
}

/// Older builds saved `custom_spec`, with the upload host's secret key, in the
/// settings file. Remove it in place. A file without it is left untouched.
pub fn scrub_saved_secret(path: &Path) -> Result<(), UploadError> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(());
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    if object.remove("custom_spec").is_none() {
        return Ok(());
    }
    let body = serde_json::to_string_pretty(&value)
        .map_err(|err| UploadError::message(format!("settings JSON: {err}")))?;
    // The file already exists, so its private permissions are kept.
    fs::write(path, body)?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at_unix: u64,
    pub provider: String,
    pub file_name: String,
    pub url: String,
    pub expires: Option<String>,
    pub bytes: u64,
}

pub fn load_history(path: &Path) -> Vec<HistoryEntry> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn append_history(path: &Path, entry: HistoryEntry) -> Result<(), UploadError> {
    let mut entries = load_history(path);
    entries.push(entry);
    if entries.len() > 200 {
        let drop_count = entries.len() - 200;
        entries.drain(0..drop_count);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    restrict_private(&file)?;
    let body = serde_json::to_string_pretty(&entries)
        .map_err(|err| UploadError::message(format!("history JSON: {err}")))?;
    file.write_all(body.as_bytes())?;
    Ok(())
}

fn restrict_private(file: &fs::File) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = file.metadata()?.permissions();
        permissions.set_mode(0o600);
        file.set_permissions(permissions)?;
    }
    #[cfg(not(unix))]
    {
        let _ = file;
    }
    Ok(())
}

pub fn format_unix(secs: u64) -> String {
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let hour = tod / 3600;
    let minute = (tod % 3600) / 60;
    let second = tod % 60;
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

pub(crate) fn civil_from_days(days: i64) -> (i32, u32, u32) {
    // Howard Hinnant's civil_from_days, public domain algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_history_append() {
        let dir = std::env::temp_dir().join(format!("rencord-upload-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let settings_path = dir.join("upload-settings.json");
        let settings = UploadSettings {
            default_action: DefaultAction::Split,
            quality: QualityPreference::Sharper,
            provider: ProviderKind::ZeroX,
            ..UploadSettings::default()
        };
        save_settings(&settings_path, &settings).unwrap();
        let loaded = load_settings(&settings_path);
        assert_eq!(loaded, settings);
        let history_path = dir.join("upload-history.json");
        append_history(
            &history_path,
            HistoryEntry {
                at_unix: 0,
                provider: "litterbox".into(),
                file_name: "a.bin".into(),
                url: "https://example.test/a".into(),
                expires: Some("24h".into()),
                bytes: 10,
            },
        )
        .unwrap();
        let history = load_history(&history_path);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].url, "https://example.test/a");
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
        let _ = fs::remove_dir_all(&dir);
    }

    const SECRET_SPEC: &str = "s3 https://bucket.example.test AKIAEXAMPLE SECRETKEYVALUE";

    fn temp_file(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rencord-upload-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("upload-settings.json")
    }

    #[test]
    fn saving_settings_never_writes_the_custom_spec() {
        let path = temp_file("no-secret");
        let settings = UploadSettings {
            provider: ProviderKind::Custom,
            custom_spec: SECRET_SPEC.into(),
            ..UploadSettings::default()
        };
        save_settings(&path, &settings).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("SECRETKEYVALUE"), "{text}");
        assert!(!text.contains("custom_spec"), "{text}");
        // The other choices still round-trip.
        assert_eq!(load_settings(&path).provider, ProviderKind::Custom);
    }

    #[test]
    fn scrubbing_removes_a_secret_an_older_build_saved() {
        let path = temp_file("scrub");
        let old = UploadSettings {
            provider: ProviderKind::Custom,
            default_action: DefaultAction::Split,
            ..UploadSettings::default()
        };
        let mut json = serde_json::to_value(&old).unwrap();
        json["custom_spec"] = SECRET_SPEC.into();
        fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("SECRETKEYVALUE"));

        scrub_saved_secret(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("SECRETKEYVALUE"), "{text}");
        let kept = load_settings(&path);
        assert_eq!(kept.provider, ProviderKind::Custom);
        assert_eq!(kept.default_action, DefaultAction::Split);
    }

    #[test]
    fn scrubbing_a_clean_or_missing_file_changes_nothing() {
        let path = temp_file("scrub-clean");
        scrub_saved_secret(&path).unwrap(); // missing file
        save_settings(&path, &UploadSettings::default()).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        scrub_saved_secret(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        fs::write(&path, "not json").unwrap();
        scrub_saved_secret(&path).unwrap(); // unparsable: left alone
        assert_eq!(fs::read_to_string(&path).unwrap(), "not json");
    }

    #[test]
    fn missing_settings_file_is_the_default() {
        let missing = std::env::temp_dir().join("rencord-upload-missing-settings.json");
        let _ = fs::remove_file(&missing);
        let settings = load_settings(&missing);
        assert_eq!(settings.default_action, DefaultAction::Ask);
        assert_eq!(settings.provider, ProviderKind::Litterbox);
        assert_eq!(settings.quality, QualityPreference::Balanced);
    }
}
