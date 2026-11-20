//! Original stand-in icons. The SVG files in `docs/design/icons` are drawn for
//! Rencord. They are not copied from Discord. If a local Discord install is
//! present, [`discord_install_icon_dir`] reports its path so a later loader can
//! read icons at runtime without committing those files.

use std::path::PathBuf;

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
    }
}
