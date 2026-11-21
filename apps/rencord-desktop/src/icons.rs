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

/// Glyphs at or below this size use the 3-bar Hexwave cut.
pub const HEXWAVE_SMALL_MAX_PX: f32 = 24.0;

/// Home-button glyph inside the existing squircle. 44 px tile -> 28 px mark.
pub fn rail_home_glyph_px(tile: f32) -> f32 {
    (tile - 16.0).clamp(18.0, 32.0)
}

/// Four-bar Hexwave above 24 px. The tray SVG is the 3-bar cut at or below that.
pub fn hexwave_svg_for_px(size_px: f32) -> &'static str {
    if size_px <= HEXWAVE_SMALL_MAX_PX {
        include_str!("../../../assets/brand/hexwave-tray.svg")
    } else {
        include_str!("../../../assets/brand/hexwave-mark.svg")
    }
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

    #[test]
    fn home_glyph_uses_the_four_bar_mark_and_the_small_cut_under_24() {
        let home = rail_home_glyph_px(44.0);
        assert!(home > HEXWAVE_SMALL_MAX_PX, "{home}");
        let mark = hexwave_svg_for_px(home);
        assert!(mark.contains("#4F6BFF"));
        assert!(mark.contains("currentColor"));
        let small = hexwave_svg_for_px(24.0);
        assert!(small.contains("Rencord tray icon"));
        assert!(!small.contains("#4F6BFF"));
        assert!(hexwave_svg_for_px(16.0).contains("Rencord tray icon"));
    }
}
