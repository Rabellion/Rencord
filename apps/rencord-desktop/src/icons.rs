//! Original stand-in icons. The SVG files in `docs/design/icons` are drawn for
//! Rencord. They are not copied from Discord. If a local Discord install is
//! present, [`discord_install_icon_dir`] reports its path so a later loader can
//! read icons at runtime without committing those files.

use std::path::PathBuf;

pub fn icon_pack_dir() -> PathBuf {
    if let Ok(config) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("rencord").join("icon-pack");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("rencord")
            .join("icon-pack");
    }
    PathBuf::from(".config/rencord/icon-pack")
}

pub fn discord_install_icon_dir() -> Option<PathBuf> {
    let candidates = [
        "/usr/share/discord/resources",
        "/opt/discord/resources",
        "/usr/lib/discord",
    ];
    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_lookup_does_not_panic() {
        let _ = discord_install_icon_dir();
        let pack = icon_pack_dir();
        assert!(pack.ends_with("icon-pack"));
    }
}
