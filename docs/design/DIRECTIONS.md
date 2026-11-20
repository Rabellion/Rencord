# Layout directions

Direction A is the chosen layout and the default. `--layout a` (or no flag) opens the Discord-shaped window. B and C stay in the same binary behind `--layout b` and `--layout c`, and behind Settings > Layout. They are not the product.

## A. Discord, perfected — chosen

Familiar rail, channel sidebar, conversation, member list, and a user panel at the bottom of the sidebar. No shop, gift button, or boost banner.

What the window does:

- 48 px rail tiles. A home button, a separator, server marks, another separator, then add and explore. The selected tile eases from a circle to a squircle and grows a pill.
- Channel rows use the clean-room hash, speaker, and pin icons. Categories are uppercase and collapsible in label only. The selected row is filled. Mentions use a red count.
- The chat header is one bar: hash, name, topic, and icons for threads, pins, the member list, search, and inbox. There is no second bar above it.
- Messages group by author. Names use role colors that stay above WCAG AA on every theme. Timestamps are muted. Avatars are 40 px and centered. Reply, edit, and delete are icons on a bar that appears on hover.
- The demo channel holds a code block with Copy, a reply, an `@Ada` mention wash, a link preview, a reaction, and an image attachment.
- Search and the composer use a themed fill (`#383A40` on Ash and Dark, `#1E1F22` on Onyx, `#EBEDEF` on Light), with placeholder text. The composer has attach, GIF, and emoji icons.
- The user panel is flush: avatar, status dot, name, status, and round mic, deafen, and settings buttons. A call adds a Voice Connected strip with a disconnect button.
- The member list is `ONLINE — 3`, with a status dot and an activity line.
- Type is Inter at the Discord scale (11 / 13 / 15 / 16). Icons are the 24 px `currentColor` strokes in `docs/design/icons/`.

Themes are Onyx, Dark, Ash, and Light. Body text, muted text, field text, placeholder text, and white-on-accent pass WCAG AA 4.5:1. Compact, Cozy, and Spacious change channel-row and server-icon size. Font size, UI scale, and icon size stay independent.

## B. Keyboard-first — still behind the flag

`--layout b`

The rail and channel tree are gone. A one-line bar says the quick switcher is home. The conversation fills the window. A narrow split pane on the right names the second channel and holds the voice card. Ctrl+K is the way to move. The second pane does not yet render that channel's messages.

## C. Unified sidebar — still behind the flag

`--layout c`

Favorites and an inbox line sit above a combined server-and-channel column. The voice card floats over the bottom right. The floating card can cover the composer on a short window.

## Screenshots

Captured from `rencord --demo --layout a` on Linux, X11, software GL. The pointer was parked on the title bar so hover chrome stays hidden.

| | |
| --- | --- |
| Dark | [a-dark.png](directions/a-dark.png) |
| Ash | [a-ash.png](directions/a-ash.png) |
| Onyx | [a-amoled.png](directions/a-amoled.png) |
| Light | [a-light.png](directions/a-light.png) |
| Voice connected | [a-voice.png](directions/a-voice.png) |
| Compact | [a-compact.png](directions/a-compact.png) |

B and C captures from the earlier pass are still in this folder (`b-*.png`, `c-*.png`). They are not the chosen direction.

## Shared behavior

- Code blocks are multi-line, monospace, on the code background, with a Copy button and a small keyword highlight. The message list is GPUI's variable-height `list`.
- Reply, edit, delete, and reactions update the session immediately. A signed-in window sends REST and rolls back if Discord returns an error. Demo mode stays local.
- The device row calls `open_call_audio` with the chosen names. A missing device falls back to the default.
- Search understands `from:`, `in:`, `has:`, `before:`, and `after:` across cached channels. A hit jumps to that message.
- The window root is an AccessKit document. GPUI's `with_animation` skips motion when reduced motion is on.
- Frame pacing is not capped at 60 in this code. The bench prints the display interval when the platform reports one. This machine's X server did not report one (`refresh_hz=unreported`). A 16.7 ms gap is a 60 Hz compositor. 120 Hz was not measured.

## Files over the cap

Not built. See the roadmap in the README and in `UX.md`. Discord enforces the size on the server.
