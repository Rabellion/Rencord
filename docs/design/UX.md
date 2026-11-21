# Rencord interface

The window should feel quieter than Discord and cheaper to run. This document is the contract for color, type, motion, and the complaints the layout is meant to answer. Numbers in the budgets section were measured on this Linux machine; they are not a promise about every GPU.

## Principles

1. **One job per region.** The rail is servers. The sidebar is channels. The center is the conversation. The right column is people. Voice sits in a card, not across the whole window.
2. **No upsells.** There is no Nitro shop, gift button, quest, boost banner, or "upgrade" row. Go Live defaults to 720p30. Higher presets stay in the list, and the settings screen says Discord may still reject them. The HDR chips set a plan. They do not start a stream and they do not claim Discord will carry 10-bit.
3. **Keyboard first.** Ctrl/Cmd+K opens the quick switcher. Alt+Up/Down moves channels. Ctrl/Cmd+Alt+Up/Down moves servers. Esc closes the switcher and settings. Ctrl/Cmd+, opens settings.
4. **The list does not draw what is off screen.** Messages use GPUI's virtual list. A row can grow for a code block, an embed, or an attachment.
5. **Themes are data.** Every color, radius, font, and gap the shell paints comes from `rencord-theme`. Switching presets does not rebuild the session, and the change eases over 180 ms unless reduced motion is on. Custom themes are free.

## Design system

### Color

The default theme is Onyx Ink. The window canvas, rail, channel list, chat, members, dock, inputs, and code blocks are `#000000`. Columns are 8 px-gapped panels drawn with a 1 px `#15152A` hairline. The accent is `#4F6BFF`. The new-messages banner, the unread badge, and the server pill use that blue with `#000000` text. Links, mentions, and syntax keywords use `#A8B8FF`, which stays above WCAG AA on black. Other presets are Light, Ash, Dark, Onyx (Discord-style, not the ink theme), AMOLED, Midnight Blurple, Nord, Soft Contrast, and Crisp (the earlier dark palette). Light mode uses a cool gray canvas, white elevated surfaces, a soft shadow, and muted text that stays above AA. The token list and the file format are in [../themes.md](../themes.md).

| Token | Onyx Ink | Crisp | Light |
| --- | --- | --- | --- |
| Canvas and panels | `#000000` | `#16181D` / `#1C1F26` | `#E6E9F0` / `#F7F8FB` |
| Hairline | `#15152A` | `#2E3440` | `#D5DAE6` |
| Input | `#000000` | `#23272F` | `#FFFFFF` |
| Text | `#EDEEF7` | `#EDF0F8` | `#1A1D26` |
| Muted | `#9AA0C0` | `#8592AD` | `#4E5870` |
| Accent | `#4F6BFF` | `#5865F2` | `#5865F2` |
| Text on accent | `#000000` | `#FFFFFF` | `#FFFFFF` |
| Link | `#A8B8FF` | `#00A8FC` | `#0068E0` |

### Type

Onyx Ink uses Inter for the UI and JetBrains Mono for timestamps, the date pill, member and dock status, the voice readout, and code. Crisp and Light keep Figtree for the UI. Message body is 14 px on Onyx and 16 px otherwise. Names are semibold in a role tint. Code is JetBrains Mono at about 12.5 px. The settings control still steps the window size through 13, 15, and 17 px. Both families are bundled.

### Space, radius, motion

Spacing steps are 4, 8, 12, 16, 24, and 32 px. Every preset uses the same four columns, set edge to edge and separated by a 1 px hairline. A gap between those columns is off unless a theme sets one; that floating-panel layout is opt-in. Corner radius is the radius token, shared by the presets. Avatars and the date pill are fully round. The window background is opaque. Hover feedback is 80 ms. Overlays fade in over 140 ms. `reduce motion` in GPUI skips the overlay fade. The Onyx speaking ring is a tight accent stroke and does not pulse.

### Density

Cozy channel rows are 32 px. Compact rows are 28 px. Spacious rows are 36 px. Grouped messages from the same author leave the avatar gutter empty instead of repeating the name.

