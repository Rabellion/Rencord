//! Capture-to-encode timing for the software path CI can run.
//!
//! GPU usage is reported as unused because the active encoder is the software
//! fallback. Hardware encoders are only listed by [`crate::probe_encoders`].

use crate::capture::{FrameSource, SyntheticSource};
use crate::encode::{active_encoder, encode_software};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamBench {
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub capture_us_avg: u64,
    pub encode_us_avg: u64,
    pub encoder: &'static str,
    pub gpu: &'static str,
}

pub fn bench_software(frames: u32) -> StreamBench {
    let frames = frames.clamp(1, 120);
    let mut source = SyntheticSource::new("bench", 320, 180);
    let mut capture_us = 0u128;
    let mut encode_us = 0u128;
    for _ in 0..frames {
        let started = Instant::now();
        let frame = source.next_frame();
        capture_us += started.elapsed().as_micros();
        let encode_started = Instant::now();
        let _unit = encode_software(&frame, 2_500);
        encode_us += encode_started.elapsed().as_micros();
    }
    let count = u128::from(frames);
    StreamBench {
        frames,
        width: 320,
        height: 180,
        capture_us_avg: (capture_us / count) as u64,
        encode_us_avg: (encode_us / count) as u64,
        encoder: active_encoder().label(),
        gpu: "unused (software encoder)",
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetBench {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub encode_us: u64,
    pub budget_us: u64,
    pub sustains: bool,
    pub encoder: &'static str,
    pub gpu: &'static str,
}

/// One software frame at each resolution the milestone asked about.
/// Hardware encode is not claimed: the GPU path is whatever [`crate::usable_encoder`] opened.
pub fn bench_preset_matrix() -> Vec<PresetBench> {
    let targets = [
        (1920, 1080, 60),
        (2560, 1440, 120),
        (3840, 2160, 60),
        (3840, 2160, 120),
    ];
    let encoder = crate::usable_encoder();
    let gpu = if encoder == crate::encode::EncoderKind::Software {
        "no GPU session"
    } else {
        encoder.label()
    };
    targets
        .into_iter()
        .map(|(width, height, fps)| {
            let mut source = SyntheticSource::new("preset", width, height);
            let frame = source.next_frame();
            let started = Instant::now();
            let _unit = encode_software(&frame, 6_000);
            let encode_us = started.elapsed().as_micros() as u64;
            let budget_us = 1_000_000 / u64::from(fps);
            PresetBench {
                width,
                height,
                fps,
                encode_us,
                budget_us,
                sustains: encode_us <= budget_us,
                encoder: "software",
                gpu,
            }
        })
        .collect()
}

pub fn format_preset_matrix(rows: &[PresetBench]) -> String {
    rows.iter()
        .map(|row| {
            format!(
                "PRESET {}x{}@{} encode_us={} budget_us={} sustains={} encoder={} gpu={}",
                row.width,
                row.height,
                row.fps,
                row.encode_us,
                row.budget_us,
                row.sustains,
                row.encoder,
                row.gpu
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_bench(bench: &StreamBench) -> String {
    format!(
        "STREAM frames={} size={}x{} capture_us={} encode_us={} encoder={} gpu={}",
        bench.frames,
        bench.width,
        bench.height,
        bench.capture_us_avg,
        bench.encode_us_avg,
        bench.encoder,
        bench.gpu
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_software_bench_records_both_stages() {
        let bench = bench_software(4);
        assert_eq!(bench.encoder, "software");
        assert!(bench.capture_us_avg < 50_000);
        assert!(bench.encode_us_avg < 50_000);
        assert!(format_bench(&bench).contains("encode_us="));
    }

    #[test]
    fn the_preset_matrix_covers_the_four_targets() {
        let rows = bench_preset_matrix();
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row.encode_us > 0));
        assert_eq!(rows[0].width, 1920);
        assert_eq!(rows[3].fps, 120);
    }
}
