//! Owner GPU pass: adapter name and BT.2390 times at 1080p and 4K.
//!
//! ```sh
//! cargo run --release -p rencord-hdr --example tonemap-bench
//! ```
//!
//! The GPU column is one submit and a device wait. It does not map the
//! storage buffer. That is the live path. A second line copies the 1080p
//! result back so the readback cost is visible on its own.

use std::time::Instant;

use rencord_hdr::{
    adapter_report, dispatch_resident, CpuToneMapper, GpuError, ToneCurve, ToneMapParams,
    ToneMapper,
};

fn main() {
    let params = ToneMapParams::hdr10_to_sdr100(ToneCurve::Bt2390);
    match adapter_report() {
        Ok(report) => println!(
            "adapter: {} | backend {} | {} | software={}",
            report.name, report.backend, report.device_type, report.software
        ),
        Err(err) => println!("adapter: none ({err})"),
    }

    for (label, width, height) in [("1080p", 1920u32, 1080u32), ("4K", 3840, 2160)] {
        let count = width as usize * height as usize;
        let frame = vec![[220.0f32, 80.0, 30.0]; count];
        let cpu_ms = cpu_ms(&params, &frame);
        match dispatch_resident(&params, width, height, &frame) {
            Ok(_) => {}
            Err(GpuError::NoAdapter) => {
                println!("{label}: GPU skipped (no adapter) | CPU BT.2390 {cpu_ms:.2} ms");
                continue;
            }
            Err(err) => {
                eprintln!("{label}: {err}");
                std::process::exit(1);
            }
        }
        let resident = dispatch_resident(&params, width, height, &frame)
            .unwrap_or_else(|err| panic!("{label}: {err}"));
        println!(
            "{label}: GPU BT.2390 {:.2} ms (no readback, {} bytes, {}) | CPU BT.2390 {cpu_ms:.2} ms",
            resident.elapsed.as_secs_f64() * 1000.0,
            resident.byte_size(),
            resident.format()
        );
        if label == "1080p" {
            let started = Instant::now();
            let pixels = resident
                .readback()
                .unwrap_or_else(|err| panic!("readback: {err}"));
            println!(
                "{label}: readback {:.2} ms for {} pixels (not used by the live encoder path)",
                started.elapsed().as_secs_f64() * 1000.0,
                pixels.len()
            );
        }
    }
}

fn cpu_ms(params: &ToneMapParams, frame: &[[f32; 3]]) -> f64 {
    let mapper = CpuToneMapper::new(*params);
    let mut output = vec![[0.0f32; 3]; frame.len()];
    mapper.map_into(frame, &mut output);
    let started = Instant::now();
    mapper.map_into(frame, &mut output);
    started.elapsed().as_secs_f64() * 1000.0
}
