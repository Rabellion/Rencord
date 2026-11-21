# HDR streaming

Milestone 5 is the HDR path that sits beside Go Live. Go Live (capture, the out-of-process worker, H.264, the viewer socket) is Milestone 4 and is not in this tree. `crates/rencord-hdr` does not open a capture device and does not replace that worker. It exposes the traits the worker can call once it exists: `HdrCaptureProbe`, `HdrEncoder`, `ToneMapper`, and `HdrSignal`.

The `hdr` cargo feature (default) maps a `DeliveryPlan` onto `rencord-media::PixelPath`. Turning the feature off builds the crate with no dependency on those Go Live types. `WorkerCommand` is unchanged.

## What runs

| Piece | In this crate | Not linked |
| --- | --- | --- |
| ST 2084 PQ, BT.2100 HLG, sRGB, scRGB (1.0 = 80 cd/m²) | Yes, with tests | |
| Primaries from CIE xy (BT.709, BT.2020, Display P3, DCI-P3) | Yes | |
| BT.2390 EETF, Hable, ACES Narkowicz fit | CPU, tested against fixed samples | GPU dispatch |
| WGSL compute shader for the same three curves | Parsed with `naga` in tests | No adapter is opened |
| HDR10 mastering display + MaxCLL/MaxFALL | HEVC prefix SEI and AV1 metadata OBUs | |
| CTA-861.3 HDR static metadata, EDID chromaticity | Yes. Linux reads `/sys/class/drm/*/edid` when the file is readable | |
| DXGI color-space ids, ScreenCaptureKit fourccs, Wayland `wp_color_manager_v1` enums, PipeWire caps strings | Classified from a `CapturedColorDesc` the capture side fills in | DXGI, ScreenCaptureKit, and PipeWire are not opened |
| NVENC, AMF, QSV, VideoToolbox, VA-API | `negotiate` returns profile, bit depth, H.273 codes, and the metadata bytes | `encode` returns `NotLinked` |
| Pixel decode | Uncompressed `RHDR` test containers | HEVC and AV1 decoders |
| Discord transport | Nothing is sent | No SFU test has been run |

`RHDR` is a length-prefixed container this crate invented so tests can round-trip 10-bit PQ samples and the real SEI or OBUs. It is not an HEVC or AV1 bitstream. A hardware encoder is expected to attach those same metadata bytes and to consume P010 (`pack_p010_sample` puts the 10-bit code in the high bits).

10-bit full range maps PQ 0..1 to 0..1023. Limited range maps it to 64..940. `quantize_pq_10(PQ(1000 cd/m²), Full)` is 769.

## Tone-map

Input is linear cd/m² RGB.

BT.2390 follows Report ITU-R BT.2390 section 5.4. The source range is normalized in PQ, the knee is `KS = 1.5 * maxLum - 0.5`, the shoulder is a Hermite spline with slope 1 at the knee and slope 0 at the target peak, then the black-lift term `b * (1 - E2)^4` is applied. The result scales luma and keeps the RGB ratios, so a neutral pixel stays neutral. The picture path clamps to the destination peak. One checked sample: a 1 000 cd/m² input on a 0..10 000 → 0..1 000 cd/m² map lands on 713.332 cd/m². The master peak on a 1 000 → 100 cd/m² map lands on the 100 cd/m² clip.

Hable uses the Uncharted 2 coefficients (A 0.15, B 0.50, C 0.10, D 0.20, E 0.02, F 0.30), white 11.2, exposure 2. Reference white is 203 cd/m² (BT.2408), and that white maps to about 0.493 display-linear. ACES is the Narkowicz fit; the same white maps to about 0.804. Both run per channel after the primary conversion, which is the usual filmic behavior and can shift saturated hues. BT.2390 does not.

The default master for the settings preview is BT.2020 PQ at 1 000 cd/m² into BT.709 at 100 cd/m². The curve is selectable: BT.2390, Hable, ACES. It starts on BT.2390.

The shader in `shader.rs` is submitted with wgpu on Vulkan, Metal, or D3D12. `GpuToneMapper` uses that dispatch and falls back to the CPU mapper when `request_adapter` fails. A unit test compares the GPU pixels with the same f32 formulas on the CPU and requires a max error under 0.002. If no adapter exists the test prints a skip line and passes, which is what CI does.

