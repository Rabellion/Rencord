#!/usr/bin/env python3
"""Hexwave app mark + Waver README header — final Rencord logo exports."""
from __future__ import annotations

import sys
from io import BytesIO
from pathlib import Path

import cairosvg
from PIL import Image, ImageDraw, ImageFilter, ImageFont

sys.path.insert(0, "/workspace/rencord/logo")
from build_logo import (  # noqa: E402
    ACCENT,
    BLACK,
    GEIST,
    HAIRLINE,
    INK_ON_DARK,
    INK_ON_LIGHT,
    WHITE,
    _inner_svg,
    geist_font,
    mark_c2_waver,
    svg_to_pil,
    wordmark_path,
)

ROOT = Path("/workspace/rencord/logo/final")

# Flat-top rounded hex + speech tail (same silhouette as Hexnode, no inner hole).
HEX = (
    "M 40 16 H 88 "
    "A 16 16 0 0 1 101.86 24 "
    "L 118 54 "
    "A 16 16 0 0 1 118 70 "
    "L 101.86 100 "
    "A 16 16 0 0 1 88 108 "
    "H 68 "
    "L 30 126 "
    "C 24 130 20 124 26 120 "
    "L 44 108 "
    "H 40 "
    "A 16 16 0 0 1 26.14 100 "
    "L 10 70 "
    "A 16 16 0 0 1 10 54 "
    "L 26.14 24 "
    "A 16 16 0 0 1 40 16 Z"
)

# Original Waver bars in the 128 viewBox.
WAVER = [
    # x, y, w, h, rx, is_accent
    (28.0, 22.0, 18.0, 84.0, 9.0, False),
    (56.0, 38.0, 14.0, 40.0, 7.0, False),
    (78.0, 26.0, 14.0, 52.0, 7.0, True),
    (100.0, 44.0, 14.0, 34.0, 7.0, False),
]


def _mass_center(bars):
    m = cx = cy = 0.0
    for x, y, w, h, rx, acc in bars:
        a = w * h
        m += a
        cx += (x + w / 2) * a
        cy += (y + h / 2) * a
    return cx / m, cy / m


WAVER_CX, WAVER_CY = _mass_center(WAVER)

# Hex *body* optical center (excludes the speech tail). Slightly above
# geometric mid (y=62) so the tail doesn't pull the glyph visually low.
# x is a blend of mass-center and bbox-center so padding stays balanced.
HEX_BODY_CX = 61.0
HEX_BODY_CY = 61.0


def rrect(x, y, w, h, rx) -> str:
    rx = min(rx, w / 2, h / 2)
    return (
        f"M {x + rx:.3f} {y:.3f} "
        f"H {x + w - rx:.3f} "
        f"A {rx:.3f} {rx:.3f} 0 0 1 {x + w:.3f} {y + rx:.3f} "
        f"V {y + h - rx:.3f} "
        f"A {rx:.3f} {rx:.3f} 0 0 1 {x + w - rx:.3f} {y + h:.3f} "
        f"H {x + rx:.3f} "
        f"A {rx:.3f} {rx:.3f} 0 0 1 {x:.3f} {y + h - rx:.3f} "
        f"V {y + rx:.3f} "
        f"A {rx:.3f} {rx:.3f} 0 0 1 {x + rx:.3f} {y:.3f} Z"
    )


def place_waver(scale: float, fatten: float = 0.0, fatten_h: float = 0.0,
                tx: float = HEX_BODY_CX, ty: float = HEX_BODY_CY):
    """Scale Waver about its mass center, then sit that center on (tx, ty).

    fatten grows bar width about the bar center (for the small-size cut).
    """
    out = []
    for x, y, w, h, rx, acc in WAVER:
        nx = tx + (x - WAVER_CX) * scale
        ny = ty + (y - WAVER_CY) * scale
        nw, nh = w * scale, h * scale
        nrx = rx * scale
        if fatten:
            nx -= nw * fatten / 2
            nw *= 1 + fatten
            nrx = min(nrx * (1 + fatten * 0.5), nw / 2, nh / 2)
        if fatten_h:
            ny -= nh * fatten_h / 2
            nh *= 1 + fatten_h
            nrx = min(nrx, nw / 2, nh / 2)
        out.append((nx, ny, nw, nh, nrx, acc))
    return out


