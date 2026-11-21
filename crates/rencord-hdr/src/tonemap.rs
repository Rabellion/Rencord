//! CPU tone-mapping. BT.2390 is the EETF in Report ITU-R BT.2390 section 5.4:
//! a 1:1 section, a Hermite shoulder, then the black-lift taper. The shoulder
//! is evaluated in PQ after the source range is normalized, so a 1 000 cd/m²
//! master is not treated as if it contained a 10 000 cd/m² highlight.
//!
//! Hable is the Uncharted 2 filmic curve (A=0.15, B=0.50, C=0.10, D=0.20,
//! E=0.02, F=0.30, white = 11.2, exposure 2). ACES is the Narkowicz fit.
//! Those two run per channel on light relative to the 203 cd/m² reference
//! white. BT.2390 scales by the destination luma so the hue stays put.

use std::sync::OnceLock;

use crate::color::{
    pq_eotf, pq_oetf, srgb_oetf, ColorMatrix, ColorPrimaries, REFERENCE_WHITE_NITS,
};

const PQ_LUT: usize = 4096;

struct PqLut {
    /// PQ code at log-spaced nits from `nits_min` to 10 000.
    oetf: Vec<f32>,
    /// Nits at uniform PQ codes.
    eotf: Vec<f32>,
    nits_min: f32,
    scale: f32,
}

impl PqLut {
    fn get() -> &'static Self {
        static LUT: OnceLock<PqLut> = OnceLock::new();
        LUT.get_or_init(|| {
            let nits_min = 1.0e-3f32;
            let nits_max = 10_000.0f32;
            let mut oetf = vec![0.0; PQ_LUT];
            let mut eotf = vec![0.0; PQ_LUT];
            let last = (PQ_LUT - 1) as f64;
            for index in 0..PQ_LUT {
                let unit = index as f64 / last;
                let nits =
                    f64::from(nits_min) * (f64::from(nits_max) / f64::from(nits_min)).powf(unit);
                oetf[index] = pq_oetf(nits) as f32;
                eotf[index] = pq_eotf(unit) as f32;
            }
            PqLut {
                oetf,
                eotf,
                nits_min,
                scale: last as f32 / (nits_max / nits_min).ln(),
            }
        })
    }

    fn oetf(&self, nits: f32) -> f32 {
        if nits <= self.nits_min {
            return self.oetf[0];
        }
        if nits >= 10_000.0 {
            return 1.0;
        }
        sample(&self.oetf, (nits / self.nits_min).ln() * self.scale)
    }

    fn eotf(&self, code: f32) -> f32 {
        sample(&self.eotf, code.clamp(0.0, 1.0) * (PQ_LUT - 1) as f32)
    }
}

fn sample(table: &[f32], position: f32) -> f32 {
    let max_index = (table.len() - 1) as f32;
    let position = position.clamp(0.0, max_index);
    let index = position as usize;
    let next = (index + 1).min(table.len() - 1);
    let t = position - index as f32;
    table[index].mul_add(1.0 - t, table[next] * t)
}

#[derive(Clone, Copy, Debug)]
struct Bt2390Knee {
    src_min: f32,
    src_max: f32,
    src0: f32,
    span: f32,
    max_lum: f32,
    min_lum: f32,
    identity: bool,
    dst_min: f32,
    dst_max: f32,
}

impl Bt2390Knee {
    fn from_params(params: &ToneMapParams) -> Self {
        let src_min = params.src_min_nits.max(0.0);
        let src_max = params.src_max_nits.max(src_min + 1.0e-3);
        let dst_min = params.dst_min_nits.max(0.0);
        let dst_max = params.dst_max_nits.max(dst_min);
        let src0 = pq_oetf(f64::from(src_min)) as f32;
        let src1 = pq_oetf(f64::from(src_max)) as f32;
        let dst0 = pq_oetf(f64::from(dst_min)) as f32;
        let dst1 = pq_oetf(f64::from(dst_max)) as f32;
        let span = (src1 - src0).max(1.0e-6);
        let max_lum = ((dst1 - src0) / span).clamp(0.0, 1.0);
        let min_lum = ((dst0 - src0) / span).clamp(0.0, 1.0);
        Self {
            src_min,
            src_max,
            src0,
            span,
            max_lum,
            min_lum,
            identity: max_lum >= 1.0 && min_lum <= 0.0,
            dst_min,
            dst_max,
        }
    }

