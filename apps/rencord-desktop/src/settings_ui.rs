//! Settings. Appearance is its own page: preset cards, a custom theme, and a
//! live preview. Voice devices stay on Voice & Video. Tokens live on Advanced.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;
use rencord_desktop::{Density, Palette, ThemeMode};
use rencord_theme::{contrast_ratio, BackgroundKind, Color, Oklch};

const FONT_STEPS: [f32; 5] = [13.0, 14.0, 15.0, 16.0, 17.0];

use super::{hdr_ui, rail_hexwave, RencordWindow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    Account,
    Appearance,
    Voice,
    Keybinds,
    Advanced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppearanceTab {
    Presets,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerTarget {
    Base,
    Accent,
    Stop(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetMenu {
    Light,
    Dark,
}

pub fn parse_settings(value: &str) -> (SettingsPage, AppearanceTab) {
    match value {
        "custom" | "customize" => (SettingsPage::Appearance, AppearanceTab::Custom),
        "advanced" | "tokens" => (SettingsPage::Advanced, AppearanceTab::Presets),
        "voice" | "video" => (SettingsPage::Voice, AppearanceTab::Presets),
        "account" => (SettingsPage::Account, AppearanceTab::Presets),
        "keybinds" | "keys" => (SettingsPage::Keybinds, AppearanceTab::Presets),
        _ => (SettingsPage::Appearance, AppearanceTab::Presets),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn page(
    window: &RencordWindow,
    view: Entity<RencordWindow>,
    palette: Palette,
    account: &str,
    inputs: &[String],
    outputs: &[String],
    chosen_in: Option<&str>,
    chosen_out: Option<&str>,
    jitter: &str,
) -> impl IntoElement {
    let page = window.settings_page;
    h_flex()
        .size_full()
        .bg(rgb(palette.app_bg))
        .font_family(palette.font_ui)
        .text_color(rgb(palette.text))
        .child(nav(view.clone(), palette, page))
        .child(
            v_flex()
                .flex_1()
                .h_full()
                .min_w(px(0.))
                .child(match page {
                    SettingsPage::Account => {
                        account_page(window, view.clone(), palette, account).into_any_element()
                    }
                    SettingsPage::Appearance => {
                        appearance_page(window, view.clone(), palette).into_any_element()
                    }
                    SettingsPage::Voice => voice_page(
                        window,
                        view.clone(),
                        palette,
                        inputs,
                        outputs,
                        chosen_in,
                        chosen_out,
                        jitter,
                    )
                    .into_any_element(),
                    SettingsPage::Keybinds => keybinds_page(palette).into_any_element(),
                    SettingsPage::Advanced => {
                        advanced_page(window, view.clone(), palette).into_any_element()
                    }
                })
                .child(footer(view, palette, &window.theme_code)),
        )
}

fn nav(view: Entity<RencordWindow>, palette: Palette, page: SettingsPage) -> impl IntoElement {
    let mut column = v_flex()
        .w(px(216.))
        .h_full()
        .flex_shrink_0()
        .bg(rgb(palette.sidebar))
        .border_r_1()
        .border_color(rgb(palette.line))
        .child(
            h_flex()
                .px(px(16.))
                .pt(px(16.))
                .pb(px(12.))
                .gap(px(10.))
                .items_center()
                .child(rail_hexwave(palette.text, palette.accent))
                .child(
                    v_flex()
                        .child(strong("Rencord", palette.text, 14.))
                        .child(quiet("Settings", palette.muted, 12.)),
                ),
        )
        .child(div().px(px(16.)).pt(px(8.)).pb(px(6.)).child(quiet(
            "User settings",
            palette.faint,
            11.,
        )));
    for (label, target) in [
        ("My Account", SettingsPage::Account),
        ("Appearance", SettingsPage::Appearance),
        ("Voice & Video", SettingsPage::Voice),
        ("Keybinds", SettingsPage::Keybinds),
        ("Advanced", SettingsPage::Advanced),
    ] {
        column = column.child(nav_row(
            view.clone(),
            palette,
            label,
            target,
            page == target,
        ));
    }
    column
        .child(div().flex_1())
        .child(div().px(px(12.)).pb(px(12.)).child(ghost(
            view,
            "settings-close",
            "Close",
            palette,
            |window, cx| {
                window.shell.close_layers();
                cx.notify();
            },
        )))
}

fn nav_row(
    view: Entity<RencordWindow>,
    palette: Palette,
    label: &'static str,
    target: SettingsPage,
    selected: bool,
) -> impl IntoElement {
    div().px(px(8.)).child(
        div()
            .id(SharedString::from(format!("nav-{label}")))
            .h(px(32.))
            .px(px(8.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .cursor_pointer()
            .bg(rgb(if selected {
                palette.selected
            } else {
                palette.sidebar
            }))
            .hover(|style| style.bg(rgb(palette.hover)))
            .on_click(move |_, _, cx| {
                view.update(cx, |window, cx| {
                    window.settings_page = target;
                    window.picker = None;
                    window.preset_menu = None;
                    cx.notify();
                });
            })
            .child(quiet(
                label,
                if selected {
                    palette.text
                } else {
                    palette.muted
                },
                13.,
            )),
    )
}

fn appearance_page(
    window: &RencordWindow,
    view: Entity<RencordWindow>,
    palette: Palette,
) -> impl IntoElement {
    let tab = window.appearance_tab;
    h_flex()
        .flex_1()
        .h_full()
        .min_h(px(0.))
        .child(
            v_flex()
                .flex_1()
                .h_full()
                .min_w(px(0.))
                .overflow_hidden()
                .px(px(28.))
                .py(px(16.))
                .gap(px(12.))
                .child(page_title(
                    if tab == AppearanceTab::Custom {
                        "Customize"
                    } else {
                        "Appearance"
                    },
                    palette,
                ))
                .child(tab_row(view.clone(), palette, tab))
                .child(if tab == AppearanceTab::Custom {
                    custom_body(window, view, palette).into_any_element()
                } else {
                    presets_body(window, view, palette).into_any_element()
                }),
        )
        .child(preview(palette))
}

fn tab_row(view: Entity<RencordWindow>, palette: Palette, tab: AppearanceTab) -> impl IntoElement {
    h_flex()
        .gap(px(4.))
        .child(tab_chip(
            view.clone(),
            palette,
            "tab-presets",
            "Presets",
            tab == AppearanceTab::Presets,
            AppearanceTab::Presets,
        ))
        .child(tab_chip(
            view,
            palette,
            "tab-custom",
            "Custom",
            tab == AppearanceTab::Custom,
            AppearanceTab::Custom,
        ))
}

fn tab_chip(
    view: Entity<RencordWindow>,
    palette: Palette,
    id: &'static str,
    label: &'static str,
    on: bool,
    tab: AppearanceTab,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(28.))
        .px(px(12.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .cursor_pointer()
        .bg(rgb(if on { palette.selected } else { palette.app_bg }))
        .border_1()
        .border_color(rgb(if on { palette.accent } else { palette.line }))
        .on_click(move |_, _, cx| {
            view.update(cx, |window, cx| {
                window.appearance_tab = tab;
                window.picker = None;
                cx.notify();
            });
        })
        .child(quiet(
            label,
            if on { palette.text } else { palette.muted },
            13.,
        ))
}

fn presets_body(
    window: &RencordWindow,
    view: Entity<RencordWindow>,
    palette: Palette,
) -> impl IntoElement {
    let prefs = &window.shell.prefs;
    let selected = if prefs.custom.is_some() {
        None
    } else {
        Some(prefs.theme)
    };
    let mut grid = h_flex().gap(px(12.)).flex_wrap();
    for mode in ThemeMode::ALL {
        grid = grid.child(preset_card(
            view.clone(),
            palette,
            mode,
            selected == Some(mode),
        ));
    }
    v_flex()
        .gap(px(12.))
        .child(section("Theme", palette))
        .child(grid)
        .child(sync_row(window, view.clone(), palette))
        .child(section("Display", palette))
        .child(segmented_density(view.clone(), palette, prefs.density))
        .child(section("Accessibility", palette))
        .child(switch_row(
            view.clone(),
            palette,
            "switch-contrast",
            "High contrast",
            "Stronger text against each surface.",
            prefs.high_contrast,
            |window| window.shell.prefs.high_contrast = !window.shell.prefs.high_contrast,
        ))
        .child(switch_row(
            view.clone(),
            palette,
            "switch-motion",
            "Reduced motion",
            "Theme changes apply without a fade.",
            prefs.reduced_motion,
            |window| window.shell.prefs.reduced_motion = !window.shell.prefs.reduced_motion,
        ))
        .child(switch_row(
            view,
            palette,
            "switch-colorblind",
            "Colorblind-safe statuses",
            "Online, idle, and do-not-disturb use a palette that stays distinct.",
            prefs.colorblind_status,
            |window| window.shell.prefs.colorblind_status = !window.shell.prefs.colorblind_status,
        ))
}

fn preset_card(
    view: Entity<RencordWindow>,
    palette: Palette,
    mode: ThemeMode,
    selected: bool,
) -> impl IntoElement {
    let theme = rencord_theme::Theme::preset(mode);
    let colors = theme.colors;
    v_flex()
        .w(px(104.))
        .gap(px(6.))
        .items_center()
        .child(
            div()
                .id(SharedString::from(format!("preset-{}", mode.id())))
                .w(px(104.))
                .h(px(58.))
                .p(px(if selected { 2. } else { 1. }))
                .rounded(px(8.))
                .bg(rgb(if selected {
                    palette.accent
                } else {
                    palette.line
                }))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    view.update(cx, |window, cx| {
                        window.choose_preset(mode);
                        cx.notify();
                    });
                })
                .child(
                    h_flex()
                        .size_full()
                        .rounded(px(6.))
                        .overflow_hidden()
                        .child(div().w(px(16.)).h_full().bg(rgb(colors.bg_rail.to_u32())))
                        .child(
                            div()
                                .w(px(28.))
                                .h_full()
                                .bg(rgb(colors.bg_sidebar.to_u32())),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .h_full()
                                .bg(rgb(colors.bg_chat.to_u32()))
                                .pt(px(8.))
                                .px(px(6.))
                                .child(
                                    div()
                                        .h(px(8.))
                                        .rounded(px(2.))
                                        .bg(rgb(colors.accent.to_u32())),
                                ),
                        ),
                ),
        )
        .child(quiet(
            mode.label(),
            if selected {
                palette.text
            } else {
                palette.muted
            },
            12.,
        ))
}

fn sync_row(
    window: &RencordWindow,
    view: Entity<RencordWindow>,
    palette: Palette,
) -> impl IntoElement {
    let prefs = &window.shell.prefs;
    v_flex()
        .gap(px(10.))
        .child(switch_row(
            view.clone(),
            palette,
            "switch-system",
            "Sync with system",
            "Use the light preset in light mode and the dark preset in dark mode.",
            prefs.sync_system,
            |window| window.shell.prefs.sync_system = !window.shell.prefs.sync_system,
        ))
        .child(
            h_flex()
                .gap(px(16.))
                .child(preset_menu(
                    view.clone(),
                    palette,
                    "menu-light",
                    "Light",
                    prefs.light_preset,
                    PresetMenu::Light,
                    window.preset_menu == Some(PresetMenu::Light),
                ))
                .child(preset_menu(
                    view,
                    palette,
                    "menu-dark",
                    "Dark",
                    prefs.dark_preset,
                    PresetMenu::Dark,
                    window.preset_menu == Some(PresetMenu::Dark),
                )),
        )
}

fn preset_menu(
    view: Entity<RencordWindow>,
    palette: Palette,
    id: &'static str,
    caption: &'static str,
    current: ThemeMode,
    which: PresetMenu,
    open: bool,
) -> impl IntoElement {
    let mut column = v_flex()
        .gap(px(4.))
        .child(quiet(caption, palette.faint, 11.))
        .child(
            div()
                .id(id)
                .h(px(32.))
                .px(px(10.))
                .min_w(px(148.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_between()
                .border_1()
                .border_color(rgb(palette.line))
                .bg(rgb(palette.elevated))
                .cursor_pointer()
                .on_click({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |window, cx| {
                            window.preset_menu = if window.preset_menu == Some(which) {
                                None
                            } else {
                                Some(which)
                            };
                            cx.notify();
                        });
                    }
                })
                .child(quiet(current.label(), palette.text, 13.))
                .child(quiet("▾", palette.muted, 12.)),
        );
    if open {
        let mut list = v_flex()
            .py(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(palette.line))
            .bg(rgb(palette.elevated));
        for mode in ThemeMode::ALL {
            let item = view.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("{id}-{}", mode.id())))
                    .h(px(28.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(palette.hover)))
                    .on_click(move |_, _, cx| {
                        item.update(cx, |window, cx| {
                            if which == PresetMenu::Light {
                                window.shell.prefs.light_preset = mode;
                            } else {
                                window.shell.prefs.dark_preset = mode;
                            }
                            window.preset_menu = None;
                            window.shell.save_appearance();
                            cx.notify();
                        });
                    })
                    .child(quiet(mode.label(), palette.text, 13.)),
            );
        }
        column = column.child(list);
    }
    column
}

fn custom_body(
    window: &RencordWindow,
    view: Entity<RencordWindow>,
    palette: Palette,
) -> impl IntoElement {
    let theme = window.shell.prefs.resolved(window.system_dark);
    let base = theme.base.unwrap_or(theme.colors.bg_base);
    let accent = theme.accent_seed.unwrap_or(theme.colors.accent);
    let (angle, stops) = match &theme.background.kind {
        BackgroundKind::Gradient { angle, stops } => (*angle, stops.clone()),
        BackgroundKind::Solid => (160.0, Vec::new()),
    };
    v_flex()
        .gap(px(14.))
        .child(section("Base and accent", palette))
        .child(color_editor(
            view.clone(),
            palette,
            "base",
            "Base",
            base,
            &window.base_hex,
            window.picker == Some(PickerTarget::Base),
            PickerTarget::Base,
            false,
        ))
        .child(color_editor(
            view.clone(),
            palette,
            "accent",
            "Accent",
            accent,
            &window.accent_hex,
            window.picker == Some(PickerTarget::Accent),
            PickerTarget::Accent,
            true,
        ))
        .child(section("Background", palette))
        .child(gradient_editor(
            view.clone(),
            palette,
            angle,
            &stops,
            window.picker,
        ))
        .child(section("Type and shape", palette))
        .child(slider_row(
            view.clone(),
            palette,
            "font",
            "Font size",
            &format!("{:.0} px", window.shell.prefs.font_px),
            index_of(&FONT_STEPS, window.shell.prefs.font_px),
            FONT_STEPS.len(),
            |window, index| {
                window.shell.prefs.font_px = FONT_STEPS[index];
            },
        ))
        .child(metric_slider(
            view.clone(),
            palette,
            "space",
            "Spacing",
            window.shell.prefs.message_spacing,
            &[0.75, 1.0, 1.25, 1.5],
            |value| format!("{value:.2}"),
            |window, value| window.shell.prefs.message_spacing = value,
        ))
        .child(metric_slider(
            view.clone(),
            palette,
            "radius",
            "Radius",
            window.shell.prefs.radius_scale,
            &[0.0, 0.75, 1.0, 1.25, 1.5],
            |value| format!("{value:.2}"),
            |window, value| window.shell.prefs.radius_scale = value,
        ))
        .child(metric_slider(
            view.clone(),
            palette,
            "sat",
            "Saturation",
            window.shell.prefs.saturation,
            &[0.4, 0.7, 0.85, 1.0],
            |value| format!("{:.0}%", value * 100.0),
            |window, value| window.shell.prefs.saturation = value,
        ))
        .child(metric_slider(
            view,
            palette,
            "grain",
            "Grain",
            theme.background.grain,
            &[0.0, 0.01, 0.02, 0.03],
            |value| format!("{:.0}%", value * 100.0),
            |window, value| {
                window.shell.prefs.ensure_custom().background.grain = value;
            },
        ))
}

#[allow(clippy::too_many_arguments)]
fn color_editor(
    view: Entity<RencordWindow>,
    palette: Palette,
    id: &'static str,
    label: &'static str,
    color: Color,
    hex_input: &Entity<InputState>,
    open: bool,
    target: PickerTarget,
    accent: bool,
) -> impl IntoElement {
    let hex = color.to_hex();
    let ok = color.to_oklch();
    let mut column = v_flex().gap(px(8.)).child(
        h_flex()
            .gap(px(10.))
            .items_center()
            .child(quiet(label, palette.muted, 13.))
            .child(swatch_button(
                view.clone(),
                palette,
                &format!("{id}-current"),
                color.to_u32(),
                28.,
                target,
            ))
            .child(
                div()
                    .px(px(8.))
                    .h(px(28.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(palette.line))
                    .flex()
                    .items_center()
                    .child(mono(&hex, palette.text)),
            )
            .child(div().w(px(120.)).child(Input::new(hex_input)))
            .child(ghost(
                view.clone(),
                &format!("{id}-apply"),
                "Apply",
                palette,
                move |window, cx| {
                    let field = if accent {
                        window.accent_hex.clone()
                    } else {
                        window.base_hex.clone()
                    };
                    let text = field.read(cx).value().to_string();
                    if let Some(parsed) = Color::parse(&text) {
                        window.recolor(parsed.to_u32(), accent);
                    }
                    cx.notify();
                },
            )),
    );
    column = column.child(swatch_row(view.clone(), palette, id, accent));
    if open {
        column = column.child(picker_popover(view, palette, id, ok, target));
    }
    column
}

fn swatch_row(
    view: Entity<RencordWindow>,
    palette: Palette,
    id: &str,
    accent: bool,
) -> impl IntoElement {
    let mut row = h_flex().gap(px(6.));
    for (index, hex) in rencord_theme::EDITOR_SWATCHES.iter().enumerate() {
        let hex = *hex;
        let button = view.clone();
        row = row.child(
            div()
                .id(SharedString::from(format!("{id}-swatch-{index}")))
                .size(px(18.))
                .rounded(px(4.))
                .bg(rgb(hex))
                .border_1()
                .border_color(rgb(palette.line))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    button.update(cx, |window, cx| {
                        window.recolor(hex, accent);
                        cx.notify();
                    });
                }),
        );
    }
    row
}

fn swatch_button(
    view: Entity<RencordWindow>,
    palette: Palette,
    id: &str,
    fill: u32,
    size: f32,
    target: PickerTarget,
) -> impl IntoElement {
    div()
        .id(SharedString::from(id.to_string()))
        .size(px(size))
        .rounded(px(6.))
        .bg(rgb(fill))
        .border_1()
        .border_color(rgb(palette.line))
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            view.update(cx, |window, cx| {
                window.picker = if window.picker == Some(target) {
                    None
                } else {
                    Some(target)
                };
                cx.notify();
            });
        })
}

fn picker_popover(
    view: Entity<RencordWindow>,
    palette: Palette,
    id: &str,
    current: Oklch,
    target: PickerTarget,
) -> impl IntoElement {
    let mut hues = h_flex().gap(px(4.));
    for step in 0..12 {
        let hue = step as f32 * 30.0;
        let color = Color::from_oklch(Oklch {
            l: current.l.clamp(0.35, 0.8),
            c: 0.14,
            h: hue,
        });
        let button = view.clone();
        hues = hues.child(
            div()
                .id(SharedString::from(format!("{id}-hue-{step}")))
                .size(px(16.))
                .rounded(px(4.))
                .bg(rgb(color.to_u32()))
                .border_1()
                .border_color(rgb(if (current.h - hue).abs() < 15.0 {
                    palette.accent
                } else {
                    palette.line
                }))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    button.update(cx, |window, cx| {
                        apply_oklch(window, target, current.l, 0.14, hue);
                        cx.notify();
                    });
                }),
        );
    }
    let mut lights = h_flex().gap(px(4.));
    for step in 0..6 {
        let lightness = 0.2 + step as f32 * 0.12;
        let color = Color::from_oklch(Oklch {
            l: lightness,
            c: current.c.max(0.04),
            h: current.h,
        });
        let button = view.clone();
        lights = lights.child(
            div()
                .id(SharedString::from(format!("{id}-l-{step}")))
                .size(px(16.))
                .rounded(px(4.))
                .bg(rgb(color.to_u32()))
                .border_1()
                .border_color(rgb(palette.line))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    button.update(cx, |window, cx| {
                        apply_oklch(window, target, lightness, current.c.max(0.04), current.h);
                        cx.notify();
                    });
                }),
        );
    }
    v_flex()
        .gap(px(6.))
        .p(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(palette.line))
        .bg(rgb(palette.elevated))
        .child(quiet("Hue", palette.faint, 11.))
        .child(hues)
        .child(quiet("Lightness", palette.faint, 11.))
        .child(lights)
        .child(mono(
            &format!(
                "OKLCH  L {:.2}  C {:.2}  H {:.0}",
                current.l, current.c, current.h
            ),
            palette.muted,
        ))
}