### Responsive frame

The four columns stay the default. Breakpoints live in `layout::resolve` and take the window's logical width, height, and aspect ratio. DPI scale changes the logical size GPUI reports; a 1440×900 monitor at 200% is a 720×450 layout.

| Frame | What the shell does |
| --- | --- |
| 16:9 and 16:10, including 1920×1080, 1440×900, 1366×768, 1280×800, 1280×720, and 1000×700 | Rail, channel list, chat, and member list. The member list stays until the chat column would drop under 420 px. |
| About 800×600 | The member list becomes an overlay (Ctrl+M). The channel list stays. |
| Narrower still, or a very short window | The channel list becomes a drawer (Ctrl+B, or the channel title). Ctrl+K still opens the quick switcher. |
| 21:9 and 32:9 (2560×1080, 3440×1440, 5120×1440) | The four columns stay. The message column is capped at about 92 characters and the spare width is a gutter. A stream panel opens only while one is actually on screen. |
| 3840×2160 | The four columns stay. The text block inside the chat column is capped; the column itself still fills. |
| Portrait: 1080×1920, 900×1440, 800×1280, 768×1366 | A narrow server rail stays. Channels and members are drawers. A channel button at the left of the header opens the channel drawer; the people icon opens the member list. The composer and the voice dock sit at the bottom of the chat. Header targets grow to about 44 px. The composer keeps the same left inset as the landscape window. |

The gaps between the channel list, the chat, and the member list are drag handles. A drag is clamped so the chat column keeps its floor. Double-click restores the theme default. Widths are stored under `$XDG_DATA_HOME/rencord/layout.json`, keyed by aspect, DPI step (100, 125, 150, 200), and monitor. On launch, a saved window that sits off a monitor is moved back onto the visible bounds and snapped to device pixels. Text and icons are drawn as vectors. The demo avatars are 128 px sources, which cover a 40 px avatar at 200%.

`--window 1440x900` opens that logical size. `--scale 1.5` sets the X11 scale before the window is created. `--screenshot path.png` writes the frame after it has painted. Captures for the sizes above are in [onyx/responsive](onyx/responsive).

Settings > Measure switches the capped reading column between the left edge and the center. The default is the left edge, which is the Discord alignment.

### Iconography

One clean-room set, 24 px viewBox, 1.75 px stroke, `currentColor`. People in the demo use illustrated avatars, not photographs. Servers without art fall back to a black tile, a hairline, and a centered initial. The active server tile's hairline is the accent. Add Server is a dashed squircle. There is no shop icon and no Send button on the desktop composer.

## What Discord gets wrong, and what this window does

These are recurring complaints from Discord's own feedback channels, client reviews, and public write-ups of the Electron app. Rencord does not claim to have fixed the service behind them.

| Complaint | What people run into | What this build does |
| --- | --- | --- |
| Clutter | Gift button, boost banners, quests, active-now, and app directory compete with the message | Four regions and a voice card. No promotional row. |
| Nitro upsells | Quality, upload size, and emoji are sold inside the chrome | No purchase surface. Go Live defaults to 720p30 and says a higher request can be refused. |
| Slow channel changes | Large servers reflow the whole Electron tree | Cached switch updates the session and the virtual list. See budgets. |
| Weak keyboard use | Most travel is the mouse. The quick switcher is easy to miss | Ctrl/Cmd+K, Alt arrows, and a visible Ctrl K button. |
| Search | Global search is heavy and easy to open by accident | A field above the channel list filters the open channel only. |
| Notification overload | Badges, pings, and inbox items stack | This milestone does not add a badge or a toast. The in-process notification sink from earlier milestones is unchanged. |
| Cramped compact mode | Compact still keeps uneven padding and hover chrome | Compact is one shorter row height, not a second layout. |
| Buried devices | Input and output are several settings pages down | Settings lists the cpal device names and remembers the choice. Opening the call still uses the default devices; the stored name is not yet passed to cpal. |
| HDR buried behind a Nitro toggle, or promised on a client that cannot show it | Official clients are 8-bit. A switch that says "HDR" with no fallback lies | Settings has Send, Receive, and the tone-map curve on one row. Send and Receive start off. The line under them says the Discord track stays tone-mapped SDR for an unknown audience. The Go Live worker still tone-maps before encode. The row uses the Onyx field: `#000000`, a `#15152A` hairline, `#4F6BFF` when a chip is on, JetBrains Mono at 11 px. |

