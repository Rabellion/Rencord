//! Big-file sheet drawn over the composer. The transfer runs on a worker thread.

use super::{icon, meta, paint, RencordWindow};
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;
use rencord_desktop::Palette;
use rencord_upload::{Flow, FlowChoice, ProviderKind, SheetScreen};
use std::path::Path;
use std::sync::atomic::Ordering;

const BUTTON_H: f32 = 32.0;

pub fn sheet(
    view: Entity<RencordWindow>,
    path_input: &Entity<InputState>,
    flow: &Flow,
    palette: Palette,
) -> impl IntoElement {
    let mut card = v_flex()
        .w(px(520.))
        .p(px(20.))
        .gap(px(12.))
        .rounded(px(8.))
        .bg(rgb(palette.chat))
        .border_1()
        .border_color(rgb(palette.line));
    card = match flow.screen {
        SheetScreen::Progress => card.child(progress_block(view.clone(), flow, palette)),
        SheetScreen::Done => card.child(done_block(view.clone(), flow, palette)),
        SheetScreen::Reassemble => {
            card.child(reassemble_block(view.clone(), path_input, flow, palette))
        }
        SheetScreen::Pick | SheetScreen::Choose => {
            card.child(choose_block(view.clone(), path_input, flow, palette))
        }
    };
    // Flex centering does not move an absolute child in this tree, so the
    // card is placed from the measured window (1180×760) and card size.
    let (top, left) = match flow.screen {
        SheetScreen::Progress if flow.error.is_some() => (278.0, 329.0),
        SheetScreen::Progress => (293.0, 329.0),
        SheetScreen::Done | SheetScreen::Reassemble => (250.0, 329.0),
        SheetScreen::Pick | SheetScreen::Choose => (126.0, 329.0),
    };
    div()
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .w(px(1180.))
        .h(px(760.))
        .bg(rgba(palette.overlay))
        .child(div().absolute().top(px(top)).left(px(left)).child(card))
}

pub fn reassemble_chip(
    view: Entity<RencordWindow>,
    note: String,
    palette: Palette,
) -> impl IntoElement {
    div().id("reassemble-chip").mt(px(6.)).child(ghost_button(
        view,
        "reassemble-open",
        "Reassemble",
        palette,
        move |window, cx| {
            window.shell.flow.open_reassemble(&note);
            cx.notify();
        },
    ))
}

