//! Transfer functions, primaries, and the conversions a tone-mapper needs.
//!
//! PQ follows SMPTE ST 2084. HLG follows ITU-R BT.2100. Primaries are built
//! from CIE xy and a white point, then inverted, so the matrices are not a
//! pasted table.

/// 1.0 in the PQ signal is this many cd/m².
pub const PQ_PEAK_NITS: f64 = 10_000.0;
/// BT.2408 reference white used when a curve is not absolute.
pub const REFERENCE_WHITE_NITS: f64 = 203.0;
/// scRGB defines code 1.0 as 80 cd/m². Values above 1.0 are the HDR headroom.
pub const SCRGB_WHITE_NITS: f64 = 80.0;

pub const PQ_M1: f64 = 2610.0 / 16384.0;
pub const PQ_M2: f64 = 2523.0 / 32.0;
pub const PQ_C1: f64 = 3424.0 / 4096.0;
pub const PQ_C2: f64 = 2413.0 / 128.0;
pub const PQ_C3: f64 = 2392.0 / 128.0;

/// Published PQ code for 100 cd/m², from the ST 2084 inverse EOTF.
pub const PQ_CODE_100_NITS: f64 = 0.508078421517;
/// Published PQ code for 203 cd/m².
pub const PQ_CODE_203_NITS: f64 = 0.580688881042;
/// Published PQ code for 1 000 cd/m².
pub const PQ_CODE_1000_NITS: f64 = 0.751827096247;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitDepth {
    Eight,
    Ten,
    Twelve,
    SixteenFloat,
}

impl BitDepth {
    pub fn bits(self) -> u8 {
        match self {
            Self::Eight => 8,
            Self::Ten => 10,
            Self::Twelve => 12,
            Self::SixteenFloat => 16,
        }
    }

    pub fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            8 => Some(Self::Eight),
            10 => Some(Self::Ten),
            12 => Some(Self::Twelve),
            16 => Some(Self::SixteenFloat),
            _ => None,
        }
    }
}

/// Transfer of the electrical signal. `Unspecified` is not treated as HDR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferFunction {
    Srgb,
    Bt709,
    Gamma22,
    Pq,
    Hlg,
    Linear,
    ScRgb,
    Unspecified,
}

impl TransferFunction {
    /// H.273 `transfer_characteristics` code, when one exists.
    pub fn h273(self) -> Option<u8> {
        match self {
            Self::Bt709 => Some(1),
            Self::Gamma22 => Some(4),
            Self::Srgb => Some(13),
            Self::Pq => Some(16),
            Self::Hlg => Some(18),
            Self::Linear | Self::ScRgb | Self::Unspecified => None,
        }
    }

    pub fn is_hdr(self) -> bool {
        matches!(self, Self::Pq | Self::Hlg | Self::ScRgb)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorPrimaries {
    Bt709,
    Bt2020,
    DisplayP3,
    DciP3,
}

impl ColorPrimaries {
    /// H.273 `colour_primaries` code.
    pub fn h273(self) -> u8 {
        match self {
            Self::Bt709 => 1,
            Self::Bt2020 => 9,
            Self::DciP3 => 11,
            Self::DisplayP3 => 12,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Bt709 => "BT.709",
            Self::Bt2020 => "BT.2020",
            Self::DisplayP3 => "Display P3",
            Self::DciP3 => "DCI-P3",
        }
    }
}

/// H.273 matrix coefficients used when a 10-bit encoder writes VUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixCoefficients {
    Identity,
    Bt709,
    Bt2020Ncl,
}

impl MatrixCoefficients {
    pub fn h273(self) -> u8 {
        match self {
            Self::Identity => 0,
            Self::Bt709 => 1,
            Self::Bt2020Ncl => 9,
        }
    }

