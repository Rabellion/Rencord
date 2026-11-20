//! Quality presets. The list is not filtered by Nitro.
//!
//! Discord can still reject a request or enforce an account limit on the
//! server. [`safe_preset`] is what the client offers until the user asks for
//! more. 120 fps is only offered when the display mode reports that refresh.

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StreamPreset {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Target video bitrate. Audio stays on the voice connection.
    pub bitrate_kbps: u32,
}

/// Presets the UI may offer. Nothing here is removed for Nitro.
pub fn stream_presets() -> &'static [StreamPreset] {
    &[
        StreamPreset {
            width: 854,
            height: 480,
            fps: 30,
            bitrate_kbps: 1_500,
        },
        StreamPreset {
            width: 1280,
            height: 720,
            fps: 30,
            bitrate_kbps: 2_500,
        },
        StreamPreset {
            width: 1280,
            height: 720,
            fps: 60,
            bitrate_kbps: 4_500,
        },
        StreamPreset {
            width: 1920,
            height: 1080,
            fps: 30,
            bitrate_kbps: 4_000,
        },
        StreamPreset {
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate_kbps: 6_000,
        },
        StreamPreset {
            width: 1920,
            height: 1080,
            fps: 120,
            bitrate_kbps: 9_000,
        },
        StreamPreset {
            width: 2560,
            height: 1440,
            fps: 30,
            bitrate_kbps: 6_000,
        },
        StreamPreset {
            width: 2560,
            height: 1440,
            fps: 60,
            bitrate_kbps: 10_000,
        },
        StreamPreset {
            width: 2560,
            height: 1440,
            fps: 120,
            bitrate_kbps: 14_000,
        },
        StreamPreset {
            width: 3840,
            height: 2160,
            fps: 30,
            bitrate_kbps: 12_000,
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
            bitrate_kbps: 20_000,
        },
    ]
}

pub fn nitro_gate_in_ui() -> bool {
    false
}

/// 720p30. Inside the quality Discord documents for accounts without Nitro.
pub fn safe_preset() -> StreamPreset {
    stream_presets()[1]
}

/// 120 fps is only honest when the local display mode reports at least 120 Hz.
pub fn fps_available(preset: StreamPreset, display_hz: Option<u32>) -> bool {
    match display_hz {
        Some(hz) => preset.fps <= hz,
        None => preset.fps <= 60,
    }
}

/// Clamp a requested bitrate to the range this preset will actually send.
pub fn clamp_bitrate(preset: StreamPreset, requested_kbps: u32) -> u32 {
    let min = (preset.bitrate_kbps / 4).max(200);
    let max = preset.bitrate_kbps.saturating_mul(2).max(min);
    requested_kbps.clamp(min, max)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerKind {
    /// Official client, mobile, or any peer that did not advertise Rencord HDR.
    UnknownOrOfficial,
    Rencord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_include_4k120_and_the_1080_1440_steps() {
        assert!(!nitro_gate_in_ui());
        let has = |h, fps| {
            stream_presets()
                .iter()
                .any(|preset| preset.height == h && preset.fps == fps)
        };
        assert!(has(2160, 120));
        assert!(has(1080, 30) && has(1080, 60) && has(1080, 120));
        assert!(has(1440, 30) && has(1440, 60) && has(1440, 120));
        assert_eq!(safe_preset().height, 720);
        assert_eq!(safe_preset().fps, 30);
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
        assert!(fps_available(safe_preset(), None));
    }

    #[test]
    fn bitrate_stays_inside_the_preset_range() {
        let preset = safe_preset();
        assert_eq!(clamp_bitrate(preset, 2_500), 2_500);
        assert!(clamp_bitrate(preset, 1) >= 200);
        assert!(clamp_bitrate(preset, 1_000_000) <= preset.bitrate_kbps * 2);
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
}