fn apply_oklch(window: &mut RencordWindow, target: PickerTarget, l: f32, c: f32, h: f32) {
    let color = Color::from_oklch(Oklch { l, c, h });
    match target {
        PickerTarget::Base => window.recolor(color.to_u32(), false),
        PickerTarget::Accent => window.recolor(color.to_u32(), true),
        PickerTarget::Stop(index) => set_stop(window, index as usize, color),
    }
}

fn gradient_editor(
    view: Entity<RencordWindow>,
    palette: Palette,
    angle: f32,
    stops: &[Color],
    picker: Option<PickerTarget>,
) -> impl IntoElement {
    let active = !stops.is_empty();
    let shown: Vec<Color> = if active {
        stops.to_vec()
    } else {
        vec![Color::rgb(palette.app_bg)]
    };
    let mut column = v_flex().gap(px(8.)).child(gradient_bar(&shown)).child({
        let mut row = h_flex().gap(px(8.)).items_center();
        if active {
            for (index, stop) in stops.iter().enumerate() {
                row = row.child(swatch_button(
                    view.clone(),
                    palette,
                    &format!("stop-{index}"),
                    stop.to_u32(),
                    22.,
                    PickerTarget::Stop(index as u8),
                ));
            }
        }
        row = row.child(ghost(
            view.clone(),
            "gradient-toggle",
            if active {
                "Solid color"
            } else {
                "Use a gradient"
            },
            palette,
            |window, cx| {
                window.toggle_gradient();
                cx.notify();
            },
        ));
        if active && stops.len() < 3 {
            row = row.child(ghost(
                view.clone(),
                "gradient-add",
                "Add stop",
                palette,
                |window, cx| {
                    add_stop(window);
                    cx.notify();
                },
            ));
        }
        row
    });
    if let Some(PickerTarget::Stop(index)) = picker {
        if let Some(stop) = stops.get(index as usize) {
            column = column.child(picker_popover(
                view.clone(),
                palette,
                "stop-picker",
                stop.to_oklch(),
                PickerTarget::Stop(index),
            ));
        }
    }
    if active {
        let steps = [0.0_f32, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
        column = column.child(slider_row(
            view,
            palette,
            "angle",
            "Angle",
            &format!("{angle:.0}°"),
            nearest(&steps, angle),
            steps.len(),
            move |window, index| set_angle(window, steps[index]),
        ));
    }
    column
}

fn gradient_bar(stops: &[Color]) -> impl IntoElement {
    let parts = 16;
    let mut bar = h_flex()
        .w(px(280.))
        .h(px(28.))
        .rounded(px(6.))
        .overflow_hidden();
    for index in 0..parts {
        let t = index as f32 / (parts - 1) as f32;
        let color = sample_stops(stops, t);
        bar = bar.child(div().flex_1().h_full().bg(rgb(color.to_u32())));
    }
    bar
}

fn sample_stops(stops: &[Color], t: f32) -> Color {
    if stops.is_empty() {
        return Color::rgb(0);
    }
    if stops.len() == 1 || t <= 0.0 {
        return stops[0];
    }
    let scaled = t * (stops.len() - 1) as f32;
    let index = (scaled.floor() as usize).min(stops.len() - 2);
    let local = scaled - index as f32;
    stops[index].lerp(stops[index + 1], local)
}

fn set_stop(window: &mut RencordWindow, index: usize, color: Color) {
    let theme = window.shell.prefs.ensure_custom();
    if let BackgroundKind::Gradient { stops, .. } = &mut theme.background.kind {
        if let Some(slot) = stops.get_mut(index) {
            *slot = color;
        }
    }
    theme.preset = None;
    window.shell.save_appearance();
}

fn add_stop(window: &mut RencordWindow) {
    let theme = window.shell.prefs.ensure_custom();
    if let BackgroundKind::Gradient { stops, .. } = &mut theme.background.kind {
        if stops.len() < 3 {
            let next = stops.last().copied().unwrap_or(theme.colors.accent);
            stops.push(next.shift_lightness(0.08));
        }
    }
    theme.preset = None;
    window.shell.save_appearance();
}

fn set_angle(window: &mut RencordWindow, angle: f32) {
    let theme = window.shell.prefs.ensure_custom();
    if let BackgroundKind::Gradient { angle: slot, .. } = &mut theme.background.kind {
        *slot = angle;
    }
    theme.preset = None;
    window.shell.save_appearance();
}

fn advanced_page(
    window: &RencordWindow,
    view: Entity<RencordWindow>,
    palette: Palette,
) -> impl IntoElement {
    let theme = window.shell.prefs.resolved(window.system_dark);
    v_flex()
        .flex_1()
        .h_full()
        .min_h(px(0.))
        .px(px(28.))
        .py(px(20.))
        .gap(px(12.))
        .child(page_title("Advanced", palette))
        .child(quiet(
            "Search a token, then edit its lightness on that row.",
            palette.muted,
            13.,
        ))
        .child(div().w(px(280.)).child(Input::new(&window.token_query)))
        .child(token_list(
            window,
            view,
            palette,
            &theme,
            &window.token_filter,
        ))
        .child(quiet(
            &rencord_theme::warnings_text(&theme),
            if rencord_theme::contrast_warnings(&theme).is_empty() {
                palette.muted
            } else {
                palette.danger
            },
            13.,
        ))
}

fn token_list(
    window: &RencordWindow,
    view: Entity<RencordWindow>,
    palette: Palette,
    theme: &rencord_theme::Theme,
    query: &str,
) -> impl IntoElement {
    let query = query.to_ascii_lowercase();
    let mut list = v_flex().flex_1().min_h(px(0.)).gap(px(2.));
    let mut shown = 0;
    for (index, key) in rencord_theme::Colors::keys().iter().enumerate() {
        if query.is_empty() {
            if !is_text_token(key) {
                continue;
            }
        } else if !key.to_ascii_lowercase().contains(query.as_str()) || shown >= 14 {
            continue;
        }
        shown += 1;
        let color = theme.colors.get(key).unwrap_or(theme.colors.accent);
        let selected = window.token_index == index;
        list = list.child(token_row(
            view.clone(),
            palette,
            theme,
            key,
            color,
            index,
            selected,
        ));
    }
    list
}

fn is_text_token(key: &str) -> bool {
    key.starts_with("text.") || key == "accent.fg"
}

fn token_row(
    view: Entity<RencordWindow>,
    palette: Palette,
    theme: &rencord_theme::Theme,
    key: &'static str,
    color: Color,
    index: usize,
    selected: bool,
) -> impl IntoElement {
    let badge = contrast_badge(theme, key);
    let mut row = h_flex()
        .id(SharedString::from(format!("token-{key}")))
        .h(px(32.))
        .px(px(8.))
        .gap(px(10.))
        .items_center()
        .rounded(px(6.))
        .bg(rgb(if selected {
            palette.selected
        } else {
            palette.app_bg
        }))
        .cursor_pointer()
        .on_click({
            let view = view.clone();
            move |_, _, cx| {
                view.update(cx, |window, cx| {
                    window.token_index = index;
                    cx.notify();
                });
            }
        })
        .child(
            div()
                .size(px(14.))
                .rounded(px(3.))
                .bg(rgb(color.to_u32()))
                .border_1()
                .border_color(rgb(palette.line)),
        )
        .child(div().w(px(180.)).child(quiet(key, palette.text, 13.)))
        .child(div().w(px(84.)).child(mono(&color.to_hex(), palette.muted)));
    if let Some(badge) = badge {
        row = row.child(contrast_pill(palette, badge));
    }
    let mut column = v_flex().child(row);
    if selected {
        let ok = color.to_oklch();
        column = column.child(div().pl(px(32.)).pb(px(6.)).child(slider_row(
            view.clone(),
            palette,
            "token-l",
            "Lightness",
            &format!("{:.2}", ok.l),
            ((ok.l / 0.1).round() as usize).min(9),
            10,
            move |window, step| {
                let lightness = step as f32 * 0.1;
                set_token_lightness(window, key, lightness);
            },
        )));
    }
    column
}

pub fn contrast_badge(theme: &rencord_theme::Theme, key: &str) -> Option<&'static str> {
    if !is_text_token(key) {
        return None;
    }
    let fg = theme.colors.get(key)?;
    let bg = if key == "accent.fg" {
        theme.colors.accent
    } else if key.contains("mention") {
        theme.colors.bg_mention
    } else {
        theme.colors.bg_chat
    };
    let ratio = contrast_ratio(fg, bg);
    Some(if ratio >= 7.0 {
        "AAA"
    } else if ratio >= 4.5 {
        "AA"
    } else {
        "Fail"
    })
}

