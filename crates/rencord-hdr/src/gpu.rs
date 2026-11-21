//! wgpu compute dispatch for the tone-map shader.
//!
//! Backends are whatever wgpu builds on this OS: Vulkan, Metal, or D3D12.
//! A missing adapter is not an error for the caller. [`dispatch`] returns
//! [`GpuError::NoAdapter`] and the CPU mapper runs instead. Pipeline creation
//! is cached. [`dispatch_resident`] times one submit and a GPU wait, and
//! leaves the storage buffer on the device. [`dispatch`] adds a mapped readback
//! for tests and the benchmark example.

use crate::color::{luma_coefficients, ColorMatrix};
use crate::shader::tonemap_wgsl;
use crate::tonemap::{ToneCurve, ToneMapParams};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const GROUP: u32 = 64;
const MAX_GROUPS_X: u32 = 65_535;

#[repr(C)]
struct GpuParams {
    curve: u32,
    src_min: f32,
    src_max: f32,
    dst_min: f32,
    dst_max: f32,
    ref_white: f32,
    exposure: f32,
    dst_peak: f32,
    m0: [f32; 4],
    m1: [f32; 4],
    m2: [f32; 4],
    luma: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterReport {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    /// Software Vulkan / a CPU device. Not a mid-range GPU timing.
    pub software: bool,
}

#[derive(Debug)]
pub struct GpuDispatch {
    pub report: AdapterReport,
    pub pixels: Vec<[f32; 3]>,
    /// Submit, GPU wait, and mapped readback. Shader compile is not included.
    pub elapsed: Duration,
}

/// Tone-mapped pixels that stay in a device-local buffer.
///
/// The live path hands this to an encoder. [`GpuResidentFrame::readback`] is
/// the only way the bytes return to the CPU, and the encoder trait does not
/// call it.
pub struct GpuResidentFrame {
    pub report: AdapterReport,
    pub width: u32,
    pub height: u32,
    /// Submit plus a GPU wait. No staging copy and no map.
    pub elapsed: Duration,
    buffer: wgpu::Buffer,
}

impl GpuResidentFrame {
    pub fn byte_size(&self) -> u64 {
        self.buffer.size()
    }

    pub fn format(&self) -> &'static str {
        "rgba32float-storage"
    }

    pub fn readback(&self) -> Result<Vec<[f32; 3]>, GpuError> {
        let session = session()?;
        let size = self.buffer.size();
        let staging = session.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hdr-stage"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = session
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("hdr-readback"),
            });
        encoder.copy_buffer_to_buffer(&self.buffer, 0, &staging, 0, size);
        session.queue.submit(Some(encoder.finish()));
        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        session
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|err| GpuError::Wgpu(err.to_string()))?;
        receiver
            .recv()
            .map_err(|err| GpuError::Wgpu(err.to_string()))?
            .map_err(|err| GpuError::Wgpu(err.to_string()))?;
        let data = slice
            .get_mapped_range()
            .map_err(|err| GpuError::Wgpu(err.to_string()))?;
        let floats: &[f32] = bytemuck_floats(&data);
        let count = floats.len() / 4;
        let mut pixels = Vec::with_capacity(count);
        for index in 0..count {
            let base = index * 4;
            pixels.push([floats[base], floats[base + 1], floats[base + 2]]);
        }
        drop(data);
        staging.unmap();
        Ok(pixels)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("no wgpu adapter (Vulkan, Metal, or D3D12)")]
    NoAdapter,
    #[error("wgpu: {0}")]
    Wgpu(String),
}

struct Session {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_layout: wgpu::BindGroupLayout,
    report: AdapterReport,
}

fn session() -> Result<&'static Session, GpuError> {
    static SESSION: OnceLock<Result<Session, String>> = OnceLock::new();
    match SESSION.get_or_init(open_session) {
        Ok(session) => Ok(session),
        Err(err) if err == "no-adapter" => Err(GpuError::NoAdapter),
        Err(err) => Err(GpuError::Wgpu(err.clone())),
    }
}