## Budgets and measurements

Measured on Linux, X11, software GL (DRI3 was unavailable), with `rencord --demo --bench`. The release binary is `target/release/rencord` built with the workspace release profile. The history in that run is 800 cached messages plus the original demo line.

| Budget | Target | Measured |
| --- | --- | --- |
| Cold start to first frame | < 1000 ms | **117 ms** release, **246 ms** debug. This is process start to the first bench frame on a warm page cache, not a packaged installer launched from a cold disk. |
| Idle RSS after that frame | < 200 MiB, and well under the Electron client's typical several-hundred-megabyte working set | **137 MiB** release (`140228` KiB), **183 MiB** debug (`187744` KiB), with the demo history loaded. |
| Channel switch from cache | < 50 ms | **669 ns** average over 200 session switches (`rencord-measure --offline`). The on-screen switch during the bench was **0–3 µs**. |
| Scrolling | Hold the display refresh. Frame gap at or under one refresh period | Frame **p50 16.7 ms** (23 frames) while the bench changed channel every frame, about 60 fps on software GL. A pure 30-row slice of the cached history costs **383 µs**. A 120 Hz panel was not attached, so a 8.3 ms budget was not measured. |
| Binary | Release executable < 80 MB | **63 MB** release. The unstripped debug binary is 840 MB and is not the shipping artifact. |

Re-run:

```sh
cargo run -p rencord-desktop --release --bin rencord -- --demo --bench
cargo run -p rencord-desktop --bin rencord-measure -- --offline
```

The responsive pass measured the breakpoint function and a live resize sweep on llvmpipe (Xvfb, refresh unreported). `layout_resolve` was **95 ns** per size. Channel-switch frame p50 was **48.0 ms** and the resize sweep was **p50 25.6 ms**, **p95 154 ms**, with the slow tail on the 5120-wide frames. Startup was **109 ms** and RSS **139 MiB**. The 16.7 ms row above is the earlier software-GL run; this machine's present path did not hold that rate. The column math is not the cost.

## HDR row

Not a stream. The three chips live in the settings card, under the device lists:

- **Send** defaults off. On means the user is willing to offer 10-bit to Rencord peers. Official clients, unknown clients, and a mixed audience still get tone-mapped SDR. One Go Live track cannot be both.
- **Receive** defaults off. On means this client will present 10-bit PQ on a display whose transfer is PQ or HLG and whose peak is above 200 cd/m², or scRGB when the display is linear and that bright. Any other display gets the tone-map. GPUI's swapchain is not an HDR surface, so the present path returns pixels and a surface name. It does not create the swapchain.
- **Curve** cycles BT.2390 (default), Hable, and ACES.

The copy under the chips is computed from that plan for a hypothetical 1 000 cd/m² BT.2020 master. It includes the sentence that the Go Live worker still tone-maps HDR to 8-bit. A second line is the local probe: EDID from sysfs on Linux when a connector exposes it, otherwise a note that DXGI or ScreenCaptureKit was not opened. The full limits, including what Discord's servers may strip, are in [HDR.md](HDR.md).

## Files over the attachment cap

Discord rejects an attachment above the account cap on the server. The client cannot remove that cap. Discord's own pages disagree on the free number: the API reference says 20 MiB, and an older account-caps table still says 10 MB. Rencord plans against the API figure (20 MiB with no Nitro, 50 MiB for Nitro Basic, 1 GiB for Nitro, boost level 2 at 50 MiB and level 3 at 100 MiB inside that server, taking the larger of the account and the boost). When a response includes `attachment_size_limit`, that value is the one that counts. A file under the planned cap can still be rejected if Discord is experimenting with a different number for that account.

