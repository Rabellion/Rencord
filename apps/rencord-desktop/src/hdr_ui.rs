//! HDR rows in Settings. Onyx Ink uses the theme field, hairline, and accent,
//! and JetBrains Mono at 11 px.

use std::sync::OnceLock;

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::*;
use rencord_desktop::Palette;
use rencord_hdr::HdrCaptureProbe;

pub struct HdrSettingsModel {
    pub send: bool,
    pub receive: bool,
    pub curve: String,
    pub summary: String,
}

pub fn probe_status() -> String {
    static CACHED: OnceLock<String> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let mut probe = rencord_hdr::HostProbe::system();
            match probe.probe() {
                Ok(report) if report.selected_colorimetry().is_some() => report.summary(),
                Ok(report) => report
                    .notes
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "No display colorimetry.".into()),
                Err(err) => err.to_string(),
            }
        })
        .clone()
}

pub fn hdr_settings_block(
    palette: Palette,
    model: HdrSettingsModel,
    on_send: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_receive: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_curve: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let probe = probe_status();
    v_flex()
        .gap(px(8.))
        .p(px(8.))
        .rounded(px(8.))
        .bg(rgb(palette.app_bg))
        .border_1()
        .border_color(rgb(palette.line))
        .child(
            div()
                .font_family("JetBrains Mono")
                .text_size(px(11.))
                .text_color(rgb(palette.muted))
                .child("HDR"),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .child(chip(
                    "hdr-send",
                    if model.send { "Send on" } else { "Send off" },
                    model.send,
                    palette,
                    on_send,
                ))
                .child(chip(
                    "hdr-receive",
                    if model.receive {
                        "Receive on"
                    } else {
                        "Receive off"
                    },
                    model.receive,
                    palette,
                    on_receive,
                ))
                .child(chip(
                    "hdr-curve",
                    &format!("Curve {}", model.curve),
                    false,
                    palette,
                    on_curve,
                )),
        )
        .child(mono_line(&model.summary, palette.muted))
        .child(mono_line(&probe, palette.faint))
}

fn chip(
    id: &'static str,
    label: &str,
    on: bool,
    palette: Palette,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let bg = if on { palette.accent } else { palette.app_bg };
    let fg = if on {
        palette.accent_text
    } else {
        palette.text
    };
    div()
        .id(id)
        .px(px(8.))
        .py(px(4.))
        .rounded(px(6.))
        .bg(rgb(bg))
        .border_1()
        .border_color(rgb(if on { palette.accent } else { palette.line }))
        .cursor_pointer()
        .font_family("JetBrains Mono")
        .text_size(px(11.))
        .text_color(rgb(fg))
        .child(label.to_string())
        .on_click(click)
}

fn mono_line(text: &str, color: u32) -> impl IntoElement {
    div()
        .font_family("JetBrains Mono")
        .text_size(px(11.))
        .text_color(rgb(color))
        .child(text.to_string())
}
