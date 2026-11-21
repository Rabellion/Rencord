//! Target-size plans. These functions do not encode or open files.

use crate::ffmpeg::EncoderChoice;
use crate::limit::{format_bytes, UploadLimit};
use crate::store::UploadSettings;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Image,
    Audio,
    Other,
}

impl MediaKind {
    pub fn from_name(name: &str) -> Self {
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => Self::Video,
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "avif" | "bmp" => Self::Image,
            "wav" | "mp3" | "flac" | "ogg" | "opus" | "m4a" | "aac" => Self::Audio,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityPreference {
    Smaller,
    Balanced,
    Sharper,
}

impl QualityPreference {
    pub fn label(self) -> &'static str {
        match self {
            Self::Smaller => "Smaller",
            Self::Balanced => "Balanced",
            Self::Sharper => "Sharper",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Smaller => Self::Balanced,
            Self::Balanced => Self::Sharper,
            Self::Sharper => Self::Smaller,
        }
    }

    fn min_bpp(self) -> f64 {
        match self {
            Self::Smaller => 0.03,
            Self::Balanced => 0.07,
            Self::Sharper => 0.12,
        }
    }

    fn image_quality(self) -> u8 {
        match self {
            Self::Smaller => 50,
            Self::Balanced => 75,
            Self::Sharper => 90,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedFile {
    pub name: String,
    pub size: u64,
    pub kind: MediaKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaProbe {
    pub duration_ms: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoSource {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
}

/// Video bitrate that spends the cap on video after audio and a 3% container reserve.
/// Returns kilobits per second. `None` when the duration is zero.
pub fn video_bitrate_kbps(limit_bytes: u64, duration_ms: u64, audio_kbps: u32) -> Option<u32> {
    if duration_ms == 0 || limit_bytes == 0 {
        return None;
    }
    let budget_bits = limit_bytes.saturating_mul(8).saturating_mul(100) / 103;
    let audio_bits = u64::from(audio_kbps).saturating_mul(duration_ms);
    let video_bits = budget_bits.saturating_sub(audio_bits);
    let kbps = video_bits / duration_ms;
    u32::try_from(kbps).ok()
}

/// Container-inclusive size estimate for a CBR video of this length.
pub fn estimated_video_bytes(video_kbps: u32, audio_kbps: u32, duration_ms: u64) -> u64 {
    let bits = u64::from(video_kbps.saturating_add(audio_kbps)).saturating_mul(duration_ms);
    bits.saturating_mul(103) / 100 / 8
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoPlan {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_kbps: u32,
    pub audio_kbps: u32,
    pub estimated_bytes: u64,
    pub bits_per_pixel: f64,
    pub passes: u8,
    pub encoder_name: String,
    pub hardware: bool,
    pub estimated_secs: f64,
    pub quality_label: &'static str,
    pub fits: bool,
}

const LADDER: &[(u32, u32)] = &[
    (3840, 2160),
    (2560, 1440),
    (1920, 1080),
    (1280, 720),
    (854, 480),
    (640, 360),
    (426, 240),
];

pub fn plan_video(
    source: VideoSource,
    limit_bytes: u64,
    quality: QualityPreference,
    encoder: &EncoderChoice,
) -> VideoPlan {
    let fps = source.fps.max(1);
    let mut audio_kbps = 96u32;
    let mut video_kbps =
        video_bitrate_kbps(limit_bytes, source.duration_ms, audio_kbps).unwrap_or(0);
    if video_kbps < 200 {
        audio_kbps = 32;
        video_kbps = video_bitrate_kbps(limit_bytes, source.duration_ms, audio_kbps).unwrap_or(0);
    }
    let (width, height) = pick_resolution(
        source.width.max(2),
        source.height.max(2),
        fps,
        video_kbps,
        quality,
    );
    let bpp = bits_per_pixel(video_kbps, width, height, fps);
    let estimated_bytes = estimated_video_bytes(video_kbps, audio_kbps, source.duration_ms);
    let passes = if encoder.two_pass { 2 } else { 1 };
    let estimated_secs = crate::ffmpeg::estimate_encode_secs(
        width,
        height,
        source.duration_ms as f64 / 1000.0,
        passes,
        encoder.hardware,
    );
    VideoPlan {
        width,
        height,
        fps,
        video_kbps,
        audio_kbps,
        estimated_bytes,
        bits_per_pixel: bpp,
        passes,
        encoder_name: encoder.name.clone(),
        hardware: encoder.hardware,
        estimated_secs,
        quality_label: quality_label(bpp),
        fits: video_kbps >= 50 && estimated_bytes > 0 && estimated_bytes <= limit_bytes,
    }
}

fn pick_resolution(
    src_w: u32,
    src_h: u32,
    fps: u32,
    video_kbps: u32,
    quality: QualityPreference,
) -> (u32, u32) {
    let min_bpp = quality.min_bpp();
    if bits_per_pixel(video_kbps, src_w, src_h, fps) >= min_bpp {
        return even(src_w, src_h);
    }
    let mut chosen = even(src_w.clamp(2, 426), src_h.clamp(2, 240));
    for &(box_w, box_h) in LADDER {
        let (width, height) = fit_box(src_w, src_h, box_w, box_h);
        if bits_per_pixel(video_kbps, width, height, fps) >= min_bpp {
            return (width, height);
        }
        chosen = (width, height);
    }
    chosen
}

fn fit_box(src_w: u32, src_h: u32, box_w: u32, box_h: u32) -> (u32, u32) {
    let scale = (box_w as f64 / src_w as f64)
        .min(box_h as f64 / src_h as f64)
        .min(1.0);
    even(
        ((src_w as f64) * scale).round().max(2.0) as u32,
        ((src_h as f64) * scale).round().max(2.0) as u32,
    )
}

fn even(width: u32, height: u32) -> (u32, u32) {
    (width.max(2) & !1, height.max(2) & !1)
}

pub fn bits_per_pixel(video_kbps: u32, width: u32, height: u32, fps: u32) -> f64 {
    let pixels = (width as f64) * (height as f64) * (fps.max(1) as f64);
    if pixels <= 0.0 {
        return 0.0;
    }
    (video_kbps as f64) * 1000.0 / pixels
}

pub fn quality_label(bits_per_pixel: f64) -> &'static str {
    if bits_per_pixel >= 0.15 {
        "sharp"
    } else if bits_per_pixel >= 0.08 {
        "balanced"
    } else if bits_per_pixel >= 0.04 {
        "soft"
    } else {
        "small and soft"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Webp,
    Avif,
}

impl ImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Webp => "WebP",
            Self::Avif => "AVIF",
        }
    }

    fn weight(self) -> u64 {
        match self {
            Self::Jpeg => 10,
            Self::Webp => 7,
            Self::Avif => 4,
        }
    }
}

/// Rough byte estimate. Higher quality and more pixels estimate larger.
pub fn estimate_image_bytes(width: u32, height: u32, format: ImageFormat, quality: u8) -> u64 {
    let pixels = u64::from(width) * u64::from(height);
    let quality = u64::from(quality.clamp(1, 100));
    let factor = format.weight() + quality;
    pixels.saturating_mul(factor) / 200 + 256
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImagePlan {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub quality: u8,
    pub estimated_bytes: u64,
    pub fits: bool,
    /// What this build will actually write. AVIF is an estimate unless ffmpeg is used later.
    pub encode_format: ImageFormat,
}

pub fn plan_image(
    width: u32,
    height: u32,
    limit_bytes: u64,
    quality: QualityPreference,
) -> ImagePlan {
    let start_quality = quality.image_quality();
    let preferred = match quality {
        QualityPreference::Sharper => ImageFormat::Jpeg,
        QualityPreference::Balanced => ImageFormat::Webp,
        QualityPreference::Smaller => ImageFormat::Avif,
    };
    let encode_format = match preferred {
        ImageFormat::Avif => ImageFormat::Webp,
        other => other,
    };
    let mut width = width.max(1);
    let mut height = height.max(1);
    let mut quality_value = start_quality;
    let mut format = preferred;
    for _ in 0..48 {
        let estimated = estimate_image_bytes(width, height, format, quality_value);
        if estimated <= limit_bytes {
            return ImagePlan {
                width,
                height,
                format,
                quality: quality_value,
                estimated_bytes: estimated,
                fits: true,
                encode_format: if format == ImageFormat::Avif {
                    ImageFormat::Webp
                } else {
                    format
                },
            };
        }
        if quality_value > 28 {
            quality_value = quality_value.saturating_sub(12);
            continue;
        }
        if width > 48 && height > 48 {
            width = (width / 2).max(16);
            height = (height / 2).max(16);
            quality_value = start_quality.min(60);
            continue;
        }
        if format != ImageFormat::Avif {
            format = ImageFormat::Avif;
            quality_value = 40;
            continue;
        }
        break;
    }
    let estimated = estimate_image_bytes(width, height, format, quality_value);
    ImagePlan {
        width,
        height,
        format,
        quality: quality_value,
        estimated_bytes: estimated,
        fits: estimated <= limit_bytes,
        encode_format,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpusPlan {
    pub bitrate_kbps: u32,
    pub estimated_bytes: u64,
    pub fits: bool,
}

pub fn opus_estimate_bytes(duration_ms: u64, bitrate_kbps: u32) -> u64 {
    u64::from(bitrate_kbps).saturating_mul(duration_ms) / 8 + 1024
}

pub fn plan_opus(duration_ms: u64, limit_bytes: u64) -> OpusPlan {
    if duration_ms == 0 || limit_bytes <= 1024 {
        return OpusPlan {
            bitrate_kbps: 0,
            estimated_bytes: 0,
            fits: false,
        };
    }
    let mut kbps = ((limit_bytes - 1024) * 8 / duration_ms) as u32;
    if kbps > 128 {
        kbps = 128;
    }
    if kbps < 6 {
        let estimated = opus_estimate_bytes(duration_ms, 6);
        return OpusPlan {
            bitrate_kbps: 6,
            estimated_bytes: estimated,
            fits: estimated <= limit_bytes,
        };
    }
    let estimated = opus_estimate_bytes(duration_ms, kbps);
    OpusPlan {
        bitrate_kbps: kbps,
        estimated_bytes: estimated,
        fits: estimated <= limit_bytes,
    }
}

pub fn estimate_volumes(file_len: u64, limit_bytes: u64) -> u32 {
    let payload = limit_bytes.saturating_sub(crate::split::HEADER_LEN as u64);
    if payload == 0 {
        return 0;
    }
    let archive_guess = file_len.saturating_add(512);
    let volumes = archive_guess.div_ceil(payload);
    u32::try_from(volumes).unwrap_or(u32::MAX).max(1)
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompressOffer {
    pub available: bool,
    pub title: String,
    pub detail: String,
    pub estimated_bytes: u64,
    pub estimated_secs: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalOffer {
    pub title: String,
    pub detail: String,
    pub host: String,
    pub warning: String,
    pub expiry_label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitOffer {
    pub title: String,
    pub detail: String,
    pub volumes: u32,
    pub format_label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SheetOffer {
    pub headline: String,
    pub meta: String,
    pub fits: bool,
    pub server_note: String,
    pub compress: CompressOffer,
    pub external: ExternalOffer,
    pub split: SplitOffer,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Precheck {
    Fits {
        limit: UploadLimit,
        file: StagedFile,
    },
    Over {
        limit: UploadLimit,
        offer: Box<SheetOffer>,
    },
}

pub fn precheck(
    file: &StagedFile,
    probe: &MediaProbe,
    limit: UploadLimit,
    settings: &UploadSettings,
    encoder: &EncoderChoice,
) -> Precheck {
    if file.size <= limit.bytes {
        Precheck::Fits {
            limit,
            file: file.clone(),
        }
    } else {
        Precheck::Over {
            offer: Box::new(offer(file, probe, limit, settings, encoder)),
            limit,
        }
    }
}

pub fn offer(
    file: &StagedFile,
    probe: &MediaProbe,
    limit: UploadLimit,
    settings: &UploadSettings,
    encoder: &EncoderChoice,
) -> SheetOffer {
    let where_label = if limit.in_guild {
        "in this server"
    } else {
        "in DMs"
    };
    SheetOffer {
        headline: "This file is over the upload limit".to_owned(),
        meta: format!(
            "{}  ·  {}  ·  limit {}  ·  {} {}",
            file.name,
            format_bytes(file.size),
            format_bytes(limit.bytes),
            limit.label(),
            where_label
        ),
        fits: false,
        server_note: "Discord checks the size on the server. This client cannot raise the cap."
            .to_owned(),
        compress: compress_offer(file, probe, limit.bytes, settings.quality, encoder),
        external: external_offer(settings),
        split: split_offer(file, limit.bytes, settings),
    }
}

fn compress_offer(
    file: &StagedFile,
    probe: &MediaProbe,
    limit_bytes: u64,
    quality: QualityPreference,
    encoder: &EncoderChoice,
) -> CompressOffer {
    match file.kind {
        MediaKind::Video => {
            let source = VideoSource {
                width: probe.width.unwrap_or(1920),
                height: probe.height.unwrap_or(1080),
                fps: probe.fps.unwrap_or(30),
                duration_ms: probe.duration_ms.unwrap_or(0),
            };
            if source.duration_ms == 0 {
                return CompressOffer {
                    available: false,
                    title: "Compress".to_owned(),
                    detail: "Duration is unknown, so a target bitrate cannot be planned. Split or upload a link."
                        .to_owned(),
                    estimated_bytes: 0,
                    estimated_secs: 0.0,
                };
            }
            let plan = plan_video(source, limit_bytes, quality, encoder);
            let pass = if plan.passes == 2 {
                "two-pass"
            } else {
                "one pass"
            };
            CompressOffer {
                available: plan.fits,
                title: "Compress".to_owned(),
                detail: format!(
                    "{}×{}  ·  {}  ·  ~{}  ·  ~{:.0} s  ·  {} {}",
                    plan.width,
                    plan.height,
                    plan.quality_label,
                    format_bytes(plan.estimated_bytes),
                    plan.estimated_secs,
                    pass,
                    plan.encoder_name
                ),
                estimated_bytes: plan.estimated_bytes,
                estimated_secs: plan.estimated_secs,
            }
        }
        MediaKind::Image => {
            let plan = plan_image(
                probe.width.unwrap_or(1920),
                probe.height.unwrap_or(1080),
                limit_bytes,
                quality,
            );
            let avif_note = if plan.format == ImageFormat::Avif {
                let encoder = crate::ffmpeg::ffmpeg_still_avif(
                    std::path::Path::new("ffmpeg"),
                    std::path::Path::new("in"),
                    std::path::Path::new("out.avif"),
                    plan.quality,
                )
                .into_iter()
                .find(|arg| arg.contains("av1"))
                .unwrap_or_else(|| "libaom-av1".into());
                format!(
                    "AVIF estimate {}. This build writes {} unless ffmpeg has an AV1 encoder ({encoder}).",
                    format_bytes(plan.estimated_bytes),
                    plan.encode_format.label(),
                )
            } else {
                format!(
                    "{} q{}  ·  {}×{}",
                    plan.encode_format.label(),
                    plan.quality,
                    plan.width,
                    plan.height
                )
            };
            CompressOffer {
                available: plan.fits,
                title: "Compress".to_owned(),
                detail: format!("{avif_note}  ·  ~{}", format_bytes(plan.estimated_bytes)),
                estimated_bytes: plan.estimated_bytes,
                estimated_secs: 2.0,
            }
        }
        MediaKind::Audio => {
            let duration = probe.duration_ms.unwrap_or(0);
            let plan = plan_opus(duration, limit_bytes);
            CompressOffer {
                available: plan.fits,
                title: "Compress".to_owned(),
                detail: if plan.fits {
                    format!(
                        "Opus {} kbps  ·  ~{}  ·  WAV encodes here, other audio uses ffmpeg",
                        plan.bitrate_kbps,
                        format_bytes(plan.estimated_bytes)
                    )
                } else {
                    "Even a 6 kbps Opus file would miss the cap.".to_owned()
                },
                estimated_bytes: plan.estimated_bytes,
                estimated_secs: 1.0,
            }
        }
        MediaKind::Other => CompressOffer {
            available: false,
            title: "Compress".to_owned(),
            detail: "No encoder for this file type. Split it, or upload a link.".to_owned(),
            estimated_bytes: 0,
            estimated_secs: 0.0,
        },
    }
}

fn external_offer(settings: &UploadSettings) -> ExternalOffer {
    let host = settings.provider.host().to_owned();
    let expiry = settings.expiry_label();
    ExternalOffer {
        title: "Upload a link".to_owned(),
        detail: format!("{host}  ·  {expiry}  ·  the original file leaves this machine"),
        host: host.clone(),
        warning: format!(
            "This uploads the original file to {host}, which is not Discord. That host can read the bytes. Rencord does not send them until you check the box and press Upload."
        ),
        expiry_label: expiry,
    }
}

fn split_offer(file: &StagedFile, limit_bytes: u64, settings: &UploadSettings) -> SplitOffer {
    let volumes = estimate_volumes(file.size, limit_bytes);
    let format_label = settings.archive.label();
    SplitOffer {
        title: "Split".to_owned(),
        detail: format!(
            "{volumes} {format_label} volumes under {}  ·  Rencord joins them in one click",
            format_bytes(limit_bytes)
        ),
        volumes,
        format_label: format_label.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg::EncoderChoice;
    use crate::limit::{LimitInput, FREE_LIMIT_BYTES};
    use crate::store::UploadSettings;

    fn encoder() -> EncoderChoice {
        EncoderChoice {
            name: "h264_nvenc".into(),
            hardware: true,
            two_pass: true,
        }
    }

    fn source_1080() -> VideoSource {
        VideoSource {
            width: 1920,
            height: 1080,
            fps: 30,
            duration_ms: 90_000,
        }
    }

    #[test]
    fn target_bitrate_stays_under_the_cap() {
        let limit = FREE_LIMIT_BYTES;
        let kbps = video_bitrate_kbps(limit, 90_000, 96).unwrap();
        let estimated = estimated_video_bytes(kbps, 96, 90_000);
        assert!(kbps > 500, "{kbps}");
        assert!(estimated <= limit, "{estimated} > {limit}");
        assert!(video_bitrate_kbps(limit, 0, 96).is_none());
    }

    #[test]
    fn quality_preference_changes_resolution_not_the_cap() {
        let limit = FREE_LIMIT_BYTES;
        let smaller = plan_video(source_1080(), limit, QualityPreference::Smaller, &encoder());
        let balanced = plan_video(
            source_1080(),
            limit,
            QualityPreference::Balanced,
            &encoder(),
        );
        let sharper = plan_video(source_1080(), limit, QualityPreference::Sharper, &encoder());
        for plan in [&smaller, &balanced, &sharper] {
            assert!(plan.fits, "{plan:?}");
            assert!(plan.estimated_bytes <= limit);
            assert_eq!(plan.width % 2, 0);
            assert_eq!(plan.height % 2, 0);
            assert!(plan.width <= 1920 && plan.height <= 1080);
            assert_eq!(plan.passes, 2);
        }
        let smaller_pixels = u64::from(smaller.width) * u64::from(smaller.height);
        let balanced_pixels = u64::from(balanced.width) * u64::from(balanced.height);
        let sharper_pixels = u64::from(sharper.width) * u64::from(sharper.height);
        assert!(smaller_pixels >= balanced_pixels);
        assert!(balanced_pixels >= sharper_pixels);
        assert!(sharper_pixels < 1920 * 1080);
    }

    #[test]
    fn image_estimates_are_monotonic_and_the_plan_fits() {
        let low = estimate_image_bytes(1280, 720, ImageFormat::Jpeg, 40);
        let high = estimate_image_bytes(1280, 720, ImageFormat::Jpeg, 90);
        let big = estimate_image_bytes(1920, 1080, ImageFormat::Jpeg, 90);
        let avif = estimate_image_bytes(1280, 720, ImageFormat::Avif, 90);
        assert!(low < high);
        assert!(high < big);
        assert!(avif < high);
        let plan = plan_image(4000, 3000, 200 * 1024, QualityPreference::Balanced);
        assert!(plan.fits, "{plan:?}");
        assert!(plan.estimated_bytes <= 200 * 1024);
    }

    #[test]
    fn opus_plan_fits_under_the_cap() {
        let plan = plan_opus(180_000, 200 * 1024);
        assert!(plan.fits, "{plan:?}");
        assert!(plan.estimated_bytes <= 200 * 1024);
        assert!(plan.bitrate_kbps <= 128);
        assert!(!plan_opus(0, 200 * 1024).fits);
    }

    #[test]
    fn precheck_opens_the_sheet_only_when_the_file_is_over() {
        let limit = crate::limit::detect_limit(LimitInput::free_dm());
        let settings = UploadSettings::default();
        let small = StagedFile {
            name: "note.png".into(),
            size: 40_000,
            kind: MediaKind::Image,
        };
        assert!(matches!(
            precheck(&small, &MediaProbe::default(), limit, &settings, &encoder()),
            Precheck::Fits { .. }
        ));
        let big = StagedFile {
            name: "lecture.mp4".into(),
            size: 48_000_000,
            kind: MediaKind::Video,
        };
        let probe = MediaProbe {
            duration_ms: Some(90_000),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30),
        };
        match precheck(&big, &probe, limit, &settings, &encoder()) {
            Precheck::Over { offer, .. } => {
                assert!(offer.compress.available, "{}", offer.compress.detail);
                assert!(offer.compress.estimated_bytes <= limit.bytes);
                assert!(offer.split.volumes >= 2);
                assert!(offer.external.warning.contains("does not send"));
                assert!(offer.server_note.contains("server"));
            }
            Precheck::Fits { .. } => panic!("48 MB is over the free cap"),
        }
    }
}
