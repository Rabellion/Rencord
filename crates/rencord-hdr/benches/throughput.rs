//! Tone-map and metadata throughput. `cargo bench -p rencord-hdr`.

use std::hint::black_box;
use std::time::Instant;

use rencord_hdr::{
    av1_hdr_metadata_obus, dispatch, hevc_hdr10_sei, CpuToneMapper, Hdr10StaticMetadata, ToneCurve,
    ToneMapParams, ToneMapper,
};

fn main() {
    let pixels = vec![[180.0f32, 40.0, 12.0]; 1920 * 1080];
    let rounds = 8u32;
    for curve in [ToneCurve::Bt2390, ToneCurve::Hable, ToneCurve::Aces] {
        let mapper = CpuToneMapper::new(ToneMapParams::hdr10_to_sdr100(curve));
        let mut output = vec![[0.0f32; 3]; pixels.len()];
        mapper.map_into(&pixels, &mut output);
        let started = Instant::now();
        let mut acc = 0.0f32;
        for _ in 0..rounds {
            mapper.map_into(&pixels, &mut output);
            acc += output[0][0] + output[pixels.len() / 2][1];
        }
        black_box(acc);
        let seconds = started.elapsed().as_secs_f64();
        let frames = f64::from(rounds);
        let mpix = pixels.len() as f64 * frames / 1_000_000.0;
        println!(
            "{:<8} 1080p x{rounds}: {:>7.2} ms/frame, {:>7.1} Mpix/s",
            curve.label(),
            seconds * 1000.0 / frames,
            mpix / seconds
        );
    }

    let metadata = Hdr10StaticMetadata::bt2020_1000();
    let started = Instant::now();
    let mut acc = 0usize;
    for _ in 0..10_000 {
        let sei = hevc_hdr10_sei(&metadata);
        let obu = av1_hdr_metadata_obus(&metadata);
        acc += sei.len() + obu.len();
    }
    black_box(acc);
    let seconds = started.elapsed().as_secs_f64();
    println!(
        "metadata x10000: {:.2} µs each (SEI+OBU)",
        seconds * 1_000_000.0 / 10_000.0
    );

    let params = ToneMapParams::hdr10_to_sdr100(ToneCurve::Bt2390);
    for (label, count) in [("1080p", 1920 * 1080), ("4K", 3840 * 2160)] {
        let frame = vec![[220.0f32, 80.0, 30.0]; count];
        match dispatch(&params, &frame) {
            Ok(first) => {
                let second = dispatch(&params, &frame).expect("second gpu frame");
                println!(
                    "gpu BT.2390 {label} {} {} software={}: {:.2} ms (submit+readback)",
                    first.report.backend,
                    first.report.name,
                    first.report.software,
                    second.elapsed.as_secs_f64() * 1000.0
                );
            }
            Err(err) => println!("gpu BT.2390 {label}: {err}"),
        }
    }
}