fn choose_block(
    view: Entity<RencordWindow>,
    path_input: &Entity<InputState>,
    flow: &Flow,
    palette: Palette,
) -> impl IntoElement {
    let over = flow.over_limit();
    let headline = if over {
        "This file is over the upload limit"
    } else if flow.staged.is_some() {
        "This file fits"
    } else {
        "Attach a file"
    };
    let mut column = v_flex()
        .w_full()
        .gap(px(12.))
        .child(paint(headline, palette.text, 15.));
    if let Some(offer) = &flow.offer {
        column = column
            .child(meta(offer.meta.clone(), palette.muted, 11., palette))
            .child(paint(offer.server_note.clone(), palette.muted, 13.));
        column = column.child(choice_row(
            view.clone(),
            flow,
            FlowChoice::Compress,
            &offer.compress.title,
            offer.compress.available,
            palette,
        ));
        column = column.child(choice_row(
            view.clone(),
            flow,
            FlowChoice::External,
            &offer.external.title,
            true,
            palette,
        ));
        column = column.child(choice_row(
            view.clone(),
            flow,
            FlowChoice::Split,
            &offer.split.title,
            true,
            palette,
        ));
    } else if let Some(file) = &flow.staged {
        column = column.child(meta(
            format!(
                "{}  ·  {}  ·  limit {}  ·  {}",
                file.name,
                rencord_upload::format_bytes(file.size),
                rencord_upload::format_bytes(flow.limit.bytes),
                flow.limit.label()
            ),
            palette.muted,
            11.,
            palette,
        ));
    } else {
        column = column.child(paint(
            "Drop a file on the composer, or type a path and choose it. Discord still enforces the cap.",
            palette.muted,
            13.,
        ));
        column = column.child(Input::new(path_input));
    }
    if flow.chosen == Some(FlowChoice::External) && flow.settings.provider == ProviderKind::Custom {
        column = column.child(
            v_flex()
                .gap(px(4.))
                .child(meta("Custom endpoint", palette.muted, 11., palette))
                .child(Input::new(path_input)),
        );
    }
    if let Some(name) = output_name(flow) {
        column = column.child(meta(format!("Output  {name}"), palette.muted, 11., palette));
    }
    column = column.child(defaults_row(view.clone(), flow, palette));
    if let Some(error) = &flow.error {
        column = column.child(paint(error.clone(), palette.danger, 13.));
    }
    let go_label: &'static str = if flow.staged.is_none() {
        "Choose file…"
    } else if !over {
        "Attach"
    } else {
        match flow.chosen {
            Some(FlowChoice::Compress) => "Compress",
            Some(FlowChoice::External) => "Upload",
            Some(FlowChoice::Split) => "Split",
            None => "Continue",
        }
    };
    column.child(action_row(view, palette, go_label, true, |window, cx| {
        if window.shell.flow.staged.is_none() {
            let text = window.attachment_path.read(cx).value().to_string();
            apply_path(window, text.trim());
        } else if window.shell.flow.settings.provider == ProviderKind::Custom
            && window.shell.flow.chosen == Some(FlowChoice::External)
        {
            let text = window.attachment_path.read(cx).value().to_string();
            let trimmed = text.trim().to_owned();
            if trimmed.starts_with("put ") || trimmed.starts_with("s3 ") {
                window.shell.flow.set_custom_spec(&trimmed);
            }
            window.start_big_file(cx);
        } else {
            window.start_big_file(cx);
        }
    }))
}

fn output_name(flow: &Flow) -> Option<String> {
    let file = flow.staged.as_ref()?;
    match flow.chosen? {
        FlowChoice::Compress => Some(rencord_upload::compressed_file_name(&file.name, file.kind)),
        FlowChoice::Split => Some(format!(
            "{}.{}.001",
            file.name,
            flow.settings.archive.label()
        )),
        FlowChoice::External => None,
    }
}

fn choice_row(
    view: Entity<RencordWindow>,
    flow: &Flow,
    choice: FlowChoice,
    title: &str,
    enabled: bool,
    palette: Palette,
) -> impl IntoElement {
    let selected = flow.chosen == Some(choice);
    let border = if selected {
        palette.accent
    } else {
        palette.line
    };
    let title = title.to_owned();
    let id = match choice {
        FlowChoice::Compress => "bf-compress",
        FlowChoice::External => "bf-link",
        FlowChoice::Split => "bf-split",
    };
    let glyph = match choice {
        FlowChoice::Compress => "compress",
        FlowChoice::External => "link",
        FlowChoice::Split => "split",
    };
    let icon_color = if selected {
        palette.accent
    } else {
        palette.muted
    };
    let mut body = v_flex()
        .w(px(400.))
        .gap(px(4.))
        .child(paint(title, palette.text, 14.))
        .child(meta_line(view.clone(), flow, choice, selected, palette));
    if choice == FlowChoice::External {
        let warning = flow
            .offer
            .as_ref()
            .map(|offer| offer.external.warning.clone())
            .unwrap_or_else(|| "This upload leaves the machine.".into());
        body = body.child(paint(warning, palette.muted, 12.));
        if selected {
            let mark = if flow.consent { "☑" } else { "☐" };
            let consent_view = view.clone();
            body = body.child(
                div()
                    .id("bf-consent")
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        consent_view.update(cx, |window, cx| {
                            window.shell.flow.toggle_consent();
                            cx.notify();
                        });
                    })
                    .child(meta(
                        format!("{mark}  I understand. Send it only because I pressed Upload."),
                        palette.muted,
                        11.,
                        palette,
                    )),
            );
        }
    }
    // Border lives on the outer box. Padding is on the inner row so a selected
    // stroke cannot shift the radio and icon relative to the other cards.
    div()
        .id(id)
        .w_full()
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(if selected {
            palette.selected
        } else {
            palette.chat
        }))
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            view.update(cx, |window, cx| {
                if enabled {
                    window.shell.flow.choose(choice);
                }
                cx.notify();
            });
        })
        .child(
            h_flex()
                .w_full()
                .gap(px(12.))
                .px(px(16.))
                .py(px(12.))
                .items_center()
                .child(fixed_slot(16., radio(selected, palette)))
                .child(fixed_slot(18., icon(glyph, 18., icon_color)))
                .child(body),
        )
}

