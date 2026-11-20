//! Go Live capture and the out-of-process stream worker.
//!
//! The desktop process does not capture pixels. It spawns `rencord-stream`,
//! talks to it with length-prefixed JSON, and reads frames from a shared
//! memory map. Presets are not hidden behind a Nitro check. Discord can still
//! refuse a quality on the server; [`safe_preset`] is the default request.

mod capture;
mod frame;
mod ipc;
mod platform;
mod preset;
mod shm;
mod worker;

pub use capture::{
    list_local_sources, open_source, synthetic_sources, CaptureBackend, CaptureError,
    CaptureSource, FrameSource, SourceKind, SyntheticSource,
};
pub use frame::{encode_png, Frame, HdrFlags};
pub use ipc::{ClientMessage, WorkerMessage};
pub use platform::{
    capture_backends, connect_session_bus, probe_linux_portal, BackendSpec, LinuxPortalProbe,
};
pub use preset::{
    clamp_bitrate, fps_available, nitro_gate_in_ui, safe_preset, select_pixel_path, stream_presets,
    PixelPath, StreamPreset, ViewerKind,
};
pub use shm::FrameMap;
pub use worker::{worker_main, WorkerClient};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WorkerCommand {
    Start {
        preset: StreamPreset,
        pixel_path: PixelPath,
        source_id: String,
    },
    Stop,
    SetBitrate {
        kbps: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkerEvent {
    Started,
    Stopped,
    FrameStats { fps: u32, bitrate_kbps: u32 },
    Failed { message: String },
}

/// Capture does not run inside the UI process. The stream worker is a
/// separate binary.
pub fn stream_worker_running() -> bool {
    false
}

/// Pull one preview frame. This does not open a portal or a GPU capturer.
pub fn start_capture() -> Result<Frame, CaptureError> {
    let mut source = open_source("synthetic:desktop")?;
    Ok(source.next_frame())
}

pub fn worker_executable() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("RENCORD_STREAM_BIN") {
        return std::path::PathBuf::from(path);
    }
    let mut path =
        std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("rencord-stream"));
    path.pop();
    let name = if cfg!(windows) {
        "rencord-stream.exe"
    } else {
        "rencord-stream"
    };
    path.push(name);
    path
}
