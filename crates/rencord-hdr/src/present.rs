//! Viewer path. A reference `RHDR` access unit is decoded to 10-bit PQ and
//! either kept for an HDR surface or tone-mapped. A real HEVC or AV1 decoder
//! is not linked. An Annex-B bitstream yields metadata only.

use crate::color::{
    nits_to_scrgb, quantize_pq_10, QuantRange, SourceColorimetry, TransferFunction,
};
use crate::encode::{decode_reference_au, DecodedHdrPicture};
use crate::metadata::{metadata_from_sei, parse_annex_b_sei, Hdr10StaticMetadata};
use crate::tonemap::{to_srgb8, CpuToneMapper, ToneCurve, ToneMapParams, ToneMapper};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PresentMode {
    Sdr,
    ToneMappedSdr { curve: ToneCurve },
    Hdr10,
    ScRgb,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresentedFrame {
    pub mode: PresentMode,
    pub srgb8: Option<Vec<[u8; 3]>>,
    pub pq10: Option<Vec<[u16; 3]>>,
    pub scrgb: Option<Vec<[f32; 3]>>,
    pub metadata: Option<Hdr10StaticMetadata>,
}

#[derive(Debug)]
pub enum ViewerFrame {
    Pixels(DecodedHdrPicture),
    /// Compressed HEVC/AV1. Metadata may still be readable. Pixels are not.
    MetadataOnly {
        metadata: Option<Hdr10StaticMetadata>,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PresentError {
    #[error("the access unit is neither RHDR nor Annex B")]
    Unrecognized,
    #[error("HEVC/AV1 pixel decode is not linked")]
    CompressedNotLinked,
}

pub fn open_access_unit(bytes: &[u8]) -> Result<ViewerFrame, PresentError> {
    if bytes.starts_with(b"RHDR") {
        return decode_reference_au(bytes)
            .map(ViewerFrame::Pixels)
            .map_err(|_| PresentError::Unrecognized);
    }
    if bytes.windows(3).any(|window| window == [0, 0, 1]) {
        let metadata = metadata_from_sei(&parse_annex_b_sei(bytes));
        if metadata.is_some() {
            return Ok(ViewerFrame::MetadataOnly { metadata });
        }
        return Err(PresentError::CompressedNotLinked);
    }
    Err(PresentError::Unrecognized)
}

pub fn decide_presentation(
    receive_hdr: bool,
    display: &SourceColorimetry,
    stream_is_hdr: bool,
) -> PresentMode {
    if !stream_is_hdr {
        return PresentMode::Sdr;
    }
    if !receive_hdr {
        return PresentMode::ToneMappedSdr {
            curve: ToneCurve::Bt2390,
        };
    }
    let display_can = display.is_hdr() && display.peak_nits > 200.0;
    if !display_can {
        return PresentMode::ToneMappedSdr {
            curve: ToneCurve::Bt2390,
        };
    }
    match display.transfer {
        TransferFunction::ScRgb | TransferFunction::Linear => PresentMode::ScRgb,
        TransferFunction::Pq | TransferFunction::Hlg => PresentMode::Hdr10,
        _ => PresentMode::ToneMappedSdr {
            curve: ToneCurve::Bt2390,
        },
    }
}

pub fn present_nits(
    nits: &[[f32; 3]],
    metadata: Option<&Hdr10StaticMetadata>,
    receive_hdr: bool,
    display: &SourceColorimetry,
    curve: ToneCurve,
) -> PresentedFrame {
    let stream_is_hdr = metadata.is_some_and(|item| item.max_luminance > 100.0);
    let mut mode = decide_presentation(receive_hdr, display, stream_is_hdr);
    if let PresentMode::ToneMappedSdr { .. } = mode {
        mode = PresentMode::ToneMappedSdr { curve };
    }
    match mode {
        PresentMode::Hdr10 => PresentedFrame {
            mode,
            srgb8: None,
            pq10: Some(
                nits.iter()
                    .map(|pixel| {
                        pixel.map(|channel| {
                            quantize_pq_10(
                                crate::color::pq_oetf(f64::from(channel.max(0.0))),
                                QuantRange::Full,
                            )
                        })
                    })
                    .collect(),
            ),
            scrgb: None,
            metadata: metadata.cloned(),
        },
        PresentMode::ScRgb => PresentedFrame {
            mode,
            srgb8: None,
            pq10: None,
            scrgb: Some(
                nits.iter()
                    .map(|pixel| {
                        pixel.map(|channel| nits_to_scrgb(f64::from(channel.max(0.0))) as f32)
                    })
                    .collect(),
            ),
            metadata: metadata.cloned(),
        },
        PresentMode::Sdr | PresentMode::ToneMappedSdr { .. } => {
            let curve = match mode {
                PresentMode::ToneMappedSdr { curve } => curve,
                _ => ToneCurve::Bt2390,
            };
            let peak = metadata
                .map(|item| item.max_luminance as f32)
                .unwrap_or(100.0);
            let params = ToneMapParams {
                curve,
                src_primaries: crate::color::ColorPrimaries::Bt2020,
                dst_primaries: crate::color::ColorPrimaries::Bt709,
                src_min_nits: 0.0,
                src_max_nits: peak.max(100.0),
                dst_min_nits: 0.0,
                dst_max_nits: if matches!(mode, PresentMode::Sdr) {
                    peak.max(100.0)
                } else {
                    100.0
                },
                reference_white_nits: 203.0,
            };
            // SDR input still goes through sRGB so the viewer has one output type.
            let mapper = CpuToneMapper::new(params);
            let srgb8 = nits
                .iter()
                .map(|pixel| {
                    if matches!(mode, PresentMode::Sdr) && peak <= 100.0 {
                        to_srgb8(pixel.map(|channel| (channel / 100.0).clamp(0.0, 1.0)))
                    } else if matches!(mode, PresentMode::Sdr) {
                        to_srgb8(pixel.map(|channel| (channel / peak).clamp(0.0, 1.0)))
                    } else {
                        to_srgb8(mapper.map_display_linear(*pixel))
                    }
                })
                .collect();
            PresentedFrame {
                mode,
                srgb8: Some(srgb8),
                pq10: None,
                scrgb: None,
                metadata: metadata.cloned(),
            }
        }
    }
}

pub fn present_decoded(
    picture: &DecodedHdrPicture,
    receive_hdr: bool,
    display: &SourceColorimetry,
    curve: ToneCurve,
) -> PresentedFrame {
    let _ = picture.codec;
    present_nits(
        &picture.nits,
        Some(&picture.metadata),
        receive_hdr,
        display,
        curve,
    )
}

/// Surface the platform shell would allocate. GPUI's swapchain is not changed here.
pub fn surface_name(mode: PresentMode) -> &'static str {
    match mode {
        PresentMode::Sdr | PresentMode::ToneMappedSdr { .. } => "8-bit sRGB",
        PresentMode::Hdr10 => "10-bit PQ BT.2020",
        PresentMode::ScRgb => "linear scRGB (1.0 = 80 nits)",
    }
}

/// wgpu texture format the shell should request. No window surface is created here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapchainFormat {
    Bgra8,
    /// PQ 10-bit, `wgpu::TextureFormat::Rgb10a2Unorm` when the compositor allows it.
    Rgb10a2Unorm,
    /// scRGB, `wgpu::TextureFormat::Rgba16Float`.
    Rgba16Float,
}

pub fn swapchain_format(
    receive_hdr: bool,
    display: &SourceColorimetry,
    stream_is_hdr: bool,
) -> SwapchainFormat {
    match decide_presentation(receive_hdr, display, stream_is_hdr) {
        PresentMode::Hdr10 => SwapchainFormat::Rgb10a2Unorm,
        PresentMode::ScRgb => SwapchainFormat::Rgba16Float,
        PresentMode::Sdr | PresentMode::ToneMappedSdr { .. } => SwapchainFormat::Bgra8,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformDecoder {
    /// D3D11 video processor / DXVA on Windows.
    D3d11va,
    VideoToolbox,
    Vaapi,
    /// Uncompressed `RHDR` from this crate.
    Reference,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecoderStatus {
    pub decoder: PlatformDecoder,
    pub linked: bool,
    pub detail: &'static str,
}

pub fn platform_decoders() -> &'static [DecoderStatus] {
    &[
        DecoderStatus {
            decoder: PlatformDecoder::D3d11va,
            linked: false,
            detail: "D3D11VA/DXVA is the Windows 10-bit decode path. It is not opened from this process.",
        },
        DecoderStatus {
            decoder: PlatformDecoder::VideoToolbox,
            linked: false,
            detail: "VideoToolbox is the macOS 10-bit decode path. It is not opened from this process.",
        },
        DecoderStatus {
            decoder: PlatformDecoder::Vaapi,
            linked: false,
            detail: "VA-API can decode HEVC Main10 when a render node exists. The decode session is not linked.",
        },
        DecoderStatus {
            decoder: PlatformDecoder::Reference,
            linked: true,
            detail: "RHDR access units decode to 10-bit PQ samples. Compressed HEVC and AV1 do not.",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{BitDepth, ColorPrimaries};
    use crate::encode::{
        HdrCodec, HdrEncodeConfig, HdrEncoder, HdrPicture, ReferenceContainerEncoder,
    };
    use crate::metadata::Hdr10StaticMetadata;

    fn display_pq() -> SourceColorimetry {
        SourceColorimetry {
            bit_depth: BitDepth::Ten,
            transfer: TransferFunction::Pq,
            primaries: ColorPrimaries::Bt2020,
            peak_nits: 1000.0,
            min_nits: 0.005,
            peak_assumed: false,
        }
    }

    #[test]
    fn hdr_display_keeps_pq_and_sdr_display_tone_maps() {
        let metadata = Hdr10StaticMetadata::bt2020_1000();
        let pixels = [[1000.0f32, 1000.0, 1000.0]];
        let hdr = present_nits(
            &pixels,
            Some(&metadata),
            true,
            &display_pq(),
            ToneCurve::Bt2390,
        );
        assert_eq!(hdr.mode, PresentMode::Hdr10);
        assert_eq!(surface_name(hdr.mode), "10-bit PQ BT.2020");
        let code = hdr.pq10.unwrap()[0][0];
        assert_eq!(
            code,
            quantize_pq_10(crate::color::pq_oetf(1000.0), QuantRange::Full)
        );

        let sdr_display = SourceColorimetry::sdr();
        let mapped = present_nits(
            &pixels,
            Some(&metadata),
            true,
            &sdr_display,
            ToneCurve::Bt2390,
        );
        assert!(matches!(mapped.mode, PresentMode::ToneMappedSdr { .. }));
        let bytes = mapped.srgb8.unwrap()[0];
        assert_eq!(bytes, [255, 255, 255]);

        let refused = present_nits(
            &pixels,
            Some(&metadata),
            false,
            &display_pq(),
            ToneCurve::Aces,
        );
        assert!(matches!(
            refused.mode,
            PresentMode::ToneMappedSdr {
                curve: ToneCurve::Aces
            }
        ));

        let scrgb_display = SourceColorimetry {
            bit_depth: BitDepth::SixteenFloat,
            transfer: TransferFunction::ScRgb,
            primaries: ColorPrimaries::Bt709,
            peak_nits: 400.0,
            min_nits: 0.0,
            peak_assumed: false,
        };
        let linear = present_nits(
            &[[80.0, 80.0, 80.0]],
            Some(&metadata),
            true,
            &scrgb_display,
            ToneCurve::Bt2390,
        );
        assert_eq!(linear.mode, PresentMode::ScRgb);
        let sample = linear.scrgb.unwrap()[0][0];
        assert!((sample - 1.0).abs() < 1e-4, "{sample}");
    }

    #[test]
    fn reference_unit_presents_and_annex_b_does_not_decode_pixels() {
        let mut encoder = ReferenceContainerEncoder;
        let config = HdrEncodeConfig {
            codec: HdrCodec::HevcMain10,
            backend: crate::encode::EncoderBackend::ReferenceContainer,
            width: 2,
            height: 2,
            fps: 60,
            bitrate_kbps: 8000,
            metadata: Hdr10StaticMetadata::bt2020_1000(),
            capability: None,
            quant_range: QuantRange::Full,
        };
        let params = encoder.negotiate(&config).unwrap();
        let pixels = vec![[400.0f32, 20.0, 5.0]; 4];
        let unit = encoder
            .encode(
                &HdrPicture {
                    width: 2,
                    height: 2,
                    linear_nits: &pixels,
                },
                &params,
            )
            .unwrap();
        match open_access_unit(&unit.bytes).unwrap() {
            ViewerFrame::Pixels(picture) => {
                let presented = present_decoded(&picture, true, &display_pq(), ToneCurve::Bt2390);
                assert_eq!(presented.mode, PresentMode::Hdr10);
                assert!(presented.pq10.unwrap().len() == 4);
            }
            ViewerFrame::MetadataOnly { .. } => panic!("reference unit should decode"),
        }
        let sei = crate::metadata::hevc_hdr10_sei(&Hdr10StaticMetadata::bt2020_1000());
        match open_access_unit(&sei).unwrap() {
            ViewerFrame::MetadataOnly { metadata } => {
                assert_eq!(metadata.unwrap().max_cll, 1000);
            }
            ViewerFrame::Pixels(_) => panic!("annex-b is not a pixel container"),
        }
        assert!(matches!(
            open_access_unit(&[0, 0, 0, 1, 0x26, 0x01, 0x00]),
            Err(PresentError::CompressedNotLinked)
        ));
    }

    #[test]
    fn hdr_display_asks_for_a_10bit_swapchain_and_only_rhdr_is_linked() {
        assert_eq!(
            swapchain_format(true, &display_pq(), true),
            SwapchainFormat::Rgb10a2Unorm
        );
        assert_eq!(
            swapchain_format(false, &display_pq(), true),
            SwapchainFormat::Bgra8
        );
        let linked: Vec<_> = platform_decoders()
            .iter()
            .filter(|status| status.linked)
            .map(|status| status.decoder)
            .collect();
        assert_eq!(linked, vec![PlatformDecoder::Reference]);
    }
}