def small_waver():
    """3-element Waver (stem + 2 bars) for 16/24px — wide gaps so 16px holds."""
    bars = [
        (34.0, 34.0, 16.0, 54.0, 8.0, False),
        (66.0, 46.0, 14.0, 30.0, 7.0, False),
        (96.0, 38.0, 14.0, 38.0, 7.0, True),
    ]
    cx, cy = _mass_center(bars)
    return [
        (HEX_BODY_CX + (x - cx), HEX_BODY_CY + (y - cy), w, h, rx, acc)
        for x, y, w, h, rx, acc in bars
    ]


def knockout_svg(bars, color: bool, ink: str, accent: str = ACCENT, title: str = "Rencord — Hexwave") -> str:
    holes = []
    accents = []
    for x, y, w, h, rx, acc in bars:
        if color and acc:
            accents.append((x, y, w, h, rx))
        else:
            holes.append(rrect(x, y, w, h, rx))
    d = HEX + " " + " ".join(holes)
    acc_p = "".join(
        f'\n  <path fill="{accent}" d="{rrect(x, y, w, h, rx)}"/>'
        for x, y, w, h, rx in accents
    )
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" fill="none">
  <title>{title}</title>
  <path fill-rule="evenodd" clip-rule="evenodd" fill="{ink}" d="{d}"/>{acc_p}
</svg>'''


def outline_svg(bars, color: bool, ink: str, accent: str = ACCENT,
                stroke: float = 9.0, hex_scale: float = 0.94,
                title: str = "Rencord — Hexwave Outline") -> str:
    # Scale hex so the stroke stays inside the 128 box and the outer
    # silhouette roughly matches the solid hex.
    sw = stroke / hex_scale
    items = [
        f'<g transform="translate(64 62) scale({hex_scale}) translate(-64 -62)">'
        f'<path fill="none" stroke="{ink}" stroke-width="{sw:.3f}" '
        f'stroke-linejoin="round" stroke-linecap="round" d="{HEX}"/>'
        f"</g>"
    ]
    for x, y, w, h, rx, acc in bars:
        fill = accent if (color and acc) else ink
        items.append(f'<path fill="{fill}" d="{rrect(x, y, w, h, rx)}"/>')
    inner = "\n  ".join(items)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" fill="none">
  <title>{title}</title>
  {inner}
</svg>'''


# ---------------------------------------------------------------------------
# Public mark functions
# ---------------------------------------------------------------------------
# A: canonical knockout, Waver at 0.56 — comfortable padding inside the hex.
BARS_A = place_waver(0.56)
# C: same placement, bars ~20% thicker so 32px gaps still read.
BARS_C = place_waver(0.54, fatten=0.22, fatten_h=0.06)
# B: outlined hex; slightly smaller Waver because the stroke eats interior.
BARS_B = place_waver(0.50)
BARS_S = small_waver()


def mark_a(ink: str = "currentColor", accent: str = ACCENT, color: bool = True) -> str:
    return knockout_svg(BARS_A, color=color, ink=ink, accent=accent,
                        title="Rencord — Hexwave")


def mark_b(ink: str = "currentColor", accent: str = ACCENT, color: bool = True) -> str:
    return outline_svg(BARS_B, color=color, ink=ink, accent=accent,
                       title="Rencord — Hexwave Outline")


def mark_c(ink: str = "currentColor", accent: str = ACCENT, color: bool = True) -> str:
    return knockout_svg(BARS_C, color=color, ink=ink, accent=accent,
                        title="Rencord — Hexwave Bold")


def mark_small(ink: str = "currentColor", accent: str = ACCENT, color: bool = True) -> str:
    return knockout_svg(BARS_S, color=color, ink=ink, accent=accent,
                        title="Rencord — Hexwave Small")


def mark_a_mono(ink: str = "currentColor", accent: str = ACCENT) -> str:
    return mark_a(ink=ink, accent=accent, color=False)


def mark_for_size(size: int, color: bool = True, ink: str = INK_ON_DARK, accent: str = ACCENT) -> str:
    """Pick the cut that holds at this raster size (canvas pixels of the mark)."""
    if size <= 24:
        return mark_small(ink=ink, accent=accent, color=color)
    if size <= 48:
        return mark_c(ink=ink, accent=accent, color=color)
    return mark_a(ink=ink, accent=accent, color=color)


