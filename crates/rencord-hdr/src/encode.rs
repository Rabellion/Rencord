//! 10-bit encode contract.
//!
//! `negotiate` builds the profile, the H.273 color description, and the HDR10
//! SEI or AV1 metadata OBUs a hardware encoder would attach. NVENC, AMF, QSV,
//! VideoToolbox, and VA-API are described and not linked. `ReferenceContainerEncoder`
//! writes an uncompressed `RHDR` access unit so tests can round-trip pixels
//! without a video codec.

use crate::color::{
    dequantize_pq_10, pq_eotf, pq_oetf, quantize_pq_10, BitDepth, ColorPrimaries,
    MatrixCoefficients, QuantRange, TransferFunction,
};
use crate::metadata::{metadata_from_sei, parse_annex_b_sei, Hdr10StaticMetadata, SeiMessage};
use crate::signal::HdrCapability;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderBackend {
    Nvenc,
    Amf,
    Qsv,
    VideoToolbox,
    Vaapi,
    ReferenceContainer,
}

impl EncoderBackend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Nvenc => "NVENC",
            Self::Amf => "AMF",
            Self::Qsv => "QSV",
            Self::VideoToolbox => "VideoToolbox",
            Self::Vaapi => "VA-API",
            Self::ReferenceContainer => "reference container",
        }
    }

    pub fn linked(self) -> bool {
        matches!(self, Self::ReferenceContainer)
    }
}

impl std::fmt::Display for EncoderBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdrCodec {
    HevcMain10,
    Av1Main10,
}

impl HdrCodec {
    pub fn label(self) -> &'static str {
        match self {
            Self::HevcMain10 => "HEVC Main10",
            Self::Av1Main10 => "AV1 Main 10-bit",
        }
    }

    pub fn tag(self) -> u8 {
        match self {
            Self::HevcMain10 => 1,
            Self::Av1Main10 => 2,
        }
    }

    pub fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            1 => Some(Self::HevcMain10),
            2 => Some(Self::Av1Main10),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorDescription {
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
    pub full_range: bool,
}

