//! The shipping executable is also the stream worker.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn worker_check_names_this_binary() {
    let output = Command::new(env!("CARGO_BIN_EXE_rencord"))
        .arg("--worker-check")
        .output()
        .expect("spawn");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    // The version tracks Cargo.toml, so a version bump cannot break this test.
    let banner = format!("WORKER_CHECK {}", env!("CARGO_PKG_VERSION"));
    assert!(text.contains(&banner), "{text}");
    assert!(text.contains("STREAM_WORKER --stream-worker"), "{text}");
}

#[test]
fn stream_worker_mode_prints_the_banner_and_is_this_exe() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rencord"))
        .arg("--stream-worker")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn worker");
    let stdout = child.stdout.take().expect("stdout");
    let reader = BufReader::new(stdout);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut lines = reader.lines();
        let _ = tx.send(lines.next());
    });
    let line = rx
        .recv_timeout(Duration::from_secs(8))
        .expect("banner wait")
        .expect("banner line")
        .expect("banner utf8");
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        line.starts_with("RENCORD_STREAM "),
        "worker mode did not start: {line}"
    );
}
