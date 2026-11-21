//! Capture sources.
//!
//! The synthetic source always runs. It is what tests, `--bench`, and the
//! source picker use when a platform capturer cannot open a desktop session.
//! Platform modules report whether the real backend is present; they do not
//! pretend a missing portal or a missing GPU library succeeded.

use crate::frame::{encode_png, Frame, HdrFlags};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Screen,
    Window,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureBackend {
    /// Windows Graphics Capture, with DXGI duplication as the fallback.
    WindowsGraphicsCapture,
    /// PipeWire fed by an xdg-desktop-portal ScreenCast session.
    PipeWirePortal,
    /// ScreenCaptureKit.
    ScreenCaptureKit,
    /// In-process pattern. Used when the platform capturer is unavailable.
    Synthetic,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CaptureSource {
    pub id: String,
    pub kind: SourceKind,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub backend: CaptureBackend,
    /// Small PNG thumbnail for the picker.
    pub thumbnail_png: Vec<u8>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CaptureError {
    #[error("unknown capture source {0}")]
    UnknownSource(String),
    #[error("{backend} is not available: {detail}")]
    Unavailable { backend: String, detail: String },
}

pub trait FrameSource {
    fn next_frame(&mut self) -> Frame;
}

pub struct SyntheticSource {
    id: String,
    width: u32,
    height: u32,
    sequence: u64,
    started: Instant,
    hdr: HdrFlags,
}

impl SyntheticSource {
    pub fn new(id: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            id: id.into(),
            width: width.max(2),
            height: height.max(2),
            sequence: 0,
            started: Instant::now(),
            hdr: HdrFlags::default(),
        }
    }

    pub fn with_hdr(mut self, hdr: HdrFlags) -> Self {
        self.hdr = hdr;
        self
    }
}

impl FrameSource for SyntheticSource {
    fn next_frame(&mut self) -> Frame {
        self.sequence += 1;
        let mut frame = Frame::new(self.width, self.height, self.sequence);
        frame.captured_at_us = self.started.elapsed().as_micros() as u64;
        frame.hdr = self.hdr;
        paint_preview(
            &mut frame.bgra,
            self.width,
            self.height,
            self.sequence,
            &self.id,
        );
        frame
    }
}

pub fn synthetic_sources() -> Vec<CaptureSource> {
    vec![
        preview_source(
            "synthetic:desktop",
            SourceKind::Screen,
            "Desktop (preview)",
            1920,
            1080,
            CaptureBackend::Synthetic,
        ),
        preview_source(
            "synthetic:window",
            SourceKind::Window,
            "Rencord (preview)",
            1280,
            720,
            CaptureBackend::Synthetic,
        ),
    ]
}

pub fn open_source(id: &str) -> Result<SyntheticSource, CaptureError> {
    let source = list_local_sources()
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| CaptureError::UnknownSource(id.to_owned()))?;
    if source.backend != CaptureBackend::Synthetic {
        return match crate::capture_os::open_platform_source(id) {
            Err(err) => Err(err),
            Ok(()) => Err(CaptureError::Unavailable {
                backend: format!("{:?}", source.backend),
                detail: "the platform capturer enumerated a source but did not copy a frame".into(),
            }),
        };
    }
    Ok(SyntheticSource::new(
        id,
        source.width.min(1280),
        source.height.min(720),
    ))
}

pub(crate) fn preview_source(
    id: &str,
    kind: SourceKind,
    title: &str,
    width: u32,
    height: u32,
    backend: CaptureBackend,
) -> CaptureSource {
    let mut pixels = vec![0u8; 320 * 180 * 4];
    paint_desktop_still(&mut pixels, 320, 180);
    CaptureSource {
        id: id.to_owned(),
        kind,
        title: title.to_owned(),
        width,
        height,
        backend,
        thumbnail_png: encode_png(&pixels, 320, 180),
    }
}

/// A still of a desktop window for the picker and the stream viewer.
/// Live frames stay on [`paint_preview`]; this is not that bar pattern.
pub(crate) fn paint_desktop_still(bgra: &mut [u8], width: u32, height: u32) {
    fill_rect(bgra, width, 0, 0, width, height, (22, 24, 32));
    let x0 = width / 12;
    let y0 = height / 10;
    let x1 = width - width / 12;
    let y1 = height - height / 12;
    fill_rect(bgra, width, x0, y0, x1, y1, (14, 16, 24));
    fill_rect(bgra, width, x0, y0, x0 + 4, y1, (79, 107, 255));
    fill_rect(bgra, width, x0, y0, x1, y0 + height / 12, (32, 36, 52));
    let lines = [0.72_f32, 0.55, 0.64, 0.38];
    for (index, portion) in lines.iter().enumerate() {
        let y = y0 + height / 7 + index as u32 * (height / 14);
        let line_h = (height / 28).max(3);
        let line_w = ((x1 - x0 - 24) as f32 * portion) as u32;
        fill_rect(
            bgra,
            width,
            x0 + 16,
            y,
            x0 + 16 + line_w,
            y + line_h,
            (168, 174, 196),
        );
    }
}

fn fill_rect(bgra: &mut [u8], stride: u32, x0: u32, y0: u32, x1: u32, y1: u32, rgb: (u8, u8, u8)) {
    let height = bgra.len() as u32 / (stride * 4).max(1);
    for y in y0..y1.min(height) {
        for x in x0..x1.min(stride) {
            let i = ((y * stride + x) * 4) as usize;
            if i + 3 >= bgra.len() {
                return;
            }
            bgra[i] = rgb.2;
            bgra[i + 1] = rgb.1;
            bgra[i + 2] = rgb.0;
            bgra[i + 3] = 255;
        }
    }
}

pub(crate) fn paint_preview(bgra: &mut [u8], width: u32, height: u32, sequence: u64, label: &str) {
    let shift = (sequence as u32).wrapping_mul(3);
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            if i + 3 >= bgra.len() {
                return;
            }
            let bar = ((x + shift) / 12) % 2 == 0;
            let (b, g, r) = if bar { (255, 107, 79) } else { (16, 16, 10) };
            bgra[i] = b;
            bgra[i + 1] = g;
            bgra[i + 2] = r;
            bgra[i + 3] = 255;
        }
    }
    let _ = label;
}

/// Sources the picker can show without spawning the worker.
pub fn list_local_sources() -> Vec<CaptureSource> {
    let mut sources = synthetic_sources();
    if let Some(portal) = crate::platform::portal_source() {
        sources.insert(0, portal);
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_frames_change_and_open_by_id() {
        let mut source = open_source("synthetic:desktop").unwrap();
        let first = source.next_frame();
        let second = source.next_frame();
        assert_ne!(first.bgra, second.bgra);
        assert_eq!(second.sequence, 2);
        assert!(first.pixel_bytes() == first.bgra.len());
        assert!(matches!(
            open_source("missing"),
            Err(CaptureError::UnknownSource(_))
        ));
    }

    #[test]
    fn thumbnails_are_pngs() {
        let source = synthetic_sources().remove(0);
        assert_eq!(&source.thumbnail_png[..8], b"\x89PNG\r\n\x1a\n");
    }
}
