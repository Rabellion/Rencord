<p align="center"><img src="assets/brand/readme-header.png" width="720" alt="Rencord"></p>

<p align="center">
  A native Discord desktop client in Rust. The interface is GPUI. Voice uses an adaptive jitter buffer instead of a fixed 40&nbsp;ms window.
</p>

<p align="center">
  <a href="#build">Build</a>
  &nbsp;·&nbsp;
  <a href="#why-rencord">Why Rencord</a>
  &nbsp;·&nbsp;
  <a href="#feature-status">Feature status</a>
  &nbsp;·&nbsp;
  <a href="#keyboard-and-settings">Keyboard</a>
  &nbsp;·&nbsp;
  <a href="#measured-on-this-machine">Measurements</a>
  &nbsp;·&nbsp;
  <a href="#architecture">Architecture</a>
  &nbsp;·&nbsp;
  <a href="#roadmap">Roadmap</a>
</p>

<p align="center">
  <a href="https://github.com/Rabellion/Rencord/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Rabellion/Rencord/ci.yml?branch=main&label=CI" alt="CI status for Rabellion/Rencord" /></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green" alt="License MIT OR Apache-2.0" /></a>
  <a href="rust-toolchain.toml"><img src="https://img.shields.io/badge/rust-1.100-blue?logo=rust&logoColor=white" alt="Rust 1.100" /></a>
  <img src="https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-informational" alt="Platforms Linux, macOS, and Windows" />
</p>

---

> [!WARNING]
> **Unofficial client. Discord can terminate the account.**
> Rencord talks to Discord as your own user. Discord's Terms of Service forbid unauthorized clients and automating a normal user account outside the official OAuth2 and bot API. That can end the account. Nothing in this repository makes that safe or allowed. Read the [disclaimer](#disclaimer) before you sign in. Use an account you can afford to lose. This project does not solve captchas, scrape, or send bulk messages.

---

## Why Rencord

Several native Rust clients already exist. Rencord is aimed at the gaps that show up in daily use, not at matching every feature on day one.

| | What you get today | What is still ahead |
| --- | --- | --- |
| **Voice cut-outs** | Adaptive jitter target from 40&nbsp;ms to 120&nbsp;ms, Opus PLC and in-band FEC, and a one-frame time-stretch instead of a hard skip. A loss simulation beats a fixed 40&nbsp;ms / 8-slot window at 1.5% and 5% loss. | Not measured on a live Discord call. Acoustic echo cancellation and noise suppression are not included. |
| **Interface** | GPUI Kit 0.10. Direction A layout. The default theme is Onyx Ink: a true-black canvas, `#15152A` hairlines, a `#4F6BFF` accent with black ink on the banner, Inter for the UI, and JetBrains Mono for timestamps and status. Themes and custom themes are free. The same four columns sit edge to edge and reflow for 16:9, 16:10, 21:9, 32:9, portrait, and small windows. Ctrl/Cmd+K, density, and type size are in settings. No Nitro surface. | Not a screen-reader pass. Plugins are still out. Layouts B and C remain behind `--layout`. |
| **Streaming** | In progress. The worker, OpenH264 encode/decode, RTP, DAVE video wrap, and the Onyx picker/viewer/PiP are in the tree. Voice and Go Live share one UDP discovery: probes resend for about 9 s, a garbage packet is ignored, and a dead path resumes. Linux asks the portal and connects a PipeWire video stream plus a monitor-audio stream. NVENC, AMF, oneVPL, VA-API, and VideoToolbox are probed with `dlopen`. | No GPU session on this machine, so hardware encode returns `NotLinked` and does not emit frames. There is no session bus and no PipeWire daemon, so no screen or system-audio buffer is copied. Windows Graphics Capture and ScreenCaptureKit are not built on this Linux image. Discovery is tested against local stubs, not a Discord media server. x264 is GPL and is not loaded. Discord may still reject a preset. |
| **HDR** | BT.2390, Hable, and ACES run as a wgpu compute shader (Vulkan, Metal, or D3D12) when an adapter exists, and on the CPU otherwise. The shader result stays in a device-local buffer and is handed to `GpuFrameSink`. The software encoder maps that buffer once so the 8-bit access unit is the tone-mapped picture. A GPU encoder sink keeps the buffer. Settings can opt in. DXGI float and 10-bit, ScreenCaptureKit 10-bit, and PipeWire P010 are classified. Send-on builds an RHDR sidecar with real HDR10 SEI. | This machine's only adapter is lavapipe. The owner command below measured 34.0 ms at 1080p and 182 ms at 4K with no readback. A mid GPU under 2 ms / 4 ms was not attached. NVENC/AMF/QSV/VideoToolbox/VA-API Main10 sessions, D3D11VA, and VideoToolbox decode are not linked. No Discord SFU test. |

