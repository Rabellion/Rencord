//! Go Live picker, viewer, and picture-in-picture.
//!
//! The pictures are preview frames from the media crate. Starting a real
//! portal or hardware encoder is still a separate step.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::*;
use rencord_desktop::Palette;
use rencord_media::{list_local_sources, safe_preset, CaptureSource, GoLive};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoLiveMode {
    Off,
    Picker,
    Viewer,
    Pip,
}

impl GoLiveMode {
    pub fn parse(value: &str) -> Self {
        match value {
            "picker" => Self::Picker,
            "viewer" => Self::Viewer,
            "pip" => Self::Pip,
            _ => Self::Off,
        }
    }
}

pub fn picker(palette: Palette) -> impl IntoElement {
    let sources = list_local_sources();
    let preset = safe_preset();
    let mut cards = h_flex().gap(px(12.));
    for source in sources {
        cards = cards.child(source_card(source, palette));
    }
    v_flex()
        .size_full()
        .bg(rgb(palette.app_bg))
        .p(px(16.))
        .gap(px(12.))
        .child(title("Go Live", palette))
        .child(note(
            format!(
                "Default {}p{} at {} kbps. Higher presets are a request. Discord can reject them, and exceeding the account cap can put the account at risk.",
                preset.height, preset.fps, preset.bitrate_kbps
            ),
            palette,
        ))
        .child(cards)
}

pub fn viewer(palette: Palette) -> impl IntoElement {
    let live = GoLive::new();
    let stats = live.stats();
    v_flex()
        .size_full()
        .bg(rgb(palette.app_bg))
        .p(px(16.))
        .gap(px(12.))
        .child(title("Stream", palette))
        .child(
            div()
                .flex_1()
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(palette.line))
                .bg(rgb(palette.stage))
                .relative()
                .child(preview_image(0., 0.))
                .child(stats_overlay(&stats, palette)),
        )
}

pub fn pip(palette: Palette) -> impl IntoElement {
    let live = GoLive::new();
    let stats = live.stats();
    div()
        .size_full()
        .bg(rgb(palette.app_bg))
        .relative()
        .child(div().absolute().top(px(24.)).left(px(24.)).child(note(
            "The call stays up. The stream sits in its own picture.".to_owned(),
            palette,
        )))
        .child(
            v_flex()
                .absolute()
                .right(px(24.))
                .bottom(px(24.))
                .w(px(360.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(palette.line))
                .bg(rgb(palette.stage))
                .overflow_hidden()
                .child(preview_image(360., 202.))
                .child(
                    h_flex()
                        .px(px(10.))
                        .h(px(36.))
                        .items_center()
                        .justify_between()
                        .child(meta_line(
                            format!(
                                "{}x{}  {} fps  {} kbps  {}",
                                stats.width,
                                stats.height,
                                stats.fps,
                                stats.bitrate_kbps,
                                stats.encoder
                            ),
                            palette,
                        ))
                        .child(meta_line("Pop out".into(), palette)),
                ),
        )
}

fn source_card(source: CaptureSource, palette: Palette) -> impl IntoElement {
    v_flex()
        .w(px(280.))
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(if source.id.ends_with("desktop") {
            palette.accent
        } else {
            palette.line
        }))
        .bg(rgb(palette.app_bg))
        .overflow_hidden()
        .child(thumbnail(&source.thumbnail_png, palette.stage))
        .child(
            v_flex()
                .p(px(10.))
                .gap(px(4.))
                .child(paint(source.title.clone(), palette.text, 14.))
                .child(paint(
                    format!("{}x{}  {:?}", source.width, source.height, source.kind),
                    palette.muted,
                    12.,
                )),
        )
}

fn thumbnail(png: &[u8], stage: u32) -> impl IntoElement {
    let image = Image::from_bytes(ImageFormat::Png, png.to_vec());
    div()
        .h(px(158.))
        .w_full()
        .bg(rgb(stage))
        .child(img(std::sync::Arc::new(image)).size_full())
}

fn preview_image(width: f32, height: f32) -> impl IntoElement {
    let source = list_local_sources()
        .into_iter()
        .find(|source| source.id == "synthetic:desktop")
        .expect("preview source");
    let image = Image::from_bytes(ImageFormat::Png, source.thumbnail_png);
    let picture = img(std::sync::Arc::new(image));
    if width <= 0.0 {
        div()
            .size_full()
            .min_h(px(180.))
            .child(picture.size_full().object_fit(ObjectFit::Contain))
    } else {
        div()
            .size_full()
            .min_h(px(height))
            .child(picture.w(px(width)).h(px(height)))
    }
}

fn stats_overlay(stats: &rencord_media::StreamStats, palette: Palette) -> impl IntoElement {
    v_flex()
        .absolute()
        .left(px(12.))
        .bottom(px(12.))
        .px(px(10.))
        .py(px(8.))
        .rounded(px(6.))
        .bg(rgb(palette.elevated))
        .border_1()
        .border_color(rgb(palette.line))
        .gap(px(2.))
        .child(meta_line(
            format!(
                "{}x{}   {} fps   {} kbps   {}",
                stats.width, stats.height, stats.fps, stats.bitrate_kbps, stats.encoder
            ),
            palette,
        ))
        .child(meta_line(
            format!(
                "dropped {}   rtt {} ms   volume {}%",
                stats.dropped_frames, stats.rtt_ms, stats.volume_percent
            ),
            palette,
        ))
}

fn title(text: &str, palette: Palette) -> impl IntoElement {
    div()
        .text_size(px(18.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(palette.text))
        .child(text.to_owned())
}

fn note(text: String, palette: Palette) -> impl IntoElement {
    div()
        .text_size(px(13.))
        .text_color(rgb(palette.muted))
        .child(text)
}

fn paint(text: String, color: u32, size: f32) -> impl IntoElement {
    div().text_size(px(size)).text_color(rgb(color)).child(text)
}

fn meta_line(text: String, palette: Palette) -> impl IntoElement {
    div()
        .font_family(palette.font_mono)
        .text_size(px(11.))
        .text_color(rgb(palette.muted))
        .child(text)
}
