//! ffmpeg argv and encoder selection.
//!
//! Rencord does not link or vendor FFmpeg or x264. When the user asks to
//! compress a video, the worker may spawn an `ffmpeg` binary that is already
//! on `PATH`. The arguments are built here and covered by tests without
//! running the binary.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncoderChoice {
    pub name: String,
    pub hardware: bool,
    pub two_pass: bool,
}

impl EncoderChoice {
    pub fn software_fallback() -> Self {
        Self {
            name: "libopenh264".into(),
            hardware: false,
            two_pass: true,
        }
    }
}

/// Prefer a hardware encoder. libx264 is only used when it is the only H.264
/// encoder the user's ffmpeg reports. It is not linked into this binary.
pub fn select_encoder(ffmpeg_encoders_text: &str) -> EncoderChoice {
    let order = [
        ("h264_nvenc", true, true),
        ("hevc_nvenc", true, true),
        ("h264_qsv", true, true),
        ("hevc_qsv", true, true),
        ("h264_amf", true, false),
        ("h264_videotoolbox", true, false),
        ("h264_vaapi", true, false),
        ("h264_mf", true, false),
        ("libopenh264", false, true),
        ("libsvtav1", false, true),
        ("libaom-av1", false, true),
        ("libx264", false, true),
    ];
    for (name, hardware, two_pass) in order {
        if encoder_listed(ffmpeg_encoders_text, name) {
            return EncoderChoice {
                name: name.to_owned(),
                hardware,
                two_pass,
            };
        }
    }
    EncoderChoice::software_fallback()
}

fn encoder_listed(text: &str, name: &str) -> bool {
    text.split_whitespace().any(|token| token == name)
}

pub fn probe_encoders(ffmpeg: &Path) -> Option<String> {
    let output = Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("ffmpeg");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq)]
pub struct FfmpegPlan {
    pub encoder: EncoderChoice,
    pub width: u32,
    pub height: u32,
    pub video_kbps: u32,
    pub audio_kbps: u32,
    pub scale: bool,
}

pub fn ffmpeg_commands(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
    plan: &FfmpegPlan,
    passlog: &Path,
) -> Vec<Vec<String>> {
    let bin = ffmpeg.display().to_string();
    let input = input.display().to_string();
    let output = output.display().to_string();
    let passlog = passlog.display().to_string();
    let rate = format!("{}k", plan.video_kbps);
    let audio = format!("{}k", plan.audio_kbps);
    let mut video = vec![
        "-c:v".into(),
        plan.encoder.name.clone(),
        "-b:v".into(),
        rate.clone(),
        "-maxrate".into(),
        rate,
        "-bufsize".into(),
        format!("{}k", plan.video_kbps.saturating_mul(2)),
    ];
    if plan.scale {
        video.push("-vf".into());
        video.push(format!("scale={}:{}", plan.width, plan.height));
    }
    if !plan.encoder.two_pass {
        let mut args = vec![bin, "-y".into(), "-i".into(), input];
        args.extend(video);
        args.extend([
            "-c:a".into(),
            "libopus".into(),
            "-b:a".into(),
            audio,
            output,
        ]);
        return vec![args];
    }
    let null = if cfg!(windows) {
        "NUL".to_owned()
    } else {
        "/dev/null".to_owned()
    };
    let mut first = vec![bin.clone(), "-y".into(), "-i".into(), input.clone()];
    first.extend(video.iter().cloned());
    first.extend([
        "-pass".into(),
        "1".into(),
        "-passlogfile".into(),
        passlog.clone(),
        "-an".into(),
        "-f".into(),
        "null".into(),
        null,
    ]);
    let mut second = vec![bin, "-y".into(), "-i".into(), input];
    second.extend(video);
    second.extend([
        "-pass".into(),
        "2".into(),
        "-passlogfile".into(),
        passlog,
        "-c:a".into(),
        "libopus".into(),
        "-b:a".into(),
        audio,
        output,
    ]);
    vec![first, second]
}

