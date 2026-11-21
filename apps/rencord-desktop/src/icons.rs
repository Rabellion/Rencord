//! Original stand-in icons. The SVG files in `docs/design/icons` are drawn for
//! Rencord. They are not copied from Discord. If a local Discord install is
//! present, [`discord_install_icon_dir`] reports its path so a later loader can
//! read icons at runtime without committing those files.

use image::codecs::png::PngEncoder;
use image::ImageEncoder;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

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

/// Home-button glyph inside the 44 px squircle. 30 px keeps the tail inside
/// the tile and leaves the four bars readable.
pub fn rail_home_glyph_px(tile: f32) -> f32 {
    if tile >= 40.0 {
        30.0
    } else {
        (tile - 14.0).clamp(24.0, 30.0)
    }
}

/// Device pixels for a CSS size at `scale` (1, 1.5, or 2).
pub fn hexwave_device_px(css_px: f32, scale: f32) -> u32 {
    (css_px * scale.max(1.0)).round().clamp(1.0, 512.0) as u32
}

/// `hexwave-mark.svg` drawn at 256 px (or larger) with anti-aliasing, then
/// Catmull-Rom filtered down to the device-pixel size. `currentColor` becomes
/// `ink`. The third bar stays the blurple baked into the file.
pub fn hexwave_png(ink: u32, css_px: f32, scale: f32) -> Vec<u8> {
    let device = hexwave_device_px(css_px, scale);
    let key = (ink, device);
    let mut cache = HEXWAVE_CACHE
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("hexwave cache");
    if let Some(png) = cache.get(&key) {
        return png.clone();
    }
    let png = render_hexwave_png(ink, device);
    cache.insert(key, png.clone());
    png
}

type HexwaveCache = Mutex<std::collections::HashMap<(u32, u32), Vec<u8>>>;

static HEXWAVE_CACHE: OnceLock<HexwaveCache> = OnceLock::new();

fn render_hexwave_png(ink: u32, device_px: u32) -> Vec<u8> {
    let source = device_px.clamp(256, 512);
    let colored = include_str!("../../../assets/brand/hexwave-mark.svg")
        .replace("currentColor", &format!("#{ink:06X}"));
    let svg = colored.replacen(
        "viewBox=\"0 0 128 128\"",
        &format!("width=\"{source}\" height=\"{source}\" viewBox=\"0 0 128 128\""),
        1,
    );
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &resvg::usvg::Options::default())
        .expect("hexwave-mark.svg");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(source, source).expect("hexwave pixmap");
    let fit = source as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(fit, fit),
        &mut pixmap.as_mut(),
    );
    let mut straight = pixmap.take();
    unpremultiply(&mut straight);
    let high = image::RgbaImage::from_raw(source, source, straight).expect("hexwave buffer");
    let fitted = if source == device_px {
        high
    } else {
        image::imageops::resize(
            &high,
            device_px,
            device_px,
            image::imageops::FilterType::CatmullRom,
        )
    };
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(
            fitted.as_raw(),
            device_px,
            device_px,
            image::ExtendedColorType::Rgba8,
        )
        .expect("hexwave png");
    png
}

fn unpremultiply(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
        } else if alpha < 255 {
            pixel[0] = ((u16::from(pixel[0]) * 255) / alpha).min(255) as u8;
            pixel[1] = ((u16::from(pixel[1]) * 255) / alpha).min(255) as u8;
            pixel[2] = ((u16::from(pixel[2]) * 255) / alpha).min(255) as u8;
        }
    }
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
    fn home_glyph_is_a_smooth_downsample_of_the_mark() {
        assert_eq!(rail_home_glyph_px(44.0), 30.0);
        assert_eq!(hexwave_device_px(30.0, 1.0), 30);
        assert_eq!(hexwave_device_px(30.0, 1.5), 45);
        assert_eq!(hexwave_device_px(30.0, 2.0), 60);
        let mark = hexwave_svg_for_px(72.0);
        assert!(mark.contains("#4F6BFF")); // hex-lint-allow: brand asset, not a painted UI color
        assert!(mark.contains("currentColor"));
        let png = hexwave_png(0x00_ED_EE_F7, 30.0, 1.0);
        let image = image::load_from_memory(&png).expect("png").to_rgba8();
        assert_eq!(image.dimensions(), (30, 30));
        let mut blue = 0;
        let mut soft = 0;
        for pixel in image.pixels() {
            let [r, _g, b, a] = pixel.0;
            if a > 0 && a < 255 {
                soft += 1;
            }
            if a > 160 && b > 160 && b > r.saturating_add(40) {
                blue += 1;
            }
        }
        assert!(soft > 40, "anti-aliased silhouette, got {soft}");
        assert!(blue > 4, "blurple bar survived filtering, got {blue}");
        let retina = image::load_from_memory(&hexwave_png(0x00_ED_EE_F7, 30.0, 2.0))
            .expect("png")
            .to_rgba8();
        assert_eq!(retina.dimensions(), (60, 60));
        let small = hexwave_svg_for_px(24.0);
        assert!(small.contains("Rencord tray icon"));
        assert!(!small.contains("#4F6BFF")); // hex-lint-allow: brand asset, not a painted UI color
    }
}
