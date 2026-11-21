//! In-process image and WAV-to-Opus encoders.
//!
//! JPEG uses the pure-Rust `image` encoder. WebP uses libwebp (BSD) for a
//! quality setting. AVIF is not linked: the plan estimates it, and ffmpeg
//! writes it when an AV1 encoder is on PATH. Video stays in ffmpeg.

use crate::plan::{ImageFormat, ImagePlan, MediaKind, MediaProbe, QualityPreference};
use crate::UploadError;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageReader};
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedImage {
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    pub quality: u8,
}

pub fn encode_image_to_fit(
    input: &DynamicImage,
    limit: u64,
    preference: QualityPreference,
) -> Result<EncodedImage, UploadError> {
    let plan = ImagePlan::from_image(input, limit, preference);
    let formats = [plan.encode_format, ImageFormat::Webp, ImageFormat::Jpeg];
    let mut width = plan.width.max(1);
    let mut height = plan.height.max(1);
    let mut quality = plan.quality.clamp(20, 95);
    let mut last = None;
    for _ in 0..16 {
        for format in formats {
            if format == ImageFormat::Avif {
                continue;
            }
            let image = resize(input, width, height);
            let bytes = encode_one(&image, format, quality)?;
            last = Some(EncodedImage {
                bytes: bytes.clone(),
                format,
                width,
                height,
                quality,
            });
            if (bytes.len() as u64) <= limit {
                return Ok(last.unwrap());
            }
        }
        if quality > 30 {
            quality = quality.saturating_sub(10);
            continue;
        }
        if width <= 32 || height <= 32 {
            break;
        }
        width = (width * 3 / 4).max(16);
        height = (height * 3 / 4).max(16);
        quality = preference_quality(preference);
    }
    Err(UploadError::message(format!(
        "could not fit the image into {limit} bytes (last try {} bytes)",
        last.map(|item| item.bytes.len()).unwrap_or(0)
    )))
}

impl ImagePlan {
    fn from_image(input: &DynamicImage, limit: u64, preference: QualityPreference) -> Self {
        crate::plan::plan_image(input.width(), input.height(), limit, preference)
    }
}

fn preference_quality(preference: QualityPreference) -> u8 {
    match preference {
        QualityPreference::Smaller => 45,
        QualityPreference::Balanced => 70,
        QualityPreference::Sharper => 85,
    }
}

fn resize(input: &DynamicImage, width: u32, height: u32) -> DynamicImage {
    if input.width() == width && input.height() == height {
        input.clone()
    } else {
        input.resize(width, height, image::imageops::FilterType::Triangle)
    }
}

fn encode_one(
    image: &DynamicImage,
    format: ImageFormat,
    quality: u8,
) -> Result<Vec<u8>, UploadError> {
    match format {
        ImageFormat::Jpeg => {
            let mut cursor = Cursor::new(Vec::new());
            let mut encoder = JpegEncoder::new_with_quality(&mut cursor, quality.clamp(1, 100));
            encoder.encode_image(image)?;
            Ok(cursor.into_inner())
        }
        ImageFormat::Webp => {
            let rgb = image.to_rgb8();
            let encoded = webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height())
                .encode(f32::from(quality.clamp(1, 100)));
            Ok(encoded.to_vec())
        }
        ImageFormat::Avif => Err(UploadError::message(
            "AVIF is written by ffmpeg when an AV1 encoder is installed",
        )),
    }
}

pub fn probe_path(path: &Path) -> Result<(MediaKind, MediaProbe, u64), UploadError> {
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let kind = MediaKind::from_name(name);
    let probe = match kind {
        MediaKind::Image => image_probe(path)?,
        MediaKind::Audio => wav_probe(path).unwrap_or_default(),
        MediaKind::Video => mp4_probe(path).unwrap_or_default(),
        MediaKind::Other => MediaProbe::default(),
    };
    Ok((kind, probe, size))
}

fn image_probe(path: &Path) -> Result<MediaProbe, UploadError> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let (width, height) = reader.into_dimensions()?;
    Ok(MediaProbe {
        width: Some(width),
        height: Some(height),
        ..MediaProbe::default()
    })
}

