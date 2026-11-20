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
}
