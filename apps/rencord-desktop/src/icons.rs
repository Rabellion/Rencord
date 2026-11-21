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

/// Home-button glyph. The 44 px tile gets 32 px so the four bars can sit on
/// whole pixels.
pub fn rail_home_glyph_px(tile: f32) -> f32 {
    if tile >= 40.0 {
        32.0
    } else {
        (tile - 12.0).clamp(18.0, 32.0)
    }
}

/// 32×32 RGBA of the rail Hexwave. GPUI paints SVG as a one-color mask, so
/// the blurple bar has to be real pixels. Bars are integer rectangles: a 3 px
/// stem, a 2 px bar, a 3×8 accent third bar, and a 2 px bar, each separated
/// by a 1 px gap.
pub fn rail_hexwave_rgba(ink: u32) -> Vec<u8> {
    const PX: usize = 32;
    const POLY: [(f32, f32); 16] = [
        (10.0, 4.0),
        (22.0, 4.0),
        (26.0, 6.0),
        (29.0, 14.0),
        (29.0, 18.0),
        (26.0, 25.0),
        (22.0, 27.0),
        (17.0, 27.0),
        (8.0, 31.0),
        (7.0, 29.0),
        (11.0, 27.0),
        (10.0, 27.0),
        (7.0, 25.0),
        (3.0, 18.0),
        (3.0, 14.0),
        (7.0, 6.0),
    ];
    // x, y, width, height. The third entry is the blurple bar, painted after
    // the knockouts.
    const BARS: [(i32, i32, i32, i32); 4] = [
        (9, 10, 3, 12),
        (13, 13, 2, 6),
        (16, 11, 3, 8),
        (20, 14, 2, 5),
    ];
    let ir = ((ink >> 16) & 0xff) as u8;
    let ig = ((ink >> 8) & 0xff) as u8;
    let ib = (ink & 0xff) as u8;
    let mut pixels = vec![0u8; PX * PX * 4];
    for y in 0..PX {
        for x in 0..PX {
            if point_in_poly(x as f32 + 0.5, y as f32 + 0.5, &POLY) {
                let i = (y * PX + x) * 4;
                pixels[i] = ir;
                pixels[i + 1] = ig;
                pixels[i + 2] = ib;
                pixels[i + 3] = 255;
            }
        }
    }
    // All four bars are knockouts. GPUI's image path was not keeping the
    // blurple texel, so the third bar is a solid element painted on top.
    for (x0, y0, w, h) in BARS {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let i = (y as usize * PX + x as usize) * 4;
                pixels[i] = 0;
                pixels[i + 1] = 0;
                pixels[i + 2] = 0;
                pixels[i + 3] = 0;
            }
        }
    }
    pixels
}

fn point_in_poly(x: f32, y: f32, poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
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
        assert_eq!(rail_home_glyph_px(44.0), 32.0);
        let mark = hexwave_svg_for_px(72.0);
        assert!(mark.contains("#4F6BFF")); // hex-lint-allow: brand asset, not a painted UI color
        assert!(mark.contains("currentColor"));
        let rgba = rail_hexwave_rgba(0x00_ED_EE_F7);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let at = |x: usize, y: usize| {
            let i = (y * 32 + x) * 4;
            rgba[i + 3]
        };
        // Four whole-pixel slits: 3, gap, 2, gap, 3, gap, 2.
        assert_eq!(at(10, 14), 0);
        assert_eq!(at(12, 14), 255);
        assert_eq!(at(14, 15), 0);
        assert_eq!(at(15, 15), 255);
        assert_eq!(at(17, 14), 0);
        assert_eq!(at(19, 14), 255);
        assert_eq!(at(20, 16), 0);
        let mut blue_hole = 0;
        for y in 11..19 {
            for x in 16..19 {
                if at(x, y) == 0 {
                    blue_hole += 1;
                }
            }
        }
        assert_eq!(blue_hole, 3 * 8);
        let small = hexwave_svg_for_px(24.0);
        assert!(small.contains("Rencord tray icon"));
        assert!(!small.contains("#4F6BFF")); // hex-lint-allow: brand asset, not a painted UI color
        assert!(hexwave_svg_for_px(16.0).contains("Rencord tray icon"));
    }
}