# ---------------------------------------------------------------------------
# App icon plate (Onyx Ink squircle)
# ---------------------------------------------------------------------------
def squircle_icon(mark_svg: str, size: int, plate: str = BLACK,
                  inset_ratio: float | None = None,
                  hairline: bool | None = None) -> Image.Image:
    if inset_ratio is None:
        inset_ratio = 0.06 if size <= 24 else (0.14 if size <= 48 else 0.18)
    if hairline is None:
        hairline = plate == BLACK and size >= 32
    im = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    dr = ImageDraw.Draw(im)
    r = max(2, int(size * 0.225))
    outline = HAIRLINE if hairline else None
    width = max(1, size // 256) if outline else 0
    dr.rounded_rectangle(
        [0, 0, size - 1, size - 1],
        radius=r,
        fill=plate,
        outline=outline,
        width=width,
    )
    inset = int(round(size * inset_ratio))
    mark = svg_to_pil(mark_svg, max(1, size - 2 * inset))
    im.alpha_composite(mark, (inset, inset))
    return im


def icon_for_size(pixel_size: int, logical_size: int | None = None,
                  color: bool = True) -> Image.Image:
    """logical_size picks the cut (so 16@2x stays the 16px design, just sharper)."""
    logical = logical_size if logical_size is not None else pixel_size
    ink, accent = INK_ON_DARK, ACCENT
    if logical <= 24:
        svg = mark_small(ink=ink, accent=accent, color=color)
        inset, hl = 0.06, False
    elif logical <= 48:
        svg = mark_c(ink=ink, accent=accent, color=color)
        inset, hl = 0.12, pixel_size >= 32
    else:
        svg = mark_a(ink=ink, accent=accent, color=color)
        inset, hl = 0.18, True
    return squircle_icon(svg, pixel_size, inset_ratio=inset, hairline=hl)


# ---------------------------------------------------------------------------
# README header — fixed black tile, same in GitHub light and dark
# ---------------------------------------------------------------------------
def readme_header_svg(css_w: float = 720, css_h: float = 200) -> str:
    """Self-contained SVG: rounded black banner + Waver lockup + wordmark."""
    margin = 12.0
    tile_x, tile_y = margin, margin
    tile_w, tile_h = css_w - margin * 2, css_h - margin * 2
    rx = 28.0

    mark_box = 88.0
    gap = 22.0
    word_size = 48.0
    d, word_w, ymin, ymax = wordmark_path("rencord", size=word_size, weight=500)
    word_cy = (ymin + ymax) / 2  # ~ -cap/2
    lock_w = mark_box + gap + word_w
    lock_x = tile_x + (tile_w - lock_w) / 2
    lock_y = tile_y + (tile_h - mark_box) / 2
    baseline = lock_y + mark_box / 2 - word_cy

    waver = mark_c2_waver(ink=INK_ON_DARK, accent=ACCENT)
    waver_inner = _inner_svg(waver)
    word_x = lock_x + mark_box + gap

    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {css_w:.0f} {css_h:.0f}" fill="none">
  <title>Rencord</title>
  <rect x="{tile_x}" y="{tile_y}" width="{tile_w}" height="{tile_h}" rx="{rx}"
        fill="{BLACK}" stroke="{HAIRLINE}" stroke-width="1.5"/>
  <svg x="{lock_x:.2f}" y="{lock_y:.2f}" width="{mark_box}" height="{mark_box}" viewBox="0 0 128 128">
    {waver_inner}
  </svg>
  <path d="{d}" transform="translate({word_x:.2f} {baseline:.2f})" fill="{INK_ON_DARK}"/>
</svg>'''


# ---------------------------------------------------------------------------
# Symbolic / tray
# ---------------------------------------------------------------------------
def symbolic_svg(bars=BARS_A, view: int = 128) -> str:
    holes = [rrect(x, y, w, h, rx) for x, y, w, h, rx, acc in bars]
    d = HEX + " " + " ".join(holes)
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{view}" height="{view}" viewBox="0 0 128 128" fill="none">
  <title>Rencord symbolic icon</title>
  <style id="current-color-scheme" type="text/css">.ColorScheme-Text {{ color: #bebebe; }}</style>
  <path fill-rule="evenodd" clip-rule="evenodd" fill="currentColor" class="ColorScheme-Text" d="{d}"/>
</svg>'''


def tray_svg() -> str:
    """Mono silhouette, 3-bar small cut so it holds at tray sizes (16–22px)."""
    return symbolic_svg(BARS_S, view=16).replace(
        "Rencord symbolic icon", "Rencord tray icon"
    )


# ---------------------------------------------------------------------------
# Exports
# ---------------------------------------------------------------------------
def write_mark_svgs():
    specs = [
        ("hexwave-a", mark_a, "Knockout (canonical)"),
        ("hexwave-b", mark_b, "Outline"),
        ("hexwave-c", mark_c, "Bold knockout"),
        ("hexwave-small", mark_small, "Small-size 3-bar"),
    ]
    for cid, fn, _ in specs:
        (ROOT / f"{cid}-mark-color.svg").write_text(
            fn(ink=INK_ON_DARK, accent=ACCENT, color=True), encoding="utf-8"
        )
        (ROOT / f"{cid}-mark-mono.svg").write_text(
            fn(ink="currentColor", accent=ACCENT, color=False), encoding="utf-8"
        )
        # baked on-dark / on-light color SVGs for convenience
        color = fn(ink=INK_ON_DARK, accent=ACCENT, color=True)
        (ROOT / f"{cid}-mark-on-dark.svg").write_text(
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">'
            f'<rect width="128" height="128" fill="#000"/>'
            f'<svg viewBox="0 0 128 128">{_inner_svg(color)}</svg></svg>',
            encoding="utf-8",
        )
        color_l = fn(ink=INK_ON_LIGHT, accent=ACCENT, color=True)
        (ROOT / f"{cid}-mark-on-light.svg").write_text(
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">'
            f'<rect width="128" height="128" fill="#fff"/>'
            f'<svg viewBox="0 0 128 128">{_inner_svg(color_l)}</svg></svg>',
            encoding="utf-8",
        )
        print("marks", cid)

    # Recommended aliases (variant A)
    for suffix in ("mark-color.svg", "mark-mono.svg", "mark-on-dark.svg", "mark-on-light.svg"):
        src = (ROOT / f"hexwave-a-{suffix}").read_text(encoding="utf-8")
        (ROOT / f"hexwave-{suffix}").write_text(src, encoding="utf-8")
    # currentColor color-with-accent (tintable body, fixed blurple bar)
    (ROOT / "hexwave-mark.svg").write_text(
        mark_a(ink="currentColor", accent=ACCENT, color=True), encoding="utf-8"
    )
    (ROOT / "hexwave-symbolic.svg").write_text(symbolic_svg(BARS_A), encoding="utf-8")
    (ROOT / "hexwave-tray.svg").write_text(tray_svg(), encoding="utf-8")


def save_ico(path: Path, images: list[Image.Image]) -> None:
    """Write a multi-size ICO with one PNG payload per size."""
    import struct
    payloads = []
    for im in images:
        buf = BytesIO()
        im.save(buf, format="PNG")
        payloads.append((im.width, im.height, buf.getvalue()))
    count = len(payloads)
    offset = 6 + 16 * count
    out = BytesIO()
    out.write(struct.pack("<HHH", 0, 1, count))
    for w, h, data in payloads:
        wb = 0 if w >= 256 else w
        hb = 0 if h >= 256 else h
        out.write(struct.pack("<BBBBHHII", wb, hb, 0, 0, 1, 32, len(data), offset))
        offset += len(data)
    for _, _, data in payloads:
        out.write(data)
    path.write_bytes(out.getvalue())


def write_icons():
    # 1024 squircle app icon — canonical color A
    icon_1024 = squircle_icon(mark_a(ink=INK_ON_DARK, accent=ACCENT, color=True), 1024)
    icon_1024.save(ROOT / "hexwave-icon-1024.png", "PNG")

    # ICO: size-appropriate cuts, PNG-compressed entries (Vista+).
    ico_sizes = [16, 24, 32, 48, 64, 256]
    ico_imgs = [icon_for_size(s, logical_size=s) for s in ico_sizes]
    save_ico(ROOT / "hexwave.ico", ico_imgs)

    # icns-ready iconset. @2x tiles use the *logical* cut of the 1x name.
    iconset = ROOT / "icon.iconset"
    iconset.mkdir(parents=True, exist_ok=True)
    # (filename, pixel size, logical size)
    icns_map = [
        ("icon_16x16.png", 16, 16),
        ("icon_16x16@2x.png", 32, 16),
        ("icon_32x32.png", 32, 32),
        ("icon_32x32@2x.png", 64, 32),
        ("icon_128x128.png", 128, 128),
        ("icon_128x128@2x.png", 256, 128),
        ("icon_256x256.png", 256, 256),
        ("icon_256x256@2x.png", 512, 256),
        ("icon_512x512.png", 512, 512),
        ("icon_512x512@2x.png", 1024, 512),
    ]
    for name, pix, logical in icns_map:
        icon_for_size(pix, logical_size=logical).save(iconset / name, "PNG")

    # Linux hicolor-style PNGs
    linux = ROOT / "linux"
    linux.mkdir(parents=True, exist_ok=True)
    for s in (512, 256, 128, 64, 48):
        icon_for_size(s).save(linux / f"hexwave-{s}.png", "PNG")

    # Also a 512 transparent mark (color A) for general use
    svg_to_pil(mark_a(ink=INK_ON_DARK, accent=ACCENT, color=True), 512).save(
        ROOT / "hexwave-mark-512.png"
    )
    print("icons written")


def write_header():
    svg = readme_header_svg(720, 200)
    (ROOT / "readme-header.svg").write_text(svg, encoding="utf-8")
    # 2x PNG
    raw = cairosvg.svg2png(bytestring=svg.encode("utf-8"), output_width=1440, output_height=400)
    Image.open(BytesIO(raw)).convert("RGBA").save(ROOT / "readme-header.png", "PNG")
    print("header written")


# ---------------------------------------------------------------------------
# Review sheet
# ---------------------------------------------------------------------------
def _paste_card(im, x, y, w, h, bg, child, radius=12, outline=None):
    card = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    cd = ImageDraw.Draw(card)
    cd.rounded_rectangle(
        [0, 0, w - 1, h - 1],
        radius=radius,
        fill=bg,
        outline=outline or (HAIRLINE if bg == BLACK else "#E4E4EA"),
        width=1,
    )
    if child:
        cx = (w - child.width) // 2
        cy = (h - child.height) // 2
        card.alpha_composite(child, (cx, cy))
    im.paste(card, (x, y), card)


def _size_slot(fn, size: int, slot_w: int, slot_h: int = 150) -> Image.Image:
    """Side-by-side dark/light tests of a mark at `size` px. Never clip the hex."""
    pad_box = 12 if size >= 48 else (10 if size >= 32 else 8)
    box = size + pad_box * 2
    gap = 8
    need_w = box * 2 + gap + 4
    if slot_w < need_w:
        # should not happen if the row reserved enough width
        slot_w = need_w
    slot = Image.new("RGBA", (slot_w, slot_h), (0, 0, 0, 0))
    sd = ImageDraw.Draw(slot)
    total = box * 2 + gap
    x0 = (slot_w - total) // 2
    y0 = (slot_h - box) // 2
    sd.rounded_rectangle([x0, y0, x0 + box - 1, y0 + box - 1], radius=8, fill=BLACK, outline=HAIRLINE)
    sd.rounded_rectangle([x0 + box + gap, y0, x0 + box + gap + box - 1, y0 + box - 1], radius=8, fill=WHITE, outline="#D0D0D8")
    md = svg_to_pil(fn(ink=INK_ON_DARK, accent=ACCENT, color=True), size)
    ml = svg_to_pil(fn(ink=INK_ON_LIGHT, accent=ACCENT, color=True), size)
    slot.alpha_composite(md, (x0 + pad_box, y0 + pad_box))
    slot.alpha_composite(ml, (x0 + box + gap + pad_box, y0 + pad_box))
    return slot


def compose_sheet() -> Image.Image:
    W = 1600
    pad = 56
    variants = [
        ("A", "Knockout", "Waver punched out of the solid hex. Single tintable silhouette; blurple bar in color.", mark_a),
        ("B", "Outline", "Solid Waver sitting inside a stroked hex + tail.", mark_b),
        ("C", "Bold knockout", "Same idea as A, with thicker bars so 32px still reads.", mark_c),
    ]

    header_h = 130
    row_h = 268
    small_h = 220
    dock_h = 280
    readme_h = 520
    H = pad + header_h + row_h * 3 + small_h + dock_h + readme_h + pad
    im = Image.new("RGB", (W, H), "#07070A")
    dr = ImageDraw.Draw(im)

    title_f = geist_font(36, 600)
    sub_f = geist_font(15, 400)
    name_f = geist_font(22, 600)
    num_f = geist_font(13, 500)
    tiny_f = geist_font(11, 400)
    label_f = geist_font(13, 500)

    dr.text((pad, pad + 4), "Rencord  ·  Hexwave  +  Waver header", font=title_f, fill=INK_ON_DARK)
    dr.text(
        (pad, pad + 52),
        "App mark: Hexnode body, Waver knocked into the hex  ·  README: Waver lockup on a fixed black tile  ·  Onyx Ink  #000  /  #F2F3FA  /  #4F6BFF",
        font=sub_f,
        fill="#9AA0C0",
    )
    dr.text(
        (pad, pad + 76),
        "Recommended app mark is A (knockout). 16 / 24px icons use the 3-bar small cut. README header is the same image in GitHub light and dark.",
        font=sub_f,
        fill="#7A80A0",
    )

    y = pad + header_h
    name_w = 190
    content_x = pad + name_w
    remain = W - pad - content_x
    # 64/32/16 slots are side-by-side dark+light, so they need 2*size + pads + gap
    bw, iw, s64, s32, s16 = 140, 100, 176, 108, 76

    def variant_row(ry, letter, name, blurb, fn, rec=False):
        dr.line([(pad, ry), (W - pad, ry)], fill="#15152A", width=1)
        tag = letter + ("  ·  rec" if rec else "")
        dr.text((pad, ry + 28), tag, font=num_f, fill=ACCENT)
        dr.text((pad, ry + 48), name, font=name_f, fill=INK_ON_DARK)
        words, lines, cur = blurb.split(), [], ""
        for w_ in words:
            trial = (cur + " " + w_).strip()
            if tiny_f.getlength(trial) < name_w - 10:
                cur = trial
            else:
                lines.append(cur)
                cur = w_
        if cur:
            lines.append(cur)
        ty = ry + 82
        for ln in lines[:5]:
            dr.text((pad, ty), ln, font=tiny_f, fill="#9AA0C0")
            ty += 15

        mark_d = svg_to_pil(fn(ink=INK_ON_DARK, accent=ACCENT, color=True), 96)
        mark_l = svg_to_pil(fn(ink=INK_ON_LIGHT, accent=ACCENT, color=True), 96)
        x = content_x
        _paste_card(im, x, ry + 36, bw, 150, BLACK, mark_d)
        dr.text((x, ry + 190), "on black", font=tiny_f, fill="#7A80A0")
        x += bw + 8
        _paste_card(im, x, ry + 36, bw, 150, WHITE, mark_l, outline="#D0D0D8")
        dr.text((x, ry + 190), "on white", font=tiny_f, fill="#7A80A0")
        x += bw + 8
        icon = squircle_icon(fn(ink=INK_ON_DARK, accent=ACCENT, color=True), 96)
        _paste_card(im, x, ry + 36, iw, 150, "#07070A", icon, outline="#15152A")
        dr.text((x, ry + 190), "app icon", font=tiny_f, fill="#7A80A0")
        x += iw + 8
        for size, sw, label in ((64, s64, "64 px"), (32, s32, "32 px"), (16, s16, "16 px")):
            # At 16px, also show the small cut in a second pair? For A and C the 16 will clog;
            # we show the variant as-is here, and the small cut in the next section.
            slot = _size_slot(fn, size, sw, 150)
            im.paste(slot, (x, ry + 36), slot)
            dr.text((x, ry + 190), label, font=tiny_f, fill="#7A80A0")
            x += sw + 8

    for i, (letter, name, blurb, fn) in enumerate(variants):
        variant_row(y + i * row_h, letter, name, blurb, fn, rec=(letter == "A"))

    # ----- small-size comparison -----
    sy = y + 3 * row_h
    dr.line([(pad, sy), (W - pad, sy)], fill="#15152A", width=1)
    dr.text((pad, sy + 16), "Small-size cut", font=name_f, fill=INK_ON_DARK)
    dr.text(
        (pad, sy + 46),
        "Four bars clog at 16px on A and C. The 3-bar small cut (stem + two chunky bars, wide gaps; the last is blurple) is used for 16 and 24px icons, tray, and the 16@2x iconset tile. 32–48px use C (bold). 64px and up use A.",
        font=tiny_f,
        fill="#7A80A0",
    )

    # four comparisons: A16, C16, Small16, Small24 — each dark+light
    comps = [
        ("A at 16", lambda **k: mark_a(**k, color=True), 16),
        ("C at 16", lambda **k: mark_c(**k, color=True), 16),
        ("Small at 16", lambda **k: mark_small(**k, color=True), 16),
        ("Small at 24", lambda **k: mark_small(**k, color=True), 24),
        ("C at 32", lambda **k: mark_c(**k, color=True), 32),
        ("A at 32", lambda **k: mark_a(**k, color=True), 32),
    ]
    cx0 = pad
    box_w = (W - pad * 2 - 16 * 5) // 6
    for i, (lab, fn, sz) in enumerate(comps):
        cx = cx0 + i * (box_w + 16)
        # 8x nearest closeup of the dark render, plus a 1:1 pip
        raw = svg_to_pil(fn(ink=INK_ON_DARK, accent=ACCENT), sz)
        close = raw.resize((raw.width * (8 if sz <= 16 else 6), raw.height * (8 if sz <= 16 else 6)), Image.Resampling.NEAREST)
        # fit in card
        card_h = 120
        max_dim = 96
        if close.width > max_dim or close.height > max_dim:
            close = close  # already ~128 for 16*8
        chip = Image.new("RGBA", (box_w, card_h), (0, 0, 0, 0))
        cd = ImageDraw.Draw(chip)
        cd.rounded_rectangle([0, 0, box_w - 1, card_h - 1], radius=12, fill=BLACK, outline=HAIRLINE)
        # left: closeup, right: actual size
        if close.width > box_w - 40:
            close = close.resize((box_w - 48, box_w - 48), Image.Resampling.NEAREST)
        chip.alpha_composite(close, (12, (card_h - close.height) // 2))
        # actual-size pip on the right
        pip_x = box_w - 16 - sz
        pip_y = (card_h - sz) // 2
        cd.rounded_rectangle([pip_x - 6, pip_y - 6, pip_x + sz + 5, pip_y + sz + 5], radius=4, fill="#111118", outline="#2A2A38")
        chip.alpha_composite(raw, (pip_x, pip_y))
        im.paste(chip, (cx, sy + 72), chip)
        dr.text((cx, sy + 72 + card_h + 6), lab, font=tiny_f, fill="#7A80A0")

    # ----- dock / taskbar -----
    dy = sy + small_h
    dr.line([(pad, dy), (W - pad, dy)], fill="#15152A", width=1)
    dr.text((pad, dy + 16), "Fake dock  /  taskbar", font=name_f, fill=INK_ON_DARK)
    dr.text(
        (pad, dy + 46),
        "Hexwave A as a 56px dock tile, and the 24px small cut in a slim taskbar. Placeholders are generic and not other apps’ marks.",
        font=tiny_f,
        fill="#7A80A0",
    )

    # mac-like dock
    dock_w, dock_bar_h = 560, 80
    dock = Image.new("RGBA", (dock_w, dock_bar_h), (0, 0, 0, 0))
    dd = ImageDraw.Draw(dock)
    dd.rounded_rectangle([0, 8, dock_w - 1, dock_bar_h - 1], radius=20, fill="#16161C", outline="#2A2A36")
    # 7 slots
    n = 7
    tile = 52
    gap = 12
    inner_w = n * tile + (n - 1) * gap
    ox = (dock_w - inner_w) // 2
    oy = 8 + (dock_bar_h - 8 - tile) // 2
    # placeholder colors
    placeholders = ["#2A2A36", "#2A2A36", "#2A2A36", None, "#2A2A36", "#2A2A36", "#2A2A36"]
    hex_icon = squircle_icon(mark_a(ink=INK_ON_DARK, accent=ACCENT, color=True), tile, inset_ratio=0.14)
    for i, ph in enumerate(placeholders):
        x = ox + i * (tile + gap)
        if ph is None:
            dock.alpha_composite(hex_icon, (x, oy))
            # dot
            dd.ellipse([x + tile // 2 - 2, dock_bar_h - 8, x + tile // 2 + 2, dock_bar_h - 4], fill="#8A8AA0")
        else:
            dd.rounded_rectangle([x, oy, x + tile - 1, oy + tile - 1], radius=int(tile * 0.225), fill=ph)
    im.paste(dock, (pad, dy + 80), dock)
    dr.text((pad, dy + 80 + dock_bar_h + 6), "dock  ·  56 px tile", font=tiny_f, fill="#7A80A0")

    # windows-like taskbar
    tb_w, tb_h = 640, 44
    tb = Image.new("RGBA", (tb_w, tb_h), (0, 0, 0, 0))
    td = ImageDraw.Draw(tb)
    td.rounded_rectangle([0, 0, tb_w - 1, tb_h - 1], radius=8, fill="#0C0C10", outline="#1E1E28")
    # start-ish pill + a few 24px icons
    td.rounded_rectangle([10, 8, 36, 36], radius=6, fill="#1A1A24")
    icon24 = icon_for_size(24)
    tb.alpha_composite(icon24, (48, 10))
    # more placeholders
    for i, xx in enumerate((80, 110, 140)):
        td.rounded_rectangle([xx, 12, xx + 20, 32], radius=4, fill="#1A1A24")
    im.paste(tb, (pad + 600, dy + 98), tb)
    dr.text((pad + 600, dy + 80 + dock_bar_h + 6), "taskbar  ·  24 px small cut", font=tiny_f, fill="#7A80A0")

    # ----- README header on GitHub light + dark -----
    ry = dy + dock_h
    dr.line([(pad, ry), (W - pad, ry)], fill="#15152A", width=1)
    dr.text((pad, ry + 16), "README header  ·  fixed black tile", font=name_f, fill=INK_ON_DARK)
    dr.text(
        (pad, ry + 46),
        "Waver mark + “rencord” wordmark on a rounded black banner, so it stays identical in GitHub light and dark. No <picture> switching. Files: readme-header.svg and readme-header.png (2×).",
        font=tiny_f,
        fill="#7A80A0",
    )

    header_svg = readme_header_svg(720, 200)
    # render at ~1x of the sheet column
    col_w = (W - pad * 2 - 24) // 2
    # header natural 720x200, scale to col_w - 40
    hw = col_w - 48
    hh = int(200 / 720 * hw)
    raw = cairosvg.svg2png(bytestring=header_svg.encode("utf-8"), output_width=hw, output_height=hh)
    header_im = Image.open(BytesIO(raw)).convert("RGBA")

    def gh_card(bg, chrome, title_fill, body_fill, label, x):
        card_h = 400
        card = Image.new("RGBA", (col_w, card_h), (0, 0, 0, 0))
        cd = ImageDraw.Draw(card)
        cd.rounded_rectangle([0, 0, col_w - 1, card_h - 1], radius=12, fill=bg, outline=chrome)
        # fake chrome bar
        cd.rounded_rectangle([0, 0, col_w - 1, 28], radius=12, fill=chrome)
        cd.rounded_rectangle([0, 16, col_w - 1, 28], fill=chrome)
        cf = geist_font(10, 500)
        cd.text((14, 8), label, font=cf, fill=body_fill)
        # header image
        card.alpha_composite(header_im, (24, 44))
        # fake readme text under it
        tf = geist_font(13, 400)
        nf = geist_font(22, 600)
        cd.text((24, 44 + hh + 20), "Rencord", font=nf, fill=title_fill)
        cd.text((24, 44 + hh + 52), "A native Discord client, written in Rust.", font=tf, fill=body_fill)
        # fake badges
        cd.rounded_rectangle([24, 44 + hh + 80, 70, 44 + hh + 100], radius=3, fill=ACCENT)
        cd.rounded_rectangle([78, 44 + hh + 80, 124, 44 + hh + 100], radius=3, fill="#15152A")
        bf = geist_font(9, 500)
        cd.text((34, 44 + hh + 83), "rust", font=bf, fill=WHITE)
        cd.text((88, 44 + hh + 83), "gpui", font=bf, fill="#C5C8DE")
        im.paste(card, (x, ry + 76), card)

    gh_card("#F6F8FA", "#D0D7DE", "#1F2328", "#656D76", "GitHub light", pad)
    gh_card("#0D1117", "#21262D", "#E6EDF3", "#8D96A0", "GitHub dark", pad + col_w + 24)

    out = ROOT / "hexwave-sheet.png"
    im.save(out, "PNG")
    print("sheet", out, im.size)
    return im


def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    write_mark_svgs()
    write_header()
    write_icons()
    compose_sheet()


if __name__ == "__main__":
    main()
