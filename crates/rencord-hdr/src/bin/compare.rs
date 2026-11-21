//! Side-by-side tone-map frame. Left is the scene clipped at 100 cd/m².
//! Right is BT.2390. Writes a PNG next to the design docs.

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use flate2::write::ZlibEncoder;
use flate2::Compression;
use rencord_hdr::{map_shader_formula, to_srgb8, ToneCurve, ToneMapParams};

fn main() {
    let width = 960u32;
    let height = 540u32;
    let params = ToneMapParams::hdr10_to_sdr100(ToneCurve::Bt2390);
    let mut rgb = Vec::with_capacity((width * 2 * height) as usize * 3);
    for y in 0..height {
        for x in 0..width * 2 {
            let local = x % width;
            let nits = (local as f32 / width as f32) * 1000.0;
            let tint = if y < height / 3 {
                [1.0, 0.35, 0.2]
            } else if y < 2 * height / 3 {
                [0.4, 1.0, 0.55]
            } else {
                [0.35, 0.45, 1.0]
            };
            let scene = [nits * tint[0], nits * tint[1], nits * tint[2]];
            let linear = if x < width {
                scene.map(|channel| (channel / 100.0).clamp(0.0, 1.0))
            } else {
                map_shader_formula(&params, scene)
            };
            let encoded = to_srgb8(linear);
            rgb.extend_from_slice(&encoded);
        }
    }
    let png = encode_png(&rgb, width * 2, height);
    let path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/design/onyx/hdr-tonemap.png"));
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, png).expect("write comparison png");
    println!("{}", path.display());
}

fn encode_png(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut raw = Vec::new();
    let stride = (width as usize) * 3;
    for row in 0..height as usize {
        raw.push(0);
        raw.extend_from_slice(&rgb[row * stride..(row + 1) * stride]);
    }
    let mut compressed = Vec::new();
    let mut encoder = ZlibEncoder::new(&mut compressed, Compression::fast());
    encoder.write_all(&raw).unwrap();
    encoder.finish().unwrap();
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    chunk(&mut png, b"IHDR", &{
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        header
    });
    chunk(&mut png, b"IDAT", &compressed);
    chunk(&mut png, b"IEND", &[]);
    png
}

fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let mut crc_bytes = kind.to_vec();
    crc_bytes.extend_from_slice(data);
    png.extend_from_slice(&crc32(&crc_bytes).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}
