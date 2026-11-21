//! QR image for `https://discord.com/ra/<fingerprint>`.
//!
//! Error correction is level H so the center mark still scans. The mark is
//! Hexwave: a hex speech bubble with the voice bars knocked out, and the
//! middle bar in `#4F6BFF`.

use flate2::write::ZlibEncoder;
use flate2::Compression;
use qrcode::types::Color;
use qrcode::{EcLevel, QrCode};
use std::io::Write;

use crate::AuthError;

const SCALE: u32 = 8;
const QUIET: u32 = 4;
#[cfg(test)]
const ACCENT: [u8; 4] = [0x4f, 0x6b, 0xff, 0xff];
const INK: [u8; 4] = [0x00, 0x00, 0x00, 0xff];
const PAPER: [u8; 4] = [0xff, 0xff, 0xff, 0xff];

/// Hexwave A knockout. `currentColor` is the bubble; the middle bar is blurple.
const HEXWAVE: &str = include_str!("../../../assets/brand/hexwave-mark.svg");

#[derive(Clone, Debug)]
pub struct QrImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn render_qr_image(url: &str) -> Result<QrImage, AuthError> {
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::H)
        .map_err(|_| AuthError::Message("Could not build the login code.".into()))?;
    let modules = code.width() as u32;
    let side = (modules + QUIET * 2) * SCALE;
    let mut image = QrImage {
        width: side,
        height: side,
        pixels: vec![0xff; (side * side * 4) as usize],
    };
    for y in 0..modules {
        for x in 0..modules {
            if code[(x as usize, y as usize)] != Color::Dark {
                continue;
            }
            fill_rect(
                &mut image,
                ((x + QUIET) * SCALE) as i32,
                ((y + QUIET) * SCALE) as i32,
                SCALE as i32,
                SCALE as i32,
                INK,
            );
        }
    }
    paint_mark(&mut image, modules);
    Ok(image)
}

pub fn render_qr_png(url: &str) -> Result<Vec<u8>, AuthError> {
    encode_png(&render_qr_image(url)?)
}

fn paint_mark(image: &mut QrImage, modules: u32) {
    let code_px = modules * SCALE;
    let origin = QUIET * SCALE;
    // About a fifth of the code, which level H still recovers.
    let mark = ((code_px as f32) * 0.20).round().max(48.0) as u32;
    let pad = SCALE * 2;
    let outer = mark + pad * 2;
    let left = origin as i32 + (code_px as i32 - outer as i32) / 2;
    let top = origin as i32 + (code_px as i32 - outer as i32) / 2;
    fill_round(
        image,
        left,
        top,
        outer as i32,
        outer as i32,
        (outer / 6) as i32,
        PAPER,
    );
    if let Some(pixmap) = render_hexwave(mark, "#000000") {
        blit_pixmap(image, &pixmap, left + pad as i32, top + pad as i32);
    }
}

/// Raster of the Hexwave mark. `fill` replaces `currentColor` (the bubble).
/// The middle bar stays `#4F6BFF`. Transparent pixels keep the background.
pub fn hexwave_png(fill: &str, side: u32) -> Result<Vec<u8>, AuthError> {
    let pixmap = render_hexwave(side.max(16), fill)
        .ok_or_else(|| AuthError::Message("Could not draw the mark.".into()))?;
    let mut pixels = Vec::with_capacity((pixmap.width() * pixmap.height() * 4) as usize);
    for chunk in pixmap.data().chunks(4) {
        let alpha = chunk[3];
        if alpha == 0 {
            pixels.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        pixels.push(unpremultiply(chunk[0], alpha));
        pixels.push(unpremultiply(chunk[1], alpha));
        pixels.push(unpremultiply(chunk[2], alpha));
        pixels.push(alpha);
    }
    encode_png(&QrImage {
        width: pixmap.width(),
        height: pixmap.height(),
        pixels,
    })
}

fn render_hexwave(side: u32, fill: &str) -> Option<resvg::tiny_skia::Pixmap> {
    let svg = HEXWAVE.replace("currentColor", fill);
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(&svg, &options).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(side, side)?;
    let scale = side as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(pixmap)
}

fn blit_pixmap(image: &mut QrImage, pixmap: &resvg::tiny_skia::Pixmap, left: i32, top: i32) {
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let data = pixmap.data();
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            let alpha = data[index + 3];
            if alpha < 16 {
                continue;
            }
            let color = [
                unpremultiply(data[index], alpha),
                unpremultiply(data[index + 1], alpha),
                unpremultiply(data[index + 2], alpha),
                255,
            ];
            put(image, left + x, top + y, color);
        }
    }
}