fn contrast_pill(palette: Palette, label: &'static str) -> impl IntoElement {
    let fail = label == "Fail";
    div()
        .px(px(6.))
        .h(px(18.))
        .rounded(px(9.))
        .flex()
        .items_center()
        .border_1()
        .border_color(rgb(if fail {
            palette.danger
        } else {
            palette.positive
        }))
        .child(quiet(
            label,
            if fail {
                palette.danger
            } else {
                palette.positive
            },
            11.,
        ))
}

fn set_token_lightness(window: &mut RencordWindow, key: &str, lightness: f32) {
    let mut theme = window.shell.prefs.ensure_custom().clone();
    if let Some(color) = theme.colors.get(key) {
        let mut ok = color.to_oklch();
        ok.l = lightness.clamp(0.0, 1.0);
        theme.colors.set(key, Color::from_oklch(ok));
    }
    theme.derived = false;
    theme.preset = None;
    window.shell.prefs.custom = Some(theme);
    window.shell.save_appearance();
}

#[allow(clippy::too_many_arguments)]
fn voice_page(
    window: &RencordWindow,
    view: Entity<RencordWindow>,
    palette: Palette,
    inputs: &[String],
    outputs: &[String],
    chosen_in: Option<&str>,
    chosen_out: Option<&str>,
    jitter: &str,
) -> impl IntoElement {
    let prefs = &window.shell.prefs;
    v_flex()
        .flex_1()
        .px(px(28.))
        .py(px(20.))
        .gap(px(14.))
        .child(page_title("Voice & Video", palette))
        .child(section("Input device", palette))
        .child(device_column(
            view.clone(),
            palette,
            inputs,
            chosen_in,
            true,
        ))
        .child(section("Output device", palette))
        .child(device_column(
            view.clone(),
            palette,
            outputs,
            chosen_out,
            false,
        ))
        .child(mono(jitter, palette.muted))
        .child(section("Streaming", palette))
        .child(hdr_ui::hdr_settings_block(
            palette,
            hdr_ui::HdrSettingsModel {
                send: prefs.hdr_send,
                receive: prefs.hdr_receive,
                curve: prefs.hdr_curve.label().to_string(),
                summary: window.shell.hdr_settings_summary(),
            },
            {
                let view = view.clone();
                move |_, _, cx| {
                    view.update(cx, |window, cx| {
                        window.shell.toggle_hdr_send();
                        cx.notify();
                    });
                }
            },
            {
                let view = view.clone();
                move |_, _, cx| {
                    view.update(cx, |window, cx| {
                        window.shell.toggle_hdr_receive();
                        cx.notify();
                    });
                }
            },
            move |_, _, cx| {
                view.update(cx, |window, cx| {
                    window.shell.cycle_hdr_curve();
                    cx.notify();
                });
            },
        ))
}