fn fixed_slot(size: f32, child: impl IntoElement) -> impl IntoElement {
    h_flex()
        .w(px(size))
        .h(px(size))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .child(child)
}

fn meta_line(
    view: Entity<RencordWindow>,
    flow: &Flow,
    choice: FlowChoice,
    selected: bool,
    palette: Palette,
) -> impl IntoElement {
    let offer = flow.offer.as_ref();
    let mut row = h_flex().w_full().gap(px(6.)).items_center().flex_wrap();
    match choice {
        FlowChoice::Compress => {
            let technical = offer
                .map(|offer| compress_technical(&offer.compress.detail))
                .unwrap_or_else(|| "No encoder for this file".into());
            row = row.child(meta(technical, palette.muted, 11., palette));
            if selected {
                let label = flow.settings.quality.label().to_owned();
                row = row.child(dropdown(view, "bf-quality", label, palette, |flow| {
                    flow.cycle_quality();
                }));
            }
        }
        FlowChoice::External => {
            if selected {
                row = row
                    .child(dropdown(
                        view.clone(),
                        "bf-host",
                        flow.settings.provider.label().to_owned(),
                        palette,
                        |flow| flow.cycle_provider(),
                    ))
                    .child(dropdown(
                        view,
                        "bf-expiry",
                        flow.settings.expiry_label(),
                        palette,
                        |flow| flow.cycle_expiry(),
                    ));
            } else {
                row = row.child(meta(
                    format!(
                        "{}  ·  {}",
                        flow.settings.provider.label(),
                        flow.settings.expiry_label()
                    ),
                    palette.muted,
                    11.,
                    palette,
                ));
            }
        }
        FlowChoice::Split => {
            let volumes = offer.map(|offer| offer.split.volumes).unwrap_or(0);
            let limit = rencord_upload::format_bytes(flow.limit.bytes);
            row = row.child(meta(
                format!("{volumes} volumes under {limit}"),
                palette.muted,
                11.,
                palette,
            ));
            if selected {
                row = row.child(dropdown(
                    view,
                    "bf-archive",
                    flow.settings.archive.label().to_owned(),
                    palette,
                    |flow| flow.cycle_archive(),
                ));
            }
        }
    }
    row
}

/// Drop the quality word from the planned sentence so the dropdown can sit in its place.
fn compress_technical(detail: &str) -> String {
    let mut parts: Vec<&str> = detail
        .split("  ·  ")
        .filter(|part| {
            !matches!(
                part.trim(),
                "sharp" | "balanced" | "soft" | "small and soft"
            )
        })
        .collect();
    if parts.len() > 3 {
        parts.truncate(3);
    }
    if parts.is_empty() {
        detail.to_owned()
    } else {
        parts.join("  ·  ")
    }
}

fn radio(selected: bool, palette: Palette) -> impl IntoElement {
    let ring = if selected {
        palette.accent
    } else {
        palette.muted
    };
    let mut dot = div()
        .size(px(16.))
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(ring))
        .flex_shrink_0()
        .items_center()
        .justify_center();
    if selected {
        dot = dot.child(div().size(px(8.)).rounded(px(4.)).bg(rgb(palette.accent)));
    }
    dot
}

