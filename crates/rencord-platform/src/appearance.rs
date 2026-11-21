//! Operating-system appearance.
//!
//! Linux asks the desktop portal for `org.freedesktop.appearance` color-scheme.
//! macOS reads `AppleInterfaceStyle`. Windows reads `AppsUseLightTheme`.
//! Each parser is covered without a live session so CI can run headless.

use std::path::PathBuf;
use std::process::Command;

use crate::data_dir;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemScheme {
    Light,
    Dark,
}

impl SystemScheme {
    pub fn prefers_dark(self) -> bool {
        matches!(self, Self::Dark)
    }
}

pub fn appearance_path(account_id: &str) -> PathBuf {
    data_dir()
        .join("accounts")
        .join(sanitize_account(account_id))
        .join("appearance.json")
}

pub fn themes_dir() -> PathBuf {
    data_dir().join("themes")
}

pub fn sanitize_account(account_id: &str) -> String {
    let mut out = String::new();
    for ch in account_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "local".into()
    } else {
        out
    }
}

pub fn system_scheme() -> Option<SystemScheme> {
    #[cfg(target_os = "linux")]
    {
        linux_scheme()
    }
    #[cfg(target_os = "macos")]
    {
        macos_scheme()
    }
    #[cfg(target_os = "windows")]
    {
        windows_scheme()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

pub fn system_reduced_motion() -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        let output = command_output(
            "gsettings",
            &["get", "org.gnome.desktop.interface", "enable-animations"],
        )?;
        parse_gnome_animations(&output)
    }
    #[cfg(target_os = "macos")]
    {
        let output = command_output(
            "defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
        )?;
        parse_macos_reduce_motion(&output)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

pub fn parse_portal_color_scheme(output: &str) -> Option<SystemScheme> {
    let mut digits = String::new();
    let mut current = String::new();
    for ch in output.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            digits = std::mem::take(&mut current);
        }
    }
    if !current.is_empty() {
        digits = current;
    }
    match digits.as_str() {
        "1" => Some(SystemScheme::Dark),
        "2" => Some(SystemScheme::Light),
        _ => None,
    }
}

pub fn parse_gnome_color_scheme(output: &str) -> Option<SystemScheme> {
    let value = output.trim().trim_matches('\'').to_ascii_lowercase();
    if value.contains("prefer-dark") {
        Some(SystemScheme::Dark)
    } else if value.contains("prefer-light") {
        Some(SystemScheme::Light)
    } else {
        None
    }
}

pub fn parse_macos_interface_style(output: &str, ok: bool) -> Option<SystemScheme> {
    if !ok {
        return Some(SystemScheme::Light);
    }
    if output.to_ascii_lowercase().contains("dark") {
        Some(SystemScheme::Dark)
    } else {
        Some(SystemScheme::Light)
    }
}

pub fn parse_windows_apps_use_light(output: &str) -> Option<SystemScheme> {
    let lower = output.to_ascii_lowercase();
    let marker = "appsuselighttheme";
    let rest = lower.split(marker).nth(1)?;
    let token = rest.split_whitespace().last()?;
    match token {
        "0x0" | "0x00000000" | "0" => Some(SystemScheme::Dark),
        "0x1" | "0x00000001" | "1" => Some(SystemScheme::Light),
        _ => None,
    }
}

pub fn parse_gnome_animations(output: &str) -> Option<bool> {
    let value = output.trim().to_ascii_lowercase();
    if value.contains("false") {
        Some(true)
    } else if value.contains("true") {
        Some(false)
    } else {
        None
    }
}

pub fn parse_macos_reduce_motion(output: &str) -> Option<bool> {
    match output.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn linux_scheme() -> Option<SystemScheme> {
    if let Some(output) = command_output(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.Read",
            "org.freedesktop.appearance",
            "color-scheme",
        ],
    ) {
        if let Some(scheme) = parse_portal_color_scheme(&output) {
            return Some(scheme);
        }
    }
    command_output(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "color-scheme"],
    )
    .as_deref()
    .and_then(parse_gnome_color_scheme)
}

#[cfg(target_os = "macos")]
fn macos_scheme() -> Option<SystemScheme> {
    let output = Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    parse_macos_interface_style(&text, output.status.success())
}

#[cfg(target_os = "windows")]
fn windows_scheme() -> Option<SystemScheme> {
    let output = command_output(
        "reg",
        &[
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ],
    )?;
    parse_windows_apps_use_light(&output)
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() && output.stdout.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_and_gnome_schemes_parse() {
        assert_eq!(
            parse_portal_color_scheme("(<uint32 1>,)"),
            Some(SystemScheme::Dark)
        );
        assert_eq!(
            parse_portal_color_scheme("(<uint32 2>,)"),
            Some(SystemScheme::Light)
        );
        assert_eq!(parse_portal_color_scheme("(<uint32 0>,)"), None);
        assert_eq!(
            parse_gnome_color_scheme("'prefer-dark'"),
            Some(SystemScheme::Dark)
        );
        assert_eq!(
            parse_gnome_color_scheme("'prefer-light'"),
            Some(SystemScheme::Light)
        );
    }

    #[test]
    fn desktop_os_parsers() {
        assert_eq!(
            parse_macos_interface_style("Dark\n", true),
            Some(SystemScheme::Dark)
        );
        assert_eq!(
            parse_macos_interface_style("", false),
            Some(SystemScheme::Light)
        );
        let windows = "    AppsUseLightTheme    REG_DWORD    0x0\r\n";
        assert_eq!(
            parse_windows_apps_use_light(windows),
            Some(SystemScheme::Dark)
        );
        assert_eq!(parse_gnome_animations("false\n"), Some(true));
        assert_eq!(parse_macos_reduce_motion("1\n"), Some(true));
        assert_eq!(sanitize_account("../ada"), "ada");
    }
}