fn device_column(
    view: Entity<RencordWindow>,
    palette: Palette,
    devices: &[String],
    chosen: Option<&str>,
    input: bool,
) -> impl IntoElement {
    if devices.is_empty() {
        return v_flex().child(quiet(
            if input {
                "No input devices found."
            } else {
                "No output devices found."
            },
            palette.muted,
            13.,
        ));
    }
    let mut column = v_flex().gap(px(4.));
    for (index, name) in devices.iter().take(6).enumerate() {
        let marked = chosen == Some(name.as_str());
        let owned = name.clone();
        let row_view = view.clone();
        column = column.child(
            div()
                .id(SharedString::from(format!(
                    "{}-device-{index}",
                    if input { "in" } else { "out" }
                )))
                .h(px(32.))
                .px(px(10.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .cursor_pointer()
                .bg(rgb(if marked {
                    palette.selected
                } else {
                    palette.elevated
                }))
                .border_1()
                .border_color(rgb(if marked { palette.accent } else { palette.line }))
                .on_click({
                    let owned = owned.clone();
                    move |_, _, cx| {
                        row_view.update(cx, |window, cx| {
                            let chosen = owned.clone();
                            if input {
                                window.shell.prefs.input_device = Some(chosen);
                            } else {
                                window.shell.prefs.output_device = Some(chosen);
                            }
                            window.reopen_audio();
                            cx.notify();
                        });
                    }
                })
                .child(quiet(&owned, palette.text, 13.)),
        );
    }
    column
}

fn keybinds_page(palette: Palette) -> impl IntoElement {
    let rows = [
        ("Quick switcher", "Ctrl K"),
        ("Settings", "Ctrl ,"),
        ("Channels", "Ctrl B"),
        ("Members", "Ctrl M"),
        ("Next channel", "Alt Down"),
        ("Previous channel", "Alt Up"),
        ("Next server", "Ctrl Alt Down"),
        ("Previous server", "Ctrl Alt Up"),
        ("Close a layer", "Esc"),
    ];
    let mut list = v_flex().gap(px(2.));
    for (action, keys) in rows {
        list = list.child(
            h_flex()
                .h(px(32.))
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(rgb(palette.line))
                .child(quiet(action, palette.text, 13.))
                .child(mono(keys, palette.muted)),
        );
    }
    v_flex()
        .flex_1()
        .px(px(28.))
        .py(px(20.))
        .gap(px(12.))
        .child(page_title("Keybinds", palette))
        .child(list)
}

fn account_page(
    window: &RencordWindow,
    view: Entity<RencordWindow>,
    palette: Palette,
    account: &str,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .px(px(28.))
        .py(px(20.))
        .gap(px(12.))
        .child(page_title("My Account", palette))
        .child(strong(account, palette.text, 18.))
        .child(quiet(
            "Appearance is stored on this machine for this account. It is not written back to Discord.",
            palette.muted,
            13.,
        ))
        .child(switch_row(
            view,
            palette,
            "switch-discord",
            "Follow Discord",
            "Read Discord's light or dark choice once. Rencord never sends a theme back.",
            window.shell.prefs.sync_discord,
            |window| window.shell.prefs.sync_discord = !window.shell.prefs.sync_discord,
        ))
}

fn preview(palette: Palette) -> impl IntoElement {
    v_flex()
        .w(px(300.))
        .h_full()
        .flex_shrink_0()
        .bg(rgb(palette.chat))
        .border_l_1()
        .border_color(rgb(palette.line))
        .child(
            h_flex()
                .h(px(40.))
                .px(px(12.))
                .items_center()
                .border_b_1()
                .border_color(rgb(palette.line))
                .child(strong("# general", palette.text, 14.)),
        )
        .child(
            v_flex()
                .px(px(12.))
                .py(px(10.))
                .gap(px(10.))
                .child(message(
                    palette,
                    "Ada",
                    "That buffer is the whole point.",
                    false,
                ))
                .child(message(palette, "Charles", "Mention ", true))
                .child(
                    v_flex()
                        .p(px(8.))
                        .gap(px(2.))
                        .rounded(px(palette.radius_code.max(4.)))
                        .bg(rgb(palette.code_bg))
                        .border_l_1()
                        .border_color(rgb(palette.accent))
                        .child(mono("fn jitter() {", palette.syntax_keyword))
                        .child(mono("    18", palette.syntax_number)),
                )
                .child(quiet("example.com", palette.link, 13.))
                .child(
                    v_flex()
                        .p(px(8.))
                        .gap(px(2.))
                        .rounded(px(8.))
                        .bg(rgb(palette.voice_bg))
                        .border_1()
                        .border_color(rgb(palette.voice_border))
                        .child(strong("Voice connected", palette.voice_label, 13.))
                        .child(mono("18 ms", palette.voice_meta)),
                )
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .h(px(28.))
                                .px(px(12.))
                                .rounded(px(6.))
                                .flex()
                                .items_center()
                                .bg(rgb(palette.accent))
                                .child(strong("Send", palette.accent_text, 13.)),
                        )
                        .child(
                            div()
                                .h(px(28.))
                                .px(px(12.))
                                .rounded(px(6.))
                                .flex()
                                .items_center()
                                .border_1()
                                .border_color(rgb(palette.line))
                                .child(quiet("Cancel", palette.text, 13.)),
                        ),
                ),
        )
}