pub fn ffmpeg_still_avif(ffmpeg: &Path, input: &Path, output: &Path, quality: u8) -> Vec<String> {
    vec![
        ffmpeg.display().to_string(),
        "-y".into(),
        "-i".into(),
        input.display().to_string(),
        "-c:v".into(),
        "libaom-av1".into(),
        "-still-picture".into(),
        "1".into(),
        "-crf".into(),
        quality.to_string(),
        output.display().to_string(),
    ]
}

/// Seconds, scaled from 1080p. Hardware is faster than a software encoder.
pub fn estimate_encode_secs(
    width: u32,
    height: u32,
    duration_secs: f64,
    passes: u8,
    hardware: bool,
) -> f64 {
    let pixels = (width as f64) * (height as f64);
    let reference = 1920.0 * 1080.0;
    let speed = if hardware { 0.4 } else { 1.6 };
    let scale = (pixels / reference).max(0.05);
    (duration_secs * speed * scale * f64::from(passes.max(1))).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_encoder_wins_and_two_pass_is_explicit() {
        let listing = "\
 V..... libx264              libx264 H.264
 V..... h264_nvenc           NVIDIA NVENC H.264 encoder
 V..... h264_vaapi           H.264/AVC (VAAPI)
";
        let choice = select_encoder(listing);
        assert_eq!(choice.name, "h264_nvenc");
        assert!(choice.hardware);
        assert!(choice.two_pass);
        let vaapi = select_encoder("V..... h264_vaapi  VAAPI\n");
        assert_eq!(vaapi.name, "h264_vaapi");
        assert!(!vaapi.two_pass);
        let only_x264 = select_encoder("V..... libx264  x264\n");
        assert_eq!(only_x264.name, "libx264");
        assert!(!only_x264.hardware);
    }

    #[test]
    fn two_pass_commands_name_both_passes_and_opus_audio() {
        let plan = FfmpegPlan {
            encoder: EncoderChoice {
                name: "h264_nvenc".into(),
                hardware: true,
                two_pass: true,
            },
            width: 854,
            height: 480,
            video_kbps: 1700,
            audio_kbps: 96,
            scale: true,
        };
        let commands = ffmpeg_commands(
            Path::new("ffmpeg"),
            Path::new("in.mp4"),
            Path::new("out.mkv"),
            &plan,
            Path::new("passlog"),
        );
        assert_eq!(commands.len(), 2);
        assert!(commands[0].iter().any(|arg| arg == "-pass"));
        assert!(commands[0].iter().any(|arg| arg == "1"));
        assert!(commands[1].iter().any(|arg| arg == "2"));
        assert!(commands[1].iter().any(|arg| arg == "libopus"));
        assert!(commands[1].iter().any(|arg| arg == "scale=854:480"));
        assert!(!commands[0].iter().any(|arg| arg == "libx264"));
        let single = ffmpeg_commands(
            Path::new("ffmpeg"),
            Path::new("in.mp4"),
            Path::new("out.mkv"),
            &FfmpegPlan {
                encoder: EncoderChoice {
                    name: "h264_vaapi".into(),
                    hardware: true,
                    two_pass: false,
                },
                scale: false,
                ..plan
            },
            Path::new("passlog"),
        );
        assert_eq!(single.len(), 1);
        assert!(!single[0].iter().any(|arg| arg == "-pass"));
    }

    #[test]
    fn still_avif_command_uses_an_av1_encoder() {
        let args = ffmpeg_still_avif(
            Path::new("ffmpeg"),
            Path::new("in.png"),
            Path::new("out.avif"),
            40,
        );
        assert!(args.iter().any(|arg| arg == "libaom-av1"));
        assert!(args.iter().any(|arg| arg == "-still-picture"));
    }

    #[test]
    fn hardware_estimate_is_faster_than_software() {
        let hardware = estimate_encode_secs(1920, 1080, 30.0, 2, true);
        let software = estimate_encode_secs(1920, 1080, 30.0, 2, false);
        assert!(hardware < software);
        assert!(hardware >= 1.0);
    }
}
