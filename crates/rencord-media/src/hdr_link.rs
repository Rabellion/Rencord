//! HDR attach point for the Go Live encode path.
//!
//! The software H.264-style access unit in `encode` stays the SDR track.
//! This module tone-maps an HDR frame onto that track with the GPU shader
//! when wgpu has an adapter, and the CPU mapper otherwise. When HDR send is
//! on it also builds a 10-bit access unit through [`Hdr10Encoder`]. The
//! reference implementation writes real HDR10 SEI. NVENC, AMF, QSV,
//! VideoToolbox, and VA-API implement the same trait and return not-linked
//! until a session exists. Their encode code is not duplicated here.

use rencord_hdr::{
    to_srgb8, CpuToneMapper, GpuError, Hdr10StaticMetadata, HdrCodec, HdrEncodeConfig, HdrEncoder,
    HdrPicture, QuantRange, ReferenceContainerEncoder, ToneCurve, ToneMapParams, ToneMapper,
};

use crate::encode::EncoderKind;
use crate::frame::{Frame, HdrFlags};

pub trait Hdr10Encoder {
    fn kind(&self) -> EncoderKind;
    fn encode_main10(
        &mut self,
        width: u32,
        height: u32,
        nits: &[[f32; 3]],
        metadata: &Hdr10StaticMetadata,
    ) -> Result<Vec<u8>, String>;
}

/// The only 10-bit encoder that emits bytes today. The payload is `RHDR`
/// plus a real HEVC HDR10 SEI, not a compressed Main10 bitstream.
pub struct ReferenceHdr10Encoder;

impl Hdr10Encoder for ReferenceHdr10Encoder {
    fn kind(&self) -> EncoderKind {
        EncoderKind::Software
    }

    fn encode_main10(
        &mut self,
        width: u32,
        height: u32,
        nits: &[[f32; 3]],
        metadata: &Hdr10StaticMetadata,
    ) -> Result<Vec<u8>, String> {
        let mut encoder = ReferenceContainerEncoder;
        let config = HdrEncodeConfig {
            codec: HdrCodec::HevcMain10,
            backend: rencord_hdr::EncoderBackend::ReferenceContainer,
            width,
            height,
            fps: 60,
            bitrate_kbps: 12_000,
            metadata: *metadata,
            capability: None,
            quant_range: QuantRange::Full,
        };
        let params = encoder.negotiate(&config).map_err(|err| err.to_string())?;
        let unit = encoder
            .encode(
                &HdrPicture {
                    width,
                    height,
                    linear_nits: nits,
                },
                &params,
            )
            .map_err(|err| err.to_string())?;
        Ok(unit.bytes)
    }
}

/// Hardware backends keep this stub until their session submits 10-bit pictures.
pub struct UnlinkedHdr10Encoder {
    pub kind: EncoderKind,
}

impl Hdr10Encoder for UnlinkedHdr10Encoder {
    fn kind(&self) -> EncoderKind {
        self.kind
    }

    fn encode_main10(
        &mut self,
        _width: u32,
        _height: u32,
        _nits: &[[f32; 3]],
        metadata: &Hdr10StaticMetadata,
    ) -> Result<Vec<u8>, String> {
        let _ = metadata.mastering_payload();
        Err(format!(
            "{} Main10 is not linked. HDR10 SEI bytes were built and not submitted.",
            self.kind.label()
        ))
    }
}

pub fn hdr10_encoder() -> Box<dyn Hdr10Encoder> {
    Box::new(ReferenceHdr10Encoder)
}

pub struct PreparedFrame {
    pub sdr: Frame,
    pub tone_map: &'static str,
    pub hdr10: Option<Vec<u8>>,
}

pub fn prepare_frame(
    frame: &Frame,
    hdr_send: bool,
    curve: ToneCurve,
) -> Result<PreparedFrame, String> {
    if !frame.hdr.is_hdr() {
        return Ok(PreparedFrame {
            sdr: frame.clone(),
            tone_map: "sdr",
            hdr10: None,
        });
    }
    let nits = nits_from_preview(frame);
    let params = ToneMapParams {
        curve,
        ..ToneMapParams::hdr10_to_sdr100(curve)
    };
    let mut linear = vec![[0.0f32; 3]; nits.len()];
    let tone_map = match rencord_hdr::dispatch(&params, &nits) {
        Ok(gpu) => {
            linear = gpu.pixels;
            if gpu.report.software {
                "wgpu-software"
            } else {
                "wgpu"
            }
        }
        Err(GpuError::NoAdapter) => {
            CpuToneMapper::new(params).map_into(&nits, &mut linear);
            "cpu"
        }
        Err(err) => return Err(err.to_string()),
    };
    let mut sdr = Frame::new(frame.width, frame.height, frame.sequence);
    sdr.captured_at_us = frame.captured_at_us;
    sdr.hdr = HdrFlags::default();
    for (index, pixel) in linear.iter().enumerate() {
        let rgb = to_srgb8(*pixel);
        let offset = index * 4;
        if offset + 3 < sdr.bgra.len() {
            sdr.bgra[offset] = rgb[2];
            sdr.bgra[offset + 1] = rgb[1];
            sdr.bgra[offset + 2] = rgb[0];
            sdr.bgra[offset + 3] = 255;
        }
    }
    let hdr10 = if hdr_send {
        Some(hdr10_encoder().encode_main10(
            frame.width,
            frame.height,
            &nits,
            &Hdr10StaticMetadata::bt2020_1000(),
        )?)
    } else {
        None
    };
    Ok(PreparedFrame {
        sdr,
        tone_map,
        hdr10,
    })
}

fn nits_from_preview(frame: &Frame) -> Vec<[f32; 3]> {
    frame
        .bgra
        .chunks(4)
        .map(|pixel| {
            let b = pixel.first().copied().unwrap_or(0) as f32 / 255.0;
            let g = pixel.get(1).copied().unwrap_or(0) as f32 / 255.0;
            let r = pixel.get(2).copied().unwrap_or(0) as f32 / 255.0;
            // The shared-memory frame is 8-bit. Until a capturer supplies
            // float or 10-bit texels, treat the preview as scene light up to
            // the 1 000 cd/m² master.
            [r * 1000.0, g * 1000.0, b * 1000.0]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_on_attaches_hdr10_sei_and_hardware_stays_unlinked() {
        let mut frame = Frame::new(2, 2, 1);
        frame.hdr = HdrFlags {
            ten_bit: true,
            pq: true,
        };
        frame.bgra = vec![
            255, 255, 255, 255, 0, 0, 0, 255, 128, 128, 128, 255, 40, 10, 10, 255,
        ];
        let prepared = prepare_frame(&frame, true, ToneCurve::Bt2390).unwrap();
        assert!(!prepared.sdr.hdr.is_hdr());
        let hdr = prepared.hdr10.expect("10-bit sidecar");
        assert!(hdr.starts_with(b"RHDR"));
        assert!(hdr.windows(4).any(|bytes| bytes == [0, 0, 0, 1]));
        let mut hardware = UnlinkedHdr10Encoder {
            kind: EncoderKind::Nvenc,
        };
        assert!(hardware
            .encode_main10(
                2,
                2,
                &[[1000.0, 1000.0, 1000.0]; 4],
                &Hdr10StaticMetadata::bt2020_1000()
            )
            .unwrap_err()
            .contains("not linked"));
    }
}
