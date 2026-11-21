# Rencord brand

Waver is the README wordmark. Hexwave is the app icon. These files are the exported bundle. Do not redraw them. `src/build_hexwave.py` is the generator that produced `final/`; it imports the logo workspace it was written against (`build_logo`), which is not in this repository.

## Colors

| Token | Hex | Use |
| --- | --- | --- |
| Black | `#000000` | Waver tile, icon squircle, ink on light |
| Ink | `#F2F3FA` | Waver word and mark on dark |
| Blurple | `#4F6BFF` | Middle bar of the color Hexwave mark |

## Usage

- **Waver** is the README and wordmark only. `readme-header.svg` and `readme-header.png` (1440×400) are the same picture. Use the PNG in the README. It stays on a fixed black tile in GitHub light and dark mode. Do not wrap it in `<picture>` or swap a light variant.
- **Hexwave** is the app icon: a hex speech bubble with the Waver bars knocked out. The color mark keeps the middle bar at `#4F6BFF`.
- **3-bar cut at 24 px and below.** `hexwave.ico` already uses that cut at 16 and 24. `hexwave-tray.svg` is the same cut for a tray or any glyph at or under 24 px. Larger sizes use `hexwave-mark.svg` (four bars).
- In the app, tint the mark's `currentColor` body with the theme text color. Leave the `#4F6BFF` bar alone. `crates/rencord-theme` is not in this tree; the desktop palette is the token source. `crates/rencord-auth` is not in this tree; the token-paste login in `apps/rencord-desktop` is the login surface that shows the mark.

## Files

| File | What it is |
| --- | --- |
| `readme-header.svg` | Waver on the black tile, vector |
| `readme-header.png` | Same header, 1440×400, used by the README |
| `hexwave-icon-1024.png` | Squircle app icon, 1024 |
| `hexwave.ico` | Windows icon, 16–256. 16 and 24 are the 3-bar cut |
| `hexwave.icns` | macOS bundle icon, packed from `icon.iconset/` |
| `icon.iconset/` | iconutil source (16 through 1024, including @2x) |
| `hexwave-mark.svg` | Hexwave A knockout. `currentColor` body, `#4F6BFF` middle bar. Login and settings, above 24 px |
| `hexwave-mark-color.svg` | Same mark with `#F2F3FA` body |
| `hexwave-mark-mono.svg` | Same mark, one ink |
| `hexwave-mark-on-dark.svg` | Color mark on `#000000` |
| `hexwave-mark-on-light.svg` | Color mark on a light field |
| `hexwave-mark-512.png` | Raster of the mark, 512 |
| `hexwave-small-mark-color.svg` | 3-bar cut, `#F2F3FA` body, `#4F6BFF` bar |
| `hexwave-small-mark-mono.svg` | 3-bar cut, one ink |
| `hexwave-small-mark-on-dark.svg` | 3-bar cut on `#000000` |
| `hexwave-small-mark-on-light.svg` | 3-bar cut on a light field |
| `hexwave-tray.svg` | 3-bar tray glyph, `currentColor`, 16 px nominal. No tray is linked yet |
| `hexwave-symbolic.svg` | Symbolic icon for hicolor, `currentColor` |
| `hexwave-sheet.png` | Contact sheet of the cuts |
| `hexwave-a-mark-color.svg` | Cut A, color. Not the shipping icon |
| `hexwave-a-mark-mono.svg` | Cut A, mono |
| `hexwave-a-mark-on-dark.svg` | Cut A on dark |
| `hexwave-a-mark-on-light.svg` | Cut A on light |
| `hexwave-b-mark-color.svg` | Cut B, color. Not the shipping icon |
| `hexwave-b-mark-mono.svg` | Cut B, mono |
| `hexwave-b-mark-on-dark.svg` | Cut B on dark |
| `hexwave-b-mark-on-light.svg` | Cut B on light |
| `hexwave-c-mark-color.svg` | Cut C, color. Not the shipping icon |
| `hexwave-c-mark-mono.svg` | Cut C, mono |
| `hexwave-c-mark-on-dark.svg` | Cut C on dark |
| `hexwave-c-mark-on-light.svg` | Cut C on light |
| `linux/hexwave-48.png` | App icon 48 |
| `linux/hexwave-64.png` | App icon 64 |
| `linux/hexwave-128.png` | App icon 128 |
| `linux/hexwave-256.png` | App icon 256 |
| `linux/hexwave-512.png` | App icon 512 |
| `linux/hicolor/<size>/apps/rencord.png` | Those sizes under the hicolor theme, named `rencord` |
| `linux/hicolor/symbolic/apps/rencord-symbolic.svg` | `hexwave-symbolic.svg` for the symbolic theme |
| `linux/rencord.desktop` | Desktop entry. `Icon=rencord`, `StartupWMClass=rencord` |
| `src/build_hexwave.py` | Generator script from the bundle |

## Where the icon is wired

- Windows: `apps/rencord-desktop/build.rs` embeds `hexwave.ico` with `embed-resource` when the target is Windows. Other targets skip it.
- macOS: `[package.metadata.bundle]` and `[package.metadata.packager]` point at `hexwave.icns`.
- Linux: install `linux/rencord.desktop` and `linux/hicolor/`. The desktop file's icon name matches the hicolor files.
- Runtime: GPUI `WindowOptions::icon` is `linux/hexwave-64.png` (X11 `_NET_WM_ICON`). A 128 px or 256 px image is accepted by the API and comes back as an empty property, so the runtime icon is 64 px. `app_id` is `rencord`, which Wayland uses to find the hicolor icon. macOS and Windows take the icon from the bundle and the exe resource.
- Tray: nothing creates a tray. `rencord_platform::TRAY_ICON_SVG` is `hexwave-tray.svg` for that later hook.
- In the window: the server-rail home button (30 px inside the 44 px tile), the login title, and the settings header use `hexwave-mark.svg`. GPUI paints an SVG element as one color, so the app draws that file with resvg at 256 px or more and scales it to the device pixels with Catmull-Rom (1×, 1.5×, and 2×). The body follows the theme text color. The third bar stays the blurple in the file. The tile's fill, border, hover, and selection pill are unchanged. A glyph at 24 px or below, such as the tray, uses the 3-bar cut.