The composer plus button, a dropped file, or a pasted path opens one sheet. It is the only step. There is no Nitro row.

1. **Compress.** Video is a target-size plan: bitrate from the cap and the duration, resolution lowered until the bits-per-pixel match the saved quality preference, then a two-pass ffmpeg command when the encoder supports it. Hardware encoders are preferred when `ffmpeg -encoders` lists one. Rencord does not ship ffmpeg or x264. Images are resized and re-encoded as WebP or JPEG in-process; AVIF is estimated and written only by ffmpeg with an AV1 encoder. 48 kHz WAV is encoded to Opus with libopus. The sheet shows the estimated size and time before anything runs.
2. **Upload a link.** Litterbox (1h, 12h, 24h, 72h), Catbox, a 0x0.st-compatible host, or a custom endpoint (`put https://…` presigned PUT, or `s3 …` multipart, which can resume). The warning names the host. The checkbox starts unchecked. The request is not built into a socket until Upload is pressed with the box checked. Links are kept in a local history file.
3. **Split.** A zip or 7z archive is cut into volumes under the cap, each with a 64-byte `RNSPLT1` header. The message carries a `rencord-split v1` note. A Rencord client shows Reassemble; other clients can concatenate the payloads and extract the archive.

Settings on the sheet remember the default action (Ask, Compress, Link, Split), the host, the quality preference (Smaller, Balanced, Sharper), the expiry, and zip versus 7z. A saved default only pre-selects the row. It does not send the file. Progress, cancel, and retry run off the UI thread.

Layout presets A, B, and C are compared in [DIRECTIONS.md](DIRECTIONS.md). A is the default. B and C stay behind `--layout`. The responsive rules above apply to A.

## Sign in

The welcome screen is Onyx Ink: a black canvas, a white QR card, and a column with the wordmark, **Log in with Discord**, and Other options. The accent button is `#4F6BFF` with black ink. Hints and the disclaimer are JetBrains Mono. Avatars stay round.

The QR encodes `https://discord.com/ra/<fingerprint>` at error-correction level H, with the Hexwave mark in the center: a hex speech bubble, voice bars knocked out, and the middle bar in `#4F6BFF`. The same mark sits to the left of the Rencord wordmark. The left card stays 280 px square in the code, confirm, and expired states. After a scan the card shows the avatar and the name. The line under it says it is waiting for approval, and "Not you? Use a new code" starts again.

Below 900 px, or whenever the window is taller than it is wide, the column stacks: copy, then the code. Those widths live in `breakpoints` next to the sidebar and member-list collapses.

Other options holds email and password (TOTP, SMS, backup codes) and, separately, a token field. A captcha does not get solved here. The app opens Discord's login page instead. Passkeys are completed in that same page. The webview is a separate process so it can own the platform toolkit: WebView2, WKWebView, or WebKitGTK. Its profile directory is removed when the window closes.

Screenshots, captured from the GPUI window on Linux: [login-qr.png](onyx/login-qr.png), [login-confirm.png](onyx/login-confirm.png), [login-error.png](onyx/login-error.png), and the stacked portrait [login-portrait.png](onyx/login-portrait.png).

## What is still rough

- A fenced code block keeps its lines, a monospace face, and a Copy control that appears when the block is hovered.
- Reply and edit are local. Edit replaces the cached string. Reply prefixes the next send. Neither is a Discord API call.
- Reactions are local counts (`wave`, `plus`), not custom emoji.
- The device row records a name. Capture and playback still open cpal's default devices.
- Search does not cross channels, and it does not query Discord.
- Presets and custom themes switch live. The captures in the README are Onyx Ink. Every preset, the editor, and a gradient example are in [themes/](themes/).
- Go Live's picker, viewer, and picture-in-picture use preview frames. The safe preset is 720p30. A 10-bit or PQ source is tone-mapped to 8-bit before that encode. The HDR chips do not open a capture. Send and Receive start off.
- Keyboard bindings need the window focused. They are not a screen-reader pass.