Serein, Concord, and Yapfast are further along as daily drivers. Milestone 1 is the shell and the text path. Milestone 2 adds a voice websocket and UDP session, tested against a local stub, with mute, deafen, gain, and discovery that resends when a probe is dropped. Milestone 3 is the quieter window described in [docs/design/UX.md](docs/design/UX.md). None of this has been signed into a real Discord voice server.

## Screenshots

Captured from `rencord --demo --voice` on Linux (X11, software GL). The default theme is Onyx Ink. Every surface is `#000000`, meeting at a 1 px `#15152A` hairline. The accent is `#4F6BFF`, and the new-messages banner uses black text on that blue. Links and mentions use `#A8B8FF`. The UI face is Inter. Timestamps, the date pill, member and dock status, and the voice readout are JetBrains Mono. Avatars in the demo are illustrated. The window uses server decorations, so the theme fills the client area with no transparent margin. Settings stays in the user dock. `--theme dark` is the Discord-style dark palette, `--theme crisp` is the earlier dark palette, and `--theme amoled` is pure black. Preset captures are in [docs/design/themes/](docs/design/themes/). `--density compact` tightens the channel rows. **Join voice** runs the in-memory loopback and then tries to open the default cpal devices. It does not open a Discord UDP session. The millisecond figure in the voice card is the loopback target, not a fixed mock value.

| Onyx Ink | Voice |
| :---: | :---: |
| <img src="docs/design/onyx/main.png" alt="Rencord in Onyx Ink: black panels, blue banner with black text, code block, and illustrated avatars" width="480" /> | <img src="docs/design/onyx/voice.png" alt="Onyx Ink with the Voice Connected waveform, hang-up control, and mono latency readout" width="480" /> |

| Compact | Hover |
| :---: | :---: |
| <img src="docs/design/onyx/compact.png" alt="Onyx Ink at compact density" width="480" /> | <img src="docs/design/onyx/hover.png" alt="Onyx Ink message hover bar with react, reply, link, and delete" width="480" /> |

Go Live, from `rencord --demo --golive picker|viewer|pip`. The pictures are preview frames. The stats line is the safe 720p30 preset on OpenH264.

| Source picker | Viewer |
| :---: | :---: |
| <img src="docs/design/onyx/golive-picker.png" alt="Onyx Ink Go Live source picker with screen and window previews" width="480" /> | <img src="docs/design/onyx/golive-viewer.png" alt="Stream viewer with resolution, fps, bitrate, encoder, dropped frames, and RTT" width="480" /> |

Picture in picture: [docs/design/onyx/golive-pip.png](docs/design/onyx/golive-pip.png).

| Over the cap | Progress |
| :---: | :---: |
| <img src="docs/design/onyx/big-file.png" alt="Onyx Ink big-file sheet: compress, upload a link, or split a 45.7 MB video under the 20 MB cap" width="480" /> | <img src="docs/design/onyx/big-file-progress.png" alt="Onyx Ink compress progress with cancel" width="480" /> |

Upload in progress: [docs/design/onyx/big-file-upload.png](docs/design/onyx/big-file-upload.png). A failed upload, with Retry: [docs/design/onyx/big-file-error.png](docs/design/onyx/big-file-error.png).

| QR sign-in | Confirm on your phone |
| :---: | :---: |
| <img src="docs/design/onyx/login-qr.png" alt="Onyx Ink welcome screen with a login QR code and a Log in with Discord button" width="480" /> | <img src="docs/design/onyx/login-confirm.png" alt="Onyx Ink login waiting for a phone confirmation, with a round avatar" width="480" /> |

## Feature status

Honest status for this repository. "Implemented" means the code path and tests exist. It does not mean a live Discord session was signed in from CI.

