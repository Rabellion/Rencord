//! The stream worker loop.
//!
//! `rencord-stream` runs this in its own process. The desktop process only
//! sends [`ClientMessage`]s and reads frames from the shared map, so an
//! encoder stall cannot freeze the window or the voice thread.

use crate::capture::{open_source, FrameSource};
use crate::ipc::{read_message, write_message, ClientMessage, WorkerMessage};
use crate::shm::FrameMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

struct CaptureRun {
    stop: Arc<AtomicBool>,
    frames: Arc<AtomicU64>,
    last_capture_us: Arc<AtomicU64>,
    width: u32,
    height: u32,
    sequence: Arc<AtomicU64>,
    join: Option<JoinHandle<()>>,
}

pub fn worker_main() -> i32 {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("rencord-stream bind failed: {err}");
            return 1;
        }
    };
    let port = match listener.local_addr() {
        Ok(addr) => addr.port(),
        Err(err) => {
            eprintln!("rencord-stream address failed: {err}");
            return 1;
        }
    };
    let shm_path = std::env::temp_dir().join(format!(
        "rencord-stream-{}-{port}.frame",
        std::process::id()
    ));
    let map = match FrameMap::create(&shm_path, 1280 * 720) {
        Ok(map) => map,
        Err(err) => {
            eprintln!("rencord-stream frame map failed: {err}");
            return 1;
        }
    };
    println!("RENCORD_STREAM {port} {}", shm_path.display());
    let _ = std::io::stdout().flush();
    match listener.accept() {
        Ok((stream, _)) => {
            if let Err(err) = serve(stream, map) {
                eprintln!("rencord-stream ended: {err}");
                return 1;
            }
        }
        Err(err) => {
            eprintln!("rencord-stream accept failed: {err}");
            return 1;
        }
    }
    let _ = std::fs::remove_file(&shm_path);
    0
}

pub fn serve(mut stream: TcpStream, mut frames: FrameMap) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    let mut running: Option<CaptureRun> = None;
    loop {
        let message: ClientMessage = match read_message(&mut stream) {
            Ok(message) => message,
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(err) => return Err(err),
        };
        let reply = match message {
            ClientMessage::ListSources => WorkerMessage::Sources {
                sources: crate::list_local_sources(),
            },
            ClientMessage::Start {
                source_id,
                width,
                height,
                fps,
                bitrate_kbps: _,
            } => match open_source(&source_id) {
                Ok(source) => {
                    stop_run(running.take());
                    running = Some(spawn_capture(source, width, height, fps, &mut frames));
                    WorkerMessage::Started
                }
                Err(err) => WorkerMessage::Failed {
                    message: err.to_string(),
                },
            },
            ClientMessage::Stop => {
                stop_run(running.take());
                WorkerMessage::Stopped
            }
            ClientMessage::Stats => stats_message(running.as_ref()),
            ClientMessage::Quit => {
                stop_run(running.take());
                let _ = write_message(&mut stream, &WorkerMessage::Stopped);
                break;
            }
        };
        write_message(&mut stream, &reply)?;
    }
    stop_run(running);
    Ok(())
}

fn spawn_capture(
    mut source: crate::capture::SyntheticSource,
    width: u32,
    height: u32,
    fps: u32,
    frames: &mut FrameMap,
) -> CaptureRun {
    let stop = Arc::new(AtomicBool::new(false));
    let count = Arc::new(AtomicU64::new(0));
    let last_capture_us = Arc::new(AtomicU64::new(0));
    let sequence = Arc::new(AtomicU64::new(0));
    let stop_flag = Arc::clone(&stop);
    let count_flag = Arc::clone(&count);
    let last_flag = Arc::clone(&last_capture_us);
    let sequence_flag = Arc::clone(&sequence);
    let path = frames.path().to_path_buf();
    let width = width.clamp(2, 1280);
    let height = height.clamp(2, 720);
    let join = thread::spawn(move || {
        let mut map = match FrameMap::open(&path) {
            Ok(map) => map,
            Err(_) => return,
        };
        let mut sized = crate::capture::SyntheticSource::new("live", width, height);
        let _ = source.next_frame();
        let frame_time = Duration::from_micros(1_000_000 / u64::from(fps.clamp(1, 120)));
        while !stop_flag.load(Ordering::Acquire) {
            let started = Instant::now();
            let mut frame = sized.next_frame();
            frame.captured_at_us = started.elapsed().as_micros() as u64;
            let elapsed = started.elapsed().as_micros() as u64;
            if map.write_frame(&frame).is_ok() {
                count_flag.fetch_add(1, Ordering::Release);
                last_flag.store(elapsed, Ordering::Release);
                sequence_flag.store(frame.sequence, Ordering::Release);
            }
            let spent = started.elapsed();
            if frame_time > spent {
                thread::sleep(frame_time - spent);
            }
        }
    });
    CaptureRun {
        stop,
        frames: count,
        last_capture_us,
        width,
        height,
        sequence,
        join: Some(join),
    }
}

