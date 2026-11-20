//! Stable video encoder.
//!
//! `rencord-hdr` extends 10-bit by calling [`VideoEncoder::encode`] with
//! [`EncodeFrame::bit_depth`] set to 10 and PQ samples in [`EncodeFrame::luma10`].
//! Do not rename [`VideoEncoder`], [`EncodeFrame`], [`EncodedBitstream`], or
//! [`EncodeError`]. Hardware backends return [`EncodeError::NotLinked`] until a
//! device session exists. OpenH264 is 8-bit only and returns
//! [`EncodeError::Unsupported10Bit`].

use crate::encode::EncoderKind;
use crate::hw::probe_all;
use openh264::encoder::Encoder;
use openh264::formats::{RgbSliceU8, YUVBuffer, YUVSource};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    #[error("{backend:?} does not encode 10-bit")]
    Unsupported10Bit { backend: EncoderKind },
    #[error("{backend:?} is not linked: {detail}")]
    NotLinked {
        backend: EncoderKind,
        detail: String,
    },
    #[error("{0}")]
    Failed(String),
}

/// One picture. Eight-bit frames use BGRA. Ten-bit frames use `luma10`.
#[derive(Clone, Copy, Debug)]
pub struct EncodeFrame<'a> {
    pub width: u32,
    pub height: u32,
    /// `8` or `10`. Ten is the HDR extension point.
    pub bit_depth: u8,
    pub bgra: &'a [u8],
    /// Low 10 bits, one sample per pixel, when `bit_depth == 10`.
    pub luma10: Option<&'a [u16]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedBitstream {
    pub nal_units: Vec<Vec<u8>>,
}

pub trait VideoEncoder {
    fn backend(&self) -> EncoderKind;
    fn supports_10bit(&self) -> bool;
    fn encode(&mut self, frame: &EncodeFrame<'_>) -> Result<EncodedBitstream, EncodeError>;
}

pub struct OpenH264Encoder {
    inner: Encoder,
}

impl OpenH264Encoder {
    pub fn new() -> Result<Self, EncodeError> {
        let inner = Encoder::new().map_err(|err| EncodeError::Failed(err.to_string()))?;
        Ok(Self { inner })
    }
}

impl VideoEncoder for OpenH264Encoder {
    fn backend(&self) -> EncoderKind {
        EncoderKind::OpenH264
    }

    fn supports_10bit(&self) -> bool {
        false
    }

    fn encode(&mut self, frame: &EncodeFrame<'_>) -> Result<EncodedBitstream, EncodeError> {
        if frame.bit_depth != 8 {
            return Err(EncodeError::Unsupported10Bit {
                backend: EncoderKind::OpenH264,
            });
        }
        if frame.width % 2 != 0 || frame.height % 2 != 0 {
            return Err(EncodeError::Failed(
                "OpenH264 wants even width and height".into(),
            ));
        }
        let rgb = bgra_to_rgb(frame.bgra, frame.width, frame.height)?;
        let slice = RgbSliceU8::new(&rgb, (frame.width as usize, frame.height as usize));
        let yuv = YUVBuffer::from_rgb_source(slice);
        let stream = self
            .inner
            .encode(&yuv)
            .map_err(|err| EncodeError::Failed(err.to_string()))?;
        let mut nal_units = Vec::new();
        for layer_index in 0..stream.num_layers() {
            let Some(layer) = stream.layer(layer_index) else {
                continue;
            };
            for nal_index in 0..layer.nal_count() {
                if let Some(nal) = layer.nal_unit(nal_index) {
                    nal_units.push(nal.to_vec());
                }
            }
        }
        let _ = YUVSource::dimensions(&yuv);
        Ok(EncodedBitstream { nal_units })
    }
}

/// Hardware session. `encode` opens the driver and returns [`EncodeError::NotLinked`]
/// with the init status when no device can take a surface. Ten-bit is advertised
/// so `rencord-hdr` can call this and branch on that error.
pub struct HardwareEncoder {
    kind: EncoderKind,
}

impl HardwareEncoder {
    pub fn new(kind: EncoderKind) -> Self {
        Self { kind }
    }
}

impl VideoEncoder for HardwareEncoder {
    fn backend(&self) -> EncoderKind {
        self.kind
    }

    fn supports_10bit(&self) -> bool {
        !matches!(self.kind, EncoderKind::OpenH264 | EncoderKind::Software)
    }

