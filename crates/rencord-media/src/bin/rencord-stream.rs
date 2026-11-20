//! Out-of-process Go Live worker.
//!
//! The desktop app starts this binary and reads `RENCORD_STREAM <port> <frame-map>`
//! from stdout. Capture stays here so a stall cannot freeze the window or the
//! voice connection.

fn main() {
    std::process::exit(rencord_media::worker_main());
}