fn open_session() -> Result<Session, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN | wgpu::Backends::METAL | wgpu::Backends::DX12,
        flags: wgpu::InstanceFlags::default(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        display: None,
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: true,
        apply_limit_buckets: false,
    }))
    .map_err(|err| match err {
        wgpu::RequestAdapterError::NotFound { .. } | wgpu::RequestAdapterError::EnvNotSet => {
            "no-adapter".into()
        }
        other => other.to_string(),
    })?;
    let info = adapter.get_info();
    let software = matches!(
        info.device_type,
        wgpu::DeviceType::Cpu | wgpu::DeviceType::Other
    ) || info.name.to_ascii_lowercase().contains("llvmpipe");
    let report = AdapterReport {
        name: info.name,
        backend: format!("{:?}", info.backend),
        device_type: format!("{:?}", info.device_type),
        software,
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("rencord-hdr"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: Default::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(|err| err.to_string())?;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rencord-hdr-tonemap"),
        source: wgpu::ShaderSource::Wgsl(tonemap_wgsl().into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("rencord-hdr-tonemap"),
        layout: None,
        module: &module,
        entry_point: Some("tonemap"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_layout = pipeline.get_bind_group_layout(0);
    Ok(Session {
        device,
        queue,
        pipeline,
        bind_layout,
        report,
    })
}

pub fn adapter_report() -> Result<AdapterReport, GpuError> {
    Ok(session()?.report.clone())
}

pub fn dispatch_resident(
    params: &ToneMapParams,
    width: u32,
    height: u32,
    nits: &[[f32; 3]],
) -> Result<GpuResidentFrame, GpuError> {
    let session = session()?;
    if nits.is_empty() {
        return Err(GpuError::Wgpu("empty frame".into()));
    }
    let count = nits.len();
    let mut input = vec![0.0f32; count * 4];
    for (index, pixel) in nits.iter().enumerate() {
        input[index * 4] = pixel[0];
        input[index * 4 + 1] = pixel[1];
        input[index * 4 + 2] = pixel[2];
        input[index * 4 + 3] = 1.0;
    }
    let gpu_params = gpu_params(params);
    let params_bytes = unsafe {
        std::slice::from_raw_parts(
            (&gpu_params as *const GpuParams).cast::<u8>(),
            std::mem::size_of::<GpuParams>(),
        )
    };
    let uniform = session.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hdr-params"),
        size: params_bytes.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    session.queue.write_buffer(&uniform, 0, params_bytes);
    let input_buffer = session.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hdr-in"),
        size: (input.len() * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    session
        .queue
        .write_buffer(&input_buffer, 0, bytemuck_bytes(&input));
    let output_size = (count * 16) as u64;
    let output = session.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hdr-out"),
        size: output_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let bind = session
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hdr-bind"),
            layout: &session.bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: input_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
            ],
        });
    let groups = (count as u32).div_ceil(GROUP);
    let groups_x = groups.min(MAX_GROUPS_X);
    let groups_y = groups.div_ceil(MAX_GROUPS_X);
    let mut encoder = session
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("hdr-tonemap"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("hdr-tonemap"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&session.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(groups_x, groups_y, 1);
    }
    let started = Instant::now();
    session.queue.submit(Some(encoder.finish()));
    session
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|err| GpuError::Wgpu(err.to_string()))?;
    let elapsed = started.elapsed();
    Ok(GpuResidentFrame {
        report: session.report.clone(),
        width,
        height,
        elapsed,
        buffer: output,
    })
}

/// Dispatch and copy the result back. Tests and the benchmark use this.
/// The Go Live path uses [`dispatch_resident`] and does not call it.
pub fn dispatch(params: &ToneMapParams, nits: &[[f32; 3]]) -> Result<GpuDispatch, GpuError> {
    if nits.is_empty() {
        let report = adapter_report()?;
        return Ok(GpuDispatch {
            report,
            pixels: Vec::new(),
            elapsed: Duration::ZERO,
        });
    }
    let started = Instant::now();
    let resident = dispatch_resident(params, nits.len() as u32, 1, nits)?;
    let pixels = resident.readback()?;
    Ok(GpuDispatch {
        report: resident.report,
        pixels,
        elapsed: started.elapsed(),
    })
}

fn gpu_params(params: &ToneMapParams) -> GpuParams {
    let matrix = ColorMatrix::convert(params.src_primaries, params.dst_primaries).to_f32();
    let luma = luma_coefficients(params.dst_primaries);
    let curve = match params.curve {
        ToneCurve::Bt2390 => 0,
        ToneCurve::Hable => 1,
        ToneCurve::Aces => 2,
    };
    GpuParams {
        curve,
        src_min: params.src_min_nits,
        src_max: params.src_max_nits,
        dst_min: params.dst_min_nits,
        dst_max: params.dst_max_nits,
        ref_white: params.reference_white_nits,
        exposure: 2.0,
        dst_peak: params.dst_max_nits,
        m0: [matrix[0][0], matrix[0][1], matrix[0][2], 0.0],
        m1: [matrix[1][0], matrix[1][1], matrix[1][2], 0.0],
        m2: [matrix[2][0], matrix[2][1], matrix[2][2], 0.0],
        luma: [luma[0] as f32, luma[1] as f32, luma[2] as f32, 0.0],
    }
}

fn bytemuck_bytes(values: &[f32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), values.len() * 4) }
}