On this machine the only adapter is lavapipe (software Vulkan, LLVM 20.1.2). Release, one submit and a device wait, pipeline already built, buffer left on the device:

| Frame | Time |
| --- | ---: |
| 1920×1080 | 34.0 ms |
| 3840×2160 | 182 ms |

Mapping that 1080p buffer back to the CPU took another 26.6 ms. Go Live does not map it. `SoftwareFrameSink` owns the buffer and the software H.264 access unit keeps the 8-bit preview.

The error against the CPU formula on 34 pixels was 0.000024. A mid-range GPU budget of under 2 ms at 1080p and under 4 ms at 4K was not measured, because no such GPU is attached. Lavapipe is faster than the CPU BT.2390 path and still too slow for a live frame.

Owner check on a real GPU:

```sh
cargo run --release -p rencord-hdr --example tonemap-bench
```

The line names the adapter and prints 1080p and 4K BT.2390 GPU time against the CPU time. Here it printed `llvmpipe (LLVM 20.1.2, 256 bits)`, software=true, 34.01 ms GPU / 86.35 ms CPU at 1080p and 181.85 ms GPU / 354.09 ms CPU at 4K.

The picture path does not call the f64 reference per pixel. PQ encode and decode use a 4096-entry table (log-spaced for the forward curve, uniform in PQ for the inverse). The knee itself is still the EETF. A neutral 100 cd/m² pixel stays within 0.002 of the f64 display-linear value.

Measured with `cargo bench -p rencord-hdr --bench throughput` on this Linux machine, release, one thread, 1920×1080, the BT.2020→BT.709 matrix included:

| Curve | ms / 1080p frame | Mpix/s |
| --- | ---: | ---: |
| BT.2390 | 87.7 | 23.7 |
| Hable | 15.6 | 133 |
| ACES | 12.6 | 165 |

HDR10 SEI plus the two AV1 metadata OBUs: 0.31 µs per pair. BT.2390 at this rate is about 11 frames per second at 1080p. It is the quality path, not a 60 fps grader. Hable and ACES are past 60 fps at 1080p on this core.

## Who gets 10-bit

Discord's clients do not decode an HDR stream. Rencord does not pretend otherwise.

A viewer is HDR-capable only when all of these are true:

1. The sender turned **HDR send** on. The default is off.
2. The viewer is Rencord and its capability has receive set, bit depth at least 10, and PQ or a 10-bit codec.
3. Every member of the audience is that kind of viewer. One official or unknown viewer, or an empty audience, keeps the Discord track on tone-mapped SDR. One Go Live track cannot be both.
4. The caller has set `sfu_hdr_passthrough_proven`. This crate never sets that flag. It defaults to false, so the Discord track stays SDR even for an exclusive Rencord audience.

When some Rencord peers asked for HDR and the track still has to be SDR, `DeliveryPlan::side_channel_hdr` is true. That means a 10-bit access unit *may* be built for a Rencord-only transport. This crate does not transmit it.

`rencord-media::planned_pixel_path` is the hook the Go Live crate calls into `HdrSignal`. It is stricter than `select_pixel_path`: a Rencord viewer that did not advertise receive stays on SDR. `output_path` is still the SDR decision. `GoLive::send_frame` tone-maps an HDR preview with the GPU shader (CPU if wgpu has no adapter) before the existing 8-bit software encode. When `hdr_send` is set it also calls `Hdr10Encoder`. The reference encoder writes `RHDR` plus HDR10 SEI. `UnlinkedHdr10Encoder` is the NVENC, AMF, QSV, VideoToolbox, and VA-API implementation of that trait: it returns not-linked and does not duplicate their session code.

### Where the bytes go

Two encodings, both meant to be ignored by a receiver that does not know them:

- RTP one-byte header extension, id suggestion 12, URI `https://rencord.app/hdr/1`, 8-byte payload. `include_in_discord_sdp` is always false. This crate does not add an `a=extmap` line to Discord's select-protocol SDP. Putting one there could make the server reject the session. That was not tried.
- HEVC user-data-unregistered SEI, payload type 5, UUID `c2f6a1e4-7b3d-4e91-8a55-0d6e9f2b14c8` (chosen for this repository, not registered), then the same 8 bytes. AV1 metadata OBUs carry the HDR10 static metadata, not this capability. A decoder that follows the spec skips an unknown SEI.