    fn map(&self, nits: f32, lut: &PqLut) -> f32 {
        let source = lut.oetf(nits.clamp(self.src_min, self.src_max));
        let normalized = ((source - self.src0) / self.span).clamp(0.0, 1.0);
        let mapped = if self.identity {
            normalized
        } else {
            hermite_f32(normalized, self.max_lum, self.min_lum)
        };
        lut.eotf(mapped * self.span + self.src0)
            .clamp(self.dst_min, self.dst_max)
    }
}

fn hermite_f32(e1: f32, max_lum: f32, min_lum: f32) -> f32 {
    let ks = (1.5 * max_lum - 0.5).clamp(0.0, 1.0);
    let e2 = if max_lum >= 1.0 || e1 <= ks || (1.0 - ks) < 1.0e-6 {
        e1
    } else {
        let t = (e1 - ks) / (1.0 - ks);
        let t2 = t * t;
        let t3 = t2 * t;
        (2.0 * t3 - 3.0 * t2 + 1.0) * ks
            + (t3 - 2.0 * t2 + t) * (1.0 - ks)
            + (-2.0 * t3 + 3.0 * t2) * max_lum
    };
    let e2 = e2.clamp(0.0, 1.0);
    (e2 + min_lum * (1.0 - e2).powi(4)).clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToneCurve {
    Bt2390,
    Hable,
    Aces,
}

impl ToneCurve {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bt2390 => "BT.2390",
            Self::Hable => "Hable",
            Self::Aces => "ACES",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Bt2390 => Self::Hable,
            Self::Hable => Self::Aces,
            Self::Aces => Self::Bt2390,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bt2390" | "bt.2390" | "eetf" => Some(Self::Bt2390),
            "hable" | "filmic" => Some(Self::Hable),
            "aces" => Some(Self::Aces),
            _ => None,
        }
    }
}

/// 1 000 cd/m² PQ input mapped onto a 0..1 000 cd/m² display, from the EETF.
pub const EETF_1000_NITS_ON_1000_DISPLAY: f64 = 713.332_140_595;
/// 4 000 cd/m² on that same 0..10 000 → 0..1 000 map.
pub const EETF_4000_NITS_ON_1000_DISPLAY: f64 = 979.798_879_174;
/// 1 000 cd/m² master, input at the master peak, 0.005..100 cd/m² display, before the final clamp.
pub const EETF_1000_MASTER_ON_100_UNCLAMPED: f64 = 100.166_087_277;
/// 100 cd/m² inside that 1 000 → 100 map.
pub const EETF_100_NITS_INSIDE_1000_TO_100: f64 = 69.660_404_695;

/// Unclamped EETF result in cd/m². The picture path clamps to the destination.
pub fn bt2390_nits_unclamped(
    nits: f64,
    src_min: f64,
    src_max: f64,
    dst_min: f64,
    dst_max: f64,
) -> f64 {
    if src_max <= src_min {
        return dst_min;
    }
    let source = pq_oetf(nits.clamp(src_min, src_max));
    let src0 = pq_oetf(src_min.max(0.0));
    let src1 = pq_oetf(src_max.max(src_min + 1e-6));
    let dst0 = pq_oetf(dst_min.max(0.0));
    let dst1 = pq_oetf(dst_max.max(dst_min));
    let span = (src1 - src0).max(1e-9);
    let normalized = ((source - src0) / span).clamp(0.0, 1.0);
    let max_lum = ((dst1 - src0) / span).clamp(0.0, 1.0);
    let min_lum = ((dst0 - src0) / span).clamp(0.0, 1.0);
    let mapped = if max_lum >= 1.0 && min_lum <= 0.0 {
        normalized
    } else {
        hermite(normalized, max_lum, min_lum)
    };
    pq_eotf(mapped * span + src0)
}

pub fn bt2390_nits(nits: f64, src_min: f64, src_max: f64, dst_min: f64, dst_max: f64) -> f64 {
    bt2390_nits_unclamped(nits, src_min, src_max, dst_min, dst_max)
        .clamp(dst_min, dst_max.max(dst_min))
}

fn hermite(e1: f64, max_lum: f64, min_lum: f64) -> f64 {
    let ks = (1.5 * max_lum - 0.5).clamp(0.0, 1.0);
    let e2 = if max_lum >= 1.0 || e1 <= ks || (1.0 - ks) < 1e-6 {
        e1
    } else {
        let t = (e1 - ks) / (1.0 - ks);
        let t2 = t * t;
        let t3 = t2 * t;
        (2.0 * t3 - 3.0 * t2 + 1.0) * ks
            + (t3 - 2.0 * t2 + t) * (1.0 - ks)
            + (-2.0 * t3 + 3.0 * t2) * max_lum
    };
    let e2 = e2.clamp(0.0, 1.0);
    let lifted = e2 + min_lum * (1.0 - e2).powi(4);
    lifted.clamp(0.0, 1.0)
}

