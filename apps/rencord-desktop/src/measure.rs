//! Offline timings plus an optional `--bench` launch of the window.
//!
//! `cargo run -p rencord-desktop --bin rencord-measure -- --offline` skips the
//! window. Without that flag it also starts `rencord --demo --bench` when the
//! sibling binary is next to this one.

use rencord_desktop::Shell;
use std::process::Command;
use std::time::Instant;

fn main() {
    let offline = std::env::args().any(|arg| arg == "--offline");
    let mut shell = Shell::demo_history(1500);
    let started = Instant::now();
    for _ in 0..200 {
        shell.step_channel(1);
    }
    let switch_ns = started.elapsed().as_nanos() / 200;
    println!("MEASURE cached_channel_switch_ns={switch_ns}");
    println!("MEASURE last_switch_us={}", shell.last_switch_us);
    println!("MEASURE visible={}", shell.visible_messages().len());
    let slice = Instant::now();
    for _ in 0..60 {
        let messages = shell.visible_messages();
        let _window = messages.iter().rev().take(30).count();
    }
    println!(
        "MEASURE scroll_slice_us={}",
        slice.elapsed().as_micros() / 60
    );

    if offline {
        return;
    }
    let mut binary = std::env::current_exe().unwrap_or_default();
    binary.pop();
    binary.push("rencord");
    if !binary.exists() {
        println!("MEASURE window_bench=skipped");
        return;
    }
    let output = Command::new(&binary)
        .args(["--demo", "--bench", "--theme", "dark"])
        .output();
    match output {
        Ok(output) => {
            print!("{}", String::from_utf8_lossy(&output.stdout));
            let err = String::from_utf8_lossy(&output.stderr);
            if !err.trim().is_empty() {
                eprintln!("{err}");
            }
        }
        Err(err) => println!("MEASURE window_bench_error={err}"),
    }
}
