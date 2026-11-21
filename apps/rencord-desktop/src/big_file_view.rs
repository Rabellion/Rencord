//! Big-file sheet drawn over the composer. The transfer runs on a worker thread.

use super::{meta, paint, RencordWindow};
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;
use rencord_desktop::Palette;
use rencord_upload::{Flow, FlowChoice, SheetScreen};
use std::path::Path;
use std::sync::atomic::Ordering;

pub fn sheet(
    view: Entity<RencordWindow>,
    path_input: &Entity<InputState>,
    flow: &Flow,
    palette: Palette,
) -> impl IntoElement {
    let mut card = v_flex()
        .w(px(560.))
        .p(px(16.))
        .gap(px(8.))
        .rounded(px(8.))
        .bg(rgb(palette.chat))
        .border_1()
        .border_color(rgb(palette.line));
    card = match flow.screen {
        SheetScreen::Progress => card.child(progress_block(view.clone(), flow, palette)),
        SheetScreen::Done => card.child(done_block(view.clone(), flow, palette)),
        SheetScreen::Reassemble => card.child(reassemble_block(view.clone(), flow, palette)),
        SheetScreen::Pick | SheetScreen::Choose => {
            card.child(choose_block(view.clone(), path_input, flow, palette))
        }
    };
    let top = match flow.screen {
        SheetScreen::Progress => 200.0,
        SheetScreen::Done | SheetScreen::Reassemble => 180.0,
        SheetScreen::Pick | SheetScreen::Choose => 36.0,
    };
    div()
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .w(px(1180.))
        .h(px(760.))
        .bg(rgba(0x000000b8))
        .child(div().absolute().top(px(top)).left(px(310.)).child(card))
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
        .gap(px(8.))
        .child(paint(headline, palette.text, 15.));
    if let Some(offer) = &flow.offer {
        column = column
            .child(meta(offer.meta.clone(), palette.muted, 11., palette))
            .child(paint(offer.server_note.clone(), palette.muted, 12.));
        column = column.child(choice_row(
            view.clone(),
            flow,
            FlowChoice::Compress,
            &offer.compress.title,
            &offer.compress.detail,
            offer.compress.available,
            palette,
        ));
        column = column.child(choice_row(
            view.clone(),
            flow,
            FlowChoice::External,
            &offer.external.title,
            &offer.external.detail,
            true,
            palette,
        ));
        column = column.child(choice_row(
            view.clone(),
            flow,
            FlowChoice::Split,
            &offer.split.title,
            &offer.split.detail,
            true,
            palette,
        ));
        if flow.chosen == Some(FlowChoice::External) {
            column = column.child(warning_box(view.clone(), flow, palette));
        }
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
            "Paste a path, type one, or drop a file on this field. Discord still enforces the cap.",
            palette.muted,
            12.,
        ));
    }
    column = column.child(div().mt(px(4.)).child(Input::new(path_input)));
    column = column.child(settings_row(view.clone(), flow, palette));
    if let Some(error) = &flow.error {
        column = column.child(paint(error.clone(), palette.danger, 12.));
    }
    if !flow.history.is_empty() {
        let entry = &flow.history[flow.history.len() - 1];
        column = column.child(meta(
            format!("last link  {}  {}", entry.file_name, entry.url),
            palette.faint,
            11.,
            palette,
        ));
    }
    let go_label: &'static str = if !over && flow.staged.is_some() {
        "Attach"
    } else {
        match flow.chosen {
            Some(FlowChoice::Compress) => "Compress",
            Some(FlowChoice::External) => "Upload",
            Some(FlowChoice::Split) => "Split",
            None => "Continue",
        }
    };
    column = column.child(
        h_flex()
            .gap(px(8.))
            .mt(px(4.))
            .child(accent_button(
                view.clone(),
                "bf-pick",
                "Pick",
                palette,
                |window, cx| {
                    let text = window.attachment_path.read(cx).value().to_string();
                    apply_path(window, text.trim());
                    cx.notify();
                },
            ))
            .child(accent_button(
                view.clone(),
                "bf-go",
                go_label,
                palette,
                |window, cx| {
                    window.start_big_file(cx);
                },
            ))
            .child(ghost_button(
                view,
                "bf-close",
                "Cancel",
                palette,
                |window, cx| {
                    window.shell.flow.close();
                    cx.notify();
                },
            )),
    );
    column
}

fn choice_row(
    view: Entity<RencordWindow>,
    flow: &Flow,
    choice: FlowChoice,
    title: &str,
    detail: &str,
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
    let detail = detail.to_owned();
    let id = match choice {
        FlowChoice::Compress => "bf-compress",
        FlowChoice::External => "bf-link",
        FlowChoice::Split => "bf-split",
    };
    div()
        .id(id)
        .w_full()
        .px(px(10.))
        .py(px(8.))
        .rounded(px(6.))
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
        .child(paint(title, palette.text, 13.))
        .child(meta(detail, palette.muted, 11., palette))
}