fn unpremultiply(channel: u8, alpha: u8) -> u8 {
    let value = (u16::from(channel) * 255 + u16::from(alpha) / 2) / u16::from(alpha);
    value.min(255) as u8
}

fn fill_rect(image: &mut QrImage, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
    for py in y..(y + h) {
        for px in x..(x + w) {
            put(image, px, py, color);
        }
    }
}

fn fill_round(image: &mut QrImage, x: i32, y: i32, w: i32, h: i32, radius: i32, color: [u8; 4]) {
    let radius = radius.max(0).min(w.min(h) / 2);
    for py in y..(y + h) {
        for px in x..(x + w) {
            if in_round(px, py, x, y, w, h, radius) {
                put(image, px, py, color);
            }
        }
    }
}

fn in_round(px: i32, py: i32, x: i32, y: i32, w: i32, h: i32, radius: i32) -> bool {
    let dx = if px < x + radius {
        x + radius - px
    } else if px >= x + w - radius {
        px - (x + w - radius - 1)
    } else {
        0
    };
    let dy = if py < y + radius {
        y + radius - py
    } else if py >= y + h - radius {
        py - (y + h - radius - 1)
    } else {
        0
    };
    dx * dx + dy * dy <= radius * radius
}

fn put(image: &mut QrImage, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= image.width as i32 || y >= image.height as i32 {
        return;
    }
    let index = ((y as u32 * image.width + x as u32) * 4) as usize;
    image.pixels[index..index + 4].copy_from_slice(&color);
}

pub fn encode_png(image: &QrImage) -> Result<Vec<u8>, AuthError> {
    let mut raw = Vec::with_capacity(((image.width * 4 + 1) * image.height) as usize);
    for y in 0..image.height {
        raw.push(0);
        let start = (y * image.width * 4) as usize;
        let end = start + (image.width * 4) as usize;
        raw.extend_from_slice(&image.pixels[start..end]);
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&raw)
        .and_then(|_| encoder.finish())
        .map_err(|_| AuthError::Message("Could not encode the login code.".into()))
        .map(|compressed| pack_png(image.width, image.height, &compressed))
}

fn pack_png(width: u32, height: u32, idat: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    #[test]
    fn png_roundtrip_keeps_the_modules_and_the_accent_mark() {
        let url = "https://discord.com/ra/UZ0-kOVzXDZTFVV5_QlpURSO2BQHrtkKWHNpIGoDI0k";
        let image = render_qr_image(url).unwrap();
        assert!(image.width > 100);
        assert_eq!(
            image.pixels.len(),
            (image.width * image.height * 4) as usize
        );
        assert_eq!(&image.pixels[0..4], &PAPER);
        let accent_near_center = (0..image.width)
            .flat_map(|x| (0..image.height).map(move |y| (x, y)))
            .any(|(x, y)| {
                let dx = x as i32 - image.width as i32 / 2;
                let dy = y as i32 - image.height as i32 / 2;
                dx.abs() < 40 && dy.abs() < 40 && pixel(&image, x, y) == ACCENT
            });
        assert!(accent_near_center, "the center tile should use the accent");

        let png = encode_png(&image).unwrap();
        assert_eq!(&png[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let (width, height, inflated) = inflate_png(&png);
        assert_eq!((width, height), (image.width, image.height));
        let mut expected = Vec::new();
        for y in 0..image.height {
            expected.push(0);
            let start = (y * image.width * 4) as usize;
            expected.extend_from_slice(&image.pixels[start..start + (image.width * 4) as usize]);
        }
        assert_eq!(inflated, expected);

        let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(
            image.width as usize,
            image.height as usize,
            |x, y| image.pixels[(y * image.width as usize + x) * 4],
        );
        let grids = prepared.detect_grids();
        assert!(
            !grids.is_empty(),
            "the code with the center mark did not scan"
        );
        let (_meta, text) = grids[0].decode().expect("decode");
        assert_eq!(text, url);
    }

    fn pixel(image: &QrImage, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * image.width + x) * 4) as usize;
        image.pixels[index..index + 4].try_into().unwrap()
    }

    fn inflate_png(png: &[u8]) -> (u32, u32, Vec<u8>) {
        assert_eq!(&png[12..16], b"IHDR");
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let data = &png[41..41 + idat_len];
        let mut decoder = ZlibDecoder::new(data);
        let mut inflated = Vec::new();
        decoder.read_to_end(&mut inflated).unwrap();
        (width, height, inflated)
    }
}
