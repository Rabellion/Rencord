//! Build a full palette from a base surface and an accent.
//!
//! Lightness moves in OKLCH. Text, links, and the accent ink are pushed until
//! they clear WCAG AA on the surfaces they sit on.

use crate::color::{best_ink, ensure_contrast, Color};
use crate::preset::{classic_metrics, ink_metrics};
use crate::theme::{Background, Chrome, ColorMode, Theme};

pub fn derive_theme(
    name: &str,
    base: Color,
    accent: Color,
    mode: Option<ColorMode>,
    chrome: Chrome,
) -> Theme {
    let mode = mode.unwrap_or(if base.is_light() {
        ColorMode::Light
    } else {
        ColorMode::Dark
    });
    let light = mode == ColorMode::Light;
    let chat = if light {
        base.shift_lightness(0.03)
    } else {
        base
    };
    let sidebar = if light {
        base.shift_lightness(0.015)
    } else {
        base.shift_lightness(0.025)
    };
    let rail = if light {
        base.shift_lightness(-0.01)
    } else {
        base.shift_lightness(-0.02)
    };
    let input = if light {
        Color::rgb(0xFF_FF_FF).mix(base, 0.15)
    } else {
        base.shift_lightness(0.06)
    };
    let text = if light {
        Color::rgb(0x1A_1D_26)
    } else {
        Color::rgb(0xF2_F3_F5)
    };
    let text = ensure_contrast(text, chat, 7.0);
    let text = ensure_contrast(text, input, 4.5);
    let muted = ensure_contrast(text.mix(chat, 0.38), chat, 4.5);
    let muted = ensure_contrast(muted, input, 4.5);
    let faint = ensure_contrast(text.mix(chat, 0.5), input, 4.5);
    let link = ensure_contrast(
        accent.shift_lightness(if light { -0.08 } else { 0.12 }),
        chat,
        4.5,
    );
    let mut accent_fill = accent;
    let mut accent_fg = best_ink(accent_fill);
    if crate::color::contrast_ratio(accent_fg, accent_fill) < 4.5 {
        accent_fill = if accent_fg.is_light() {
            accent.shift_lightness(-0.12)
        } else {
            accent.shift_lightness(0.12)
        };
        accent_fg = best_ink(accent_fill);
        accent_fg = ensure_contrast(accent_fg, accent_fill, 4.5);
    }
    let mut colors = crate::preset::colors_from_public(
        chat,
        sidebar,
        rail,
        input,
        text,
        muted,
        faint,
        accent_fill,
        accent_fg,
        link,
        light,
    );
    colors.text_primary = text;
    colors.text_muted = muted;
    colors.text_faint = faint;
    colors.text_link = link;
    colors.text_secondary = ensure_contrast(muted.mix(text, 0.35), chat, 4.5);
    colors.accent = accent_fill;
    colors.accent_fg = accent_fg;
    colors.accent_on = crate::preset::on_accent(accent_fill, accent_fg);
    colors.border_accent = accent_fill;
    colors.text_mention = ensure_contrast(colors.text_mention, colors.bg_mention, 4.5);
    colors.syntax_keyword = link;
    if chrome == Chrome::Ink {
        colors.bg_home = colors.bg_rail;
        colors.bg_home_active = colors.bg_rail;
        colors.text_home = colors.accent;
        colors.text_home_active = colors.accent;
        colors.border_home = colors.border_hairline;
        colors.border_home_active = colors.border_hairline;
        colors.bg_add = colors.bg_rail;
        colors.border_add = colors.border_strong;
        colors.text_add = colors.text_muted;
        colors.bg_badge = colors.accent;
        colors.text_badge = colors.accent_fg;
        colors.bg_voice = colors.bg_chat;
        colors.border_voice = colors.border_hairline;
        colors.border_hover = colors.border_strong;
        colors.text_voice = colors.text_secondary;
    }
    let metrics = if chrome == Chrome::Ink {
        ink_metrics(light)
    } else {
        classic_metrics(light)
    };
    let slug = slugify(name);
    Theme {
        id: slug,
        name: if name.trim().is_empty() {
            "Custom".into()
        } else {
            name.trim().to_owned()
        },
        mode,
        chrome,
        preset: None,
        colors,
        metrics,
        background: Background::default(),
        base: Some(base),
        accent_seed: Some(accent),
        derived: true,
    }
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if (ch == ' ' || ch == '-' || ch == '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_owned();
    if out.is_empty() {
        "custom".into()
    } else {
        out
    }
}