fn message(palette: Palette, name: &str, body: &str, mention: bool) -> impl IntoElement {
    v_flex()
        .gap(px(2.))
        .child(strong(name, palette.text, 13.))
        .child(if mention {
            h_flex()
                .gap(px(4.))
                .child(quiet(body, palette.text, 13.))
                .child(
                    div()
                        .px(px(4.))
                        .rounded(px(4.))
                        .bg(rgb(palette.mention_bg))
                        .child(quiet("@Ada", palette.mention_fg, 13.)),
                )
                .into_any_element()
        } else {
            quiet(body, palette.text, 13.).into_any_element()
        })
}

fn footer(
    view: Entity<RencordWindow>,
    palette: Palette,
    code: &Entity<InputState>,
) -> impl IntoElement {
    h_flex()
        .h(px(52.))
        .px(px(16.))
        .gap(px(8.))
        .items_center()
        .flex_shrink_0()
        .border_t_1()
        .border_color(rgb(palette.line))
        .bg(rgb(palette.sidebar))
        .child(primary(
            view.clone(),
            "copy-theme",
            "Copy theme code",
            palette,
            |window, cx| {
                if let Ok(code) = window.shell.prefs.resolved(window.system_dark).to_code() {
                    cx.write_to_clipboard(ClipboardItem::new_string(code));
                    window.shell.note("Copied the theme code.");
                }
                cx.notify();
            },
        ))
        .child(ghost(
            view.clone(),
            "paste-theme",
            "Paste code",
            palette,
            |window, cx| {
                let text = window.theme_code.read(cx).value().to_string();
                window.import_theme_code(&text);
                cx.notify();
            },
        ))
        .child(ghost(
            view.clone(),
            "open-themes",
            "Open themes folder",
            palette,
            |window, cx| {
                let dir = rencord_platform::themes_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
                window
                    .shell
                    .note(format!("Themes folder: {}", dir.display()));
                cx.notify();
            },
        ))
        .child(ghost(
            view.clone(),
            "save-theme",
            "Save as file",
            palette,
            |window, cx| {
                window.save_theme_folder();
                cx.notify();
            },
        ))
        .child(div().flex_1())
        .child(div().w(px(240.)).child(Input::new(code)))
}

