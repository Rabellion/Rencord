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

Tokens are sRGB. AMOLED uses true black (`#000000`) for the window and the conversation so an OLED panel can turn those pixels off. Dark is the default and keeps a lifted gray so panels separate on an LCD. Light inverts the text and surfaces. Accent is a single blue used for the selected server, links, mentions, and avatars. Danger red is reserved for destructive states and is not used as decoration.

| Token | AMOLED | Dark | Light |
| --- | --- | --- | --- |
| App / chat | `#000000` | `#111214` / `#1A1B1E` | `#F2F3F5` / `#FFFFFF` |
| Rail | `#000000` | `#0C0D0F` | `#E3E5E8` |
| Sidebar | `#050505` | `#17181B` | `#FFFFFF` |
| Elevated | `#101010` | `#232428` | `#FFFFFF` |
| Hover | `#161616` | `#2B2D31` | `#EAEBEE` |
| Selected | `#1C1C1C` | `#35373C` | `#E0E1E6` |
| Text | `#F2F3F5` | `#F2F3F5` | `#1E1F22` |
| Muted | `#A0A2A8` | `#B5BAC1` | `#4E5058` |
| Accent | `#5B8CFF` | `#5865F2` | `#5865F2` |
| Line | `#1A1A1A` | `#2E3035` | `#E1E2E4` |

### Type

The UI font is the platform UI font. Sizes at the middle step are 11, 13, 15, 18, and 24 px. The settings control picks 13, 15, or 17 px for the window. Bold marks strong markdown. Italic is drawn in the muted color because a true italic face is not bundled. Code and links use the accent color.

### Space, radius, motion

Spacing steps are 4, 8, 12, 16, 24, and 32 px. Radii are 0, 6 (rows and fields), 12 (panels and the voice card), and a pill for round avatars. Hover feedback is 80 ms. Overlays fade in over 140 ms with the default ease-out curve. Layout moves, when they animate, use 180 ms. `reduce motion` in GPUI skips the overlay fade.

### Density

Cozy rows are 68 px. Compact rows are 44 px. Both heights are fixed so the virtual list can keep its measurement. Grouped messages from the same author leave the avatar gutter empty instead of repeating the name.

### Iconography

Servers use a one-letter mark, not a downloaded icon. People use the same treatment. Voice, reply, and edit are words (`Mute`, `Reply`, `Edit`) so the window stays readable without an icon font. There is no shop icon.

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

## What is still rough

- Rows are one height, so a fenced code block is flattened onto the message line instead of growing the row.
- Reply and edit are local. Edit replaces the cached string. Reply prefixes the next send. Neither is a Discord API call.
- Reactions are local counts (`wave`, `plus`), not custom emoji.
- The device row records a name. Capture and playback still open cpal's default devices.
- Search does not cross channels, and it does not query Discord.
- Light theme exists and switches live. The captures in the README are dark and AMOLED.
- Keyboard bindings need the window focused. They are not a screen-reader pass.