impl ColorDescription {
    pub fn hdr10(range: QuantRange) -> Self {
        Self {
            primaries: ColorPrimaries::Bt2020.h273(),
            transfer: TransferFunction::Pq.h273().unwrap_or(16),
            matrix: MatrixCoefficients::Bt2020Ncl.h273(),
            full_range: matches!(range, QuantRange::Full),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackendContract {
    pub backend: EncoderBackend,
    pub hevc_main10: bool,
    pub av1_10bit: bool,
    pub linked: bool,
    pub hints: &'static [&'static str],
}

pub fn backend_contract(backend: EncoderBackend) -> BackendContract {
    match backend {
        EncoderBackend::Nvenc => BackendContract {
            backend,
            hevc_main10: true,
            av1_10bit: true,
            linked: false,
            hints: &[
                "HEVC: NV_ENC_HEVC_PROFILE_MAIN10_GUID and NV_ENC_BIT_DEPTH_10.",
                "AV1 10-bit only when the device reports it. Profile is still Main (0).",
                "Attach the prefix SEI from EncodeParameters, or fill the mastering-display fields.",
                "The NVENC driver is not linked.",
            ],
        },
        EncoderBackend::Amf => BackendContract {
            backend,
            hevc_main10: true,
            av1_10bit: true,
            linked: false,
            hints: &[
                "AMF_VIDEO_ENCODER_HEVC_PROFILE = MAIN_10.",
                "AMF_VIDEO_ENCODER_HEVC_COLOR_BIT_DEPTH = 10.",
                "Set mastering display and content light level on the encoder.",
                "AMF is not linked.",
            ],
        },
        EncoderBackend::Qsv => BackendContract {
            backend,
            hevc_main10: true,
            av1_10bit: true,
            linked: false,
            hints: &[
                "MFX_PROFILE_HEVC_MAIN10 with FourCC P010.",
                "mfxExtMasteringDisplayColourVolume and mfxExtContentLightLevelInfo.",
                "oneVPL / Media SDK is not linked.",
            ],
        },
        EncoderBackend::VideoToolbox => BackendContract {
            backend,
            hevc_main10: true,
            av1_10bit: false,
            linked: false,
            hints: &[
                "kVTProfileLevel_HEVC_Main10_AutoLevel.",
                "kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange.",
                "kVTCompressionPropertyKey_MasteringDisplayColorVolume and ContentLightLevelInfo.",
                "VideoToolbox is not linked.",
            ],
        },
        EncoderBackend::Vaapi => BackendContract {
            backend,
            hevc_main10: true,
            av1_10bit: true,
            linked: false,
            hints: &[
                "VAProfileHEVCMain10, or VAProfileAV1Profile0 with VA_RT_FORMAT_YUV420_10.",
                "Pack the Annex-B SEI or the AV1 metadata OBUs from EncodeParameters.",
                "libva is not linked.",
            ],
        },
        EncoderBackend::ReferenceContainer => BackendContract {
            backend,
            hevc_main10: true,
            av1_10bit: true,
            linked: true,
            hints: &[
                "Writes RHDR, an uncompressed 10-bit PQ container with the real SEI or OBUs inside.",
                "This is not an HEVC or AV1 bitstream.",
            ],
        },
    }
}

pub fn backend_contracts() -> &'static [EncoderBackend] {
    &[
        EncoderBackend::Nvenc,
        EncoderBackend::Amf,
        EncoderBackend::Qsv,
        EncoderBackend::VideoToolbox,
        EncoderBackend::Vaapi,
        EncoderBackend::ReferenceContainer,
    ]
}

#[derive(Clone, Debug)]
pub struct HdrEncodeConfig {
    pub codec: HdrCodec,
    pub backend: EncoderBackend,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub metadata: Hdr10StaticMetadata,
    pub capability: Option<HdrCapability>,
    pub quant_range: QuantRange,
}

#[derive(Clone, Debug)]
pub struct EncodeParameters {
    pub backend: EncoderBackend,
    pub codec: HdrCodec,
    pub profile_name: &'static str,
    pub bit_depth: u8,
    pub width: u32,
    pub height: u32,
    pub color: ColorDescription,
    pub metadata: Hdr10StaticMetadata,
    pub metadata_bytes: Vec<u8>,
    pub hints: &'static [&'static str],
}

#[derive(Clone, Debug)]
pub struct EncodedAccessUnit {
    pub codec: HdrCodec,
    pub bytes: Vec<u8>,
    pub metadata: Hdr10StaticMetadata,
    pub compressed: bool,
}