#[allow(clippy::too_many_arguments)]
fn slider_row(
    view: Entity<RencordWindow>,
    palette: Palette,
    id: &'static str,
    label: &'static str,
    value: &str,
    index: usize,
    steps: usize,
    apply: impl Fn(&mut RencordWindow, usize) + 'static,
) -> impl IntoElement {
    let apply = std::rc::Rc::new(apply);
    let mut track = h_flex().w(px(220.)).h(px(22.)).items_center();
    for step in 0..steps {
        let button = view.clone();
        let apply = apply.clone();
        let on = step <= index;
        track = track.child(
            div()
                .id(SharedString::from(format!("{id}-{step}")))
                .flex_1()
                .h(px(22.))
                .flex()
                .items_center()
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    button.update(cx, |window, cx| {
                        apply(window, step);
                        window.shell.save_appearance();
                        cx.notify();
                    });
                })
                .child(div().w_full().h(px(4.)).bg(rgb(if on {
                    palette.accent
                } else {
                    palette.line
                }))),
        );
    }
    h_flex()
        .h(px(28.))
        .gap(px(12.))
        .items_center()
        .child(div().w(px(88.)).child(quiet(label, palette.muted, 13.)))
        .child(track)
        .child(div().w(px(56.)).child(mono(value, palette.text)))
}