fn bytemuck_floats(bytes: &[u8]) -> &[f32] {
    unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<f32>(), bytes.len() / 4) }
}

/// Same f32 formulas as the WGSL shader, including the primary matrix.
/// This is the comparison reference for a GPU run. The f64 EETF stays the
/// published-sample reference and is what [`crate::bt2390_nits`] checks.
pub fn map_shader_formula(params: &ToneMapParams, rgb_nits: [f32; 3]) -> [f32; 3] {
    if rgb_nits.iter().any(|channel| !channel.is_finite()) {
        return [0.0, 0.0, 0.0];
    }
    let gpu = gpu_params(params);
    let converted = [
        (gpu.m0[0] * rgb_nits[0] + gpu.m0[1] * rgb_nits[1] + gpu.m0[2] * rgb_nits[2]).max(0.0),
        (gpu.m1[0] * rgb_nits[0] + gpu.m1[1] * rgb_nits[1] + gpu.m1[2] * rgb_nits[2]).max(0.0),
        (gpu.m2[0] * rgb_nits[0] + gpu.m2[1] * rgb_nits[1] + gpu.m2[2] * rgb_nits[2]).max(0.0),
    ];
    match params.curve {
        ToneCurve::Bt2390 => {
            let y = converted[0] * gpu.luma[0]
                + converted[1] * gpu.luma[1]
                + converted[2] * gpu.luma[2];
            if y <= 1.0e-6 {
                return [0.0, 0.0, 0.0];
            }
            let mapped = bt2390_channel(y, &gpu);
            let scale = mapped / y;
            let peak = gpu.dst_peak.max(0.001);
            converted.map(|channel| (channel * scale / peak).clamp(0.0, 1.0))
        }
        ToneCurve::Hable => {
            let white = gpu.ref_white.max(1.0);
            let scale = hable_partial(11.2);
            converted.map(|channel| {
                (hable_partial(channel / white * gpu.exposure) / scale).clamp(0.0, 1.0)
            })
        }
        ToneCurve::Aces => {
            let white = gpu.ref_white.max(1.0);
            converted.map(|channel| aces_fit(channel / white))
        }
    }
}

fn bt2390_channel(nits: f32, params: &GpuParams) -> f32 {
    let src0 = pq_oetf(params.src_min);
    let src1 = pq_oetf((params.src_max).max(params.src_min + 0.001));
    let dst0 = pq_oetf(params.dst_min);
    let dst1 = pq_oetf(params.dst_max.max(params.dst_min));
    let span = (src1 - src0).max(1.0e-7);
    let source = pq_oetf(nits.clamp(params.src_min, params.src_max));
    let normalized = ((source - src0) / span).clamp(0.0, 1.0);
    let max_lum = ((dst1 - src0) / span).clamp(0.0, 1.0);
    let min_lum = ((dst0 - src0) / span).clamp(0.0, 1.0);
    let mapped = if max_lum >= 1.0 && min_lum <= 0.0 {
        normalized
    } else {
        hermite(normalized, max_lum, min_lum)
    };
    pq_eotf(mapped * span + src0).clamp(params.dst_min, params.dst_max.max(params.dst_min))
}