pub fn mp4_probe(path: &Path) -> Option<MediaProbe> {
    let mut file = File::open(path).ok()?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).ok()?;
    probe_mp4_bytes(&data)
}

pub fn probe_mp4_bytes(data: &[u8]) -> Option<MediaProbe> {
    let index = data.windows(4).position(|window| window == b"mvhd")?;
    let body = data.get(index + 4..)?;
    let version = *body.first()?;
    let (timescale, duration) = if version == 0 {
        let timescale = u32::from_be_bytes(body.get(12..16)?.try_into().ok()?);
        let duration = u32::from_be_bytes(body.get(16..20)?.try_into().ok()?);
        (u64::from(timescale), u64::from(duration))
    } else {
        let timescale = u32::from_be_bytes(body.get(20..24)?.try_into().ok()?);
        let duration = u64::from_be_bytes(body.get(24..32)?.try_into().ok()?);
        (u64::from(timescale), duration)
    };
    if timescale == 0 {
        return None;
    }
    Some(MediaProbe {
        duration_ms: Some(duration.saturating_mul(1000) / timescale),
        ..MediaProbe::default()
    })
}

pub fn wav_probe(path: &Path) -> Option<MediaProbe> {
    let mut file = File::open(path).ok()?;
    let mut header = [0u8; 44];
    file.read_exact(&mut header).ok()?;
    probe_wav_header(&header)
}

