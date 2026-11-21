//! Platform services for Milestone 1.
//!
//! The token store is a file created with mode `0600` on Unix. An OS keyring
//! (Secret Service, Keychain, Credential Manager) is the intended production
//! backend; this crate exposes the trait and the file implementation that CI
//! can run without a secret service. Official login is described, not opened:
//! Milestone 1 accepts a pasted user token.

mod appearance;

pub use appearance::{
    appearance_path, parse_gnome_animations, parse_gnome_color_scheme, parse_macos_interface_style,
    parse_macos_reduce_motion, parse_portal_color_scheme, parse_windows_apps_use_light,
    sanitize_account, system_reduced_motion, system_scheme, themes_dir, SystemScheme,
};

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
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
        windows_macos: "Ephemeral WebView2 or WKWebView via gpui-wry. Load the hosted login page, accept only a discord.com origin, read the session token from the official handoff, store it, and destroy the webview.",
        linux: "gpui-wry does not cover Linux. The login window has to be a separate GTK4 / WebKitGTK 6.0 process, same origin check, then torn down. It is not linked in Milestone 1.",
    }
}

pub trait SecretStore {
    fn save_token(&self, token: &str) -> Result<(), PlatformError>;
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
    fn save_token(&self, token: &str) -> Result<(), PlatformError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        restrict_private(&file)?;
        file.write_all(token.as_bytes())?;
        Ok(())
    }

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

fn restrict_private(file: &fs::File) -> io::Result<()> {
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
    fn file_secret_roundtrip_and_mode() {
        let path = std::env::temp_dir().join(format!("rencord-token-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        let store = FileSecretStore::new(&path);
        assert!(store.load_token().unwrap().is_none());
        store.save_token("abc").unwrap();
        assert_eq!(store.load_token().unwrap().as_deref(), Some("abc"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
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