fn dropdown(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: String,
    palette: Palette,
    apply: impl Fn(&mut Flow) + 'static,
) -> impl IntoElement {
    h_flex()
        .id(id)
        .h(px(22.))
        .px(px(8.))
        .gap(px(4.))
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(palette.line))
        .bg(rgb(palette.elevated))
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            view.update(cx, |window, cx| {
                apply(&mut window.shell.flow);
                let path = rencord_platform::data_dir().join("upload-settings.json");
                let _ = rencord_upload::save_settings(&path, &window.shell.flow.settings);
                cx.notify();
            });
        })
        .child(meta(label, palette.text, 11., palette))
        .child(paint("▾", palette.muted, 11.))
}

fn defaults_row(view: Entity<RencordWindow>, flow: &Flow, palette: Palette) -> impl IntoElement {
    let open = flow.defaults_open;
    let mark = if open { "▴" } else { "▾" };
    let toggle = view.clone();
    let mut column = v_flex().gap(px(8.)).child(
        h_flex()
            .id("bf-defaults")
            .h(px(28.))
            .gap(px(6.))
            .items_center()
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                toggle.update(cx, |window, cx| {
                    window.shell.flow.toggle_defaults();
                    cx.notify();
                });
            })
            .child(meta(
                format!("Defaults  {mark}"),
                palette.muted,
                11.,
                palette,
            )),
    );
    if open {
        let action = flow.settings.default_action.label().to_owned();
        column = column.child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(meta("Start on", palette.muted, 11., palette))
                .child(dropdown(view, "bf-default", action, palette, |flow| {
                    flow.cycle_default()
                })),
        );
    }
    column
}

fn progress_block(view: Entity<RencordWindow>, flow: &Flow, palette: Palette) -> impl IntoElement {
    let snap = flow
        .progress
        .clone()
        .unwrap_or(rencord_upload::ProgressSnap {
            phase: "Working".into(),
            done: 0,
            total: 1,
            detail: String::new(),
        });
    let percent = snap.percent();
    let failed = flow.error.is_some();
    let title = if failed {
        failed_title(&snap.phase)
    } else {
        snap.phase.clone()
    };
    let mut column = v_flex()
        .w_full()
        .gap(px(12.))
        .child(paint(
            title,
            if failed { palette.danger } else { palette.text },
            15.,
        ))
        .child(meta(
            format!(
                "{} / {}  ·  {percent}%",
                rencord_upload::format_bytes(snap.done),
                rencord_upload::format_bytes(snap.total)
            ),
            palette.muted,
            11.,
            palette,
        ))
        .child(meta(snap.detail.clone(), palette.muted, 11., palette))
        .child(progress_bar(percent, palette, failed));
    if let Some(error) = &flow.error {
        column = column.child(paint(error.clone(), palette.danger, 13.));
    }
    let mut actions = h_flex()
        .w_full()
        .gap(px(8.))
        .justify_end()
        .items_center()
        .child(ghost_button(
            view.clone(),
            "bf-cancel-job",
            "Cancel",
            palette,
            |window, cx| {
                if let Some(flag) = &window.upload_cancel {
                    flag.store(true, Ordering::Relaxed);
                }
                window.shell.flow.fail("Cancelled.".into());
                cx.notify();
            },
        ));
    if failed {
        actions = actions.child(accent_button(
            view,
            "bf-retry",
            "Retry",
            palette,
            |window, cx| {
                window.shell.flow.error = None;
                window.start_big_file(cx);
            },
        ));
    }
    column.child(actions)
}

fn failed_title(phase: &str) -> String {
    match phase {
        "Uploading" => "Upload failed".into(),
        "Compressing" => "Compress failed".into(),
        "Splitting" => "Split failed".into(),
        "Sending" => "Send failed".into(),
        "Reading" => "Read failed".into(),
        other if other.ends_with("failed") => other.to_owned(),
        other => format!("{other} failed"),
    }
}

fn progress_bar(percent: u32, palette: Palette, failed: bool) -> impl IntoElement {
    let width = 480.0 * (percent as f32) / 100.0;
    let fill = if failed {
        palette.danger
    } else {
        palette.accent
    };
    div()
        .w_full()
        .h(px(6.))
        .rounded(px(3.))
        .bg(rgb(palette.line))
        .child(
            div()
                .w(px(width.max(2.)))
                .h(px(6.))
                .rounded(px(3.))
                .bg(rgb(fill)),
        )
}