pub fn hable_filmic(x: f64) -> f64 {
    hable_partial(x * 2.0) / hable_partial(11.2)
}

fn hable_f32(x: f32) -> f32 {
    let (a, b, c, d, e, f) = (0.15f32, 0.50, 0.10, 0.20, 0.02, 0.30);
    ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f
}

fn aces_f32(x: f32) -> f32 {
    let (a, b, c, d, e) = (2.51f32, 0.03, 2.43, 0.59, 0.14);
    ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
}

fn hable_partial(x: f64) -> f64 {
    let (a, b, c, d, e, f) = (0.15, 0.50, 0.10, 0.20, 0.02, 0.30);
    ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f
}

pub fn aces_narkowicz(x: f64) -> f64 {
    let (a, b, c, d, e) = (2.51, 0.03, 2.43, 0.59, 0.14);
    let y = (x * (a * x + b)) / (x * (c * x + d) + e);
    y.clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug)]
pub struct ToneMapParams {
    pub curve: ToneCurve,
    pub src_primaries: ColorPrimaries,
    pub dst_primaries: ColorPrimaries,
    pub src_min_nits: f32,
    pub src_max_nits: f32,
    pub dst_min_nits: f32,
    pub dst_max_nits: f32,
    pub reference_white_nits: f32,
}

impl ToneMapParams {
    pub fn hdr10_to_sdr100(curve: ToneCurve) -> Self {
        Self {
            curve,
            src_primaries: ColorPrimaries::Bt2020,
            dst_primaries: ColorPrimaries::Bt709,
            src_min_nits: 0.0,
            src_max_nits: 1000.0,
            dst_min_nits: 0.0,
            dst_max_nits: 100.0,
            reference_white_nits: REFERENCE_WHITE_NITS as f32,
        }
    }
}

/// Display-linear 0..1, where 1 is the destination peak (BT.2390) or the
/// filmic paper white (Hable, ACES).
pub trait ToneMapper {
    fn curve(&self) -> ToneCurve;
    fn map_display_linear(&self, rgb_nits: [f32; 3]) -> [f32; 3];
    fn map_into(&self, src: &[[f32; 3]], dst: &mut [[f32; 3]]);
}

#[derive(Clone, Debug)]
pub struct CpuToneMapper {
    params: ToneMapParams,
    matrix: [[f32; 3]; 3],
    luma: [f32; 3],
    hable_white: f32,
    knee: Bt2390Knee,
}

impl CpuToneMapper {
    pub fn new(params: ToneMapParams) -> Self {
        let luma_f = crate::color::luma_coefficients(params.dst_primaries);
        let knee = Bt2390Knee::from_params(&params);
        Self {
            matrix: ColorMatrix::convert(params.src_primaries, params.dst_primaries).to_f32(),
            luma: [luma_f[0] as f32, luma_f[1] as f32, luma_f[2] as f32],
            hable_white: hable_partial(11.2) as f32,
            knee,
            params,
        }
    }

    pub fn params(&self) -> ToneMapParams {
        self.params
    }

    fn map_one(&self, rgb_nits: [f32; 3]) -> [f32; 3] {
        if rgb_nits.iter().any(|channel| !channel.is_finite()) {
            return [0.0, 0.0, 0.0];
        }
        let converted = apply_f32(self.matrix, rgb_nits);
        let converted = converted.map(|channel| channel.max(0.0));
        match self.params.curve {
            ToneCurve::Bt2390 => self.map_bt2390(converted),
            ToneCurve::Hable => self.map_relative(converted, true),
            ToneCurve::Aces => self.map_relative(converted, false),
        }
    }

    fn map_bt2390(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.map_bt2390_lut(rgb, PqLut::get())
    }

    fn map_bt2390_lut(&self, rgb: [f32; 3], lut: &PqLut) -> [f32; 3] {
        let y = rgb[0] * self.luma[0] + rgb[1] * self.luma[1] + rgb[2] * self.luma[2];
        if y <= 1e-6 {
            return [0.0, 0.0, 0.0];
        }
        let mapped = self.knee.map(y, lut);
        let scale = mapped / y;
        let peak = self.params.dst_max_nits.max(1e-3);
        rgb.map(|channel| (channel * scale / peak).clamp(0.0, 1.0))
    }