| Capability | Status | Notes |
| --- | --- | --- |
| QR login | Implemented | Discord remote auth, gateway v2. RSA-OAEP SHA-256, a QR of `https://discord.com/ra/<fingerprint>`, phone confirm, then the encrypted token. Tested against scripted frames, not a live phone in CI. |
| Discord login page | Implemented | A separate window loads `https://discord.com/login` in the system webview (WebView2, WKWebView, or WebKitGTK). The password, captcha, passkey, and SSO stay on Discord's page. Rencord reads the session Discord returns and closes the window. |
| Email, password, MFA | Implemented as fallback | Used when the webview cannot start. TOTP, SMS, and backup codes go to Discord's login endpoints. A captcha hands off to the login window. Rencord does not solve captchas. Passkeys are completed in that window. |
| Token paste | Advanced only | Hidden under Other options. Same session handling as the other paths. |
| Session storage | Implemented | OS keychain only (Credential Manager, Keychain, or Secret Service). Not a file. Several accounts can be saved, switched, and removed. Log out calls `/auth/logout` and deletes the keychain item. A 401 returns to the welcome screen. |
| Guild list, channel list, DM list | Implemented | Demo fixture plus REST bootstrap (`/users/@me`, guilds, channels, DMs). |
| Message history | Implemented | Bounded to 4000 messages per channel in memory, with a SQLite copy. The timeline is a variable-height virtual list. |
| Markdown | Implemented | Headings, quotes, fenced code, bold, italic, inline code, links, mentions, spoilers. A fenced block keeps its lines, a monospace face, a background, and a Copy button. |
| Sending messages | Implemented locally; REST client tested against a mock | `POST /channels/{id}/messages` exists. The demo composer appends locally. A live send was not verified. |
| Gateway live updates | Implemented against a local socket | Identify, heartbeat, `READY`, `MESSAGE_CREATE`. After the socket drops, the client reconnects and sends Resume with the saved session id and sequence. Not verified against Discord's gateway. |
| Voice join, audio in and out | Partial | `LiveCall` speaks the voice gateway and UDP against a local stub: identify or resume, IP discovery, select protocol, `aead_xchacha20_poly1305_rtpsize`, speaking, and a DAVE prepare / transition-ready exchange. Discovery resends the 74-byte probe (250 ms, 500 ms, 1 s, then 1.5 s) for 9 s, ignores garbage, retries the websocket once, then names the server and the probe count. A 5 s UDP keepalive and a 15 s quiet timer resume the path. The same code serves Go Live. Tested against local stubs. Mute, deafen, and per-SSRC gain are in that path. **Join voice** still runs the loopback, then opens the default cpal devices. Settings lists device names and remembers the choice; that name is not yet passed into cpal. A live Discord voice server was not used. |
| DAVE (E2EE) | Implemented | `davey` 0.2. Two-member MLS encrypt/decrypt test. Opcode routing keeps the transport up when a welcome fails. |
| Adaptive jitter buffer | Implemented | See [Voice](#voice). |
| Screen share / camera / Go Live | In progress | Worker, OpenH264 encode and decode, RTP, DAVE wrap, A/V drift math, and `--golive` screens. `connect_stream` uses the voice UDP discovery and keepalive, so a dropped probe is resent and a quiet path resumes. Tested against local stubs, not Discord's media servers. The Linux path calls ScreenCast and PipeWire, including a monitor-audio stream. This machine has no session bus and no PipeWire daemon, so no pixels or samples are copied. Hardware encoders are probed and return `NotLinked`. |
| HDR | GPU shader when an adapter exists | wgpu compute BT.2390, CPU fallback, capture-format classification, RHDR+SEI sidecar when send is on. See [HDR](docs/design/HDR.md). Lavapipe is not the 2 ms budget. Hardware Main10 encode and platform 10-bit decode are not linked. |
| Notifications, tray, hotkeys | Stub | In-process notification sink. Sessions are in the OS keychain. Tray and global hotkeys are still absent. |
| Themes & custom themes, free | Implemented | Onyx Ink is the default. Light, Ash, Dark, Onyx, AMOLED, Midnight Blurple, Nord, Soft Contrast, and Crisp ship with the app. Custom colors, gradients, and theme files are not gated. The choice is stored per account and is not written back to Discord unless Follow Discord is on. See [docs/themes.md](docs/themes.md). |
| Responsive layout | Implemented | Breakpoints use width, height, and aspect ratio. 16:9 and 16:10 keep the four columns, including 1440×900, 1366×768, 1280×800, and 1280×720. 21:9 and 32:9 cap the message measure and leave the spare width as a gutter unless a stream, thread, or profile panel is open. Portrait keeps a narrow rail, a channel button, and drawers for channels and members. Column edges drag, double-click resets, and widths are stored per aspect and scale. `--window WxH` and `--scale` open a sized window. |
| Search | Implemented locally | `from:`, `in:`, `has:`, `before:`, and `after:` across cached channels, plus the guild search URL. A hit jumps to that message. Not verified against Discord's search service. |
| Reactions, reply, edit | Implemented locally; REST client tested against a mock | Optimistic update with rollback. Demo mode does not call Discord. |
| Big-file flow | Implemented | Discord still rejects a file over the account cap. The composer compares the size with the cap from `premium_type` and the guild boost tier, or uses `attachment_size_limit` when the API sends one. Over the cap, one sheet offers compress, an external link, or a split. Nothing is uploaded until you press the button, and a third-party host also needs the warning checked. |

## Build

Rust **1.100** is pinned in `rust-toolchain.toml`. You also need a C toolchain, CMake, `pkg-config`, and libopus.

### Linux

```sh
sudo apt-get install pkg-config cmake g++ \
  libopus-dev libasound2-dev libssl-dev libdbus-1-dev \
  libgtk-3-dev libwebkit2gtk-4.1-dev libfontconfig1-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev \
  libx11-dev libxcb1-dev libxcursor-dev libxi-dev libxrandr-dev \
  libpipewire-0.3-dev libspa-0.2-dev pipewire libva-dev

cargo test --workspace
cargo run -p rencord-desktop -- --demo
```

`--demo` opens the GPUI window on local fixture data. It does not contact Discord. The starting palette is Onyx Ink. `--theme` selects a preset (`onyx-ink`, `dark`, `ash`, `onyx`, `amoled`, `light`, `midnight-blurple`, `nord`, `soft-contrast`, `crisp`). `--theme-file` loads a TOML or JSON theme. `--open-settings` opens Appearance. `--layout a` is the default; `b` and `c` are the other presets. `--density compact` tightens the channel list. `--voice` starts the loopback call. `--golive picker`, `--golive viewer`, or `--golive pip` opens the Go Live screens. `--window 1440x900` sets the frame (logical pixels); `--scale 1.5` (or `150%`) sets `GPUI_X11_SCALE_FACTOR` before the window opens. `--screenshot path.png` grabs that frame and quits. `--bench` prints startup, RSS, frame gaps, and a resize sweep, then quits. `--bench stream` prints capture-to-encode time for OpenH264 and the hardware probes, then quits. Omit `--demo` to open the welcome screen: a QR code, **Log in with Discord**, and Other options. The QR needs a network path to `remote-auth-gateway.discord.gg`. **Log in with Discord** needs WebKitGTK on Linux, WebView2 on Windows, or WKWebView on macOS. A secret service (GNOME Keyring or KWallet) has to be running or the session stays in memory for that process only. Rencord will not write the token to a file.

```sh
cargo run -p rencord-desktop --bin rencord-measure -- --offline
```

## Keyboard and settings

| Action | Keys |
| --- | --- |
| Quick switcher | Ctrl+K or Cmd+K. Esc closes it. The header search icon opens it too. |
| Next / previous channel | Alt+Down / Alt+Up |
| Next / previous server | Ctrl+Alt+Down / Ctrl+Alt+Up |
| Settings | Ctrl+, or Cmd+, or the Settings button |

Settings opens to a left nav. Appearance shows preset cards, sync with the system, density, and accessibility switches. Custom edits a base and an accent with an OKLCH plane, a hex field, and a gradient, plus sliders for type size, spacing, radius, saturation, and grain. Advanced lists tokens with contrast badges. Voice & Video holds the input and output devices, the loopback line, and the HDR row (Send, Receive, curve), which stays off and does not start a capture. There is no Nitro row. Appearance stays on this machine unless Follow Discord is on, and even then Rencord only reads Discord's theme.

<img src="docs/design/onyx/hdr-settings.png" alt="Onyx Ink settings with HDR send and receive off and the BT.2390 curve." width="640" />

## Measured on this machine

Release build, `rencord --demo --bench`, Linux, X11, software GL, about 800 cached messages. A 120 Hz panel was not attached. The full write-up is [docs/design/UX.md](docs/design/UX.md).

| Budget | Target | Measured |
| --- | --- | --- |
| First frame | < 1000 ms | 117 ms release, 246 ms debug. Process start to the first bench frame on a warm page cache. |
| Idle RSS | < 200 MiB | 137 MiB release (`140228` KiB), 183 MiB debug. |
| Cached channel switch | < 50 ms | 669 ns average over 200 session switches. On-screen switches in the bench were 0–3 µs. |
| Frame gap | About one 60 Hz frame | p50 16.7 ms over 23 frames while the bench changed channel every frame. |
| Release binary | < 80 MB | 63 MB. The unstripped debug binary is 840 MB and is not the shipping build. |

HDR tone-map, same machine, release. The CPU bench number includes the BT.2020 to BT.709 matrix. The GPU number is one submit and a device wait after the pipeline exists. The storage buffer is not mapped.

| Path | Frame | Time |
| --- | --- | ---: |
| CPU BT.2390 | 1080p | 91.8 ms |
| CPU Hable | 1080p | 17.1 ms |
| CPU ACES | 1080p | 13.0 ms |
| wgpu BT.2390, lavapipe (LLVM 20.1.2), no readback | 1080p | 34.0 ms |
| wgpu BT.2390, lavapipe, no readback | 3840×2160 | 182 ms |

A mapped readback of the 1080p buffer added 26.6 ms on lavapipe. `dispatch_resident` does not do that copy. `SoftwareFrameSink` maps once, because the linked encoder is the CPU luma encoder, and writes the tone-mapped sRGB into the access unit. A hardware sink keeps the buffer. Lavapipe is a software Vulkan device, so the 2 ms (1080p) and 4 ms (4K) mid-GPU budgets were not measured. HDR10 SEI and the AV1 metadata OBUs together take 0.30 µs. Details are in [docs/design/HDR.md](docs/design/HDR.md).

Owner GPU pass, from a machine with a real adapter:

```sh
cargo run --release -p rencord-hdr --example tonemap-bench
```

It prints the adapter name and the 1080p and 4K BT.2390 GPU time (no readback) next to the CPU time. On this VM that command printed `llvmpipe (LLVM 20.1.2, 256 bits)`, software=true, 34.01 ms / 86.35 ms at 1080p and 181.85 ms / 354.09 ms at 4K.

<img src="docs/design/onyx/hdr-tonemap.png" alt="Left: highlights clipped to white at 100 cd/m². Right: the same ramp tone-mapped with BT.2390, color kept." width="720" />

Left of that frame is the ramp clipped at 100 cd/m². Right is BT.2390.

`RENCORD_DATA_DIR` overrides the data directory. The default is `$XDG_DATA_HOME/rencord` or `~/.local/share/rencord`.

### macOS

```sh
xcode-select --install
brew install opus pkg-config cmake
cargo test --workspace
cargo run -p rencord-desktop -- --demo
```

GPUI uses Metal on macOS. This environment did not compile or run the macOS target. The crates avoid Linux-only APIs except for the `0600` permission bit, which is cfg-gated.

### Windows

Install the Visual Studio C++ build tools, CMake, and a libopus the `audiopus` crate can find. Then:

```powershell
cargo test --workspace
cargo run -p rencord-desktop -- --demo
```

GPUI uses its DirectX / wgpu backend. This environment did not produce a Windows binary. CI runs on Linux only.

There is no installer, Flatpak, or Homebrew cask yet.

## Voice

Discord voice is 48 kHz Opus, usually 20 ms frames. A fixed two-frame buffer has nowhere to put a packet that arrives 60–90 ms late, so the reader skips ahead and the sentence drops out. Rencord's adaptive buffer:

- starts at an 80 ms target and moves between 40 ms and 120 ms
- estimates inter-arrival jitter (RFC 3550 style) and raises the target when a packet is late or concealed
- lowers the target only after a run of clean frames
- uses Opus in-band FEC when the next packet carries the missing frame, otherwise Opus PLC
- advances one frame when concealment is exhausted, instead of jumping to the newest packet
- drops a frame on purpose when the buffer is deeper than the target plus 40 ms (time-stretch compression)

`crates/rencord-voice` compares that buffer with `FixedWindow`, a baseline of the fixed 40 ms behavior, under a deterministic loss and delay trace. The adaptive buffer records fewer hard gaps at 1.5% and at 5% loss. That comparison is a simulation, not a recording of a Discord call.

Milestone 2 drives a real websocket and UDP socket, but the peer is a process on localhost:

- Hello, then Identify, or Resume (`op` 7) with `server_id`, `session_id`, `token`, and `seq_ack` after the socket is replaced
- Ready supplies the UDP address and SSRC. The client binds the matching IP family, sends the 74-byte discovery probe on an unconnected socket, and resends at 250 ms, 500 ms, 1 s, then every 1.5 s until 9 s. Only a type `0x0002` reply of length 70, with the same SSRC, from that server is accepted. A garbage packet is ignored. If the deadline passes, the websocket is opened once more. The error names voice or stream, the server `host:port`, and how many probes were sent. After that, an 8-byte UDP keepalive goes out every 5 s. Fifteen seconds without an inbound packet resumes the gateway. Windows disables `SIO_UDP_CONNRESET` so an ICMP unreachable does not abort the receive. This is the same path Go Live uses. It has been tested against local stubs that drop probes, send garbage, or stay silent.
- Session Description installs the 32-byte `aead_xchacha20_poly1305_rtpsize` key; Speaking follows
- A DAVE prepare transition is answered with transition-ready. The MLS group itself is covered by the davey round-trip test, not by the stub
- Microphone frames are Opus-encoded, sealed, and written to UDP. Received packets are unsealed, jitter-buffered, then scaled by per-SSRC gain. Mute suppresses the packet. Deafen zeros playback
- The main gateway uses the same idea: on a clean close it reconnects and sends gateway Resume (`op` 6) when it still has a session id

None of that has been checked against `gateway.discord.gg` or a Discord voice server. The desktop button does not call `LiveCall` yet. It prints loopback stats and, when cpal can see devices, monitors the default microphone through the same jitter buffer.

DAVE v1 sits on top of the transport cipher. Media is encrypted with `davey` before `aead_xchacha20_poly1305_rtpsize`. Silence packets are left to davey's own passthrough rules. If an MLS welcome fails, the session reports it and leaves the UDP key in place.

## Architecture

```mermaid
flowchart LR
  login["QR, Discord page, or token"] --> api["rencord-api"]
  login --> gw["rencord-gateway"]
  api --> store["rencord-store"]
  gw --> store
  store --> ui["rencord desktop / GPUI Kit"]
  gw --> voice["rencord-voice"]
  ui --> voice
  voice --> dave["davey DAVE"]
  voice --> jitter["Adaptive jitter + Opus"]
  ui --> media["rencord-media presets"]
  ui --> upload["rencord-upload big files"]
  login --> platform["rencord-platform secrets"]
```

```text
rencord/
├── apps/rencord-desktop/     GPUI shell and the session controller tests
└── crates/
    ├── rencord-model/        Snowflakes, users, guilds, channels, messages
    ├── rencord-protocol/     Gateway and voice websocket payloads
    ├── rencord-api/          REST client and per-route rate-limit buckets
    ├── rencord-gateway/      Identify, heartbeat, dispatch, zlib-stream
    ├── rencord-store/        In-memory session and SQLite drafts/history
    ├── rencord-markdown/     UI-free Discord markdown AST
    ├── rencord-voice/        RTP, transport AEAD, DAVE, Opus, jitter
    ├── rencord-media/        Stream presets, HDR policy, worker messages
    ├── rencord-hdr/          HDR probes, tone-map, 10-bit metadata, signalling
    ├── rencord-platform/     Data dir, system appearance, and the legacy token file
    ├── rencord-theme/        Semantic tokens, presets, custom theme files
    ├── rencord-upload/       Cap check, compress, external hosts, split volumes
    └── rencord-auth/         Remote auth, login window, keychain, session state
```

The UI does not embed egui. Protocol crates do not depend on GPUI.

## Roadmap

| Milestone | State | What landed |
| --- | --- | --- |
| 1. Shell and adaptive jitter | On `main` | GPUI window, guild and DM lists, markdown history, local send, gateway identify against a local socket, jitter simulation. Token paste is now an advanced fallback. |
| 2. Live voice path | On `main` | Gateway resume, voice websocket, resilient UDP discovery (resend, ignore garbage, one full retry, 5 s keepalive, quiet-path resume), transport cipher, speaking, DAVE control frames, mute, deafen, per-SSRC gain, default cpal devices. Local stubs only. |
| 3. UI | Chosen layout, ready to land | Direction A: Discord-shaped window with WCAG AA themes (Onyx, Dark, Ash, Light), grouped messages, hover actions, a flush user panel, and the six fixes (variable-height code blocks, REST reply/edit/delete/reactions, named audio devices, cross-channel search, AccessKit labels, refresh reporting). Screenshots: [docs/design/DIRECTIONS.md](docs/design/DIRECTIONS.md). |
| 3c. Responsive window | In the shell | The four-column shell holds 16:9, 16:10, 21:9, 32:9, 9:16, and small frames down to about 800×600. Reading width is capped on wide and 4K frames. Column splitters persist per aspect and DPI step. Window restore is clamped to the visible monitors. Captures: [docs/design/onyx/responsive](docs/design/onyx/responsive). |
| 3b. Other layouts | Behind `--layout` | `b` keyboard-first and `c` unified sidebar stay in the binary. They are not the default. |
| 4. Threads, search, uploads, notifications | Not started | Search across cached channels is in. Threads, uploads, and notifications are not. Sessions use the OS keychain. |
| 5. Voice against Discord | Not started | Point `LiveCall` at a real voice server and keep DAVE rekeys without dropping the call. |
| 6. Go Live | In progress | OpenH264 emits and decodes H.264. A loopback test captures a synthetic frame, encodes it, packetizes RTP, wraps DAVE, and presents the decoded picture. The stream UDP connection is the voice discovery and keepalive with a stream role, tested on local stubs. `VideoEncoder` in `rencord-media` is the stable 8/10-bit trait (`EncodeFrame.bit_depth`, `luma10`, `EncodeError::Unsupported10Bit` or `NotLinked`) documented in `crates/rencord-media/src/encoder.rs`. `rencord-hdr` keeps its own `HdrEncoder`. Linux calls xdg-desktop-portal ScreenCast and a PipeWire video plus monitor-audio stream. Windows enumerates DXGI under `cfg(windows)`. macOS `dlopen`s ScreenCaptureKit under `cfg(macos)`. None of those copied a frame on this Linux machine: no session bus, no PipeWire daemon, no `/dev/dri`. NVENC, AMF, oneVPL, VA-API, and VideoToolbox are probed and `HardwareEncoder::encode` returns `NotLinked`. 120 fps is offered only when the display reports that refresh and the encoder sustains the budget. |
| 7. HDR between Rencord users (Milestone 5) | GPU shader linked; hardware codec not | wgpu runs the BT.2390 compute shader on Vulkan, Metal, or D3D12 and falls back to the CPU. The shader submit leaves the storage buffer on the GPU. `SoftwareFrameSink` maps it once for the CPU luma encoder. A GPU encoder sink would keep the buffer. DXGI `R16G16B16A16_FLOAT` and `R10G10B10A2`, ScreenCaptureKit 10-bit, and PipeWire P010 are classified. Send-on attaches an RHDR sidecar with HDR10 SEI through `Hdr10Encoder`; the NVENC/AMF/QSV/VideoToolbox/VA-API implementations of that trait return not-linked. D3D11VA, VideoToolbox, and VA-API decode are not opened. The swapchain format is chosen (`Rgb10a2Unorm` or `Rgba16Float`) and no HDR surface is created. Lavapipe, no readback, measured 34.0 ms at 1080p and 182 ms at 4K, which misses the 2 ms / 4 ms mid-GPU budget. `cargo run --release -p rencord-hdr --example tonemap-bench` is the owner check. [docs/design/HDR.md](docs/design/HDR.md). |
| 8. Files over the attachment cap | Implemented | Discord enforces the per-file cap on the server. This client cannot lift it. The composer sheet plans against the API reference default of 20 MiB with no Nitro (the older account-caps table still says 10 MB; Nitro Basic 50 MiB; Nitro 1 GiB; boost level 2 is 50 MiB and level 3 is 100 MiB, server-only, taking the maximum). `attachment_size_limit` wins when the API sends it. Over the cap: compress (ffmpeg two-pass when a hardware encoder is on PATH, WebP or JPEG in-process, Opus for 48 kHz WAV), upload to Litterbox, Catbox, a 0x0.st-compatible host, or a presigned PUT / S3-compatible endpoint and post the link, or split into zip or 7z volumes under the cap. A Rencord message with the split note has a Reassemble control. Screenshots: [docs/design/onyx/big-file.png](docs/design/onyx/big-file.png), [docs/design/onyx/big-file-progress.png](docs/design/onyx/big-file-progress.png). |

The UI must not promise that Discord will accept a Go Live preset or a 10-bit stream. The capture worker publishes preview frames and tone-maps HDR to 8-bit. It does not send them to Discord. A 10-bit track stays off until a caller has proof the SFU forwarded HDR. There is no such proof in this repository.

## FAQ

**How do I sign in?**  
The welcome screen shows a QR code. Scan it with the Discord mobile app and confirm. **Log in with Discord** opens Discord's own login page in a system webview, including captcha, 2FA, passkeys, and SSO. Email and password are under Other options and are only the fallback when that window cannot start. A pasted user token is there too, labeled advanced. Rencord does not read another app's storage and does not solve captchas.

**Does Join voice put me in a Discord channel?**  
No. The button runs the in-process loopback and then tries to open the default microphone and speaker. `LiveCall` can complete discovery, select protocol, transport encryption, speaking, and a voice-gateway resume, but only against the local stub in tests. It has not been pointed at a Discord voice server.

**Will 4K / 120 work without Nitro?**  
The preset list is not gated. The client asks for 720p30 until you pick something else. Discord's service can refuse a higher request, and sending more than the account is allowed can put the account at risk. On this machine there is no `/dev/dri` node. VA-API cannot open a GPU, oneVPL `MFXInit` returns -3, and NVENC is not installed. OpenH264 is the encoder that emits frames. Release, one warmed frame, no GPU: 720p30 encode 22042 µs (budget 33333, so that preset fits), 320×180 capture 71 µs and encode 1528 µs. The higher presets do not sustain: 1080p60 took 56303 µs (about 18 fps, budget 16666), 1440p120 took 106907 µs (about 9 fps, budget 8333), 2160p60 took 107639 µs (about 9 fps, budget 16666), and 2160p120 took 146323 µs (about 7 fps, budget 8333). x264 is installed and is not loaded, because it is GPL. PipeWire `connect` fails with "Creation failed" because no daemon is running, so screen and system audio are not captured.

**Is the SQLite file encrypted?**  
No. Sessions are not in that file. They are in the OS keychain. Log out is in Settings: it calls Discord's logout endpoint and deletes the keychain item. An old `0600` token file from an earlier build is read once, moved into the keychain, and deleted. If the keychain cannot be opened, the session stays in memory for that process and is not written to disk.

## Contributing

Issues and pull requests are welcome. Please:

- Keep the license `MIT OR Apache-2.0`. Do not add GPL dependencies.
- Do not copy Yapfast or Oxidecord source. Read them if you want; write new code.
- Keep GPUI as the only UI toolkit.
- Add a test for protocol or voice changes. `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` should pass.
- UI colors come from `rencord-theme` tokens. Do not put raw hex in `apps/rencord-desktop`. The hex lint test fails the build.
- Do not add captcha solving, mass DMs, invite raids, or other automation.

## Acknowledgements

- [Serein](https://github.com/ViceVerse-cz/Serein) for the public crate layout and for documenting the fixed jitter window in issue 1033. Rencord does not include Serein's code.
- [davey](https://github.com/Snazzah/davey) by Snazzah, for the DAVE implementation.
- [GPUI Kit](https://github.com/longbridge/gpui-kit) and [GPUI](https://github.com/zed-industries/zed) from Zed.
- [libopus](https://opus-codec.org/) for the codec, PLC, and FEC.

## Disclaimer

Rencord is not Discord. It is not endorsed by Discord. Using it can violate the [Discord Terms of Service](https://discord.com/terms/) and the [self-bot policy](https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots), and Discord may suspend or terminate the account. Streaming above the official quality caps adds further policy risk. You are responsible for how you use this software.

## License

Rencord is dual-licensed under either of

- the MIT License ([LICENSE-MIT](LICENSE-MIT))
- the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option. Third-party licenses are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