fn done_block(view: Entity<RencordWindow>, flow: &Flow, palette: Palette) -> impl IntoElement {
    let outcome = flow.outcome.clone().unwrap_or_else(|| "Done".into());
    v_flex()
        .w_full()
        .gap(px(12.))
        .child(paint("Finished", palette.text, 15.))
        .child(meta(outcome, palette.muted, 11., palette))
        .child(h_flex().w_full().justify_end().child(ghost_button(
            view,
            "bf-done-close",
            "Close",
            palette,
            |window, cx| {
                window.shell.flow.close();
                cx.notify();
            },
        )))
}

fn reassemble_block(
    view: Entity<RencordWindow>,
    path_input: &Entity<InputState>,
    flow: &Flow,
    palette: Palette,
) -> impl IntoElement {
    let summary = flow
        .parsed_note()
        .map(|note| {
            format!(
                "{}  ·  {} parts  ·  {}",
                note.name,
                note.parts,
                note.format.label()
            )
        })
        .unwrap_or_else(|| "Pick the .001 file.".into());
    v_flex()
        .w_full()
        .gap(px(12.))
        .child(paint("Reassemble", palette.text, 15.))
        .child(meta(summary, palette.muted, 11., palette))
        .child(Input::new(path_input))
        .children(
            flow.error
                .clone()
                .map(|error| paint(error, palette.danger, 13.)),
        )
        .child(action_row(view, palette, "Join", false, |window, cx| {
            let text = window.attachment_path.read(cx).value().to_string();
            window.join_parts(text.trim());
            cx.notify();
        }))
}

fn action_row(
    view: Entity<RencordWindow>,
    palette: Palette,
    primary: &'static str,
    _cancel_closes: bool,
    go: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap(px(8.))
        .justify_end()
        .items_center()
        .child(ghost_button(
            view.clone(),
            "bf-close",
            "Cancel",
            palette,
            |window, cx| {
                window.shell.flow.close();
                cx.notify();
            },
        ))
        .child(accent_button(view, "bf-go", primary, palette, go))
}

struct ButtonStyle {
    fill: u32,
    ink: u32,
    border: bool,
}

fn accent_button(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    palette: Palette,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    button_shell(
        view,
        id,
        label,
        ButtonStyle {
            fill: palette.accent,
            ink: palette.accent_text,
            border: false,
        },
        palette,
        action,
    )
}

fn ghost_button(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    palette: Palette,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    button_shell(
        view,
        id,
        label,
        ButtonStyle {
            fill: palette.chat,
            ink: palette.text,
            border: true,
        },
        palette,
        action,
    )
}

fn button_shell(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    style: ButtonStyle,
    palette: Palette,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    let mut button = h_flex()
        .id(id)
        .h(px(BUTTON_H))
        .px(px(14.))
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .bg(rgb(style.fill))
        .cursor_pointer();
    if style.border {
        button = button.border_1().border_color(rgb(palette.line));
    }
    button
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(13.))
                .text_color(rgb(style.ink))
                .child(label.to_owned()),
        )
        .on_click(move |_, _, cx| {
            view.update(cx, |window, cx| action(window, cx));
        })
}

fn apply_path(window: &mut RencordWindow, text: &str) {
    if text.starts_with("put ") || text.starts_with("s3 ") {
        window.shell.flow.set_custom_spec(text);
        window.shell.flow.settings.provider = ProviderKind::Custom;
        window.shell.flow.error = None;
        return;
    }
    if text.is_empty() {
        window.shell.flow.open_picker();
        window.shell.flow.error = Some("Type a path, or drop the file on the composer.".into());
        return;
    }
    match window.shell.flow.stage_path(Path::new(text)) {
        Ok(()) => window.shell.flow.error = None,
        Err(err) => {
            window.shell.flow.open = true;
            window.shell.flow.error = Some(err.to_string());
        }
    }
}
