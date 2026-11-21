//! Go Live capture and the out-of-process stream worker.
//!
//! The desktop process does not capture pixels. It spawns `rencord-stream`,
//! talks to it with length-prefixed JSON, and reads frames from a shared
//! memory map. Presets are not hidden behind a Nitro check. Discord can still
//! refuse a quality on the server; [`safe_preset`] is the default request.

mod bench;
mod capture;
mod capture_os;
mod encode;
mod encoder;
mod frame;
mod golive;
mod hdr;
mod hdr_link;
mod hw;
mod ipc;
mod loopback;
mod platform;
mod portal;
mod preset;
mod rtp;
mod shm;
mod sync;
mod worker;

pub use bench::{
    bench_preset_matrix, bench_software, format_bench, format_preset_matrix, PresetBench,
    StreamBench,
};
pub use capture::{
    list_local_sources, open_source, synthetic_sources, CaptureBackend, CaptureError,
    CaptureSource, FrameSource, SourceKind, SyntheticSource,
};
pub use encode::{
    active_encoder, annex_b, decode_software, encode_software, probe_encoders, AccessUnit,
    EncoderKind,
};
pub use encoder::{
    EncodeError, EncodeFrame, EncodedBitstream, HardwareEncoder, OpenH264Encoder, VideoEncoder,
};
pub use frame::{encode_png, Frame, HdrFlags};
pub use golive::{connect_stream, GoLive, StreamPhase, StreamStats};
pub use hdr::{
    hdr_wire_enabled, hdr_wire_format, output_path, planned_pixel_path, tone_map_pq_to_sdr,
};
pub use hdr_link::{
    hdr10_encoder, prepare_frame, GpuFrameSink, Hdr10Encoder, PreparedFrame, ReferenceHdr10Encoder,
    SoftwareFrameSink, UnlinkedHdr10Encoder,
};
pub use hw::{probe_all, usable_encoder, EncoderProbe};
pub use ipc::{ClientMessage, WorkerMessage};
pub use loopback::loopback_frame;
pub use platform::{
    capture_backends, connect_session_bus, probe_linux_portal, BackendSpec, LinuxPortalProbe,
};
pub use portal::{pipewire_linked, request_screencast_blocking};
pub use preset::{
    clamp_bitrate, fps_available, nitro_gate_in_ui, safe_preset, select_pixel_path, stream_presets,
    PixelPath, StreamPreset, ViewerKind,
};
pub use rtp::{depacketize, packetize, write_rtp, RtpPacket};
pub use shm::FrameMap;
pub use sync::{correction_samples, drift_us, is_system_monitor};
pub use worker::{worker_main, WorkerClient};

/// Capture does not run inside the UI process. The window re-spawns itself with
/// this argument so a stall stays in the other process.
///
/// Hidden argument the shipping executable uses to enter [`worker_main`].
pub const STREAM_WORKER_ARG: &str = "--stream-worker";