fn stats_message(running: Option<&CaptureRun>) -> WorkerMessage {
    let Some(running) = running else {
        return WorkerMessage::Stats {
            frames: 0,
            last_capture_us: 0,
            width: 0,
            height: 0,
            sequence: 0,
        };
    };
    WorkerMessage::Stats {
        frames: running.frames.load(Ordering::Acquire),
        last_capture_us: running.last_capture_us.load(Ordering::Acquire),
        width: running.width,
        height: running.height,
        sequence: running.sequence.load(Ordering::Acquire),
    }
}

fn stop_run(running: Option<CaptureRun>) {
    if let Some(running) = running {
        running.stop.store(true, Ordering::Release);
        if let Some(join) = running.join {
            let _ = join.join();
        }
    }
}

pub struct WorkerClient {
    child: std::process::Child,
    stream: TcpStream,
    frames: FrameMap,
}

impl WorkerClient {
    pub fn spawn(bin: impl Into<PathBuf>) -> Result<Self, String> {
        let mut child = std::process::Command::new(bin.into())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|err| format!("spawn rencord-stream: {err}"))?;
        let stdout = child.stdout.take().ok_or("worker stdout missing")?;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        std::io::BufRead::read_line(&mut reader, &mut line)
            .map_err(|err| format!("read worker banner: {err}"))?;
        let mut parts = line.split_whitespace();
        if parts.next() != Some("RENCORD_STREAM") {
            return Err(format!("worker banner was {line:?}"));
        }
        let port: u16 = parts
            .next()
            .ok_or("worker banner missing port")?
            .parse()
            .map_err(|err| format!("worker port: {err}"))?;
        let shm = parts.next().ok_or("worker banner missing frame map")?;
        let stream = TcpStream::connect(("127.0.0.1", port))
            .map_err(|err| format!("connect worker: {err}"))?;
        stream
            .set_nodelay(true)
            .map_err(|err| format!("worker nodelay: {err}"))?;
        let frames = FrameMap::open(shm).map_err(|err| format!("open frame map: {err}"))?;
        Ok(Self {
            child,
            stream,
            frames,
        })
    }

    pub fn request(&mut self, message: &ClientMessage) -> Result<WorkerMessage, String> {
        write_message(&mut self.stream, message).map_err(|err| err.to_string())?;
        read_message(&mut self.stream).map_err(|err| err.to_string())
    }

    pub fn latest_frame(&self) -> Option<crate::frame::Frame> {
        self.frames.read_frame()
    }

    pub fn worker_pid(&self) -> u32 {
        self.child.id()
    }

    pub fn shutdown(mut self) -> Result<(), String> {
        let _ = self.request(&ClientMessage::Quit);
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for WorkerClient {
    fn drop(&mut self) {
        let _ = write_message(&mut self.stream, &ClientMessage::Quit);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn an_in_process_worker_publishes_frames() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let path = std::env::temp_dir().join(format!("rencord-worker-test-{port}.frame"));
        let map = FrameMap::create(&path, 64 * 64).unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _ = serve(stream, map);
        });
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write_message(
            &mut stream,
            &ClientMessage::Start {
                source_id: "synthetic:window".into(),
                width: 32,
                height: 18,
                fps: 60,
                bitrate_kbps: 2_500,
            },
        )
        .unwrap();
        let started: WorkerMessage = read_message(&mut stream).unwrap();
        assert_eq!(started, WorkerMessage::Started);
        let frames = FrameMap::open(&path).unwrap();
        let mut saw = None;
        for _ in 0..50 {
            if let Some(frame) = frames.read_frame() {
                saw = Some(frame);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let frame = saw.expect("frame");
        assert_eq!(frame.width, 32);
        assert!(frame.sequence >= 1);
        write_message(&mut stream, &ClientMessage::Quit).unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
