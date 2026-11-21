//! Platform services.
//!
//! Tokens belong in the OS keychain (`rencord-auth`). [`FileSecretStore`] is
//! the Milestone 1 file, kept so an existing `0600` file can be read once and
//! deleted after the keychain accepts it. New sessions are not written here.

mod appearance;

pub use appearance::{
    appearance_path, parse_gnome_animations, parse_gnome_color_scheme, parse_macos_interface_style,
    parse_macos_reduce_motion, parse_portal_color_scheme, parse_windows_apps_use_light,
    sanitize_account, system_reduced_motion, system_scheme, themes_dir, SystemScheme,
};

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

pub fn data_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("RENCORD_DATA_DIR") {
        return PathBuf::from(override_dir);
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("rencord")
}

/// What a later milestone needs in order to replace token paste.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficialLoginRequirements {
    pub hosted_url: &'static str,
    pub windows_macos: &'static str,
    pub linux: &'static str,
}

pub fn official_login_requirements() -> OfficialLoginRequirements {
    OfficialLoginRequirements {
        hosted_url: "https://discord.com/login",
        windows_macos: "A separate login window uses WebView2 or WKWebView through wry. It loads the hosted login page. The session is read from Discord's own response, stored in the OS keychain, and the webview is destroyed. Rencord does not attempt to solve a captcha.",
        linux: "The login window is a separate process using WebKitGTK through wry. It loads the hosted page, reads the session Discord returns, stores it in the keychain, and exits. Email and password are the fallback when WebKitGTK cannot start.",
    }
}

pub trait SecretStore {
    fn load_token(&self) -> Result<Option<String>, PlatformError>;
    fn clear_token(&self) -> Result<(), PlatformError>;
}

#[derive(Clone, Debug)]
pub struct FileSecretStore {
    path: PathBuf,
}

impl FileSecretStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn in_data_dir() -> Self {
        Self::new(data_dir().join("token"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl SecretStore for FileSecretStore {
    fn load_token(&self) -> Result<Option<String>, PlatformError> {
        match fs::read_to_string(&self.path) {
            Ok(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(trimmed.to_owned()))
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn clear_token(&self) -> Result<(), PlatformError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

/// Freedesktop application id. Matches `rencord.desktop` and `StartupWMClass`.
pub const APP_ID: &str = "rencord";

/// 3-bar Hexwave for a system tray. No tray is created in this build.
pub const TRAY_ICON_SVG: &str = include_str!("../../../assets/brand/hexwave-tray.svg");

/// Symbolic icon installed under hicolor as `rencord-symbolic.svg`.
pub const SYMBOLIC_ICON_SVG: &str = include_str!("../../../assets/brand/hexwave-symbolic.svg");

/// In-process notification sink. Desktop toasts are a later milestone.
#[derive(Default)]
pub struct RecordingNotifier {
    pub items: std::sync::Mutex<Vec<Notification>>,
}

impl RecordingNotifier {
    pub fn push(&self, title: impl Into<String>, body: impl Into<String>) {
        self.items.lock().expect("notifier").push(Notification {
            title: title.into(),
            body: body.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_legacy_token_file_is_read_then_cleared() {
        let path = std::env::temp_dir().join(format!("rencord-token-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        let store = FileSecretStore::new(&path);
        assert!(store.load_token().unwrap().is_none());
        // Rencord no longer writes this file; an older build did.
        fs::write(&path, "abc\n").unwrap();
        assert_eq!(store.load_token().unwrap().as_deref(), Some("abc"));
        store.clear_token().unwrap();
        assert!(store.load_token().unwrap().is_none());
    }

    #[test]
    fn brand_icons_match_the_desktop_entry() {
        assert_eq!(APP_ID, "rencord");
        assert!(TRAY_ICON_SVG.contains("currentColor"));
        assert!(TRAY_ICON_SVG.contains("Rencord tray icon"));
        assert!(SYMBOLIC_ICON_SVG.contains("currentColor"));
        let desktop = include_str!("../../../assets/brand/linux/rencord.desktop");
        assert!(desktop.contains("Icon=rencord"));
        assert!(desktop.contains("StartupWMClass=rencord"));
        assert!(desktop.contains("Name=Rencord"));
        let ico = include_bytes!("../../../assets/brand/hexwave.ico");
        let sizes = ico_widths(ico);
        assert!(sizes.contains(&16), "{sizes:?}");
        assert!(sizes.contains(&24), "{sizes:?}");
        assert!(sizes.contains(&256), "{sizes:?}");
        let icns = include_bytes!("../../../assets/brand/hexwave.icns");
        assert_eq!(&icns[..4], b"icns");
        assert!(icns.windows(4).any(|chunk| chunk == b"icp4"));
    }

    fn ico_widths(bytes: &[u8]) -> Vec<u16> {
        assert!(bytes.len() >= 6);
        let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
        (0..count)
            .map(|index| {
                let width = bytes[6 + index * 16];
                if width == 0 {
                    256
                } else {
                    u16::from(width)
                }
            })
            .collect()
    }

    #[test]
    fn official_login_is_documented_not_implemented_as_a_solver() {
        let plan = official_login_requirements();
        assert_eq!(plan.hosted_url, "https://discord.com/login");
        assert!(plan.linux.contains("WebKitGTK"));
        assert!(!plan.windows_macos.to_lowercase().contains("captcha solver"));
    }
}
