//! Gradient, grain, blur, and dim for theme backdrops.
//!
//! The results are PNG bytes so the UI can paint them without its own color math.

use crate::color::Color;
use image::{ImageBuffer, Rgba, RgbaImage};

pub fn gradient_png(stops: &[Color], angle_deg: f32, size: u32) -> Vec<u8> {
    let size = size.clamp(2, 512);
    let mut image: RgbaImage = ImageBuffer::new(size, size);
    if stops.is_empty() {
        return encode(&image);
    }
    let rad = angle_deg.to_radians();
    let dx = rad.cos();
    let dy = rad.sin();
    let mut min_p = f32::MAX;
    let mut max_p = f32::MIN;
    for corner in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let p = corner.0 * dx + corner.1 * dy;
        min_p = min_p.min(p);
        max_p = max_p.max(p);
    }
    let span = (max_p - min_p).max(0.0001);
    for y in 0..size {
        for x in 0..size {
            let px = x as f32 / (size - 1) as f32;
            let py = y as f32 / (size - 1) as f32;
            let t = ((px * dx + py * dy) - min_p) / span;
            let color = sample_stops(stops, t.clamp(0.0, 1.0));
            image.put_pixel(x, y, Rgba([color.r, color.g, color.b, color.a]));
        }
    }
    encode(&image)
}

pub fn grain_png(size: u32) -> Vec<u8> {
    let size = size.clamp(8, 256);
    let mut image: RgbaImage = ImageBuffer::new(size, size);
    for y in 0..size {
        for x in 0..size {
            let n = hash_pixel(x, y);
            let shade = if n % 2 == 0 { 255 } else { 0 };
            image.put_pixel(x, y, Rgba([shade, shade, shade, 255]));
        }
    }
    encode(&image)
}

pub fn prepare_image(bytes: &[u8], blur: f32, dim: f32, max_edge: u32) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let mut rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let edge = width.max(height);
    if edge > max_edge && max_edge > 0 {
        let scale = max_edge as f32 / edge as f32;
        let w = ((width as f32) * scale).round().max(1.0) as u32;
        let h = ((height as f32) * scale).round().max(1.0) as u32;
        rgba = image::imageops::resize(&rgba, w, h, image::imageops::FilterType::Triangle);
    }
    if blur >= 0.5 {
        box_blur(&mut rgba, blur.round() as i32);
    }
    if dim > 0.0 {
        let keep = 1.0 - dim.clamp(0.0, 1.0);
        for pixel in rgba.pixels_mut() {
            pixel[0] = (pixel[0] as f32 * keep).round() as u8;
            pixel[1] = (pixel[1] as f32 * keep).round() as u8;
            pixel[2] = (pixel[2] as f32 * keep).round() as u8;
        }
    }
    Some(encode(&rgba))
}

fn sample_stops(stops: &[Color], t: f32) -> Color {
    if stops.len() == 1 {
        return stops[0];
    }
    let scaled = t * (stops.len() - 1) as f32;
    let index = (scaled.floor() as usize).min(stops.len() - 1);
    let next = (index + 1).min(stops.len() - 1);
    let frac = scaled - index as f32;
    stops[index].lerp(stops[next], frac)
}

fn box_blur(image: &mut RgbaImage, radius: i32) {
    if radius <= 0 {
        return;
    }
    let (width, height) = image.dimensions();
    let source = image.clone();
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            for oy in -radius..=radius {
                for ox in -radius..=radius {
                    let sx = x as i32 + ox;
                    let sy = y as i32 + oy;
                    if sx >= 0 && sy >= 0 && (sx as u32) < width && (sy as u32) < height {
                        let pixel = source.get_pixel(sx as u32, sy as u32);
                        for channel in 0..4 {
                            sum[channel] += u32::from(pixel[channel]);
                        }
                        count += 1;
                    }
                }
            }
            if let Some(count) = Some(count).filter(|count| *count > 0) {
                let channel = |sum: u32| sum.checked_div(count).unwrap_or(0) as u8;
                image.put_pixel(
                    x,
                    y,
                    Rgba([
                        channel(sum[0]),
                        channel(sum[1]),
                        channel(sum[2]),
                        channel(sum[3]),
                    ]),
                );
            }
        }
    }
}

fn hash_pixel(x: u32, y: u32) -> u32 {
    let mut n = x
        .wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263));
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^ (n >> 16)
}

fn encode(image: &RgbaImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    let dynamic = image::DynamicImage::ImageRgba8(image.clone());
    let _ = dynamic.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    );
    bytes
}
