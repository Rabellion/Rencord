//! Media types for later milestones.
//!
//! Milestone 1 does not capture, encode, or send a stream. The types here are
//! the contract the UI and a future out-of-process worker will share:
//! quality presets are not hidden behind a Nitro check, HDR capture is
//! tone-mapped to SDR unless both ends are Rencord and an SFU experiment has
//! succeeded, and the worker protocol is explicit about living in another
//! process.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamPreset {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Target video bitrate. Audio is separate.
    pub bitrate_kbps: u32,
}

/// Presets the UI may offer. Nothing in this list is removed for Nitro.
pub fn stream_presets() -> &'static [StreamPreset] {
    &[
        StreamPreset {
            width: 854,
            height: 480,
            fps: 30,
            bitrate_kbps: 2_000,
        },
        StreamPreset {
            width: 1280,
            height: 720,
            fps: 30,
            bitrate_kbps: 4_000,
        },
        StreamPreset {
            width: 1280,
            height: 720,
            fps: 60,
            bitrate_kbps: 8_000,
        },
        StreamPreset {
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate_kbps: 12_000,
        },
        StreamPreset {
            width: 2560,
            height: 1440,
            fps: 60,
            bitrate_kbps: 16_000,
        },
        StreamPreset {
            width: 3840,
            height: 2160,
            fps: 60,
            bitrate_kbps: 16_000,
        },
        StreamPreset {
            width: 3840,
            height: 2160,
            fps: 120,
            bitrate_kbps: 16_000,
        },
    ]
}

pub fn nitro_gate_in_ui() -> bool {
    false
}

/// 120 fps is only honest when the local display mode reports at least 120 Hz.
pub fn fps_available(preset: StreamPreset, display_hz: Option<u32>) -> bool {
    match display_hz {
        Some(hz) => preset.fps <= hz,
        None => preset.fps <= 60,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerKind {
    /// Official client, mobile, or any peer that did not advertise Rencord HDR.
    UnknownOrOfficial,
    Rencord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelPath {
    /// Capture is already SDR.
    Sdr,
    /// HDR frames are tone-mapped to BT.709 8-bit before encode.
    ToneMapHdrToSdr,
    /// 10-bit path. Only after a measured SFU passthrough test.
    RencordHdr10,
}

pub fn select_pixel_path(
    capture_is_hdr: bool,
    viewer: ViewerKind,
    sfu_hdr_passthrough_proven: bool,
) -> PixelPath {
    if !capture_is_hdr {
        return PixelPath::Sdr;
    }
    if viewer == ViewerKind::Rencord && sfu_hdr_passthrough_proven {
        PixelPath::RencordHdr10
    } else {
        PixelPath::ToneMapHdrToSdr
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkerCommand {
    Start {
        preset: StreamPreset,
        pixel_path: PixelPath,
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

/// Milestone 1 has the protocol and no worker process.
pub fn stream_worker_running() -> bool {
    false
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CaptureError {
    #[error("screen capture is not implemented in Milestone 1")]
    NotImplemented,
}

pub fn start_capture() -> Result<(), CaptureError> {
    Err(CaptureError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_include_4k120_without_a_nitro_flag() {
        assert!(!nitro_gate_in_ui());
        let top = stream_presets()
            .iter()
            .find(|preset| preset.height == 2160 && preset.fps == 120);
        assert!(top.is_some());
    }

    #[test]
    fn unknown_refresh_does_not_advertise_120() {
        let preset = stream_presets()
            .iter()
            .copied()
            .find(|preset| preset.fps == 120)
            .unwrap();
        assert!(!fps_available(preset, None));
        assert!(fps_available(preset, Some(120)));
        assert!(fps_available(
            StreamPreset {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate_kbps: 4_000
            },
            None
        ));
    }

    #[test]
    fn hdr_falls_back_to_tone_map_unless_rencord_and_proven() {
        assert_eq!(
            select_pixel_path(true, ViewerKind::UnknownOrOfficial, true),
            PixelPath::ToneMapHdrToSdr
        );
        assert_eq!(
            select_pixel_path(true, ViewerKind::Rencord, false),
            PixelPath::ToneMapHdrToSdr
        );
        assert_eq!(
            select_pixel_path(true, ViewerKind::Rencord, true),
            PixelPath::RencordHdr10
        );
        assert_eq!(
            select_pixel_path(false, ViewerKind::Rencord, true),
            PixelPath::Sdr
        );
    }

    #[test]
    fn worker_is_not_running() {
        assert!(!stream_worker_running());
        assert_eq!(start_capture(), Err(CaptureError::NotImplemented));
        let _ = WorkerCommand::Stop;
        let _ = WorkerEvent::Stopped;
    }
}
