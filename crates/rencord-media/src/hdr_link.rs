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
use crate::encoder::VideoEncoder;
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
        width: u32,
        height: u32,
        _nits: &[[f32; 3]],
        metadata: &Hdr10StaticMetadata,
    ) -> Result<Vec<u8>, String> {
        let _ = metadata.mastering_payload();
        let pixels = (width as usize).saturating_mul(height as usize);
        let luma10 = vec![0u16; pixels];
        let mut encoder = crate::encoder::HardwareEncoder::new(self.kind);
        match encoder.encode(&crate::encoder::EncodeFrame {
            width,
            height,
            bit_depth: 10,
            bgra: &[],
            luma10: Some(&luma10),
        }) {
            Err(crate::encoder::EncodeError::NotLinked { detail, .. }) => Err(format!(
                "{} Main10 is not linked. {detail}",
                self.kind.label()
            )),
            Err(err) => Err(err.to_string()),
            Ok(_) => Err(format!(
                "{} Main10 returned bytes this build does not submit",
                self.kind.label()
            )),
        }
    }
}

pub fn hdr10_encoder() -> Box<dyn Hdr10Encoder> {
    Box::new(ReferenceHdr10Encoder)
}

pub struct PreparedFrame {
    pub sdr: Frame,
    pub tone_map: &'static str,
    pub hdr10: Option<Vec<u8>>,
    /// Set when the tone-map stayed in a device-local buffer.
    pub gpu_resident_bytes: Option<u64>,
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
            gpu_resident_bytes: None,
        });
    }
    let nits = nits_from_preview(frame);
    let params = ToneMapParams {
        curve,
        ..ToneMapParams::hdr10_to_sdr100(curve)
    };
    let mut sdr = frame.clone();
    sdr.hdr = HdrFlags::default();
    // dispatch_resident leaves the buffer on the device. The software luma
    // encoder cannot sample it, so SoftwareFrameSink maps once and the access
    // unit gets the tone-mapped picture. A GPU encoder sink keeps the buffer.
    let (tone_map, resident_bytes) =
        match rencord_hdr::dispatch_resident(&params, frame.width, frame.height, &nits) {
            Ok(resident) => {
                let bytes = resident.byte_size();
                let mut sink = SoftwareFrameSink { pixels: Vec::new() };
                sink.accept(resident)?;
                write_sdr(&mut sdr, &sink.pixels);
                ("wgpu-resident", Some(bytes))
            }
            Err(GpuError::NoAdapter) => {
                let mut linear = vec![[0.0f32; 3]; nits.len()];
                CpuToneMapper::new(params).map_into(&nits, &mut linear);
                write_sdr(&mut sdr, &linear);
                ("cpu", None)
            }
            Err(err) => return Err(err.to_string()),
        };
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
        gpu_resident_bytes: resident_bytes,
    })
}

/// Receives the device-local tone-map. A GPU encoder keeps the buffer.
pub trait GpuFrameSink {
    fn accept(&mut self, frame: rencord_hdr::GpuResidentFrame) -> Result<(), String>;
}

/// The software luma encoder reads CPU bytes, so this sink maps the buffer
/// once. That copy is outside the timed GPU submit.
pub struct SoftwareFrameSink {
    pub pixels: Vec<[f32; 3]>,
}

impl GpuFrameSink for SoftwareFrameSink {
    fn accept(&mut self, frame: rencord_hdr::GpuResidentFrame) -> Result<(), String> {
        if frame.byte_size() == 0 {
            return Err("empty GPU tone-map".into());
        }
        self.pixels = frame.readback().map_err(|err| err.to_string())?;
        Ok(())
    }
}

fn write_sdr(frame: &mut Frame, linear: &[[f32; 3]]) {
    for (index, pixel) in linear.iter().enumerate() {
        let rgb = to_srgb8(*pixel);
        let offset = index * 4;
        if offset + 3 < frame.bgra.len() {
            frame.bgra[offset] = rgb[2];
            frame.bgra[offset + 1] = rgb[1];
            frame.bgra[offset + 2] = rgb[0];
            frame.bgra[offset + 3] = 255;
        }
    }
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
        assert_ne!(
            &prepared.sdr.bgra[8..12],
            &[128, 128, 128, 255],
            "the SDR access unit must carry the tone-mapped picture"
        );
        let hdr = prepared.hdr10.expect("10-bit sidecar");
        assert!(
            prepared.tone_map == "cpu" || prepared.gpu_resident_bytes.is_some(),
            "{}",
            prepared.tone_map
        );
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