#[derive(Clone, Debug)]
pub struct HdrPicture<'a> {
    pub width: u32,
    pub height: u32,
    /// Linear cd/m² RGB in the metadata primaries. One entry per pixel, row-major.
    pub linear_nits: &'a [[f32; 3]],
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedHdrPicture {
    pub width: u32,
    pub height: u32,
    pub codec: HdrCodec,
    pub quant_range: QuantRange,
    pub metadata: Hdr10StaticMetadata,
    pub nits: Vec<[f32; 3]>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EncodeError {
    #[error("{0}")]
    Invalid(String),
    #[error("{backend} is not linked. {detail}")]
    NotLinked {
        backend: EncoderBackend,
        detail: String,
    },
    #[error("the access unit is not an RHDR container")]
    NotReferenceContainer,
}

pub trait HdrEncoder {
    fn backend(&self) -> EncoderBackend;
    fn negotiate(&self, config: &HdrEncodeConfig) -> Result<EncodeParameters, EncodeError>;
    fn encode(
        &mut self,
        picture: &HdrPicture<'_>,
        params: &EncodeParameters,
    ) -> Result<EncodedAccessUnit, EncodeError>;
}

#[derive(Clone, Debug)]
pub struct HardwareContractEncoder {
    pub backend: EncoderBackend,
}

impl HdrEncoder for HardwareContractEncoder {
    fn backend(&self) -> EncoderBackend {
        self.backend
    }

    fn negotiate(&self, config: &HdrEncodeConfig) -> Result<EncodeParameters, EncodeError> {
        validate_config(config)?;
        let contract = backend_contract(self.backend);
        if self.backend == EncoderBackend::ReferenceContainer {
            return Err(EncodeError::Invalid(
                "use ReferenceContainerEncoder for the software container".into(),
            ));
        }
        match config.codec {
            HdrCodec::HevcMain10 if !contract.hevc_main10 => {
                return Err(EncodeError::Invalid(format!(
                    "{} has no HEVC Main10 contract",
                    self.backend
                )));
            }
            HdrCodec::Av1Main10 if !contract.av1_10bit => {
                return Err(EncodeError::Invalid(format!(
                    "{} has no AV1 10-bit contract",
                    self.backend
                )));
            }
            _ => {}
        }
        Ok(build_parameters(config, contract.hints))
    }

    fn encode(
        &mut self,
        _picture: &HdrPicture<'_>,
        params: &EncodeParameters,
    ) -> Result<EncodedAccessUnit, EncodeError> {
        Err(EncodeError::NotLinked {
            backend: self.backend,
            detail: format!(
                "{} {} parameters were built ({} metadata bytes, {}-bit) but the driver is not linked",
                params.profile_name,
                params.codec.label(),
                params.metadata_bytes.len(),
                params.bit_depth
            ),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct ReferenceContainerEncoder;

impl HdrEncoder for ReferenceContainerEncoder {
    fn backend(&self) -> EncoderBackend {
        EncoderBackend::ReferenceContainer
    }

    fn negotiate(&self, config: &HdrEncodeConfig) -> Result<EncodeParameters, EncodeError> {
        validate_config(config)?;
        Ok(build_parameters(
            config,
            backend_contract(EncoderBackend::ReferenceContainer).hints,
        ))
    }

    fn encode(
        &mut self,
        picture: &HdrPicture<'_>,
        params: &EncodeParameters,
    ) -> Result<EncodedAccessUnit, EncodeError> {
        if picture.width != params.width || picture.height != params.height {
            return Err(EncodeError::Invalid(
                "picture dimensions do not match the negotiated config".into(),
            ));
        }
        let expected = picture.width as usize * picture.height as usize;
        if picture.linear_nits.len() != expected || picture.width == 0 || picture.height == 0 {
            return Err(EncodeError::Invalid(
                "picture dimensions do not match the pixel buffer".into(),
            ));
        }
        if expected > 8_294_400 {
            return Err(EncodeError::Invalid(
                "reference container refuses frames above 8.3 megapixels".into(),
            ));
        }
        let mut bytes = Vec::with_capacity(16 + params.metadata_bytes.len() + expected * 6);
        bytes.extend_from_slice(b"RHDR");
        bytes.push(1);
        bytes.push(params.codec.tag());
        bytes.extend_from_slice(&(picture.width as u16).to_le_bytes());
        bytes.extend_from_slice(&(picture.height as u16).to_le_bytes());
        bytes.push(BitDepth::Ten.bits());
        bytes.push(if params.color.full_range { 0 } else { 1 });
        bytes.extend_from_slice(&(params.metadata_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&params.metadata_bytes);
        let range = if params.color.full_range {
            QuantRange::Full
        } else {
            QuantRange::Limited
        };
        for pixel in picture.linear_nits {
            for channel in pixel {
                let code = quantize_pq_10(pq_oetf(f64::from(channel.max(0.0))), range);
                bytes.extend_from_slice(&code.to_le_bytes());
            }
        }
        Ok(EncodedAccessUnit {
            codec: params.codec,
            bytes,
            metadata: params.metadata,
            compressed: false,
        })
    }
}

pub fn decode_reference_au(bytes: &[u8]) -> Result<DecodedHdrPicture, EncodeError> {
    if bytes.len() < 16 || &bytes[0..4] != b"RHDR" || bytes[4] != 1 {
        return Err(EncodeError::NotReferenceContainer);
    }
    let codec = HdrCodec::from_tag(bytes[5])
        .ok_or_else(|| EncodeError::Invalid(format!("unknown RHDR codec tag {}", bytes[5])))?;
    let width = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
    let height = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
    let range = if bytes[11] == 0 {
        QuantRange::Full
    } else {
        QuantRange::Limited
    };
    let meta_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    if bytes.len() < 16 + meta_len {
        return Err(EncodeError::Invalid("truncated metadata".into()));
    }
    let meta_bytes = &bytes[16..16 + meta_len];
    let metadata = parse_embedded_metadata(codec, meta_bytes).ok_or_else(|| {
        EncodeError::Invalid("RHDR metadata did not contain HDR10 SEI or OBUs".into())
    })?;
    let pixels = &bytes[16 + meta_len..];
    let count = width as usize * height as usize;
    if pixels.len() != count * 6 {
        return Err(EncodeError::Invalid(
            "RHDR pixel section does not match the dimensions".into(),
        ));
    }
    let mut nits = Vec::with_capacity(count);
    for index in 0..count {
        let base = index * 6;
        let mut pixel = [0.0f32; 3];
        for channel in 0..3 {
            let code =
                u16::from_le_bytes([pixels[base + channel * 2], pixels[base + channel * 2 + 1]]);
            pixel[channel] = pq_eotf(dequantize_pq_10(code, range)) as f32;
        }
        nits.push(pixel);
    }
    Ok(DecodedHdrPicture {
        width,
        height,
        codec,
        quant_range: range,
        metadata,
        nits,
    })
}

fn parse_embedded_metadata(codec: HdrCodec, bytes: &[u8]) -> Option<Hdr10StaticMetadata> {
    match codec {
        HdrCodec::HevcMain10 => metadata_from_sei(&parse_annex_b_sei(bytes)),
        HdrCodec::Av1Main10 => parse_av1_metadata(bytes),
    }
}

fn parse_av1_metadata(bytes: &[u8]) -> Option<Hdr10StaticMetadata> {
    let mut index = 0;
    let mut mastering = None;
    let mut cll = None;
    while index < bytes.len() {
        let header = bytes[index];
        index += 1;
        let obu_type = (header >> 3) & 0x0f;
        let has_size = header & 0x02 != 0;
        if !has_size {
            break;
        }
        let size = read_leb128(bytes, &mut index)? as usize;
        if index + size > bytes.len() {
            break;
        }
        let payload = &bytes[index..index + size];
        index += size;
        if obu_type == 5 && !payload.is_empty() {
            match payload[0] {
                2 if payload.len() == 25 => {
                    mastering = crate::metadata::parse_mastering_payload(&payload[1..]);
                }
                1 if payload.len() == 5 => {
                    cll = crate::metadata::parse_content_light_payload(&payload[1..]);
                }
                _ => {}
            }
        }
    }
    let mut metadata = mastering?;
    if let Some((max_cll, max_fall)) = cll {
        metadata.max_cll = max_cll;
        metadata.max_fall = max_fall;
    }
    Some(metadata)
}

fn read_leb128(data: &[u8], index: &mut usize) -> Option<u32> {
    let mut value = 0u32;
    let mut shift = 0;
    for _ in 0..4 {
        if *index >= data.len() {
            return None;
        }
        let byte = data[*index];
        *index += 1;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

fn validate_config(config: &HdrEncodeConfig) -> Result<(), EncodeError> {
    if config.width == 0 || config.height == 0 || config.width % 2 != 0 || config.height % 2 != 0 {
        return Err(EncodeError::Invalid(
            "width and height must be even and non-zero for 4:2:0".into(),
        ));
    }
    if config.fps == 0 || config.bitrate_kbps == 0 {
        return Err(EncodeError::Invalid(
            "fps and bitrate must be non-zero".into(),
        ));
    }
    if config.metadata.max_luminance <= config.metadata.min_luminance {
        return Err(EncodeError::Invalid(
            "mastering max luminance must exceed the minimum".into(),
        ));
    }
    Ok(())
}

fn build_parameters(config: &HdrEncodeConfig, hints: &'static [&'static str]) -> EncodeParameters {
    let mut messages = vec![
        crate::metadata::mastering_sei(&config.metadata),
        crate::metadata::content_light_sei(&config.metadata),
    ];
    if let Some(capability) = &config.capability {
        messages.push(SeiMessage {
            payload_type: 5,
            payload: capability.user_data_payload(),
        });
    }
    let metadata_bytes = match config.codec {
        HdrCodec::HevcMain10 => crate::metadata::hevc_prefix_sei(&messages),
        HdrCodec::Av1Main10 => crate::metadata::av1_hdr_metadata_obus(&config.metadata),
    };
    let profile_name = match config.codec {
        HdrCodec::HevcMain10 => "Main 10",
        HdrCodec::Av1Main10 => "Main profile, 10-bit 4:2:0",
    };
    EncodeParameters {
        backend: config.backend,
        codec: config.codec,
        profile_name,
        bit_depth: 10,
        width: config.width,
        height: config.height,
        color: ColorDescription::hdr10(config.quant_range),
        metadata: config.metadata,
        metadata_bytes,
        hints,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(backend: EncoderBackend, codec: HdrCodec) -> HdrEncodeConfig {
        HdrEncodeConfig {
            codec,
            backend,
            width: 64,
            height: 64,
            fps: 60,
            bitrate_kbps: 12_000,
            metadata: Hdr10StaticMetadata::bt2020_1000(),
            capability: None,
            quant_range: QuantRange::Full,
        }
    }

    #[test]
    fn hardware_backends_negotiate_and_refuse_to_encode() {
        for backend in [
            EncoderBackend::Nvenc,
            EncoderBackend::Amf,
            EncoderBackend::Qsv,
            EncoderBackend::VideoToolbox,
            EncoderBackend::Vaapi,
        ] {
            let contract = backend_contract(backend);
            assert!(!contract.linked);
            assert!(contract.hevc_main10);
            let mut encoder = HardwareContractEncoder { backend };
            let codec = if contract.av1_10bit {
                HdrCodec::Av1Main10
            } else {
                HdrCodec::HevcMain10
            };
            let params = encoder.negotiate(&config(backend, codec)).unwrap();
            assert_eq!(params.bit_depth, 10);
            assert_eq!(params.color.primaries, 9);
            assert_eq!(params.color.transfer, 16);
            assert!(!params.metadata_bytes.is_empty());
            let picture = HdrPicture {
                width: 64,
                height: 64,
                linear_nits: &[[100.0, 100.0, 100.0]; 64 * 64],
            };
            let err = encoder.encode(&picture, &params).unwrap_err();
            assert!(matches!(err, EncodeError::NotLinked { .. }), "{err:?}");
        }
        let vt = HardwareContractEncoder {
            backend: EncoderBackend::VideoToolbox,
        };
        assert!(vt
            .negotiate(&config(EncoderBackend::VideoToolbox, HdrCodec::Av1Main10))
            .is_err());
    }

    #[test]
    fn reference_container_roundtrips_a_thousand_nit_pixel() {
        let mut encoder = ReferenceContainerEncoder;
        let cfg = config(EncoderBackend::ReferenceContainer, HdrCodec::HevcMain10);
        let params = encoder.negotiate(&cfg).unwrap();
        let pixels = vec![[1000.0f32, 100.0, 0.0]; 64 * 64];
        let picture = HdrPicture {
            width: 64,
            height: 64,
            linear_nits: &pixels,
        };
        let unit = encoder.encode(&picture, &params).unwrap();
        assert!(!unit.compressed);
        assert!(unit.bytes.starts_with(b"RHDR"));
        let decoded = decode_reference_au(&unit.bytes).unwrap();
        assert_eq!(decoded.metadata.max_cll, 1000);
        assert!((decoded.metadata.max_luminance - 1000.0).abs() < 1e-3);
        assert!(
            (decoded.nits[0][0] - 1000.0).abs() < 8.0,
            "{}",
            decoded.nits[0][0]
        );
        assert!(
            (decoded.nits[0][1] - 100.0).abs() < 2.0,
            "{}",
            decoded.nits[0][1]
        );
        let annex = crate::metadata::hevc_prefix_sei(&[crate::metadata::mastering_sei(
            &Hdr10StaticMetadata::bt2020_1000(),
        )]);
        assert!(matches!(
            decode_reference_au(&annex),
            Err(EncodeError::NotReferenceContainer)
        ));
    }
}