#[allow(clippy::too_many_arguments)]
fn metric_slider(
    view: Entity<RencordWindow>,
    palette: Palette,
    id: &'static str,
    label: &'static str,
    current: f32,
    steps: &'static [f32],
    format: impl Fn(f32) -> String + 'static,
    apply: impl Fn(&mut RencordWindow, f32) + 'static,
) -> impl IntoElement {
    let index = nearest(steps, current);
    let value = format(steps[index]);
    let apply = std::rc::Rc::new(apply);
    slider_row(
        view,
        palette,
        id,
        label,
        &value,
        index,
        steps.len(),
        move |window, step| {
            apply(window, steps[step]);
        },
    )
}

fn segmented_density(
    view: Entity<RencordWindow>,
    palette: Palette,
    current: Density,
) -> impl IntoElement {
    let mut row = h_flex()
        .h(px(32.))
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(8.))
        .bg(rgb(palette.input))
        .w(px(280.));
    for density in [Density::Compact, Density::Cozy, Density::Spacious] {
        let on = density == current;
        let button = view.clone();
        row = row.child(
            div()
                .id(SharedString::from(format!("density-{}", density.as_str())))
                .flex_1()
                .h_full()
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(rgb(if on { palette.elevated } else { palette.input }))
                .border_1()
                .border_color(rgb(if on { palette.accent } else { palette.input }))
                .on_click(move |_, _, cx| {
                    button.update(cx, |window, cx| {
                        window.shell.prefs.density = density;
                        window.shell.save_appearance();
                        cx.notify();
                    });
                })
                .child(quiet(density.label(), palette.text, 13.)),
        );
    }
    row
}