fn pq_oetf(nits: f32) -> f32 {
    let m1 = crate::color::PQ_M1 as f32;
    let m2 = crate::color::PQ_M2 as f32;
    let c1 = crate::color::PQ_C1 as f32;
    let c2 = crate::color::PQ_C2 as f32;
    let c3 = crate::color::PQ_C3 as f32;
    let normalized = (nits / 10_000.0).clamp(0.0, 1.0);
    let powered = normalized.max(0.0).powf(m1);
    ((c1 + c2 * powered) / (1.0 + c3 * powered)).powf(m2)
}

fn pq_eotf(code: f32) -> f32 {
    let m1 = crate::color::PQ_M1 as f32;
    let m2 = crate::color::PQ_M2 as f32;
    let c1 = crate::color::PQ_C1 as f32;
    let c2 = crate::color::PQ_C2 as f32;
    let c3 = crate::color::PQ_C3 as f32;
    let narrow = code.clamp(0.0, 1.0).powf(1.0 / m2);
    let numer = (narrow - c1).max(0.0);
    let denom = (c2 - c3 * narrow).max(1.0e-7);
    10_000.0 * (numer / denom).powf(1.0 / m1)
}

fn hermite(e1: f32, max_lum: f32, min_lum: f32) -> f32 {
    let ks = (1.5 * max_lum - 0.5).clamp(0.0, 1.0);
    let e2 = if max_lum < 1.0 && e1 > ks && (1.0 - ks) > 1.0e-6 {
        let t = (e1 - ks) / (1.0 - ks);
        let t2 = t * t;
        let t3 = t2 * t;
        (2.0 * t3 - 3.0 * t2 + 1.0) * ks
            + (t3 - 2.0 * t2 + t) * (1.0 - ks)
            + (-2.0 * t3 + 3.0 * t2) * max_lum
    } else {
        e1
    };
    let e2 = e2.clamp(0.0, 1.0);
    (e2 + min_lum * (1.0 - e2).powi(4)).clamp(0.0, 1.0)
}

fn hable_partial(x: f32) -> f32 {
    let (a, b, c, d, e, f) = (0.15f32, 0.50, 0.10, 0.20, 0.02, 0.30);
    ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f
}

fn aces_fit(x: f32) -> f32 {
    let (a, b, c, d, e) = (2.51f32, 0.03, 2.43, 0.59, 0.14);
    ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_matches_the_shader_formula_when_an_adapter_exists() {
        let params = ToneMapParams::hdr10_to_sdr100(ToneCurve::Bt2390);
        let mut pixels = Vec::new();
        for step in 0..32 {
            let nits = step as f32 * 40.0;
            pixels.push([nits, nits * 0.4, nits * 0.1]);
        }
        pixels.push([1000.0, 1000.0, 1000.0]);
        pixels.push([203.0, 40.0, 10.0]);
        let gpu = match dispatch(&params, &pixels) {
            Ok(gpu) => gpu,
            Err(GpuError::NoAdapter) => {
                eprintln!("hdr gpu test skipped: no Vulkan, Metal, or D3D12 adapter");
                return;
            }
            Err(err) => panic!("{err}"),
        };
        assert_eq!(gpu.pixels.len(), pixels.len());
        let mut max_error = 0.0f32;
        for (index, pixel) in pixels.iter().enumerate() {
            let cpu = map_shader_formula(&params, *pixel);
            for channel in 0..3 {
                max_error = max_error.max((gpu.pixels[index][channel] - cpu[channel]).abs());
            }
        }
        assert!(
            max_error < 0.002,
            "max error {max_error} on {} ({})",
            gpu.report.name,
            gpu.report.backend
        );
        eprintln!(
            "hdr gpu {} {} software={} {:.3} ms for {} pixels, max error {max_error:.6}",
            gpu.report.backend,
            gpu.report.name,
            gpu.report.software,
            gpu.elapsed.as_secs_f64() * 1000.0,
            pixels.len()
        );
    }
}