`HdrSignal::discord_gateway_payload` returns `None`. The JSON from `side_channel_json` is labelled as not a Discord gateway field. Do not drop it into a voice opcode or a gateway dispatch. Unknown opcodes are a good way to get the session closed.

### What Discord can do to it

Not measured. The likely outcomes, and the reason the default is SDR:

- The media server transcodes Go Live to 8-bit H.264. SEI and 10-bit both disappear. Viewers see SDR or a broken stream.
- The server forwards the bitstream untouched. A Rencord viewer can read the SEI. An official client skips the SEI and decodes whatever the codec is. If the codec is HEVC Main10, an official client that only expects H.264 will show nothing.
- The server strips unknown RTP header extensions and unknown JSON keys, which is allowed. Capability exchange then never arrives.
- The server rejects a non-H.264 codec or a payload it does not list. The call drops.

Until someone records a passthrough test against Discord and sets the flag on purpose, Rencord tone-maps.

## Probes

`CapturedColorDesc` is the struct Milestone 4 should fill from the API it already used for capture.

- Windows: `dxgi_color_space` is the `DXGI_COLOR_SPACE_TYPE` value. 12 is RGB full-range PQ BT.2020. 1 is scRGB (linear, 1.0 = 80 cd/m²). 18 and 19 are HLG BT.2020. Peak nits come from `DXGI_OUTPUT_DESC1::MaxLuminance` when the capture side has it. If that field is missing on an HDR space, the peak is assumed (1 000 cd/m² for PQ/HLG) and `peak_assumed` is set.
- macOS: fourccs `420v`, `420f`, `x420`, `xf20`, `l10r`, `RGhA`. EDR headroom above 1.0 sets linear light and `peak = reference_white * headroom`. Reference white defaults to 100 cd/m² when the display does not say, and that default is marked assumed. 10-bit without headroom stays `Unspecified` and is not called HDR. The primaries assumed for EDR are Display P3, which is the usual Apple framebuffer, not a fact about every display.
- Wayland: transfer 11 is PQ, 13 is HLG, primaries 6 is BT.2020, 9 is Display P3 (`wp_color_manager_v1`). `wayland_max_lum_nits` is cd/m². `wayland_min_lum_scaled` is the protocol's min, cd/m² times 10 000. This crate does not bind the protocol. GPUI already owns the Wayland socket.
- PipeWire: a caps string such as `video/x-raw,format=P010,colorimetry=bt2100-pq,peak-nits=400`. The socket is not opened. A present socket is only a note on the host report.
- Linux EDID: `HostProbe` reads connector EDID files. The HDR block's "desired content max luminance" is what CTA-861.3 stores (`50 * 2^(code/32)` cd/m²). That is a desired level, not a measured panel peak. No HDR block means 8-bit sRGB at 100 cd/m².

## Viewer

`decide_presentation`:

- Stream is SDR, or the display peaks at 200 cd/m² or below, or receive is off: 8-bit sRGB. HDR sources are tone-mapped. SDR sources are divided by their peak and sRGB-encoded.
- Receive is on, the stream is HDR, and the display transfer is PQ or HLG: 10-bit PQ codes. The swapchain is not created. `surface_name` reports `10-bit PQ BT.2020` so the shell can ask the platform for one later. GPUI's current swapchain is not an HDR surface.
- The same, with scRGB or linear and a peak above 200: scRGB floats, 1.0 = 80 cd/m².

`open_access_unit` on Annex-B returns mastering metadata when the SEI is there, and `CompressedNotLinked` when the bitstream has no HDR SEI. It does not decode slices.

## Settings

Settings → HDR. Three chips, JetBrains Mono at 11 px, on a black field with a `#15152A` hairline. The accent `#4F6BFF` fills a chip that is on, with black text, which is the Onyx Ink accent pair. Send, Receive, and the curve. Both send and receive start off. The line under the chips is the plan for a hypothetical 1 000 cd/m² master with no known audience, plus an explicit note that the Go Live worker still tone-maps HDR to 8-bit. A second line is the local probe (EDID on Linux, or a note that DXGI / ScreenCaptureKit was not opened).

Turning the chips on does not start a stream.