    fn map_relative(&self, rgb: [f32; 3], hable: bool) -> [f32; 3] {
        let white = self.params.reference_white_nits.max(1.0);
        rgb.map(|channel| {
            let relative = channel / white;
            if hable {
                (hable_f32(relative * 2.0) / self.hable_white).clamp(0.0, 1.0)
            } else {
                aces_f32(relative)
            }
        })
    }
}

impl ToneMapper for CpuToneMapper {
    fn curve(&self) -> ToneCurve {
        self.params.curve
    }

    fn map_display_linear(&self, rgb_nits: [f32; 3]) -> [f32; 3] {
        self.map_one(rgb_nits)
    }

    fn map_into(&self, src: &[[f32; 3]], dst: &mut [[f32; 3]]) {
        let count = src.len().min(dst.len());
        if self.params.curve == ToneCurve::Bt2390 {
            let lut = PqLut::get();
            for index in 0..count {
                let pixel = src[index];
                if pixel.iter().any(|channel| !channel.is_finite()) {
                    dst[index] = [0.0, 0.0, 0.0];
                    continue;
                }
                let converted = apply_f32(self.matrix, pixel).map(|channel| channel.max(0.0));
                dst[index] = self.map_bt2390_lut(converted, lut);
            }
            return;
        }
        for index in 0..count {
            dst[index] = self.map_one(src[index]);
        }
    }
}

pub fn to_srgb8(display_linear: [f32; 3]) -> [u8; 3] {
    display_linear.map(|channel| {
        let encoded = srgb_oetf(f64::from(channel.clamp(0.0, 1.0)));
        (encoded * 255.0).round().clamp(0.0, 255.0) as u8
    })
}

fn apply_f32(matrix: [[f32; 3]; 3], value: [f32; 3]) -> [f32; 3] {
    [
        matrix[0][0] * value[0] + matrix[0][1] * value[1] + matrix[0][2] * value[2],
        matrix[1][0] * value[0] + matrix[1][1] * value[1] + matrix[1][2] * value[2],
        matrix[2][0] * value[0] + matrix[2][1] * value[1] + matrix[2][2] * value[2],
    ]
}

/// GPU-side program. The WGSL is the tone-map the presenter would dispatch.
/// Executing it needs a GPU adapter, which this crate does not open.
pub struct GpuToneMapper {
    curve: ToneCurve,
    shader: String,
    cpu: CpuToneMapper,
}

impl GpuToneMapper {
    pub fn new(params: ToneMapParams) -> Self {
        Self {
            curve: params.curve,
            shader: crate::shader::tonemap_wgsl(),
            cpu: CpuToneMapper::new(params),
        }
    }

    pub fn shader_source(&self) -> &str {
        &self.shader
    }

    /// No wgpu device is linked. Callers that need pixels use the CPU mapper,
    /// which is the numerical reference for the shader.
    pub fn dispatch_status(&self) -> &'static str {
        "WGSL compute shader is generated and parsed in tests. No GPU adapter is opened, so dispatch falls back to the CPU reference."
    }
}

impl ToneMapper for GpuToneMapper {
    fn curve(&self) -> ToneCurve {
        self.curve
    }

    fn map_display_linear(&self, rgb_nits: [f32; 3]) -> [f32; 3] {
        self.cpu.map_display_linear(rgb_nits)
    }

    fn map_into(&self, src: &[[f32; 3]], dst: &mut [[f32; 3]]) {
        self.cpu.map_into(src, dst);
    }
}

