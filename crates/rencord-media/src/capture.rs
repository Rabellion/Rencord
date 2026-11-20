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
        return Err(CaptureError::Unavailable {
            backend: format!("{:?}", source.backend),
            detail: crate::platform::backend_detail(source.backend),
        });
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
    let mut pixels = vec![0u8; 160 * 90 * 4];
    paint_preview(&mut pixels, 160, 90, 3, title);
    CaptureSource {
        id: id.to_owned(),
        kind,
        title: title.to_owned(),
        width,
        height,
        backend,
        thumbnail_png: encode_png(&pixels, 160, 90),
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
            let (b, g, r) = if bar { (40, 70, 210) } else { (12, 12, 18) };
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