fn switch_row(
    view: Entity<RencordWindow>,
    palette: Palette,
    id: &'static str,
    label: &'static str,
    detail: &'static str,
    on: bool,
    toggle: impl Fn(&mut RencordWindow) + 'static,
) -> impl IntoElement {
    h_flex()
        .id(id)
        .h(px(36.))
        .gap(px(12.))
        .items_center()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            view.update(cx, |window, cx| {
                toggle(window);
                window.shell.save_appearance();
                cx.notify();
            });
        })
        .child(
            v_flex()
                .flex_1()
                .child(quiet(label, palette.text, 13.))
                .child(quiet(detail, palette.faint, 12.)),
        )
        .child(switch_knob(palette, on))
}

fn switch_knob(palette: Palette, on: bool) -> impl IntoElement {
    h_flex()
        .w(px(36.))
        .h(px(20.))
        .px(px(2.))
        .rounded(px(10.))
        .items_center()
        .bg(rgb(if on { palette.accent } else { palette.input }))
        .border_1()
        .border_color(rgb(if on { palette.accent } else { palette.line }))
        .justify_between()
        .child(if on {
            div().size(px(0.)).into_any_element()
        } else {
            div()
                .size(px(14.))
                .rounded(px(7.))
                .bg(rgb(palette.text))
                .into_any_element()
        })
        .child(if on {
            div()
                .size(px(14.))
                .rounded(px(7.))
                .bg(rgb(palette.accent_text))
                .into_any_element()
        } else {
            div().size(px(0.)).into_any_element()
        })
}

fn primary(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    palette: Palette,
    click: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(32.))
        .px(px(12.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .cursor_pointer()
        .bg(rgb(palette.accent))
        .on_click(move |_, _, cx| {
            view.update(cx, |window, cx| {
                click(window, cx);
            });
        })
        .child(strong(label, palette.accent_text, 13.))
}

fn ghost(
    view: Entity<RencordWindow>,
    id: &str,
    label: &str,
    palette: Palette,
    click: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    let label = label.to_string();
    div()
        .id(SharedString::from(id.to_string()))
        .h(px(32.))
        .px(px(12.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .cursor_pointer()
        .border_1()
        .border_color(rgb(palette.line))
        .bg(rgb(palette.elevated))
        .hover(|style| style.bg(rgb(palette.hover)))
        .on_click(move |_, _, cx| {
            view.update(cx, |window, cx| {
                click(window, cx);
            });
        })
        .child(quiet(&label, palette.text, 13.))
}

fn page_title(label: &'static str, palette: Palette) -> impl IntoElement {
    strong(label, palette.text, 20.)
}

fn section(label: &'static str, palette: Palette) -> impl IntoElement {
    quiet(label, palette.faint, 12.)
}

fn strong(label: &str, color: u32, size: f32) -> impl IntoElement {
    div()
        .text_size(px(size))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(color))
        .line_height(px(size + 4.))
        .child(label.to_string())
}

fn quiet(label: &str, color: u32, size: f32) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .line_height(px(size + 4.))
        .child(label.to_string())
}

fn mono(label: &str, color: u32) -> impl IntoElement {
    div()
        .font_family("JetBrains Mono")
        .text_size(px(12.))
        .text_color(rgb(color))
        .child(label.to_string())
}

fn index_of(steps: &[f32], current: f32) -> usize {
    nearest(steps, current)
}

fn nearest(steps: &[f32], current: f32) -> usize {
    steps
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            (*left - current)
                .abs()
                .partial_cmp(&(*right - current).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}
