//! WGSL compute shader for the same three curves as [`crate::CpuToneMapper`].
//! The source is generated from the ST 2084 constants so the GPU program cannot
//! drift from the CPU reference. Dispatch itself is not linked.

use crate::color::{PQ_C1, PQ_C2, PQ_C3, PQ_M1, PQ_M2};

pub fn tonemap_wgsl() -> String {
    format!(
        r#"
// Rencord HDR tone-map. curve 0 = BT.2390, 1 = Hable, 2 = ACES.
// Input pixels are linear cd/m2 RGB in the source primaries.
// Output pixels are display-linear 0..1 in the destination primaries.

struct Params {{
  curve: u32,
  src_min: f32,
  src_max: f32,
  dst_min: f32,
  dst_max: f32,
  ref_white: f32,
  exposure: f32,
  dst_peak: f32,
  m0: vec4<f32>,
  m1: vec4<f32>,
  m2: vec4<f32>,
  luma: vec4<f32>,
}}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> pixels_in: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> pixels_out: array<vec4<f32>>;

const PQ_M1: f32 = {m1:.8};
const PQ_M2: f32 = {m2:.8};
const PQ_C1: f32 = {c1:.8};
const PQ_C2: f32 = {c2:.8};
const PQ_C3: f32 = {c3:.8};

fn pq_oetf(nits: f32) -> f32 {{
  let normalized = clamp(nits / 10000.0, 0.0, 1.0);
  let powered = pow(max(normalized, 0.0), PQ_M1);
  return pow((PQ_C1 + PQ_C2 * powered) / (1.0 + PQ_C3 * powered), PQ_M2);
}}

fn pq_eotf(code: f32) -> f32 {{
  let narrow = pow(clamp(code, 0.0, 1.0), 1.0 / PQ_M2);
  let numer = max(narrow - PQ_C1, 0.0);
  let denom = max(PQ_C2 - PQ_C3 * narrow, 0.0000001);
  return 10000.0 * pow(numer / denom, 1.0 / PQ_M1);
}}

fn hermite(e1: f32, max_lum: f32, min_lum: f32) -> f32 {{
  let ks = clamp(1.5 * max_lum - 0.5, 0.0, 1.0);
  var e2: f32 = e1;
  if (max_lum < 1.0 && e1 > ks && (1.0 - ks) > 0.000001) {{
    let t = (e1 - ks) / (1.0 - ks);
    let t2 = t * t;
    let t3 = t2 * t;
    e2 = (2.0 * t3 - 3.0 * t2 + 1.0) * ks
      + (t3 - 2.0 * t2 + t) * (1.0 - ks)
      + (-2.0 * t3 + 3.0 * t2) * max_lum;
  }}
  e2 = clamp(e2, 0.0, 1.0);
  let lifted = e2 + min_lum * pow(1.0 - e2, 4.0);
  return clamp(lifted, 0.0, 1.0);
}}

fn bt2390_channel(nits: f32) -> f32 {{
  let src0 = pq_oetf(params.src_min);
  let src1 = pq_oetf(max(params.src_max, params.src_min + 0.001));
  let dst0 = pq_oetf(params.dst_min);
  let dst1 = pq_oetf(max(params.dst_max, params.dst_min));
  let span = max(src1 - src0, 0.0000001);
  let source = pq_oetf(clamp(nits, params.src_min, params.src_max));
  let normalized = clamp((source - src0) / span, 0.0, 1.0);
  let max_lum = clamp((dst1 - src0) / span, 0.0, 1.0);
  let min_lum = clamp((dst0 - src0) / span, 0.0, 1.0);
  var mapped: f32 = normalized;
  if (!(max_lum >= 1.0 && min_lum <= 0.0)) {{
    mapped = hermite(normalized, max_lum, min_lum);
  }}
  let out_nits = pq_eotf(mapped * span + src0);
  return clamp(out_nits, params.dst_min, max(params.dst_max, params.dst_min));
}}

fn hable_partial(x: f32) -> f32 {{
  let a = 0.15;
  let b = 0.50;
  let c = 0.10;
  let d = 0.20;
  let e = 0.02;
  let f = 0.30;
  return ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f;
}}

fn aces_fit(x: f32) -> f32 {{
  let a = 2.51;
  let b = 0.03;
  let c = 2.43;
  let d = 0.59;
  let e = 0.14;
  return clamp((x * (a * x + b)) / (x * (c * x + d) + e), 0.0, 1.0);
}}

fn convert(rgb: vec3<f32>) -> vec3<f32> {{
  let r = dot(params.m0.xyz, rgb);
  let g = dot(params.m1.xyz, rgb);
  let b = dot(params.m2.xyz, rgb);
  return max(vec3<f32>(r, g, b), vec3<f32>(0.0));
}}

@compute @workgroup_size(64)
fn tonemap(@builtin(global_invocation_id) gid: vec3<u32>) {{
  let index = gid.x;
  let count = arrayLength(&pixels_in);
  if (index >= count) {{
    return;
  }}
  let sample = pixels_in[index];
  let rgb = convert(sample.xyz);
  var out_rgb = vec3<f32>(0.0);
  if (params.curve == 0u) {{
    let y = dot(params.luma.xyz, rgb);
    if (y <= 0.000001) {{
      out_rgb = vec3<f32>(0.0);
    }} else {{
      let mapped = bt2390_channel(y);
      let scale = mapped / y;
      let peak = max(params.dst_peak, 0.001);
      out_rgb = clamp((rgb * scale) / peak, vec3<f32>(0.0), vec3<f32>(1.0));
    }}
  }} else if (params.curve == 1u) {{
    let white = max(params.ref_white, 1.0);
    let scale = hable_partial(11.2);
    out_rgb = vec3<f32>(
      clamp(hable_partial((rgb.x / white) * params.exposure) / scale, 0.0, 1.0),
      clamp(hable_partial((rgb.y / white) * params.exposure) / scale, 0.0, 1.0),
      clamp(hable_partial((rgb.z / white) * params.exposure) / scale, 0.0, 1.0),
    );
  }} else {{
    let white = max(params.ref_white, 1.0);
    out_rgb = vec3<f32>(
      aces_fit(rgb.x / white),
      aces_fit(rgb.y / white),
      aces_fit(rgb.z / white),
    );
  }}
  pixels_out[index] = vec4<f32>(out_rgb, 1.0);
}}
"#,
        m1 = PQ_M1,
        m2 = PQ_M2,
        c1 = PQ_C1,
        c2 = PQ_C2,
        c3 = PQ_C3,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{PQ_C1, PQ_M1};

    #[test]
    fn shader_parses_as_wgsl_and_carries_the_pq_constants() {
        let source = tonemap_wgsl();
        assert!(source.contains("fn bt2390_channel"));
        assert!(source.contains("fn hable_partial"));
        assert!(source.contains("fn aces_fit"));
        assert!(source.contains("@workgroup_size(64)"));
        let module = naga::front::wgsl::parse_str(&source).expect("wgsl parse");
        assert!(
            module
                .entry_points
                .iter()
                .any(|entry| entry.name == "tonemap"),
            "missing compute entry point"
        );
        let rendered = format!("{PQ_M1:.8}");
        assert!(source.contains(&rendered), "shader missed {rendered}");
        assert!(source.contains(&format!("{PQ_C1:.8}")));
    }
}