pub fn measure_content_light(pixels: &[[f32; 3]], primaries: ColorPrimaries) -> (u16, u16) {
    let coef = crate::color::luma_coefficients(primaries);
    let mut max_cll = 0.0f32;
    let mut sum = 0.0f32;
    for pixel in pixels {
        let y = pixel[0] * coef[0] as f32 + pixel[1] * coef[1] as f32 + pixel[2] * coef[2] as f32;
        let y = y.max(0.0);
        max_cll = max_cll.max(y);
        sum += y;
    }
    let fall = if pixels.is_empty() {
        0.0
    } else {
        sum / pixels.len() as f32
    };
    (
        max_cll.round().clamp(0.0, 65535.0) as u16,
        fall.round().clamp(0.0, 65535.0) as u16,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f64, expected: f64, tol: f64) {
        assert!(
            (actual - expected).abs() <= tol,
            "actual {actual} expected {expected}"
        );
    }

    #[test]
    fn bt2390_matches_the_published_eetf_samples() {
        close(
            bt2390_nits_unclamped(100.0, 0.0, 10_000.0, 0.0, 1000.0),
            100.0,
            1e-3,
        );
        close(
            bt2390_nits_unclamped(1000.0, 0.0, 10_000.0, 0.0, 1000.0),
            EETF_1000_NITS_ON_1000_DISPLAY,
            1e-3,
        );
        close(
            bt2390_nits_unclamped(4000.0, 0.0, 10_000.0, 0.0, 1000.0),
            EETF_4000_NITS_ON_1000_DISPLAY,
            1e-3,
        );
        close(
            bt2390_nits_unclamped(10_000.0, 0.0, 10_000.0, 0.0, 1000.0),
            1000.0,
            1e-2,
        );
        close(
            bt2390_nits_unclamped(100.0, 0.0, 1000.0, 0.005, 100.0),
            EETF_100_NITS_INSIDE_1000_TO_100,
            1e-3,
        );
        close(
            bt2390_nits_unclamped(1000.0, 0.0, 1000.0, 0.005, 100.0),
            EETF_1000_MASTER_ON_100_UNCLAMPED,
            1e-3,
        );
        close(bt2390_nits(1000.0, 0.0, 1000.0, 0.005, 100.0), 100.0, 1e-6);
        close(bt2390_nits(203.0, 0.0, 1000.0, 0.0, 1000.0), 203.0, 1e-3);
    }

    #[test]
    fn bt2390_is_monotonic_and_filmic_anchors_hold() {
        let mut previous = 0.0;
        for step in 0..=40 {
            let nits = f64::from(step) * 25.0;
            let mapped = bt2390_nits(nits, 0.0, 1000.0, 0.0, 100.0);
            assert!(mapped + 1e-6 >= previous);
            previous = mapped;
        }
        close(hable_filmic(0.0), 0.0, 1e-12);
        close(hable_filmic(1.0), 0.492_918_545_991, 1e-9);
        close(aces_narkowicz(0.0), 0.0, 1e-12);
        close(aces_narkowicz(1.0), 0.803_797_468_354, 1e-9);
        assert!(aces_narkowicz(8.0) <= 1.0);
    }

    #[test]
    fn cpu_mapper_preserves_neutral_and_compresses_the_master_peak() {
        let mapper = CpuToneMapper::new(ToneMapParams::hdr10_to_sdr100(ToneCurve::Bt2390));
        let mid = mapper.map_display_linear([100.0, 100.0, 100.0]);
        assert!((mid[0] - mid[1]).abs() < 1e-4);
        assert!((mid[1] - mid[2]).abs() < 1e-4);
        let expected = (bt2390_nits(100.0, 0.0, 1000.0, 0.0, 100.0) / 100.0) as f32;
        assert!(
            (mid[0] - expected).abs() < 2e-3,
            "mid {mid:?} expected {expected}"
        );
        let peak = mapper.map_display_linear([1000.0, 1000.0, 1000.0]);
        assert!(peak[0] > 0.98 && peak[0] <= 1.0, "{peak:?}");
        let bytes = to_srgb8(peak);
        assert_eq!(bytes, [255, 255, 255]);
        let hable = CpuToneMapper::new(ToneMapParams::hdr10_to_sdr100(ToneCurve::Hable));
        let white = hable.map_display_linear([203.0, 203.0, 203.0]);
        assert!((white[0] - 0.492_918_55).abs() < 2e-3, "{white:?}");
        let aces = CpuToneMapper::new(ToneMapParams::hdr10_to_sdr100(ToneCurve::Aces));
        let paper = aces.map_display_linear([203.0, 203.0, 203.0]);
        assert!((paper[0] - 0.803_797_5).abs() < 2e-3, "{paper:?}");
    }

    #[test]
    fn content_light_and_buffer_mapping() {
        assert_eq!(
            measure_content_light(&[[1000.0, 1000.0, 1000.0]], ColorPrimaries::Bt2020),
            (1000, 1000)
        );
        assert_eq!(
            measure_content_light(
                &[[1000.0, 1000.0, 1000.0], [0.0, 0.0, 0.0]],
                ColorPrimaries::Bt2020
            ),
            (1000, 500)
        );
        let mapper = CpuToneMapper::new(ToneMapParams::hdr10_to_sdr100(ToneCurve::Aces));
        let src = [[0.0, 0.0, 0.0], [203.0, 0.0, 0.0]];
        let mut dst = [[1.0; 3]; 1];
        mapper.map_into(&src, &mut dst);
        assert_eq!(dst.len(), 1);
        assert!(dst[0][0] < 0.01);
    }
}