fn warning_box(view: Entity<RencordWindow>, flow: &Flow, palette: Palette) -> impl IntoElement {
    let warning = flow
        .offer
        .as_ref()
        .map(|offer| offer.external.warning.clone())
        .unwrap_or_else(|| "This upload leaves the machine.".into());
    let mark = if flow.consent { "☑" } else { "☐" };
    v_flex()
        .gap(px(4.))
        .p(px(8.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(palette.line))
        .child(paint(warning, palette.text, 12.))
        .child(
            div()
                .id("bf-consent")
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    view.update(cx, |window, cx| {
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
        )
}

fn settings_row(view: Entity<RencordWindow>, flow: &Flow, palette: Palette) -> impl IntoElement {
    let settings = &flow.settings;
    v_flex()
        .gap(px(4.))
        .child(meta(
            format!(
                "Settings  {}  ·  {}  ·  {}  ·  {}",
                settings.default_action.label(),
                settings.provider.label(),
                settings.quality.label(),
                settings.expiry_label()
            ),
            palette.muted,
            11.,
            palette,
        ))
        .child(
            h_flex()
                .gap(px(6.))
                .child(cycle(
                    view.clone(),
                    "bf-default",
                    "Default",
                    palette,
                    |flow| {
                        flow.cycle_default();
                    },
                ))
                .child(cycle(view.clone(), "bf-host", "Host", palette, |flow| {
                    flow.cycle_provider();
                }))
                .child(cycle(
                    view.clone(),
                    "bf-quality",
                    "Quality",
                    palette,
                    |flow| {
                        flow.cycle_quality();
                    },
                ))
                .child(cycle(
                    view.clone(),
                    "bf-expiry",
                    "Expiry",
                    palette,
                    |flow| {
                        flow.cycle_expiry();
                    },
                ))
                .child(cycle(view, "bf-archive", "Archive", palette, |flow| {
                    flow.cycle_archive();
                })),
        )
}

fn cycle(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    palette: Palette,
    apply: impl Fn(&mut Flow) + 'static,
) -> impl IntoElement {
    ghost_button(view, id, label, palette, move |window, cx| {
        apply(&mut window.shell.flow);
        let path = rencord_platform::data_dir().join("upload-settings.json");
        let _ = rencord_upload::save_settings(&path, &window.shell.flow.settings);
        cx.notify();
    })
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
    let width = 440.0 * (percent as f32) / 100.0;
    v_flex()
        .gap(px(8.))
        .child(paint(snap.phase.clone(), palette.text, 15.))
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
        .child(
            div()
                .w(px(440.))
                .h(px(6.))
                .rounded(px(3.))
                .bg(rgb(palette.line))
                .child(
                    div()
                        .w(px(width.max(2.)))
                        .h(px(6.))
                        .rounded(px(3.))
                        .bg(rgb(palette.accent)),
                ),
        )
        .child(error_line(flow, palette))
        .child(
            h_flex()
                .gap(px(8.))
                .child(ghost_button(
                    view.clone(),
                    "bf-cancel-job",
                    "Cancel",
                    palette,
                    |window, cx| {
                        if let Some(flag) = &window.upload_cancel {
                            flag.store(true, Ordering::Relaxed);
                        }
                        window.shell.flow.fail("Cancelled".into());
                        cx.notify();
                    },
                ))
                .child(ghost_button(
                    view,
                    "bf-retry",
                    "Retry",
                    palette,
                    |window, cx| {
                        if window.shell.flow.can_retry() {
                            window.shell.flow.error = None;
                            window.start_big_file(cx);
                        }
                        cx.notify();
                    },
                )),
        )
}

fn done_block(view: Entity<RencordWindow>, flow: &Flow, palette: Palette) -> impl IntoElement {
    let outcome = flow.outcome.clone().unwrap_or_else(|| "Done".into());
    v_flex()
        .gap(px(8.))
        .child(paint("Finished", palette.text, 15.))
        .child(meta(outcome, palette.muted, 11., palette))
        .child(ghost_button(
            view,
            "bf-done-close",
            "Close",
            palette,
            |window, cx| {
                window.shell.flow.close();
                cx.notify();
            },
        ))
}

fn reassemble_block(
    view: Entity<RencordWindow>,
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
        .unwrap_or_else(|| "Paste a rencord-split note, then Pick the .001 file.".into());
    v_flex()
        .gap(px(8.))
        .child(paint("Reassemble", palette.text, 15.))
        .child(meta(summary, palette.muted, 11., palette))
        .child(meta(
            flow.reassemble_note.clone(),
            palette.faint,
            11.,
            palette,
        ))
        .child(error_line(flow, palette))
        .child(
            h_flex()
                .gap(px(8.))
                .child(accent_button(
                    view.clone(),
                    "bf-join",
                    "Join",
                    palette,
                    |window, cx| {
                        let text = window.attachment_path.read(cx).value().to_string();
                        window.join_parts(text.trim());
                        cx.notify();
                    },
                ))
                .child(ghost_button(
                    view,
                    "bf-reassemble-close",
                    "Close",
                    palette,
                    |window, cx| {
                        window.shell.flow.close();
                        cx.notify();
                    },
                )),
        )
}

fn error_line(flow: &Flow, palette: Palette) -> impl IntoElement {
    let text = flow.error.clone().unwrap_or_default();
    div().child(paint(text, palette.danger, 12.))
}

fn accent_button(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    palette: Palette,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(28.))
        .px(px(12.))
        .rounded(px(6.))
        .items_center()
        .justify_center()
        .bg(rgb(palette.accent))
        .cursor_pointer()
        .child(paint(label, palette.accent_text, 13.))
        .on_click(move |_, _, cx| {
            view.update(cx, |window, cx| action(window, cx));
        })
}

fn ghost_button(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    palette: Palette,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(28.))
        .px(px(10.))
        .rounded(px(6.))
        .items_center()
        .justify_center()
        .border_1()
        .border_color(rgb(palette.line))
        .cursor_pointer()
        .child(paint(label, palette.text, 12.))
        .on_click(move |_, _, cx| {
            view.update(cx, |window, cx| action(window, cx));
        })
}

fn apply_path(window: &mut RencordWindow, text: &str) {
    if text.starts_with("put ") || text.starts_with("s3 ") {
        window.shell.flow.set_custom_spec(text);
        window.shell.flow.settings.provider = rencord_upload::ProviderKind::Custom;
        window.shell.flow.error = None;
        return;
    }
    if text.is_empty() {
        window.shell.flow.open_picker();
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
