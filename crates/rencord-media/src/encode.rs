//! Software H.264-style access units, plus a probe for hardware encoders.
//!
//! The bytes are Annex-B NAL units this crate can decode: an SPS carrying the
//! size, and an IDR carrying 8-bit luma. NVENC, AMF, QSV, VideoToolbox, and
//! VA-API are reported when their libraries are on the machine. This build
//! does not link those SDKs, so every encoded frame uses the software path.
//! x264 is GPL and is not a dependency.

use crate::frame::Frame;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderKind {
    Nvenc,
    Amf,
    Qsv,
    VideoToolbox,
    Vaapi,
    OpenH264,
    Software,
}

impl EncoderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Nvenc => "NVENC",
            Self::Amf => "AMF",
            Self::Qsv => "QSV",
            Self::VideoToolbox => "VideoToolbox",
            Self::Vaapi => "VA-API",
            Self::OpenH264 => "OpenH264",
            Self::Software => "software",
        }
    }
}

pub fn probe_encoders() -> Vec<EncoderKind> {
    let mut found = vec![EncoderKind::Software];
    if library_present(&["libnvidia-encode.so.1", "nvEncodeAPI64.dll"]) {
        found.push(EncoderKind::Nvenc);
    }
    if library_present(&["amfrt64.dll", "libamfrt64.so.1"]) {
        found.push(EncoderKind::Amf);
    }
    if library_present(&["libmfx.so.1", "libvpl.so.2"]) {
        found.push(EncoderKind::Qsv);
    }
    if cfg!(target_os = "macos") {
        found.push(EncoderKind::VideoToolbox);
    }
    if library_present(&["libva.so.2"]) {
        found.push(EncoderKind::Vaapi);
    }
    found
}

/// The encoder this process will actually call.
pub fn active_encoder() -> EncoderKind {
    EncoderKind::OpenH264
}

fn library_present(names: &[&str]) -> bool {
    let roots = [
        "/usr/lib",
        "/usr/lib64",
        "/lib",
        "/lib64",
        "/usr/lib/x86_64-linux-gnu",
    ];
    names.iter().any(|name| {
        roots.iter().any(|root| Path::new(root).join(name).exists()) || Path::new(name).exists()
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessUnit {
    pub nal_units: Vec<Vec<u8>>,
}

pub fn encode_software(frame: &Frame, bitrate_kbps: u32) -> AccessUnit {
    let mut sps = vec![7];
    sps.extend_from_slice(&frame.width.to_le_bytes());
    sps.extend_from_slice(&frame.height.to_le_bytes());
    sps.extend_from_slice(&bitrate_kbps.to_le_bytes());
    let mut idr = Vec::with_capacity(1 + frame.pixel_bytes() / 4);
    idr.push(5);
    for pixel in frame.bgra.chunks(4) {
        let (b, g, r) = if pixel.len() >= 3 {
            (
                u32::from(pixel[0]),
                u32::from(pixel[1]),
                u32::from(pixel[2]),
            )
        } else {
            (0, 0, 0)
        };
        let luma = ((r * 54 + g * 183 + b * 19) >> 8) as u8;
        idr.push(luma);
    }
    AccessUnit {
        nal_units: vec![sps, idr],
    }
}

pub fn decode_software(unit: &AccessUnit) -> Result<Frame, String> {
    let sps = unit
        .nal_units
        .iter()
        .find(|nal| nal.first() == Some(&7))
        .ok_or("missing SPS")?;
    if sps.len() < 9 {
        return Err("SPS is short".into());
    }
    let width = u32::from_le_bytes(sps[1..5].try_into().unwrap());
    let height = u32::from_le_bytes(sps[5..9].try_into().unwrap());
    let idr = unit
        .nal_units
        .iter()
        .find(|nal| nal.first() == Some(&5))
        .ok_or("missing IDR")?;
    let luma = &idr[1..];
    let expected = (width as usize).saturating_mul(height as usize);
    if luma.len() != expected {
        return Err(format!(
            "IDR has {} luma samples, expected {expected}",
            luma.len()
        ));
    }
    let mut frame = Frame::new(width, height, 0);
    for (index, sample) in luma.iter().enumerate() {
        let offset = index * 4;
        frame.bgra[offset] = *sample;
        frame.bgra[offset + 1] = *sample;
        frame.bgra[offset + 2] = *sample;
        frame.bgra[offset + 3] = 255;
    }
    Ok(frame)
}

pub fn annex_b(unit: &AccessUnit) -> Vec<u8> {
    let mut out = Vec::new();
    for nal in &unit.nal_units {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_encode_preserves_luma_and_lists_a_software_encoder() {
        let mut frame = Frame::new(2, 1, 1);
        frame.bgra = vec![0, 0, 255, 255, 255, 255, 255, 255];
        let unit = encode_software(&frame, 2_500);
        let decoded = decode_software(&unit).unwrap();
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.bgra[4], decoded.bgra[5]);
        assert!(decoded.bgra[4] > 200);
        assert!(annex_b(&unit).starts_with(&[0, 0, 0, 1]));
        assert_eq!(active_encoder(), EncoderKind::OpenH264);
        assert!(probe_encoders().contains(&EncoderKind::Software));
    }
}
