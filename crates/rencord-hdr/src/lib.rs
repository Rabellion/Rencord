//! HDR for a later Go Live worker.
//!
//! The capture process, the H.264 encoder, and the worker protocol belong to
//! Milestone 4. This crate is the piece that worker can call:
//! [`HdrCaptureProbe`] classifies a display, [`HdrEncoder`] describes a 10-bit
//! encode and attaches HDR10 metadata, [`ToneMapper`] maps HDR to SDR, and
//! [`HdrSignal`] decides who is allowed to see the 10-bit path.
//!
//! What actually runs here is the software half: PQ and HLG, EDID parsing, the
//! CPU tone-map, SEI and AV1 OBU bytes, and an uncompressed `RHDR` container.
//! NVENC, AMF, QSV, VideoToolbox, VA-API, and a real HEVC/AV1 decoder are not
//! linked. Discord's servers have not been asked to carry any of it.
//!
//! `rencord-media` calls [`crate::HdrSignal`] from `planned_pixel_path`. This
//! crate does not depend on the Go Live types, so the worker can keep its own
//! `output_path` (still tone-mapped SDR) while the plan lives here.

mod color;
mod encode;
mod gpu;
mod metadata;
mod present;
mod probe;
mod shader;
mod signal;
mod tonemap;

pub use color::{
    dequantize_pq_10, hlg_signal_to_nits, hlg_signal_to_scene, luma, nits_to_scrgb,
    pack_p010_sample, pq_eotf, pq_oetf, primaries_to_xyz, quantize_pq_10, signal_to_nits,
    srgb_eotf, srgb_oetf, unpack_p010_sample, BitDepth, Chromaticities, ColorMatrix,
    ColorPrimaries, MatrixCoefficients, QuantRange, SourceColorimetry, TransferFunction, Xy,
    PQ_CODE_1000_NITS, PQ_CODE_100_NITS, PQ_CODE_203_NITS, PQ_PEAK_NITS, REFERENCE_WHITE_NITS,
    SCRGB_WHITE_NITS,
};
pub use encode::{
    backend_contract, backend_contracts, decode_reference_au, BackendContract, ColorDescription,
    DecodedHdrPicture, EncodeError, EncodeParameters, EncodedAccessUnit, EncoderBackend,
    HardwareContractEncoder, HdrCodec, HdrEncodeConfig, HdrEncoder, HdrPicture,
    ReferenceContainerEncoder,
};
pub use gpu::{
    adapter_report, dispatch, dispatch_resident, map_shader_formula, AdapterReport, GpuDispatch,
    GpuError, GpuResidentFrame,
};
pub use metadata::{
    av1_hdr_metadata_obus, hevc_hdr10_sei, hevc_prefix_sei, metadata_from_sei, parse_annex_b_sei,
    Hdr10StaticMetadata, SeiMessage,
};
pub use present::{
    decide_presentation, open_access_unit, platform_decoders, present_decoded, present_nits,
    surface_name, swapchain_format, DecoderStatus, PlatformDecoder, PresentError, PresentMode,
    PresentedFrame, SwapchainFormat, ViewerFrame,
};
pub use probe::{
    capture_pixel_format, classify_apple_format, classify_dxgi, parse_edid_chromaticity,
    parse_edid_hdr, parse_pipewire_caps, samples_to_nits, wayland_primaries, wayland_transfer,
    ApplePixelClass, CapturePixelFormat, CapturedColorDesc, DescriptorProbe, DisplayHdrInfo,
    DisplaySource, EdidHdrBlock, HdrCaptureProbe, HdrSourceReport, HostProbe, Platform, ProbeError,
    DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
};
pub use signal::{
    decode_capability_payload, decode_capability_sei, AudienceMember, DeliveryPlan, DiscordTrack,
    HdrCapability, HdrSession, HdrSignal, RtpOneByteExtension, CAPABILITY_UUID, RTP_EXTENSION_ID,
    RTP_EXTENSION_URI,
};
pub use tonemap::{
    aces_narkowicz, bt2390_nits, bt2390_nits_unclamped, hable_filmic, measure_content_light,
    to_srgb8, CpuToneMapper, GpuToneMapper, ToneCurve, ToneMapParams, ToneMapper,
    EETF_1000_MASTER_ON_100_UNCLAMPED, EETF_1000_NITS_ON_1000_DISPLAY,
    EETF_100_NITS_INSIDE_1000_TO_100, EETF_4000_NITS_ON_1000_DISPLAY,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportLevel {
    /// Runs in this process with no GPU and no Discord session.
    Software,
    /// The contract is real. The platform library is not linked.
    NotLinked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    pub name: &'static str,
    pub level: SupportLevel,
    pub detail: &'static str,
}

pub fn capabilities() -> &'static [Capability] {
    &[
        Capability {
            name: "PQ, HLG, sRGB, scRGB",
            level: SupportLevel::Software,
            detail: "ST 2084 and BT.2100 formulas, with tests against published PQ codes.",
        },
        Capability {
            name: "BT.2390 / Hable / ACES tone-map",
            level: SupportLevel::Software,
            detail: "CPU reference. The WGSL compute shader parses, and is not dispatched.",
        },
        Capability {
            name: "HDR10 SEI and AV1 metadata OBU",
            level: SupportLevel::Software,
            detail: "Mastering display, MaxCLL, and MaxFALL. Annex-B emulation prevention included.",
        },
        Capability {
            name: "EDID and descriptor probes",
            level: SupportLevel::Software,
            detail: "CTA-861.3 HDR blocks, DXGI color-space ids, ScreenCaptureKit fourccs, Wayland enums, PipeWire caps strings. Live DXGI, ScreenCaptureKit, and PipeWire are not opened.",
        },
        Capability {
            name: "Rencord capability signalling",
            level: SupportLevel::Software,
            detail: "RTP extension and user-data SEI. No Discord gateway payload. SFU forwarding is untested.",
        },
        Capability {
            name: "10-bit hardware encode",
            level: SupportLevel::NotLinked,
            detail: "NVENC, AMF, QSV, VideoToolbox, and VA-API negotiate parameters and then return NotLinked.",
        },
        Capability {
            name: "HEVC / AV1 pixel decode",
            level: SupportLevel::NotLinked,
            detail: "RHDR test containers decode. Compressed bitstreams return metadata only.",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_table_names_what_is_linked() {
        let items = capabilities();
        assert!(items
            .iter()
            .any(|item| item.level == SupportLevel::Software && item.name.contains("tone-map")));
        assert!(items
            .iter()
            .any(|item| item.level == SupportLevel::NotLinked && item.name.contains("hardware")));
        let gpu = GpuToneMapper::new(ToneMapParams::hdr10_to_sdr100(ToneCurve::Bt2390));
        let status = gpu.dispatch_status();
        assert!(
            status.contains("wgpu") || status.contains("No Vulkan"),
            "{status}"
        );
        assert!(gpu.shader_source().contains("fn tonemap"));
    }
}
