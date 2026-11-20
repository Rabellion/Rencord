# Rencord interface

The window should feel quieter than Discord and cheaper to run. This document is the contract for color, type, motion, and the complaints the layout is meant to answer. Numbers in the budgets section were measured on this Linux machine; they are not a promise about every GPU.

## Principles

1. **One job per region.** The rail is servers. The sidebar is channels. The center is the conversation. The right column is people. Voice sits in a card, not across the whole window.
2. **No upsells.** There is no Nitro shop, gift button, quest, boost banner, or "upgrade" row. Stream presets stay in code until capture exists, and the settings screen says Go Live is absent.
3. **Keyboard first.** Ctrl/Cmd+K opens the quick switcher. Alt+Up/Down moves channels. Ctrl/Cmd+Alt+Up/Down moves servers. Esc closes the switcher and settings. Ctrl/Cmd+, opens settings.
4. **The list does not draw what is off screen.** Messages use GPUI's uniform list. Rows share one height so scrolling does not remeasure the whole history.
5. **Themes are data.** AMOLED, dark, and light are token sets. Switching them does not rebuild the session.

## Design system

### Color

The default theme is Crisp Minimal. The window canvas is `#16181D`. Rail, channel list, chat, and members are `#1C1F26` panels separated by a 12 px gap. Inputs are `#23272F`. Onyx keeps that structure on a `#000000` canvas with `#0C0E12` panels. Light uses `#E6E9F0` behind `#F7F8FB` panels. Accent is blurple `#5865F2`, used for the new-messages banner, mentions, and links. Body text is `#EDF0F8`. Meta text is `#8592AD`.

| Token | Crisp | Onyx | Light |
| --- | --- | --- | --- |
| Canvas | `#16181D` | `#000000` | `#E6E9F0` |
| Panels | `#1C1F26` | `#0C0E12` | `#F7F8FB` |
| Input | `#23272F` | `#14171C` | `#FFFFFF` |
| Text | `#EDF0F8` | `#EDF0F8` | `#1A1D26` |
| Muted | `#8592AD` | `#8592AD` | `#4E5870` |
| Accent | `#5865F2` | `#5865F2` | `#5865F2` |

### Type

The UI face is Figtree, bundled under the SIL Open Font License. Message body is 16 px. Names are 16 px semibold in a muted role tint. Timestamps, topics, and activity are 12 px muted. Code is JetBrains Mono at 13–14 px. The settings control still steps the window size through 13, 15, and 17 px.

### Space, radius, motion

Spacing steps are 4, 8, 12, 16, 24, and 32 px. Controls use a 6 px radius. Panels, the composer, code, embeds, and media use 12 px. Avatars and the date pill are fully round. Columns do not use drop shadows. A 1 px hairline separates a panel from the canvas. Hover feedback is 80 ms. Overlays fade in over 140 ms. `reduce motion` in GPUI skips the overlay fade.

### Density

Cozy channel rows are 32 px. Compact rows are 28 px. Spacious rows are 36 px. Grouped messages from the same author leave the avatar gutter empty instead of repeating the name.

### Iconography

One clean-room set, 24 px viewBox, 1.5 px stroke, `currentColor`. People in the demo use photographs. Servers without art fall back to a muted tile and a centered initial. There is no shop icon and no Send button on the desktop composer.

## What Discord gets wrong, and what this window does

These are recurring complaints from Discord's own feedback channels, client reviews, and public write-ups of the Electron app. Rencord does not claim to have fixed the service behind them.

| Complaint | What people run into | What this build does |
| --- | --- | --- |
| Clutter | Gift button, boost banners, quests, active-now, and app directory compete with the message | Four regions and a voice card. No promotional row. |
| Nitro upsells | Quality, upload size, and emoji are sold inside the chrome | No purchase surface. Settings states that Go Live is not built. |
| Slow channel changes | Large servers reflow the whole Electron tree | Cached switch updates the session and the virtual list. See budgets. |
| Weak keyboard use | Most travel is the mouse. The quick switcher is easy to miss | Ctrl/Cmd+K, Alt arrows, and a visible Ctrl K button. |
| Search | Global search is heavy and easy to open by accident | A field above the channel list filters the open channel only. |
| Notification overload | Badges, pings, and inbox items stack | This milestone does not add a badge or a toast. The in-process notification sink from earlier milestones is unchanged. |
| Cramped compact mode | Compact still keeps uneven padding and hover chrome | Compact is one shorter row height, not a second layout. |
| Buried devices | Input and output are several settings pages down | Settings lists the cpal device names and remembers the choice. Opening the call still uses the default devices; the stored name is not yet passed to cpal. |

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

## Later: files over the attachment cap

Not part of the current UI fixes. Discord rejects attachments above the account cap on the server. The client cannot remove that cap.

The later milestone is one dialog, with a default the user can save:

1. Compress to fit: re-encode video, or downscale and re-encode images.
2. Upload the original to an external host (Litterbox, Catbox, or a host the user configures) and post the link. Rencord renders that link as an inline preview.
3. Optionally split the file into parts.

The dialog should be the only step. There is no second settings hunt and no Nitro upsell.

Layout presets A, B, and C are compared in [DIRECTIONS.md](DIRECTIONS.md). They are not on `main`.

## What is still rough

- Rows are one height, so a fenced code block is flattened onto the message line instead of growing the row.
- Reply and edit are local. Edit replaces the cached string. Reply prefixes the next send. Neither is a Discord API call.
- Reactions are local counts (`wave`, `plus`), not custom emoji.
- The device row records a name. Capture and playback still open cpal's default devices.
- Search does not cross channels, and it does not query Discord.
- Light theme exists and switches live. The captures in the README are dark and AMOLED.
- Keyboard bindings need the window focused. They are not a screen-reader pass.