    fn encode(&mut self, frame: &EncodeFrame<'_>) -> Result<EncodedBitstream, EncodeError> {
        let probe = probe_all()
            .into_iter()
            .find(|probe| probe.kind == self.kind);
        let Some(probe) = probe else {
            return Err(EncodeError::NotLinked {
                backend: self.kind,
                detail: "no probe for this backend".into(),
            });
        };
        if !probe.usable {
            return Err(EncodeError::NotLinked {
                backend: self.kind,
                detail: probe.detail,
            });
        }
        if frame.bit_depth == 10 && frame.luma10.is_none() {
            return Err(EncodeError::Failed(
                "10-bit encode needs luma10 samples".into(),
            ));
        }
        Err(EncodeError::NotLinked {
            backend: self.kind,
            detail: format!(
                "session is usable ({}) but the GPU surface path is not submitting {}x{} yet",
                probe.detail, frame.width, frame.height
            ),
        })
    }
}

fn bgra_to_rgb(bgra: &[u8], width: u32, height: u32) -> Result<Vec<u8>, EncodeError> {
    let pixels = (width as usize).saturating_mul(height as usize);
    if bgra.len() < pixels * 4 {
        return Err(EncodeError::Failed("BGRA buffer is short".into()));
    }
    let mut rgb = Vec::with_capacity(pixels * 3);
    for pixel in bgra.chunks(4).take(pixels) {
        rgb.push(pixel[2]);
        rgb.push(pixel[1]);
        rgb.push(pixel[0]);
    }
    Ok(rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openh264_emits_nals_and_refuses_10bit() {
        let mut encoder = OpenH264Encoder::new().unwrap();
        assert!(!encoder.supports_10bit());
        let bgra = vec![255u8; 64 * 64 * 4];
        let frame = EncodeFrame {
            width: 64,
            height: 64,
            bit_depth: 8,
            bgra: &bgra,
            luma10: None,
        };
        let stream = encoder.encode(&frame).unwrap();
        assert!(!stream.nal_units.is_empty());
        let ten = EncodeFrame {
            bit_depth: 10,
            luma10: Some(&[0u16; 64 * 64]),
            ..frame
        };
        assert!(matches!(
            encoder.encode(&ten),
            Err(EncodeError::Unsupported10Bit { .. })
        ));
    }

    #[test]
    fn hardware_without_a_device_is_not_linked() {
        let mut encoder = HardwareEncoder::new(EncoderKind::Vaapi);
        assert!(encoder.supports_10bit());
        let bgra = vec![0u8; 16];
        let frame = EncodeFrame {
            width: 2,
            height: 2,
            bit_depth: 8,
            bgra: &bgra,
            luma10: None,
        };
        assert!(matches!(
            encoder.encode(&frame),
            Err(EncodeError::NotLinked { .. })
        ));
    }

    #[test]
    fn capture_encode_packetize_and_decode_a_frame() {
        use crate::capture::{FrameSource, SyntheticSource};
        use crate::rtp::packetize;
        use crate::sync::drift_us;
        use openh264::decoder::Decoder;
        use rencord_voice::dave::{DaveEngine, DaveOps, ScriptedDave};

        let mut source = SyntheticSource::new("e2e", 64, 64);
        let captured = source.next_frame();
        let video_us = captured.captured_at_us;
        let audio_us = video_us;
        assert_eq!(drift_us(video_us, audio_us), 0);

        let mut encoder = OpenH264Encoder::new().unwrap();
        let encoded = encoder
            .encode(&EncodeFrame {
                width: captured.width,
                height: captured.height,
                bit_depth: 8,
                bgra: &captured.bgra,
                luma10: None,
            })
            .unwrap();
        let mut dave = DaveEngine::Scripted(ScriptedDave {
            ready: true,
            ..ScriptedDave::default()
        });
        let mut receiver = DaveEngine::Scripted(ScriptedDave {
            ready: true,
            ..ScriptedDave::default()
        });
        let mut plain = Vec::new();
        let mut sequence = 1u16;
        for nal in &encoded.nal_units {
            let mut packets = packetize(nal, 1200, sequence, 90_000, 7).unwrap();
            sequence = sequence.wrapping_add(packets.len() as u16);
            for packet in &mut packets {
                packet.payload = dave.encrypt_video(&packet.payload).unwrap();
                packet.payload = receiver.decrypt_video(1, &packet.payload).unwrap();
            }
            plain.extend(packets);
        }
        let mut decoder = Decoder::new().unwrap();
        let mut presented = false;
        for packet in &plain {
            if decoder.decode(&packet.payload).ok().flatten().is_some() {
                presented = true;
            }
        }
        assert!(presented, "decoder did not present a picture");
    }
}
