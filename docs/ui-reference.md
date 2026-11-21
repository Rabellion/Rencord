# Desktop layout reference

Rencord’s signed-in window follows the classic Discord desktop layout: a server rail, a channel sidebar, a chat column, and a member list. The visual target is that layout as it appears in a current desktop client (Home and direct messages, a guild channel, the member list docked on a wide window, and the same list as a centred dialog on a narrow window). The colours stay Onyx Ink, and the Home button is the Hexwave mark. This note records the sizes those columns use, and where each number comes from. It does not copy another client’s code, logo, font file, or artwork.

gg sans is Discord’s own face and is not available for other projects ([gg sans FAQ](https://support.discord.com/hc/en-us/articles/9507780972951-gg-sans-Font-Update-FAQ), updated 29 November 2022). Rencord draws the same sizes in Segoe UI Variable, then Segoe UI, then the bundled Inter face.

## Sources

- [How do I join a Server?](https://support.discord.com/hc/en-us/articles/360034842871-How-do-I-join-a-Server) (Discord support, 11 March 2025). The far-left column is the server list. Each server is a circular icon. Direct messages open from the Discord icon at the top of that list, not from inside a server.
- [Beginner’s Guide to Discord](https://support.discord.com/hc/en-us/articles/360045138571-Beginner-s-Guide-to-Discord). A server is text channels and voice channels.
- [Keyboard Navigation FAQ](https://support.discord.com/hc/en-us/articles/1500000056121-Keyboard-Navigation-FAQ) (14 December 2020). The desktop sections are the server list, the channel list, messages, and the member list.
- [How to Change Discord Color Themes and Customize Appearance Settings](https://support.discord.com/hc/en-us/articles/207260127-How-to-Change-Discord-Color-Themes-and-Customize-Appearance-Settings) (27 June 2026) and [Making Discord on Desktop Look Just Right](https://discord.com/blog/making-discord-on-desktop-look-just-right-display-settings-to-ease-the-eyes) (16 April 2026). UI density is Compact, Default, or Spacious. Chat display is Default (avatars, a gap between people) or Compact (no avatars, the name and the text on one line). Group spacing separates different people, not one person’s own run of messages.
- [How do I switch to compact text mode?](https://support.discord.com/hc/en-us/articles/217047657-How-do-I-switch-to-compact-text-mode) (30 October 2022). Compact mode drops the avatars.
- [Discord brand guidelines](https://discord.com/branding). Blurple is `#5865F2`. Rencord does not use that colour or the Discord marks. The accent here is Onyx `#4F6BFF`.
- [GuildbarRevert.css](https://github.com/scattagain/VencordStuff/blob/main/css/GuildbarRevert.css) (Vencord, read 21 November 2026). Restores the classic rail: icon 48px, padding 12px, rail width `48 + 12 + 12` = 72px, circle at rest, selected corner `27%` (about 13px on a 48px icon).
- Matojeje, [Revert refresh 2025](https://gist.github.com/Matojeje/ae534d72779c942d0ecd0682289ef7cf) (17 April 2025). Sets `--custom-guild-list-width: 72px`, `--custom-guild-sidebar-width: 311px` (rail plus channel column; 311 − 72 = 239), `--custom-channel-header-height: 48px`, and the separator under Home at 32×2px.
- refact0r, [midnight-discord](https://github.com/refact0r/midnight-discord) layout notes (2025–2026). The client’s own script sets member rows to 42px and group headers to 38px, DM rows to 44px in the default density (40 compact, 50 cozy), and refuses a CSS-only row height that fights the virtual list.
- DarkIRC 1.0.13 notes ([hiqporoid/darkirc-vencord-theme](https://github.com/hiqporoid/darkirc-vencord-theme)). Message text 14px was their theme choice; they record Discord’s member rows at 42px and keep channel icons at 16px in that theme. Classic channel icons in the pre-refresh client are 20px; Rencord uses 20px so the hash mark matches the 32px row.
- discrub, [`messageChunking.ts`](https://github.com/prathercc/discrub/blob/048b15b6/src/utils/messageChunking.ts). Same author, both type 0, and less than `7 * 60 * 1000` ms. A date divider is a new local calendar day (`exportService.ts` in the same tree).

## Measured layout

| Piece | Value | Source |
| --- | --- | --- |
| Server rail width | 72px | GuildbarRevert: 48px icon + 12px padding each side. Mato sets the variable to 72px. |
| Server icon | 48px | GuildbarRevert `--guildbar-avatar-size`. |
| Rail padding | 12px | GuildbarRevert `--custom-guild-list-padding`. |
| Icon gap | 8px | Classic list rhythm. The 2025 refresh opened this up; classic stays on an 8px step. |
| Icon radius at rest | 50% (circle) | GuildbarRevert `border-radius: 50%`. Discord support calls the icon circular. |
| Icon radius, hover and selected | 16px | Classic selected squircle. GuildbarRevert’s 2025 restore uses 27% (about 13px). Rencord uses 16px so the hover and the selected state read as the same rounded square. |
| Selected pill | 4×40px | Classic guild pill: a short bar when idle is unread (8×8) and a tall bar when selected. |
| Unread pill | 8×8px | Same pill control, short. |
| Home separator | 32×2px | Mato revert theme, the rule under the Home button. |
| Channel sidebar width | 240px | Mato sidebar 311px minus the 72px rail is 239px. The classic column is 240px. |
| Sidebar header | 48px | Mato `--custom-channel-header-height: 48px`. Same height as the chat header. |
| Category label | 12px, weight 700, uppercase, tracking 0.02em | Classic channel category. Top margin 16px, bottom 4px, side padding 8px. |
| Channel row | 32px, padding 0 8px, margin 0 8px, radius 4px | Classic cozy row. The row does not shrink when the list is long; the list scrolls. |
| Channel icon | 20px | Hash for text, speaker for voice. |
| Channel name | 16px, weight 500 selected or unread, 400 otherwise | Same UI face as the rest of the window. |
| Hover row | fill lifts one step | Discord’s hover modifier. Onyx uses the theme hover token. |
| Selected row | fill is the selected token, name at full text colour | No extra square border. |
| User panel height | 52px | Classic account bar under the channel list. |
| User avatar | 32px, status dot 10px | Sits in the 52px bar with 8px insets. |
| User icon buttons | 32px | Mute, deafen, settings, in a row that does not wrap. |
| Chat header | 48px | Mato channel header variable. |
| Header content | hash, name, 1px rule, topic | Topic truncates. An empty topic draws no rule and no filler sentence. |
| Message avatar | 40px | Cozy display. Compact display omits it ([compact mode article](https://support.discord.com/hc/en-us/articles/217047657-How-do-I-switch-to-compact-text-mode)). |
| Message gutter | 72px | 16px inset + 40px avatar + 16px gap. |
| Username | 16px, weight 500 | UI face, not monospace. |
| Timestamp | 12px, weight 400, muted | Same UI face. Full local date and time on hover. |
| Body | 16px, weight 400, line height 1.375 | UI face. |
| Grouping | under 7 minutes, same author, both normal messages | discrub `CHUNK_WINDOW_MS`. A reply or a system line starts a new group. |
| Date divider | only when the local calendar day changes | discrub export renders `MMMM d, yyyy` between those messages. |
| System line | one row, an arrow, “Name joined the server.” | Message type 7. Pins (type 6) and boosts (types 8–11) use the same row. |
| Hover on a message | a flat hover fill, no extra box | |
| Composer | about 44px inside an 8px radius, 16px side margin | The field’s placeholder is `Message #` plus the open channel. |
| Member list width | 240px | Same column as the classic member sidebar. |
| Member row | 42px, avatar 32px | midnight-discord, sizes the client script actually lays out. |
| Role header | 12px, weight 700, uppercase | Grouped by status when roles are not loaded. |
| Direct messages | only on Home | Discord support: the top icon opens direct messages. They are not a category inside a server. |

## Onyx Ink in place of Discord’s palette

Layout, spacing, type, and behaviour follow the table. Paint comes from Onyx Ink.

| Discord role | Discord (brand or classic dark) | Onyx Ink |
| --- | --- | --- |
| Accent | `#5865F2` blurple ([brand page](https://discord.com/branding)) | `#4F6BFF` |
| App, rail, sidebar, chat | layered greys (`#1e1f22`, `#2b2d31`, `#313338`) | `#000000` |
| Hairline and divider | a grey rule | `#15152A` |
| Primary text | near-white | `#EDEEF7` |
| Muted text and timestamps | grey | `#9AA0C0` |
| Hover and selected fills | translucent white on grey | theme hover and selected tokens on black |

## What this file does not include

No Discord logo, no Clyde, no gg sans file, and no copied client stylesheet. The links above are the reference shots and the measurements.
