# Layout directions

Three presets share one session, the Onyx / Dark / Ash / Light themes, and one message list. Switch them with Settings > Layout or `--layout a|b|c`. This pull request is not merged until one direction is chosen.

The November 2026 research note is reflected in the shared chrome: no second server-name bar, a flush dock of separate circular mic, deafen, and settings buttons, Compact / Cozy / Spacious plus independent font, UI scale, and server-icon size, a soft ping wash instead of harsh yellow, Ctrl+K with a Ctrl+/ cheat sheet, Ctrl+I mentions with Mark all read, local favorites, member list hidden under 1100 px and the channel column under 760 px, and a split-pane toggle (Ctrl+\\). Icons in `docs/design/icons/` are original 24 px `currentColor` strokes. A runtime pack can live in `~/.config/rencord/icon-pack/`. No Discord asset files are in the repo.

Icons in `docs/design/icons/` are original 24 px strokes (hash, speaker, pin, inbox, search). They are not Discord's files. `discord_install_icon_dir()` only reports a local install path if one exists. It does not copy those assets into the repo.

## A. Discord, perfected

`--layout a`

Familiar rail, channel sidebar, conversation, member list, voice card at the bottom of the sidebar. Same places a Discord user already looks, without a shop, gift button, or boost banner. Code blocks, reply, edit, and delete sit on the message.

Borrows the information architecture people already know.

Pros: lowest learning cost, screenshots match the muscle memory of the official client, density and keyboard shortcuts are the improvement.

Cons: still four columns, so a narrow window feels tight. The member list is often empty space.

Performance: this is the layout the bench used. Debug build on this machine, software GL: first frame 251 ms, RSS about 183 MiB, frame p50 16.7 ms. The X server did not report a refresh interval (`refresh_hz=unreported`). A 16.7 ms gap is what a 60 Hz compositor produces. A 120 Hz budget would be 8.3 ms. That display was not attached, so 120 Hz is not a measured result. GPUI is asked to follow the display interval. This build does not set a 60 fps cap.

## B. Keyboard-first

`--layout b`

The server rail and channel tree are gone. A one-line bar says the quick switcher is home. The conversation fills the window. A narrow split pane on the right names the second channel and holds the voice card. Ctrl+K is the way to move.

Borrows Linear and Raycast: the palette is the navigation, chrome stays out of the way.

Pros: more room for the message and the code block. Fewer things to tab through.

Cons: the second pane does not yet render that channel's messages (this guild's only other channel is voice, so the pane says there is no second text channel). Discoverability of servers is worse until the palette is learned.

Performance: same list and same session as A. Hiding the rail removes a column of buttons. That does not change the channel-switch budget (805 ns average over 200 cached switches in `rencord-measure --offline`).

## C. Unified sidebar

`--layout c`

Favorites and an inbox line sit above a combined server-and-channel column. The member list stays. The voice card floats over the bottom right, the stand-in for a picture-in-picture call.

Borrows Arc's sidebar and Telegram's compact navigation. The inbox is a label, not a mention query yet.

Pros: one place for "where am I" and a voice card that does not steal a row from the channel list.

Cons: the floating card covers the composer on a short window. Favorites are a fixed "# general", not a user list.

Performance: same data path. The extra labels are a handful of text nodes.

## Screenshots

Captured from `rencord --demo` on Linux, X11, software GL.

| | Dark | AMOLED |
| --- | --- | --- |
| A | [a-dark.png](directions/a-dark.png) | [a-amoled.png](directions/a-amoled.png) |
| B | [b-dark.png](directions/b-dark.png) | [b-amoled.png](directions/b-amoled.png) |
| C | [c-dark.png](directions/c-dark.png) | [c-amoled.png](directions/c-amoled.png) |

## What the six fixes do in every layout

- Code blocks are multi-line, monospace, on the code background, with a Copy button and a small keyword highlight. The message list is GPUI's variable-height `list`, which measures and caches row heights.
- Reply, edit, delete, and reactions update the session immediately. A signed-in window sends REST (`message_reference`, PATCH, DELETE, reaction PUT/DELETE) and rolls back if Discord returns an error. Demo mode stays local. Gateway `MESSAGE_DELETE` is decoded. Tests hit a local HTTP stub.
- The device row calls `open_call_audio` with the chosen names. A missing device falls back to the default. Switching while a call session is open replaces the streams.
- Search understands `from:`, `in:`, `has:`, `before:`, and `after:` across cached channels. A hit jumps to that message. `remote_search_path` builds Discord's guild search URL.
- Focus rings use a 1 px accent border. The window root is an AccessKit document with a label. GPUI's `with_animation` already skips motion when reduced motion is on. Body text, muted text, and accent-on-white pass WCAG AA 4.5:1 on AMOLED, dark, and light.
- Frame pacing is not capped at 60 in this code. The bench prints the display interval when the platform reports one.

## Files over the cap

Not built. See the roadmap in the README and in `UX.md`. Discord enforces the size on the server.
