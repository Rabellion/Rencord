# Third-party notices

Rencord is licensed under MIT OR Apache-2.0. The crates below are linked as
dependencies. Their licenses travel with the corresponding crate sources in
the Cargo registry; this file records the ones that matter for attribution.

## Libraries

| Project | Use in Rencord | License |
| --- | --- | --- |
| [davey](https://github.com/Snazzah/davey) 0.2 | DAVE / MLS voice end-to-end encryption (`DaveSession`) | MIT |
| [OpenMLS](https://github.com/openmls/openmls) | MLS implementation used by davey | MIT |
| [GPUI Kit](https://github.com/longbridge/gpui-kit) 0.10 | Desktop UI (`gpui-kit`, `gpui-component`, `gpui-base`) | Apache-2.0 |
| [GPUI](https://github.com/zed-industries/zed) via the `gpui-pre` 0.3.12 snapshots | GPU UI runtime pinned by GPUI Kit | Apache-2.0 |
| [libopus](https://opus-codec.org/) via `audiopus` | Opus encode, decode, PLC, and in-band FEC | BSD-3-Clause |
| [RustCrypto](https://github.com/RustCrypto) `chacha20poly1305` | Voice transport mode `aead_xchacha20_poly1305_rtpsize` | MIT OR Apache-2.0 |
| [cpal](https://github.com/RustAudio/cpal) | Optional microphone and speaker devices | Apache-2.0 |
| [rusqlite](https://github.com/rusqlite/rusqlite) / SQLite | Bounded local message and draft cache | MIT (rusqlite); SQLite public domain |

## What was not copied

[Serein](https://github.com/ViceVerse-cz/Serein) (MIT OR Apache-2.0) is the
architectural reference for the crate split and for the voice failure this
project is trying to avoid. Serein issue
[#1033](https://github.com/ViceVerse-cz/Serein/issues/1033) describes a fixed
~40 ms jitter window that falls apart around 1.5% packet loss. Rencord's
`FixedWindow` is an original baseline that reproduces that documented behavior
(40 ms startup, 8 reorder slots, 3 concealment frames, then a hard skip) so
the adaptive buffer can be compared with it. Serein source was not vendored
and is not linked.

[Yapfast](https://github.com/yapfast/yapfast) (GPL-3.0-or-later) and
[Oxidecord](https://github.com/s1099/oxidecord) (GPL-3.0-only) were treated as
reference reading only. No source from either repository is included here.
Rencord does not depend on GPL libraries, including x264 and FFmpeg.

The transport ciphertext layout (RTP header as additional data, 16-byte tag,
4-byte big-endian nonce suffix) follows Discord's published
`aead_xchacha20_poly1305_rtpsize` mode as implemented by public voice
libraries. The implementation in `rencord-voice` is original.