    pub fn for_primaries(primaries: ColorPrimaries) -> Self {
        match primaries {
            ColorPrimaries::Bt2020 => Self::Bt2020Ncl,
            ColorPrimaries::Bt709 | ColorPrimaries::DisplayP3 | ColorPrimaries::DciP3 => {
                Self::Bt709
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceColorimetry {
    pub bit_depth: BitDepth,
    pub transfer: TransferFunction,
    pub primaries: ColorPrimaries,
    pub peak_nits: f32,
    pub min_nits: f32,
    /// The peak came from a default, not from the display or the bitstream.
    pub peak_assumed: bool,
}

impl SourceColorimetry {
    pub fn sdr() -> Self {
        Self {
            bit_depth: BitDepth::Eight,
            transfer: TransferFunction::Srgb,
            primaries: ColorPrimaries::Bt709,
            peak_nits: 100.0,
            min_nits: 0.0,
            peak_assumed: false,
        }
    }

    /// BT.2020 PQ container. 1 000 cd/m² is the usual master, not a measurement.
    pub fn hdr10(peak_nits: f32) -> Self {
        Self {
            bit_depth: BitDepth::Ten,
            transfer: TransferFunction::Pq,
            primaries: ColorPrimaries::Bt2020,
            peak_nits,
            min_nits: 0.005,
            peak_assumed: false,
        }
    }

    pub fn is_hdr(self) -> bool {
        self.transfer.is_hdr()
            || (self.transfer == TransferFunction::Linear && self.peak_nits > 100.5)
    }

    pub fn summary(self) -> String {
        format!(
            "{}-bit {} {} peak {:.0} nits{}",
            self.bit_depth.bits(),
            transfer_label(self.transfer),
            self.primaries.label(),
            self.peak_nits,
            if self.peak_assumed { " (assumed)" } else { "" }
        )
    }
}

pub fn transfer_label(transfer: TransferFunction) -> &'static str {
    match transfer {
        TransferFunction::Srgb => "sRGB",
        TransferFunction::Bt709 => "BT.709",
        TransferFunction::Gamma22 => "gamma 2.2",
        TransferFunction::Pq => "PQ",
        TransferFunction::Hlg => "HLG",
        TransferFunction::Linear => "linear",
        TransferFunction::ScRgb => "scRGB",
        TransferFunction::Unspecified => "unspecified",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xy {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chromaticities {
    pub red: Xy,
    pub green: Xy,
    pub blue: Xy,
    pub white: Xy,
}

impl Chromaticities {
    pub fn bt709() -> Self {
        Self {
            red: Xy { x: 0.640, y: 0.330 },
            green: Xy { x: 0.300, y: 0.600 },
            blue: Xy { x: 0.150, y: 0.060 },
            white: Xy {
                x: 0.3127,
                y: 0.3290,
            },
        }
    }

    pub fn bt2020() -> Self {
        Self {
            red: Xy { x: 0.708, y: 0.292 },
            green: Xy { x: 0.170, y: 0.797 },
            blue: Xy { x: 0.131, y: 0.046 },
            white: Xy {
                x: 0.3127,
                y: 0.3290,
            },
        }
    }

    pub fn display_p3() -> Self {
        Self {
            red: Xy { x: 0.680, y: 0.320 },
            green: Xy { x: 0.265, y: 0.690 },
            blue: Xy { x: 0.150, y: 0.060 },
            white: Xy {
                x: 0.3127,
                y: 0.3290,
            },
        }
    }

    pub fn dci_p3() -> Self {
        Self {
            red: Xy { x: 0.680, y: 0.320 },
            green: Xy { x: 0.265, y: 0.690 },
            blue: Xy { x: 0.150, y: 0.060 },
            white: Xy {
                x: 0.3140,
                y: 0.3510,
            },
        }
    }

    pub fn for_primaries(primaries: ColorPrimaries) -> Self {
        match primaries {
            ColorPrimaries::Bt709 => Self::bt709(),
            ColorPrimaries::Bt2020 => Self::bt2020(),
            ColorPrimaries::DisplayP3 => Self::display_p3(),
            ColorPrimaries::DciP3 => Self::dci_p3(),
        }
    }
}

/// Closest named primary set. Distance is on the red and green xy only.
pub fn match_primaries(samples: Chromaticities) -> ColorPrimaries {
    let sets = [
        ColorPrimaries::Bt709,
        ColorPrimaries::Bt2020,
        ColorPrimaries::DisplayP3,
        ColorPrimaries::DciP3,
    ];
    sets.into_iter()
        .min_by(|left, right| {
            primary_distance(samples, Chromaticities::for_primaries(*left)).total_cmp(
                &primary_distance(samples, Chromaticities::for_primaries(*right)),
            )
        })
        .unwrap_or(ColorPrimaries::Bt709)
}

fn primary_distance(a: Chromaticities, b: Chromaticities) -> f64 {
    let d = |p: Xy, q: Xy| (p.x - q.x).powi(2) + (p.y - q.y).powi(2);
    d(a.red, b.red) + d(a.green, b.green) + d(a.blue, b.blue)
}

#[derive(Clone, Copy, Debug)]
pub struct ColorMatrix {
    pub rows: [[f64; 3]; 3],
}

impl ColorMatrix {
    pub fn to_xyz(primaries: ColorPrimaries) -> Self {
        Self {
            rows: primaries_to_xyz(Chromaticities::for_primaries(primaries)),
        }
    }

    pub fn from_xyz(primaries: ColorPrimaries) -> Self {
        let forward = primaries_to_xyz(Chromaticities::for_primaries(primaries));
        Self {
            rows: invert3(forward).unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
        }
    }

    pub fn convert(from: ColorPrimaries, to: ColorPrimaries) -> Self {
        if from == to {
            return Self::identity();
        }
        Self::from_xyz(to).compose(Self::to_xyz(from))
    }

    pub fn identity() -> Self {
        Self {
            rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    pub fn compose(self, other: Self) -> Self {
        let mut rows = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                rows[i][j] = (0..3).map(|k| self.rows[i][k] * other.rows[k][j]).sum();
            }
        }
        Self { rows }
    }

    pub fn apply(self, value: [f64; 3]) -> [f64; 3] {
        [
            dot(self.rows[0], value),
            dot(self.rows[1], value),
            dot(self.rows[2], value),
        ]
    }

    pub fn to_f32(self) -> [[f32; 3]; 3] {
        self.rows.map(|row| row.map(|value| value as f32))
    }
}

fn dot(row: [f64; 3], value: [f64; 3]) -> f64 {
    row[0] * value[0] + row[1] * value[1] + row[2] * value[2]
}

pub fn primaries_to_xyz(c: Chromaticities) -> [[f64; 3]; 3] {
    let red = xy_y1(c.red);
    let green = xy_y1(c.green);
    let blue = xy_y1(c.blue);
    let white = xy_y1(c.white);
    let unscaled = [
        [red[0], green[0], blue[0]],
        [red[1], green[1], blue[1]],
        [red[2], green[2], blue[2]],
    ];
    let scale = solve3(unscaled, white).unwrap_or([1.0, 1.0, 1.0]);
    [
        [red[0] * scale[0], green[0] * scale[1], blue[0] * scale[2]],
        [red[1] * scale[0], green[1] * scale[1], blue[1] * scale[2]],
        [red[2] * scale[0], green[2] * scale[1], blue[2] * scale[2]],
    ]
}

fn xy_y1(p: Xy) -> [f64; 3] {
    if p.y.abs() < 1e-12 {
        return [0.0, 0.0, 0.0];
    }
    [p.x / p.y, 1.0, (1.0 - p.x - p.y) / p.y]
}

fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let inv = invert3(m)?;
    Some([dot(inv[0], b), dot(inv[1], b), dot(inv[2], b)])
}

fn invert3(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let a = m[0][0];
    let b = m[0][1];
    let c = m[0][2];
    let d = m[1][0];
    let e = m[1][1];
    let f = m[1][2];
    let g = m[2][0];
    let h = m[2][1];
    let i = m[2][2];
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    Some([
        [
            (e * i - f * h) * inv_det,
            (c * h - b * i) * inv_det,
            (b * f - c * e) * inv_det,
        ],
        [
            (f * g - d * i) * inv_det,
            (a * i - c * g) * inv_det,
            (c * d - a * f) * inv_det,
        ],
        [
            (d * h - e * g) * inv_det,
            (b * g - a * h) * inv_det,
            (a * e - b * d) * inv_det,
        ],
    ])
}

pub fn luma_coefficients(primaries: ColorPrimaries) -> [f64; 3] {
    match primaries {
        ColorPrimaries::Bt2020 => [0.2627, 0.6780, 0.0593],
        // Display P3 and DCI-P3 share the P3 primaries. Relative luma for the
        // D65 sets uses the BT.709 weights; DCI white is close enough for MaxFALL.
        ColorPrimaries::Bt709 | ColorPrimaries::DisplayP3 | ColorPrimaries::DciP3 => {
            [0.2126, 0.7152, 0.0722]
        }
    }
}

pub fn luma(rgb: [f64; 3], primaries: ColorPrimaries) -> f64 {
    let c = luma_coefficients(primaries);
    rgb[0] * c[0] + rgb[1] * c[1] + rgb[2] * c[2]
}

/// ST 2084 inverse EOTF: linear cd/m² to a PQ code in 0..1.
pub fn pq_oetf(nits: f64) -> f64 {
    let normalized = (nits.max(0.0) / PQ_PEAK_NITS).clamp(0.0, 1.0);
    let powered = normalized.powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * powered) / (1.0 + PQ_C3 * powered)).powf(PQ_M2)
}

/// ST 2084 EOTF: PQ code in 0..1 to linear cd/m².
pub fn pq_eotf(code: f64) -> f64 {
    if code <= 0.0 {
        return 0.0;
    }
    let narrow = code.clamp(0.0, 1.0).powf(1.0 / PQ_M2);
    let numer = (narrow - PQ_C1).max(0.0);
    let denom = PQ_C2 - PQ_C3 * narrow;
    if denom <= 1e-12 {
        return PQ_PEAK_NITS;
    }
    PQ_PEAK_NITS * (numer / denom).powf(1.0 / PQ_M1)
}

pub fn srgb_oetf(linear: f64) -> f64 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.0031308 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

pub fn srgb_eotf(encoded: f64) -> f64 {
    let encoded = encoded.clamp(0.0, 1.0);
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// ARIB STD-B67 inverse OETF. 1.0 is scene-white, not a nit value.
pub fn hlg_signal_to_scene(signal: f64) -> f64 {
    let signal = signal.clamp(0.0, 1.0);
    let a = 0.17883277;
    let b = 0.28466892;
    let c = 0.55991073;
    if signal <= 0.5 {
        (signal * signal) / 3.0
    } else {
        (((signal - c) / a).exp() + b) / 12.0
    }
}

/// BT.2100 HLG OOTF for an achromatic signal. `peak_nits` is the display peak.
pub fn hlg_signal_to_nits(signal: f64, peak_nits: f64) -> f64 {
    let scene = hlg_signal_to_scene(signal);
    let peak = peak_nits.max(1.0);
    let gamma = 1.2 + 0.42 * (peak / 1000.0).log10();
    peak * scene.powf(gamma)
}

pub fn signal_to_nits(signal: f64, transfer: TransferFunction, peak_nits: f64) -> f64 {
    match transfer {
        TransferFunction::Pq => pq_eotf(signal),
        TransferFunction::Hlg => hlg_signal_to_nits(signal, peak_nits),
        TransferFunction::ScRgb => signal * SCRGB_WHITE_NITS,
        TransferFunction::Linear => signal,
        TransferFunction::Srgb | TransferFunction::Bt709 => srgb_eotf(signal) * peak_nits.max(0.0),
        TransferFunction::Gamma22 => signal.clamp(0.0, 1.0).powf(2.2) * peak_nits.max(0.0),
        TransferFunction::Unspecified => srgb_eotf(signal) * peak_nits.max(0.0),
    }
}

pub fn nits_to_scrgb(nits: f64) -> f64 {
    nits / SCRGB_WHITE_NITS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuantRange {
    /// 0..1023 maps the full PQ 0..1 range.
    Full,
    /// Limited 10-bit video range, 64..940.
    Limited,
}

pub fn quantize_pq_10(code: f64, range: QuantRange) -> u16 {
    let code = code.clamp(0.0, 1.0);
    let value = match range {
        QuantRange::Full => (code * 1023.0).round(),
        QuantRange::Limited => (64.0 + code * 876.0).round(),
    };
    value.clamp(0.0, 1023.0) as u16
}

pub fn dequantize_pq_10(sample: u16, range: QuantRange) -> f64 {
    let sample = sample.min(1023) as f64;
    match range {
        QuantRange::Full => sample / 1023.0,
        QuantRange::Limited => ((sample - 64.0) / 876.0).clamp(0.0, 1.0),
    }
}

/// P010 stores a 10-bit sample in the high bits of a `u16`.
pub fn pack_p010_sample(ten_bit: u16) -> u16 {
    (ten_bit & 0x03ff) << 6
}

pub fn unpack_p010_sample(stored: u16) -> u16 {
    stored >> 6
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f64, expected: f64, tol: f64) {
        assert!(
            (actual - expected).abs() <= tol,
            "actual {actual} expected {expected} tol {tol}"
        );
    }

    #[test]
    fn pq_matches_published_codes_and_roundtrips() {
        close(pq_oetf(0.0), 7.309_573_6e-7, 1e-9);
        close(pq_oetf(100.0), PQ_CODE_100_NITS, 1e-12);
        close(pq_oetf(203.0), PQ_CODE_203_NITS, 1e-12);
        close(pq_oetf(1000.0), PQ_CODE_1000_NITS, 1e-12);
        close(pq_oetf(10_000.0), 1.0, 1e-12);
        assert_eq!(pq_eotf(0.0), 0.0);
        for nits in [1.0, 100.0, 203.0, 1000.0, 4000.0, 10_000.0] {
            close(pq_eotf(pq_oetf(nits)), nits, 1e-6);
        }
    }

    #[test]
    fn ten_bit_codes_and_p010_packing() {
        assert_eq!(quantize_pq_10(pq_oetf(1000.0), QuantRange::Full), 769);
        assert_eq!(quantize_pq_10(pq_oetf(1000.0), QuantRange::Limited), 723);
        assert_eq!(quantize_pq_10(0.0, QuantRange::Limited), 64);
        assert_eq!(quantize_pq_10(1.0, QuantRange::Limited), 940);
        assert_eq!(quantize_pq_10(1.0, QuantRange::Full), 1023);
        let packed = pack_p010_sample(769);
        assert_eq!(unpack_p010_sample(packed), 769);
        assert_eq!(packed & 0x3f, 0);
    }

    #[test]
    fn srgb_mid_gray_and_hlg_anchors() {
        close(srgb_oetf(0.5), 0.735_356_983_052, 1e-12);
        close(srgb_eotf(srgb_oetf(0.18)), 0.18, 1e-12);
        close(hlg_signal_to_scene(0.5), 0.25 / 3.0, 1e-12);
        close(hlg_signal_to_scene(0.0), 0.0, 1e-15);
        close(hlg_signal_to_nits(1.0, 1000.0), 1000.0, 1e-4);
        close(hlg_signal_to_nits(0.0, 1000.0), 0.0, 1e-12);
        close(nits_to_scrgb(80.0), 1.0, 1e-12);
        close(nits_to_scrgb(800.0), 10.0, 1e-12);
    }

    #[test]
    fn primary_matrices_preserve_white_and_roundtrip() {
        for primaries in [
            ColorPrimaries::Bt709,
            ColorPrimaries::Bt2020,
            ColorPrimaries::DisplayP3,
        ] {
            let white = ColorMatrix::to_xyz(primaries).apply([1.0, 1.0, 1.0]);
            let d65 = xy_y1(Xy {
                x: 0.3127,
                y: 0.3290,
            });
            close(white[0], d65[0], 1e-9);
            close(white[1], d65[1], 1e-9);
            close(white[2], d65[2], 1e-9);
            let back = ColorMatrix::from_xyz(primaries).apply(white);
            close(back[0], 1.0, 1e-9);
            close(back[1], 1.0, 1e-9);
            close(back[2], 1.0, 1e-9);
        }
        let converted = ColorMatrix::convert(ColorPrimaries::Bt2020, ColorPrimaries::Bt709);
        let neutral = converted.apply([1.0, 1.0, 1.0]);
        close(neutral[0], 1.0, 1e-9);
        close(neutral[1], 1.0, 1e-9);
        close(neutral[2], 1.0, 1e-9);
        let red = converted.apply([1.0, 0.0, 0.0]);
        assert!(red[0] > 1.0, "BT.2020 red is outside BT.709");
        assert!(match_primaries(Chromaticities::bt2020()) == ColorPrimaries::Bt2020);
        assert!(match_primaries(Chromaticities::display_p3()) == ColorPrimaries::DisplayP3);
    }

    #[test]
    fn h273_codes_for_hdr10() {
        assert_eq!(TransferFunction::Pq.h273(), Some(16));
        assert_eq!(TransferFunction::Hlg.h273(), Some(18));
        assert_eq!(ColorPrimaries::Bt2020.h273(), 9);
        assert_eq!(
            MatrixCoefficients::for_primaries(ColorPrimaries::Bt2020).h273(),
            9
        );
        assert!(SourceColorimetry::hdr10(1000.0).is_hdr());
        assert!(!SourceColorimetry::sdr().is_hdr());
    }
}