pub fn probe_wav_header(header: &[u8]) -> Option<MediaProbe> {
    if header.len() < 44 || &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return None;
    }
    let channels = u16::from_le_bytes(header[22..24].try_into().ok()?) as u64;
    let rate = u32::from_le_bytes(header[24..28].try_into().ok()?) as u64;
    let bits = u16::from_le_bytes(header[34..36].try_into().ok()?) as u64;
    let data_bytes = u32::from_le_bytes(header[40..44].try_into().ok()?) as u64;
    if channels == 0 || rate == 0 || bits == 0 {
        return None;
    }
    let frame = channels * bits / 8;
    if frame == 0 {
        return None;
    }
    Some(MediaProbe {
        duration_ms: Some(data_bytes.saturating_mul(1000) / (rate * frame)),
        ..MediaProbe::default()
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpusEncode {
    pub packets: Vec<Vec<u8>>,
    pub bitrate_kbps: u32,
    pub payload_bytes: u64,
}

/// Encode 16-bit little-endian PCM WAV at 48 kHz to Opus packets.
/// Other rates and containers are left to ffmpeg.
pub fn encode_wav_opus(wav: &[u8], limit: u64) -> Result<OpusEncode, UploadError> {
    let probe = probe_wav_header(wav).ok_or_else(|| UploadError::message("not a WAV file"))?;
    let duration = probe.duration_ms.unwrap_or(0);
    let plan = crate::plan::plan_opus(duration, limit);
    if !plan.fits {
        return Err(UploadError::message(
            "Opus cannot fit this audio under the cap",
        ));
    }
    let header = wav
        .get(..44)
        .ok_or_else(|| UploadError::message("WAV header is short"))?;
    let channels = u16::from_le_bytes(header[22..24].try_into().unwrap_or([0, 0]));
    let rate = u32::from_le_bytes(header[24..28].try_into().unwrap_or([0, 0, 0, 0]));
    let bits = u16::from_le_bytes(header[34..36].try_into().unwrap_or([0, 0]));
    if rate != 48_000 || bits != 16 || !(1..=2).contains(&channels) {
        return Err(UploadError::message(
            "in-process Opus encodes 48 kHz 16-bit WAV. Other audio needs ffmpeg.",
        ));
    }
    let pcm = pcm_i16(&wav[44..]);
    let frame_samples = 960 * channels as usize;
    let channel_enum = if channels == 1 {
        audiopus::Channels::Mono
    } else {
        audiopus::Channels::Stereo
    };
    let mut encoder = audiopus::coder::Encoder::new(
        audiopus::SampleRate::Hz48000,
        channel_enum,
        audiopus::Application::Audio,
    )?;
    encoder.set_bitrate(audiopus::Bitrate::BitsPerSecond(
        plan.bitrate_kbps as i32 * 1000,
    ))?;
    let mut packets = Vec::new();
    let mut payload_bytes = 0u64;
    let mut offset = 0usize;
    while offset + frame_samples <= pcm.len() {
        let mut packet = vec![0u8; 4000];
        let written = encoder.encode(&pcm[offset..offset + frame_samples], &mut packet)?;
        packet.truncate(written);
        payload_bytes += written as u64;
        packets.push(packet);
        offset += frame_samples;
    }
    if payload_bytes > limit {
        return Err(UploadError::message(format!(
            "opus payload is {payload_bytes} bytes, over the {limit} byte cap"
        )));
    }
    Ok(OpusEncode {
        packets,
        bitrate_kbps: plan.bitrate_kbps,
        payload_bytes,
    })
}

fn pcm_i16(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect()
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), UploadError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::QualityPreference;
    use image::{ImageBuffer, Rgb};

    #[test]
    fn jpeg_and_webp_land_under_a_tight_limit() {
        let mut image: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(160, 90);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = Rgb([(x * 3) as u8, (y * 5) as u8, ((x * y) % 255) as u8]);
        }
        let dynamic = DynamicImage::ImageRgb8(image);
        let encoded = encode_image_to_fit(&dynamic, 8_000, QualityPreference::Smaller).unwrap();
        assert!(
            encoded.bytes.len() as u64 <= 8_000,
            "{}",
            encoded.bytes.len()
        );
        assert!(matches!(
            encoded.format,
            ImageFormat::Jpeg | ImageFormat::Webp
        ));
    }

    #[test]
    fn mp4_mvhd_and_wav_header_report_duration() {
        let mut mvhd = vec![0u8, 0, 0, 0];
        mvhd.extend_from_slice(&[0u8; 8]);
        mvhd.extend_from_slice(&1000u32.to_be_bytes());
        mvhd.extend_from_slice(&2500u32.to_be_bytes());
        mvhd.extend_from_slice(&[0u8; 8]);
        let mut data = Vec::new();
        data.extend(b"xxxx");
        data.extend(b"mvhd");
        data.extend(&mvhd);
        let probe = probe_mp4_bytes(&data).unwrap();
        assert_eq!(probe.duration_ms, Some(2500));

        let mut wav = vec![0u8; 44];
        wav[0..4].copy_from_slice(b"RIFF");
        wav[8..12].copy_from_slice(b"WAVE");
        wav[22..24].copy_from_slice(&1u16.to_le_bytes());
        wav[24..28].copy_from_slice(&48_000u32.to_le_bytes());
        wav[34..36].copy_from_slice(&16u16.to_le_bytes());
        wav[40..44].copy_from_slice(&(48_000u32 * 2).to_le_bytes());
        let audio = probe_wav_header(&wav).unwrap();
        assert_eq!(audio.duration_ms, Some(1000));
    }

    #[test]
    fn wav_opus_payload_fits() {
        let samples = 48_000u32;
        let data_bytes = samples * 2;
        let mut wav = vec![0u8; 44 + data_bytes as usize];
        wav[0..4].copy_from_slice(b"RIFF");
        wav[8..12].copy_from_slice(b"WAVE");
        wav[22..24].copy_from_slice(&1u16.to_le_bytes());
        wav[24..28].copy_from_slice(&48_000u32.to_le_bytes());
        wav[34..36].copy_from_slice(&16u16.to_le_bytes());
        wav[40..44].copy_from_slice(&data_bytes.to_le_bytes());
        for (index, sample) in wav[44..].chunks_exact_mut(2).enumerate() {
            let value = ((index as i16).wrapping_mul(13)) / 8;
            sample.copy_from_slice(&value.to_le_bytes());
        }
        let encoded = encode_wav_opus(&wav, 32 * 1024).unwrap();
        assert!(
            encoded.payload_bytes <= 32 * 1024,
            "{}",
            encoded.payload_bytes
        );
        assert!(!encoded.packets.is_empty());
        assert!(encoded.bitrate_kbps <= 128);
    }
}
