//! HDR source detection.
//!
//! Milestone 4 owns screen capture. This module does not open DXGI, Screen
//! Capture Kit, or PipeWire. It classifies the descriptors that capture code
//! already has, parses CTA-861 HDR blocks out of EDID, and on Linux reads
//! `/sys/class/drm/*/edid` when the file is present.

use std::fs;
use std::path::{Path, PathBuf};

use crate::color::{
    match_primaries, BitDepth, Chromaticities, ColorPrimaries, SourceColorimetry, TransferFunction,
    Xy,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
    Unknown,
}

pub fn current_platform() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else {
        Platform::Unknown
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProbeError {
    #[error("the capture descriptor has no color space to classify")]
    Insufficient,
    #[error("edid header is not a base block")]
    BadEdid,
}

/// What a capture backend already knows. Every field is optional.
#[derive(Clone, Debug, Default)]
pub struct CapturedColorDesc {
    pub name: String,
    pub bits_per_color: Option<u8>,
    pub dxgi_color_space: Option<u32>,
    pub apple_pixel_format: Option<u32>,
    pub apple_edr_headroom: Option<f32>,
    pub apple_reference_white_nits: Option<f32>,
    pub wayland_transfer: Option<u32>,
    pub wayland_primaries: Option<u32>,
    /// `wp_image_description` max_lum, in cd/m².
    pub wayland_max_lum_nits: Option<u32>,
    /// Protocol min_lum, which is cd/m² times 10 000.
    pub wayland_min_lum_scaled: Option<u32>,
    pub pipewire_caps: Option<String>,
    /// `DXGI_FORMAT_*` when the capture desc came from a Windows texture.
    pub dxgi_format: Option<u32>,
    pub edid: Option<Vec<u8>>,
    pub mastering_max_nits: Option<f32>,
    pub mastering_min_nits: Option<f32>,
}

impl CapturedColorDesc {
    pub fn resolve(&self) -> Result<SourceColorimetry, ProbeError> {
        if let Some(edid) = &self.edid {
            if let Some(color) = colorimetry_from_edid(edid) {
                return Ok(apply_overrides(color, self));
            }
        }
        if let Some(space) = self.dxgi_color_space {
            if let Some(color) = colorimetry_from_dxgi(space, self.bits_per_color, self) {
                return Ok(color);
            }
        }
        if self.apple_pixel_format.is_some() || self.apple_edr_headroom.is_some() {
            return Ok(colorimetry_from_apple(self));
        }
        if self.wayland_transfer.is_some() || self.wayland_primaries.is_some() {
            return Ok(colorimetry_from_wayland(self));
        }
        if let Some(caps) = &self.pipewire_caps {
            return Ok(apply_overrides(parse_pipewire_caps(caps), self));
        }
        Err(ProbeError::Insufficient)
    }
}

fn apply_overrides(mut color: SourceColorimetry, desc: &CapturedColorDesc) -> SourceColorimetry {
    if let Some(peak) = desc.mastering_max_nits {
        color.peak_nits = peak;
        color.peak_assumed = false;
    }
    if let Some(min) = desc.mastering_min_nits {
        color.min_nits = min;
    }
    if let Some(bits) = desc.bits_per_color.and_then(BitDepth::from_bits) {
        color.bit_depth = bits;
    }
    color
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplaySource {
    Edid,
    Dxgi,
    ScreenCaptureKit,
    Wayland,
    PipeWire,
    Descriptor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayHdrInfo {
    pub name: String,
    pub colorimetry: SourceColorimetry,
    pub source: DisplaySource,
    pub eotfs: Vec<TransferFunction>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HdrSourceReport {
    pub platform: Platform,
    pub displays: Vec<DisplayHdrInfo>,
    pub selected: Option<usize>,
    pub notes: Vec<String>,
}

impl HdrSourceReport {
    pub fn selected_colorimetry(&self) -> Option<SourceColorimetry> {
        self.selected
            .and_then(|index| self.displays.get(index))
            .map(|display| display.colorimetry)
    }

    pub fn summary(&self) -> String {
        match self.selected_colorimetry() {
            Some(color) => format!(
                "{} display{}. {}",
                self.displays.len(),
                if self.displays.len() == 1 { "" } else { "s" },
                color.summary()
            ),
            None => {
                if self.notes.is_empty() {
                    "No display colorimetry.".into()
                } else {
                    self.notes.join(" ")
                }
            }
        }
    }
}

pub trait HdrCaptureProbe {
    fn probe(&mut self) -> Result<HdrSourceReport, ProbeError>;
}

#[derive(Clone, Debug, Default)]
pub struct DescriptorProbe {
    pub desc: CapturedColorDesc,
}

impl HdrCaptureProbe for DescriptorProbe {
    fn probe(&mut self) -> Result<HdrSourceReport, ProbeError> {
        let color = self.desc.resolve()?;
        let source = if self.desc.edid.is_some() {
            DisplaySource::Edid
        } else if self.desc.dxgi_color_space.is_some() {
            DisplaySource::Dxgi
        } else if self.desc.apple_pixel_format.is_some() || self.desc.apple_edr_headroom.is_some() {
            DisplaySource::ScreenCaptureKit
        } else if self.desc.wayland_transfer.is_some() {
            DisplaySource::Wayland
        } else if self.desc.pipewire_caps.is_some() {
            DisplaySource::PipeWire
        } else {
            DisplaySource::Descriptor
        };
        Ok(HdrSourceReport {
            platform: current_platform(),
            displays: vec![DisplayHdrInfo {
                name: if self.desc.name.is_empty() {
                    "capture".into()
                } else {
                    self.desc.name.clone()
                },
                colorimetry: color,
                source,
                eotfs: vec![color.transfer],
            }],
            selected: Some(0),
            notes: vec![
                "Colorimetry came from a capture descriptor. This crate did not open the device."
                    .into(),
            ],
        })
    }
}

#[derive(Clone, Debug)]
pub struct HostProbe {
    pub edid_root: PathBuf,
    pub wayland_display: Option<String>,
    pub pipewire_runtime: Option<PathBuf>,
}

impl HostProbe {
    pub fn system() -> Self {
        Self {
            edid_root: PathBuf::from("/sys/class/drm"),
            wayland_display: std::env::var("WAYLAND_DISPLAY").ok(),
            pipewire_runtime: std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        }
    }
}

impl HdrCaptureProbe for HostProbe {
    fn probe(&mut self) -> Result<HdrSourceReport, ProbeError> {
        let mut notes = Vec::new();
        match current_platform() {
            Platform::Linux => notes.push(
                "Linux color comes from EDID under sysfs. wp_color_management_v1 is not bound, and PipeWire is not opened."
                    .into(),
            ),
            Platform::Windows => notes.push(
                "DXGI is not opened here. Pass a CapturedColorDesc with dxgi_color_space from the capture backend."
                    .into(),
            ),
            Platform::MacOs => notes.push(
                "ScreenCaptureKit is not opened here. Pass a CapturedColorDesc with the pixel format and EDR headroom."
                    .into(),
            ),
            Platform::Unknown => notes.push("No platform probe for this target.".into()),
        }
        if self.wayland_display.is_some() {
            notes.push(
                "WAYLAND_DISPLAY is set. The color-management protocol was not queried.".into(),
            );
        }
        if let Some(runtime) = &self.pipewire_runtime {
            let socket = pipewire_socket_path(runtime);
            if socket.exists() {
                notes.push(format!(
                    "PipeWire socket {} exists. Format colorimetry is only parsed from a caps string the capture side supplies.",
                    socket.display()
                ));
            }
        }
        let displays = read_edid_dir(&self.edid_root);
        if displays.is_empty() && current_platform() == Platform::Linux {
            notes.push(format!(
                "No readable EDID under {}.",
                self.edid_root.display()
            ));
        }
        let selected = select_display(&displays);
        Ok(HdrSourceReport {
            platform: current_platform(),
            displays,
            selected,
            notes,
        })
    }
}

pub fn pipewire_socket_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("pipewire-0")
}

fn select_display(displays: &[DisplayHdrInfo]) -> Option<usize> {
    if displays.is_empty() {
        return None;
    }
    let hdr = displays
        .iter()
        .enumerate()
        .filter(|(_, display)| display.colorimetry.is_hdr())
        .max_by(|left, right| {
            left.1
                .colorimetry
                .peak_nits
                .total_cmp(&right.1.colorimetry.peak_nits)
        });
    Some(hdr.map(|(index, _)| index).unwrap_or(0))
}

pub fn read_edid_dir(root: &Path) -> Vec<DisplayHdrInfo> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut displays = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path().join("edid");
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        if bytes.len() < 128 {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(color) = colorimetry_from_edid(&bytes) {
            let eotfs = edid_eotfs(&bytes);
            displays.push(DisplayHdrInfo {
                name,
                colorimetry: color,
                source: DisplaySource::Edid,
                eotfs,
            });
        }
    }
    displays
}

pub mod dxgi {
    pub const RGB_FULL_G22_NONE_P709: u32 = 0;
    pub const RGB_FULL_G10_NONE_P709: u32 = 1;
    pub const RGB_FULL_G2084_NONE_P2020: u32 = 12;
    pub const YCBCR_STUDIO_G2084_LEFT_P2020: u32 = 13;
    pub const RGB_STUDIO_G2084_NONE_P2020: u32 = 14;
    pub const YCBCR_STUDIO_G2084_TOPLEFT_P2020: u32 = 16;
    pub const RGB_FULL_G22_NONE_P2020: u32 = 17;
    pub const YCBCR_STUDIO_GHLG_TOPLEFT_P2020: u32 = 18;
    pub const YCBCR_FULL_GHLG_TOPLEFT_P2020: u32 = 19;
}

pub fn classify_dxgi(color_space: u32) -> Option<(TransferFunction, ColorPrimaries)> {
    use dxgi::*;
    match color_space {
        RGB_FULL_G22_NONE_P709 => Some((TransferFunction::Gamma22, ColorPrimaries::Bt709)),
        RGB_FULL_G10_NONE_P709 => Some((TransferFunction::ScRgb, ColorPrimaries::Bt709)),
        RGB_FULL_G2084_NONE_P2020
        | YCBCR_STUDIO_G2084_LEFT_P2020
        | RGB_STUDIO_G2084_NONE_P2020
        | YCBCR_STUDIO_G2084_TOPLEFT_P2020 => Some((TransferFunction::Pq, ColorPrimaries::Bt2020)),
        RGB_FULL_G22_NONE_P2020 => Some((TransferFunction::Gamma22, ColorPrimaries::Bt2020)),
        YCBCR_STUDIO_GHLG_TOPLEFT_P2020 | YCBCR_FULL_GHLG_TOPLEFT_P2020 => {
            Some((TransferFunction::Hlg, ColorPrimaries::Bt2020))
        }
        _ => None,
    }
}

fn colorimetry_from_dxgi(
    space: u32,
    bits: Option<u8>,
    desc: &CapturedColorDesc,
) -> Option<SourceColorimetry> {
    let (transfer, primaries) = classify_dxgi(space)?;
    let assumed = desc.mastering_max_nits.is_none();
    let peak = desc.mastering_max_nits.unwrap_or(match transfer {
        TransferFunction::Pq => 1000.0,
        TransferFunction::Hlg => 1000.0,
        TransferFunction::ScRgb => 400.0,
        _ => 100.0,
    });
    let bit_depth = bits
        .and_then(BitDepth::from_bits)
        .unwrap_or(if transfer.is_hdr() {
            BitDepth::Ten
        } else {
            BitDepth::Eight
        });
    Some(SourceColorimetry {
        bit_depth,
        transfer,
        primaries,
        peak_nits: peak,
        min_nits: desc.mastering_min_nits.unwrap_or(0.0),
        peak_assumed: assumed && transfer.is_hdr(),
    })
}

pub mod apple {
    pub const YUV420_8_VIDEO: u32 = fourcc(b"420v");
    pub const YUV420_8_FULL: u32 = fourcc(b"420f");
    pub const YUV420_10_VIDEO: u32 = fourcc(b"x420");
    pub const YUV420_10_FULL: u32 = fourcc(b"xf20");
    pub const RGB_10_LE: u32 = fourcc(b"l10r");
    pub const RGBA_HALF: u32 = fourcc(b"RGhA");

    pub const fn fourcc(bytes: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplePixelClass {
    pub bit_depth: BitDepth,
    pub half_float: bool,
}

pub fn classify_apple_format(format: u32) -> Option<ApplePixelClass> {
    match format {
        apple::YUV420_8_VIDEO | apple::YUV420_8_FULL => Some(ApplePixelClass {
            bit_depth: BitDepth::Eight,
            half_float: false,
        }),
        apple::YUV420_10_VIDEO | apple::YUV420_10_FULL | apple::RGB_10_LE => {
            Some(ApplePixelClass {
                bit_depth: BitDepth::Ten,
                half_float: false,
            })
        }
        apple::RGBA_HALF => Some(ApplePixelClass {
            bit_depth: BitDepth::SixteenFloat,
            half_float: true,
        }),
        _ => None,
    }
}

fn colorimetry_from_apple(desc: &CapturedColorDesc) -> SourceColorimetry {
    let class = desc
        .apple_pixel_format
        .and_then(classify_apple_format)
        .unwrap_or(ApplePixelClass {
            bit_depth: BitDepth::Eight,
            half_float: false,
        });
    let headroom = desc.apple_edr_headroom.unwrap_or(1.0);
    let reference = desc.apple_reference_white_nits.unwrap_or(100.0);
    if headroom > 1.01 {
        SourceColorimetry {
            bit_depth: if class.half_float {
                BitDepth::SixteenFloat
            } else {
                class.bit_depth
            },
            // EDR headroom is extended-linear on top of the SDR white, not a PQ tag.
            transfer: TransferFunction::Linear,
            primaries: ColorPrimaries::DisplayP3,
            peak_nits: reference * headroom,
            min_nits: 0.0,
            peak_assumed: desc.apple_reference_white_nits.is_none(),
        }
    } else {
        SourceColorimetry {
            bit_depth: class.bit_depth,
            transfer: if class.bit_depth == BitDepth::Eight {
                TransferFunction::Srgb
            } else {
                TransferFunction::Unspecified
            },
            primaries: ColorPrimaries::DisplayP3,
            peak_nits: reference,
            min_nits: 0.0,
            peak_assumed: false,
        }
    }
}

/// `wp_color_manager_v1.transfer_function` values from the staging protocol.
pub fn wayland_transfer(value: u32) -> Option<TransferFunction> {
    match value {
        1 => Some(TransferFunction::Bt709),
        2 => Some(TransferFunction::Gamma22),
        5 => Some(TransferFunction::Linear),
        9 | 10 => Some(TransferFunction::Srgb),
        11 => Some(TransferFunction::Pq),
        13 => Some(TransferFunction::Hlg),
        _ => None,
    }
}

pub fn wayland_primaries(value: u32) -> Option<ColorPrimaries> {
    match value {
        1 => Some(ColorPrimaries::Bt709),
        6 => Some(ColorPrimaries::Bt2020),
        8 => Some(ColorPrimaries::DciP3),
        9 => Some(ColorPrimaries::DisplayP3),
        _ => None,
    }
}

fn colorimetry_from_wayland(desc: &CapturedColorDesc) -> SourceColorimetry {
    let transfer = desc
        .wayland_transfer
        .and_then(wayland_transfer)
        .unwrap_or(TransferFunction::Unspecified);
    let primaries = desc
        .wayland_primaries
        .and_then(wayland_primaries)
        .unwrap_or(if transfer.is_hdr() {
            ColorPrimaries::Bt2020
        } else {
            ColorPrimaries::Bt709
        });
    let peak = desc
        .wayland_max_lum_nits
        .map(|value| value as f32)
        .or(desc.mastering_max_nits)
        .unwrap_or(if transfer == TransferFunction::Pq {
            10_000.0
        } else if transfer == TransferFunction::Hlg {
            1000.0
        } else {
            100.0
        });
    let min = desc
        .wayland_min_lum_scaled
        .map(|value| value as f32 / 10_000.0)
        .unwrap_or(0.0);
    let assumed = desc.wayland_max_lum_nits.is_none() && desc.mastering_max_nits.is_none();
    SourceColorimetry {
        bit_depth: if transfer.is_hdr() {
            BitDepth::Ten
        } else {
            BitDepth::Eight
        },
        transfer,
        primaries,
        peak_nits: peak,
        min_nits: min,
        peak_assumed: assumed && transfer.is_hdr(),
    }
}

/// Pixel layout a capture backend can hand the tone-mapper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePixelFormat {
    /// 8-bit BGRA. The SDR preview path.
    Bgra8,
    /// `DXGI_FORMAT_R16G16B16A16_FLOAT` (10). scRGB, 1.0 = 80 cd/m².
    DxgiRgba16Float,
    /// `DXGI_FORMAT_R10G10B10A2_UNORM` (24). PQ in the 10-bit channels.
    DxgiR10G10B10A2,
    /// ScreenCaptureKit `x420` / `xf20` 10-bit 4:2:0. Reported, not unpacked here.
    SckYuv42010,
    /// PipeWire `P010`.
    PipeWireP010,
}

pub const DXGI_FORMAT_R16G16B16A16_FLOAT: u32 = 10;
pub const DXGI_FORMAT_R10G10B10A2_UNORM: u32 = 24;

pub fn capture_pixel_format(desc: &CapturedColorDesc) -> CapturePixelFormat {
    if desc.dxgi_format == Some(DXGI_FORMAT_R16G16B16A16_FLOAT) {
        return CapturePixelFormat::DxgiRgba16Float;
    }
    if desc.dxgi_format == Some(DXGI_FORMAT_R10G10B10A2_UNORM) {
        return CapturePixelFormat::DxgiR10G10B10A2;
    }
    if let Some(format) = desc.apple_pixel_format {
        if matches!(
            format,
            apple::YUV420_10_VIDEO | apple::YUV420_10_FULL | apple::RGB_10_LE
        ) {
            return CapturePixelFormat::SckYuv42010;
        }
    }
    if desc
        .pipewire_caps
        .as_deref()
        .is_some_and(|caps| caps.to_ascii_lowercase().contains("p010"))
    {
        return CapturePixelFormat::PipeWireP010;
    }
    CapturePixelFormat::Bgra8
}

/// scRGB half-float or 10-bit PQ texels to linear cd/m² RGB.
pub fn samples_to_nits(format: CapturePixelFormat, bytes: &[u8]) -> Result<Vec<[f32; 3]>, String> {
    match format {
        CapturePixelFormat::DxgiRgba16Float => {
            if bytes.len() % 8 != 0 {
                return Err("R16G16B16A16_FLOAT length is not a multiple of 8".into());
            }
            let mut out = Vec::with_capacity(bytes.len() / 8);
            for pixel in bytes.chunks_exact(8) {
                let r = f16_to_f32(u16::from_le_bytes([pixel[0], pixel[1]]));
                let g = f16_to_f32(u16::from_le_bytes([pixel[2], pixel[3]]));
                let b = f16_to_f32(u16::from_le_bytes([pixel[4], pixel[5]]));
                out.push([r * 80.0, g * 80.0, b * 80.0]);
            }
            Ok(out)
        }
        CapturePixelFormat::DxgiR10G10B10A2 => {
            if bytes.len() % 4 != 0 {
                return Err("R10G10B10A2 length is not a multiple of 4".into());
            }
            let mut out = Vec::with_capacity(bytes.len() / 4);
            for pixel in bytes.chunks_exact(4) {
                let packed = u32::from_le_bytes(pixel.try_into().unwrap());
                let channel = |shift: u32| {
                    let code = ((packed >> shift) & 0x3ff) as u16;
                    crate::color::pq_eotf(crate::color::dequantize_pq_10(
                        code,
                        crate::color::QuantRange::Full,
                    )) as f32
                };
                out.push([channel(0), channel(10), channel(20)]);
            }
            Ok(out)
        }
        CapturePixelFormat::Bgra8
        | CapturePixelFormat::SckYuv42010
        | CapturePixelFormat::PipeWireP010 => Err(format!(
            "{format:?} is not a packed RGB texel this function expands"
        )),
    }
}

fn f16_to_f32(bits: u16) -> f32 {
    let sign = (bits >> 15) & 1;
    let exp = (bits >> 10) & 0x1f;
    let frac = bits & 0x3ff;
    let out = if exp == 0 {
        if frac == 0 {
            (sign as u32) << 31
        } else {
            let mut mantissa = frac;
            let mut exponent = 127 - 14;
            while mantissa & 0x400 == 0 {
                mantissa <<= 1;
                exponent -= 1;
            }
            mantissa &= 0x3ff;
            ((sign as u32) << 31) | (exponent << 23) | ((mantissa as u32) << 13)
        }
    } else if exp == 31 {
        ((sign as u32) << 31) | (0xff << 23) | ((frac as u32) << 13)
    } else {
        ((sign as u32) << 31) | (((exp as u32) + (127 - 15)) << 23) | ((frac as u32) << 13)
    };
    f32::from_bits(out)
}

pub fn parse_pipewire_caps(caps: &str) -> SourceColorimetry {
    let tokens: Vec<String> = caps
        .split([',', ';', ' ', '/', '=', ':', '\n', '\t'])
        .map(|part| part.trim().to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    let has = |needle: &str| {
        tokens
            .iter()
            .any(|token| token == needle || token.contains(needle))
    };
    let transfer = if has("2084")
        || has("smpte2084")
        || has("bt2100-pq")
        || tokens.iter().any(|t| t == "pq")
    {
        TransferFunction::Pq
    } else if has("hlg") || has("arib-std-b67") {
        TransferFunction::Hlg
    } else if has("scrgb") {
        TransferFunction::ScRgb
    } else if has("linear") {
        TransferFunction::Linear
    } else {
        TransferFunction::Srgb
    };
    let primaries = if has("bt2020") || has("bt2100") || has("2020") {
        ColorPrimaries::Bt2020
    } else if has("display-p3") || has("displayp3") {
        ColorPrimaries::DisplayP3
    } else {
        ColorPrimaries::Bt709
    };
    let bit_depth = if has("p010") || has("x420") || has("y210") || has("10bit") || has("10-bit") {
        BitDepth::Ten
    } else if has("p016") || has("12bit") {
        BitDepth::Twelve
    } else {
        BitDepth::Eight
    };
    let mut peak = None;
    for window in tokens.windows(2) {
        if window[0] == "peak-nits" || window[0] == "max-luminance" || window[0] == "max_lum" {
            peak = window[1].parse::<f32>().ok();
        }
    }
    let peak_assumed = peak.is_none() && transfer.is_hdr();
    SourceColorimetry {
        bit_depth: if transfer.is_hdr() && bit_depth == BitDepth::Eight {
            BitDepth::Ten
        } else {
            bit_depth
        },
        transfer,
        primaries,
        peak_nits: peak.unwrap_or(if transfer.is_hdr() { 1000.0 } else { 100.0 }),
        min_nits: if transfer == TransferFunction::Pq {
            0.005
        } else {
            0.0
        },
        peak_assumed,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EdidHdrBlock {
    pub traditional_sdr: bool,
    pub traditional_hdr: bool,
    pub pq: bool,
    pub hlg: bool,
    pub static_metadata_type_1: bool,
    pub desired_content_max_nits: Option<f32>,
    pub desired_content_max_fall_nits: Option<f32>,
    pub desired_content_min_nits: Option<f32>,
}

pub fn parse_edid_hdr(bytes: &[u8]) -> Vec<EdidHdrBlock> {
    let mut found = Vec::new();
    if bytes.len() < 128 || bytes[0..8] != [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00] {
        return found;
    }
    let extensions = bytes[126] as usize;
    for extension in 0..extensions {
        let start = 128 * (extension + 1);
        if start + 128 > bytes.len() {
            break;
        }
        let block = &bytes[start..start + 128];
        if block[0] != 0x02 || block[1] != 0x03 {
            continue;
        }
        let dtd = block[2] as usize;
        let mut index = 4usize;
        let end = dtd.min(127);
        while index < end {
            let header = block[index];
            let len = (header & 0x1f) as usize;
            let tag = header >> 5;
            if index + 1 + len > 127 {
                break;
            }
            let payload = &block[index + 1..index + 1 + len];
            if tag == 7 && payload.first() == Some(&0x06) {
                if let Some(parsed) = parse_hdr_static_payload(payload) {
                    found.push(parsed);
                }
            }
            index += 1 + len;
        }
    }
    found
}

fn parse_hdr_static_payload(payload: &[u8]) -> Option<EdidHdrBlock> {
    if payload.len() < 2 {
        return None;
    }
    let eotf = payload[1];
    let kinds = payload.get(2).copied().unwrap_or(0);
    let max_code = payload.get(3).copied();
    let fall_code = payload.get(4).copied();
    let min_code = payload.get(5).copied();
    let max_nits = max_code.map(desired_content_luminance);
    let fall_nits = fall_code.map(desired_content_luminance);
    let min_nits = match (max_nits, min_code) {
        (Some(max), Some(code)) => Some(desired_content_min(max, code)),
        _ => None,
    };
    Some(EdidHdrBlock {
        traditional_sdr: eotf & 0x01 != 0,
        traditional_hdr: eotf & 0x02 != 0,
        pq: eotf & 0x04 != 0,
        hlg: eotf & 0x08 != 0,
        static_metadata_type_1: kinds & 0x01 != 0,
        desired_content_max_nits: max_nits,
        desired_content_max_fall_nits: fall_nits,
        desired_content_min_nits: min_nits,
    })
}

pub fn desired_content_luminance(code: u8) -> f32 {
    50.0 * 2.0f32.powf(f32::from(code) / 32.0)
}

pub fn desired_content_min(max_nits: f32, code: u8) -> f32 {
    max_nits * (f32::from(code) / 255.0).powi(2) / 100.0
}

pub fn parse_edid_chromaticity(bytes: &[u8]) -> Option<Chromaticities> {
    if bytes.len() < 35 {
        return None;
    }
    // VESA E-EDID: byte 25 holds the low bits of red and green, byte 26 the
    // low bits of blue and white, and bytes 27..34 the high 8 bits.
    let low_rg = bytes[25];
    let low_bw = bytes[26];
    let coord = |high: u8, low: u8, shift: u8| -> f64 {
        let value = (u16::from(high) << 2) | u16::from((low >> shift) & 0x03);
        f64::from(value) / 1024.0
    };
    Some(Chromaticities {
        red: Xy {
            x: coord(bytes[27], low_rg, 6),
            y: coord(bytes[28], low_rg, 4),
        },
        green: Xy {
            x: coord(bytes[29], low_rg, 2),
            y: coord(bytes[30], low_rg, 0),
        },
        blue: Xy {
            x: coord(bytes[31], low_bw, 6),
            y: coord(bytes[32], low_bw, 4),
        },
        white: Xy {
            x: coord(bytes[33], low_bw, 2),
            y: coord(bytes[34], low_bw, 0),
        },
    })
}

fn colorimetry_from_edid(bytes: &[u8]) -> Option<SourceColorimetry> {
    if bytes.len() < 128 || bytes[0..8] != [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00] {
        return None;
    }
    let chromaticity = parse_edid_chromaticity(bytes);
    let primaries = chromaticity
        .map(match_primaries)
        .unwrap_or(ColorPrimaries::Bt709);
    let hdr = parse_edid_hdr(bytes).into_iter().next();
    let Some(hdr) = hdr else {
        return Some(SourceColorimetry {
            bit_depth: BitDepth::Eight,
            transfer: TransferFunction::Srgb,
            primaries,
            peak_nits: 100.0,
            min_nits: 0.0,
            peak_assumed: false,
        });
    };
    let transfer = if hdr.pq {
        TransferFunction::Pq
    } else if hdr.hlg {
        TransferFunction::Hlg
    } else {
        TransferFunction::Gamma22
    };
    let (peak, assumed) = match hdr.desired_content_max_nits {
        Some(value) => (value, false),
        None if transfer.is_hdr() => (1000.0, true),
        None => (100.0, false),
    };
    Some(SourceColorimetry {
        bit_depth: if transfer.is_hdr() {
            BitDepth::Ten
        } else {
            BitDepth::Eight
        },
        transfer,
        primaries,
        peak_nits: peak,
        min_nits: hdr.desired_content_min_nits.unwrap_or(0.0),
        peak_assumed: assumed,
    })
}

fn edid_eotfs(bytes: &[u8]) -> Vec<TransferFunction> {
    let mut eotfs = Vec::new();
    for block in parse_edid_hdr(bytes) {
        if block.traditional_sdr {
            eotfs.push(TransferFunction::Gamma22);
        }
        if block.pq {
            eotfs.push(TransferFunction::Pq);
        }
        if block.hlg {
            eotfs.push(TransferFunction::Hlg);
        }
    }
    if eotfs.is_empty() {
        eotfs.push(TransferFunction::Srgb);
    }
    eotfs
}

/// Test helper: a 256-byte EDID with one CTA-861 HDR static metadata block.
#[cfg(test)]
fn synthetic_edid(hdr: &EdidHdrBlock) -> Vec<u8> {
    let mut bytes = vec![0u8; 256];
    bytes[0..8].copy_from_slice(&[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]);
    write_chromaticity(&mut bytes, Chromaticities::bt2020());
    bytes[126] = 1;
    fix_checksum(&mut bytes[0..128]);
    bytes[128] = 0x02;
    bytes[129] = 0x03;
    let payload = hdr_payload(hdr);
    bytes[130] = (4 + 1 + payload.len()) as u8;
    bytes[132] = (7 << 5) | (payload.len() as u8);
    bytes[133..133 + payload.len()].copy_from_slice(&payload);
    fix_checksum(&mut bytes[128..256]);
    bytes
}

#[cfg(test)]
fn hdr_payload(hdr: &EdidHdrBlock) -> Vec<u8> {
    let mut eotf = 0u8;
    if hdr.traditional_sdr {
        eotf |= 0x01;
    }
    if hdr.traditional_hdr {
        eotf |= 0x02;
    }
    if hdr.pq {
        eotf |= 0x04;
    }
    if hdr.hlg {
        eotf |= 0x08;
    }
    let mut payload = vec![0x06, eotf, u8::from(hdr.static_metadata_type_1)];
    if let Some(max) = hdr.desired_content_max_nits {
        payload.push(luminance_code_for(max));
        payload.push(
            hdr.desired_content_max_fall_nits
                .map(luminance_code_for)
                .unwrap_or(0),
        );
        payload.push(0);
    }
    payload
}

#[cfg(test)]
fn luminance_code_for(nits: f32) -> u8 {
    if nits <= 50.0 {
        return 0;
    }
    let code = (nits / 50.0).log2() * 32.0;
    code.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
fn write_chromaticity(bytes: &mut [u8], c: Chromaticities) {
    let pack = |value: f64| -> u16 { (value * 1024.0).round().clamp(0.0, 1023.0) as u16 };
    let red_x = pack(c.red.x);
    let red_y = pack(c.red.y);
    let green_x = pack(c.green.x);
    let green_y = pack(c.green.y);
    let blue_x = pack(c.blue.x);
    let blue_y = pack(c.blue.y);
    let white_x = pack(c.white.x);
    let white_y = pack(c.white.y);
    bytes[25] = (((red_x & 3) as u8) << 6)
        | (((red_y & 3) as u8) << 4)
        | (((green_x & 3) as u8) << 2)
        | ((green_y & 3) as u8);
    bytes[26] = (((blue_x & 3) as u8) << 6)
        | (((blue_y & 3) as u8) << 4)
        | (((white_x & 3) as u8) << 2)
        | ((white_y & 3) as u8);
    bytes[27] = (red_x >> 2) as u8;
    bytes[28] = (red_y >> 2) as u8;
    bytes[29] = (green_x >> 2) as u8;
    bytes[30] = (green_y >> 2) as u8;
    bytes[31] = (blue_x >> 2) as u8;
    bytes[32] = (blue_y >> 2) as u8;
    bytes[33] = (white_x >> 2) as u8;
    bytes[34] = (white_y >> 2) as u8;
}

#[cfg(test)]
fn fix_checksum(block: &mut [u8]) {
    if block.len() < 128 {
        return;
    }
    block[127] = 0;
    let sum: u32 = block.iter().take(127).map(|byte| u32::from(*byte)).sum();
    block[127] = (256 - (sum % 256)) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxgi_distinguishes_scrgb_pq_and_hlg() {
        assert_eq!(
            classify_dxgi(dxgi::RGB_FULL_G2084_NONE_P2020),
            Some((TransferFunction::Pq, ColorPrimaries::Bt2020))
        );
        assert_eq!(
            classify_dxgi(dxgi::RGB_FULL_G10_NONE_P709),
            Some((TransferFunction::ScRgb, ColorPrimaries::Bt709))
        );
        assert_eq!(
            classify_dxgi(dxgi::YCBCR_FULL_GHLG_TOPLEFT_P2020),
            Some((TransferFunction::Hlg, ColorPrimaries::Bt2020))
        );
        assert_eq!(
            classify_dxgi(dxgi::RGB_FULL_G22_NONE_P709),
            Some((TransferFunction::Gamma22, ColorPrimaries::Bt709))
        );
        let desc = CapturedColorDesc {
            dxgi_color_space: Some(dxgi::RGB_FULL_G2084_NONE_P2020),
            bits_per_color: Some(10),
            mastering_max_nits: Some(1000.0),
            mastering_min_nits: Some(0.005),
            ..CapturedColorDesc::default()
        };
        let color = desc.resolve().unwrap();
        assert_eq!(color.bit_depth, BitDepth::Ten);
        assert_eq!(color.transfer, TransferFunction::Pq);
        assert_eq!(color.primaries, ColorPrimaries::Bt2020);
        assert!((color.peak_nits - 1000.0).abs() < f32::EPSILON);
        assert!(!color.peak_assumed);
        assert!(color.is_hdr());
    }

    #[test]
    fn apple_edr_headroom_sets_peak_and_ten_bit_without_headroom_is_not_hdr() {
        assert_eq!(
            classify_apple_format(apple::YUV420_10_VIDEO)
                .unwrap()
                .bit_depth,
            BitDepth::Ten
        );
        assert!(classify_apple_format(apple::RGBA_HALF).unwrap().half_float);
        let edr = CapturedColorDesc {
            apple_pixel_format: Some(apple::RGBA_HALF),
            apple_edr_headroom: Some(2.0),
            apple_reference_white_nits: Some(100.0),
            ..CapturedColorDesc::default()
        };
        let color = edr.resolve().unwrap();
        assert!(color.is_hdr());
        assert_eq!(color.transfer, TransferFunction::Linear);
        assert_eq!(color.primaries, ColorPrimaries::DisplayP3);
        assert!((color.peak_nits - 200.0).abs() < 0.1);
        let ten = CapturedColorDesc {
            apple_pixel_format: Some(apple::YUV420_10_VIDEO),
            apple_edr_headroom: Some(1.0),
            ..CapturedColorDesc::default()
        };
        let sdr = ten.resolve().unwrap();
        assert!(!sdr.is_hdr());
        assert_eq!(sdr.bit_depth, BitDepth::Ten);
        assert_eq!(sdr.transfer, TransferFunction::Unspecified);
    }

    #[test]
    fn wayland_and_pipewire_names() {
        assert_eq!(wayland_transfer(11), Some(TransferFunction::Pq));
        assert_eq!(wayland_transfer(13), Some(TransferFunction::Hlg));
        assert_eq!(wayland_primaries(6), Some(ColorPrimaries::Bt2020));
        assert_eq!(wayland_primaries(9), Some(ColorPrimaries::DisplayP3));
        let wayland = CapturedColorDesc {
            wayland_transfer: Some(11),
            wayland_primaries: Some(6),
            wayland_max_lum_nits: Some(1000),
            wayland_min_lum_scaled: Some(50),
            ..CapturedColorDesc::default()
        };
        let color = wayland.resolve().unwrap();
        assert_eq!(color.transfer, TransferFunction::Pq);
        assert_eq!(color.primaries, ColorPrimaries::Bt2020);
        assert!((color.peak_nits - 1000.0).abs() < 0.1);
        assert!((color.min_nits - 0.005).abs() < 0.0001);
        let caps =
            parse_pipewire_caps("video/x-raw,format=P010,colorimetry=bt2100-pq,peak-nits=400");
        assert_eq!(caps.bit_depth, BitDepth::Ten);
        assert_eq!(caps.transfer, TransferFunction::Pq);
        assert_eq!(caps.primaries, ColorPrimaries::Bt2020);
        assert!((caps.peak_nits - 400.0).abs() < 0.1);
        assert!(!caps.peak_assumed);
    }

    #[test]
    fn edid_hdr_block_reports_pq_hlg_and_desired_nits() {
        assert!((desired_content_luminance(0) - 50.0).abs() < 0.01);
        let code = luminance_code_for(1000.0);
        let decoded = desired_content_luminance(code);
        assert!(
            (decoded - 1000.0).abs() < 20.0,
            "code {code} decoded {decoded}"
        );
        let bytes = synthetic_edid(&EdidHdrBlock {
            traditional_sdr: true,
            traditional_hdr: false,
            pq: true,
            hlg: true,
            static_metadata_type_1: true,
            desired_content_max_nits: Some(1000.0),
            desired_content_max_fall_nits: Some(400.0),
            desired_content_min_nits: None,
        });
        let blocks = parse_edid_hdr(&bytes);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].pq && blocks[0].hlg && blocks[0].static_metadata_type_1);
        let color = colorimetry_from_edid(&bytes).unwrap();
        assert_eq!(color.transfer, TransferFunction::Pq);
        assert_eq!(color.bit_depth, BitDepth::Ten);
        assert!(color.peak_nits > 900.0 && color.peak_nits < 1100.0);
        let chroma = parse_edid_chromaticity(&bytes).unwrap();
        assert_eq!(match_primaries(chroma), ColorPrimaries::Bt2020);
        let dir = std::env::temp_dir().join(format!("rencord-edid-{}", std::process::id()));
        let connector = dir.join("card0-DP-1");
        fs::create_dir_all(&connector).unwrap();
        fs::write(connector.join("edid"), &bytes).unwrap();
        let displays = read_edid_dir(&dir);
        assert_eq!(displays.len(), 1);
        assert!(displays[0].colorimetry.is_hdr());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn descriptor_probe_and_host_probe_do_not_panic() {
        let mut probe = DescriptorProbe {
            desc: CapturedColorDesc {
                name: "test".into(),
                dxgi_color_space: Some(dxgi::RGB_FULL_G10_NONE_P709),
                mastering_max_nits: Some(480.0),
                ..CapturedColorDesc::default()
            },
        };
        let report = probe.probe().unwrap();
        assert_eq!(report.displays[0].source, DisplaySource::Dxgi);
        assert!(report.selected_colorimetry().unwrap().is_hdr());
        assert!(report.summary().contains("scRGB"));
        let mut host = HostProbe {
            edid_root: PathBuf::from("/tmp/rencord-missing-edid-root"),
            wayland_display: Some("wayland-0".into()),
            pipewire_runtime: None,
        };
        let host_report = host.probe().unwrap();
        assert!(host_report.displays.is_empty());
        assert!(host_report
            .notes
            .iter()
            .any(|note| note.contains("WAYLAND")));
        assert!(HostProbe::system().probe().is_ok());
    }

    #[test]
    fn capture_formats_expand_scrgb_and_pq10() {
        let float = CapturedColorDesc {
            dxgi_format: Some(DXGI_FORMAT_R16G16B16A16_FLOAT),
            ..CapturedColorDesc::default()
        };
        assert_eq!(
            capture_pixel_format(&float),
            CapturePixelFormat::DxgiRgba16Float
        );
        let one = f16_to_f32(0x3c00);
        assert!((one - 1.0).abs() < 1e-4, "{one}");
        let bytes = 0x3c00u16.to_le_bytes();
        let mut packed = Vec::new();
        for _ in 0..4 {
            packed.extend_from_slice(&bytes);
        }
        let nits = samples_to_nits(CapturePixelFormat::DxgiRgba16Float, &packed).unwrap();
        assert!((nits[0][0] - 80.0).abs() < 0.1, "{nits:?}");
        let ten = CapturedColorDesc {
            dxgi_format: Some(DXGI_FORMAT_R10G10B10A2_UNORM),
            ..CapturedColorDesc::default()
        };
        assert_eq!(
            capture_pixel_format(&ten),
            CapturePixelFormat::DxgiR10G10B10A2
        );
        let code = crate::color::quantize_pq_10(
            crate::color::pq_oetf(1000.0),
            crate::color::QuantRange::Full,
        ) as u32;
        let word = code | (code << 10) | (code << 20) | (3 << 30);
        let expanded =
            samples_to_nits(CapturePixelFormat::DxgiR10G10B10A2, &word.to_le_bytes()).unwrap();
        assert!(
            (expanded[0][0] - 1000.0).abs() < 8.0,
            "{expanded:?} code {code}"
        );
        let apple = CapturedColorDesc {
            apple_pixel_format: Some(apple::YUV420_10_VIDEO),
            ..CapturedColorDesc::default()
        };
        assert_eq!(
            capture_pixel_format(&apple),
            CapturePixelFormat::SckYuv42010
        );
        let pipe = CapturedColorDesc {
            pipewire_caps: Some("video/x-raw,format=P010".into()),
            ..CapturedColorDesc::default()
        };
        assert_eq!(
            capture_pixel_format(&pipe),
            CapturePixelFormat::PipeWireP010
        );
    }
}
