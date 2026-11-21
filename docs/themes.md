# Themes

Rencord's colors, type, radii, spacing, and shadows are semantic tokens in `rencord-theme`. The desktop shell reads those tokens. It does not embed hex colors. A unit test walks `apps/rencord-desktop/src` and fails if a raw `#RRGGBB` or `0xRRGGBB` literal shows up.

Switching themes is live. The window eases the old palette into the new one over 180 ms unless reduced motion is on.

Custom themes are free. Discord sells client themes and gradients behind Nitro. Rencord does not.

## Presets

| Id | Name | Notes |
| --- | --- | --- |
| `onyx-ink` | Onyx Ink | Default. True black, `#15152A` hairlines, `#4F6BFF` accent, black ink on the accent, `#A8B8FF` links, Inter and JetBrains Mono, 8 px gaps, outlined squircle server tiles, round avatars. |
| `dark` | Dark | Discord-style dark surfaces. |
| `ash` | Ash | Discord-style ash. |
| `onyx` | Onyx | Discord-style onyx. Near-black chat, black rail. Not Onyx Ink. |
| `amoled` | AMOLED | Pure black panels. |
| `light` | Light | Lifted panels, readable muted text, a soft shadow, AA contrast. |
| `midnight-blurple` | Midnight Blurple | Cool gradient, slight grain. |
| `nord` | Nord | Cool blue-gray. |
| `soft-contrast` | Soft Contrast | Warm paper, strong text, still quiet. |
| `crisp` | Crisp | The earlier Rencord dark palette. |

`--theme` accepts those ids. `ink` is Onyx Ink. `crisp` and `c` are Crisp.

## Sync

Settings can follow the operating system, with one preset for light and another for dark.

- Linux reads the settings portal `org.freedesktop.appearance` `color-scheme`, then GNOME's `color-scheme` if the portal is silent.
- macOS reads `AppleInterfaceStyle`.
- Windows reads `AppsUseLightTheme`.

The window checks about once a second, so a system change shows up without a restart.

Appearance is stored per account under the data directory, in `accounts/<id>/appearance.json`. It is not written back to Discord. "Follow Discord" is opt-in and only reads the legacy `user_settings.theme` / `client_theme` fields on READY. On the first sign-in, if nothing is saved yet, those fields are used once as the starting preset.

## Custom themes

Settings > Appearance can:

- pick a base color and an accent; the rest of the palette is derived in OKLCH and contrast-corrected
- turn on a gradient, grain from 0 to 3%, and an optional background image with blur and dim (the image path is stored on the theme)
- scale type, message spacing, corner radius, and saturation
- edit every color token
- show a preview of the sidebar, a mention, a code line, the voice strip, and buttons
- warn when a pair drops under WCAG AA (4.5:1 for text, 3:1 for status dots)

High contrast, reduced motion, and colorblind-safe status colors are toggles. Colorblind status uses the Okabe–Ito set (blue, orange, reddish purple, gray) and then lifts each color until it clears the chat surface.

## Files

The document shape is `docs/themes/theme.schema.json`. TOML and JSON use the same fields.

Examples:

- [examples/onyx-ink.toml](themes/examples/onyx-ink.toml)
- [examples/midnight-blurple.json](themes/examples/midnight-blurple.json)
- [examples/harbor.toml](themes/examples/harbor.toml)

```sh
rencord --demo --theme-file docs/themes/examples/harbor.toml
```

A theme code is the prefix `rencord1.` plus URL-safe base64 of the zlib-compressed JSON document. Settings can copy one and import one.

Drop `.toml` or `.json` files in the themes directory (`$RENCORD_DATA_DIR/themes` or `~/.local/share/rencord/themes`). The window reloads that folder about once a second. Saving from Settings writes the current theme there.

## For other UI work

New interface code should take a `Palette` (or a `Theme`) and use its fields: `palette.text`, `palette.bg` via `palette.chat`, `palette.accent`, `palette.status_online`, and so on. Do not add a hex literal under `apps/rencord-desktop/src`. Put new colors on the token struct in `rencord-theme` and map them in `Palette::from_theme`.
