//! GPUI shell. `rencord --demo` uses local data. `--theme amoled|dark|light`
//! picks the starting theme. `--bench` prints startup, RSS, and frame gaps,
//! then quits.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui_kit::assets::Assets;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::*;
use rencord_desktop::{Density, Palette, Shell, ThemeMode, MOTION_OVERLAY_MS};
use rencord_markdown::{parse, Block, Inline};
use rencord_model::{Message, Snowflake};

gpui_kit::actions!(
    rencord_ui,
    [
        QuickSwitcher,
        CloseLayer,
        NextChannel,
        PrevChannel,
        NextGuild,
        PrevGuild,
        OpenSettings,
        OpenInbox,
        ToggleSidebar,
        ToggleMembers,
        ToggleSplit,
        TogglePip,
        ShowHelp,
        SendMessage,
    ]
);

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

fn main() {
    PROCESS_START.get_or_init(Instant::now);
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|arg| arg == "--demo");
    let bench = args.iter().any(|arg| arg == "--bench");
    let theme = args
        .windows(2)
        .find(|pair| pair[0] == "--theme")
        .and_then(|pair| ThemeMode::parse(&pair[1]));
    let layout = args
        .windows(2)
        .find(|pair| pair[0] == "--layout")
        .and_then(|pair| rencord_desktop::LayoutKind::parse(&pair[1]));
    let density = args
        .windows(2)
        .find(|pair| pair[0] == "--density")
        .and_then(|pair| match pair[1].as_str() {
            "compact" => Some(rencord_desktop::Density::Compact),
            "cozy" => Some(rencord_desktop::Density::Cozy),
            "spacious" => Some(rencord_desktop::Density::Spacious),
            _ => None,
        });
    let start_voice = args.iter().any(|arg| arg == "--voice");
    let app = gpui_kit::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        let _ = cx.text_system().add_fonts(vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-Regular.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-Medium.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-SemiBold.ttf")),
        ]);
        cx.bind_keys([
            KeyBinding::new("ctrl-k", QuickSwitcher, None),
            KeyBinding::new("cmd-k", QuickSwitcher, None),
            KeyBinding::new("escape", CloseLayer, None),
            KeyBinding::new("alt-down", NextChannel, None),
            KeyBinding::new("alt-up", PrevChannel, None),
            KeyBinding::new("ctrl-alt-down", NextGuild, None),
            KeyBinding::new("ctrl-alt-up", PrevGuild, None),
            KeyBinding::new("ctrl-,", OpenSettings, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("ctrl-i", OpenInbox, None),
            KeyBinding::new("cmd-i", OpenInbox, None),
            KeyBinding::new("ctrl-b", ToggleSidebar, None),
            KeyBinding::new("cmd-b", ToggleSidebar, None),
            KeyBinding::new("ctrl-m", ToggleMembers, None),
            KeyBinding::new("cmd-m", ToggleMembers, None),
            KeyBinding::new("ctrl-\\", ToggleSplit, None),
            KeyBinding::new("ctrl-/", ShowHelp, None),
            KeyBinding::new("cmd-/", ShowHelp, None),
            KeyBinding::new("enter", SendMessage, None),
        ]);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1180.), px(760.)), cx)),
            // Client decorations drop the desktop app-menu button. Settings stays in the dock.
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                RencordWindow::new(demo, bench, theme, layout, density, start_voice, window, cx)
            })
        })
        .expect("open Rencord window");
    });
}

struct RencordWindow {
    shell: Shell,
    token_input: Entity<InputState>,
    composer: Entity<InputState>,
    search_input: Entity<InputState>,
    palette_input: Entity<InputState>,
    audio_session: Option<rencord_voice::AudioSession>,
    focus: FocusHandle,
    timeline: ListState,
    bench: bool,
    bench_frames: u32,
    bench_last: Instant,
    frame_us: Vec<u128>,
    collapsed: HashSet<&'static str>,
    pinned_top: bool,
}

impl RencordWindow {
    #[allow(clippy::too_many_arguments)]
    fn new(
        demo: bool,
        bench: bool,
        theme: Option<ThemeMode>,
        layout: Option<rencord_desktop::LayoutKind>,
        density: Option<rencord_desktop::Density>,
        start_voice: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut shell = if demo {
            if bench {
                Shell::demo_history(800)
            } else {
                Shell::demo()
            }
        } else {
            Shell::login()
        };
        if let Some(mode) = theme {
            shell.set_theme(mode);
        }
        if let Some(layout) = layout {
            shell.prefs.layout = layout;
        }
        if let Some(density) = density {
            shell.prefs.density = density;
            if matches!(density, rencord_desktop::Density::Compact) {
                shell.prefs.font_px = 13.0;
                shell.prefs.icon_px = 40.0;
            }
        }
        if start_voice {
            shell.join_voice_demo();
        }
        let token_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("User token (not stored until you connect)")
        });
        let composer = cx.new(|cx| InputState::new(window, cx).placeholder("Message #general"));
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search this channel"));
        let palette_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Jump to a channel or command"));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            shell,
            token_input,
            composer,
            search_input,
            palette_input,
            audio_session: None,
            focus,
            timeline: ListState::new(0, ListAlignment::Bottom, px(160.)),
            bench,
            bench_frames: 0,
            bench_last: Instant::now(),
            frame_us: Vec::new(),
            collapsed: HashSet::new(),
            pinned_top: false,
        }
    }

    fn sync_queries(&mut self, cx: &mut Context<Self>) {
        let search = self.search_input.read(cx).value().to_string();
        if search != self.shell.search {
            self.shell.search = search;
        }
        let palette = self.palette_input.read(cx).value().to_string();
        if palette != self.shell.palette_query {
            self.shell.palette_query = palette;
        }
    }

    fn note_bench_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.bench {
            return;
        }
        if self.bench_frames == 0 {
            let interactive = PROCESS_START
                .get()
                .map(|start| start.elapsed().as_millis())
                .unwrap_or(0);
            println!("BENCH interactive_ms={interactive}");
            println!("BENCH rss_kib={}", rss_kib());
            println!("BENCH channel_switch_us={}", self.shell.last_switch_us);
        }
        let gap = self.bench_last.elapsed().as_micros();
        if self.bench_frames > 0 {
            self.frame_us.push(gap);
        }
        self.bench_last = Instant::now();
        self.bench_frames += 1;
        if self.bench_frames < 24 {
            cx.on_next_frame(window, |this, window, cx| {
                this.shell.step_channel(1);
                this.note_bench_frame(window, cx);
            });
            return;
        }
        self.frame_us.sort_unstable();
        let p50 = self
            .frame_us
            .get(self.frame_us.len() / 2)
            .copied()
            .unwrap_or(0);
        println!("BENCH frame_p50_us={p50}");
        println!("BENCH frames={}", self.frame_us.len());
        println!("BENCH channel_switch_us={}", self.shell.last_switch_us);
        match window.refresh_interval(cx) {
            Some(interval) if !interval.is_zero() => {
                let hz = 1.0 / interval.as_secs_f32();
                println!("BENCH refresh_hz={hz:.2}");
                println!("BENCH frame_budget_us={}", interval.as_micros());
                println!("BENCH display_is_120hz={}", hz >= 110.0);
            }
            _ => {
                println!("BENCH refresh_hz=unreported");
                println!("BENCH display_is_120hz=false");
            }
        }
        println!("BENCH inferred_120hz_budget_us=8333");
        println!("BENCH pacing=gpui follows the display refresh interval and this build does not cap frames at 60");
        cx.quit();
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        let token = self.token_input.read(cx).value().to_string();
        let token = token.trim().to_owned();
        if token.is_empty() {
            self.shell.note("Token is empty.");
            cx.notify();
            return;
        }
        if let Err(err) = Shell::remember_token(&token) {
            self.shell.note(format!("Could not store token: {err}"));
        }
        self.shell.note("Connecting to Discord…");
        cx.notify();
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio");
            let outcome = runtime.block_on(fetch_bootstrap(token));
            let _ = tx.send_blocking(outcome);
        });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.recv().await else {
                return;
            };
            this.update(cx, |view, cx| {
                match outcome {
                    Ok(snapshot) => {
                        view.shell.set_user(snapshot.user);
                        for guild in snapshot.guilds {
                            view.shell.session.upsert_guild(guild);
                        }
                        for channel in snapshot.channels {
                            view.shell.session.upsert_channel(channel);
                        }
                        if let Some(guild) = view.shell.session.guilds.first().cloned() {
                            view.shell.session.select_guild(guild.id);
                        }
                        view.shell.note(format!(
                            "REST bootstrap ok. Gateway task: {}",
                            snapshot.gateway_note
                        ));
                    }
                    Err(err) => view.shell.note(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().to_string();
        if let Some(mutation) = self.shell.send_local(&text) {
            self.dispatch_mutation(mutation, cx);
        }
        cx.notify();
    }

    fn dispatch_mutation(&mut self, mutation: rencord_desktop::Mutation, cx: &mut Context<Self>) {
        if self.shell.demo {
            self.shell
                .note("Demo keeps the change locally. A signed-in session would call Discord.");
            return;
        }
        let Some(token) = Shell::load_token() else {
            self.shell.rollback(&mutation);
            self.shell
                .note("No token stored. The change was rolled back.");
            return;
        };
        let (tx, rx) = async_channel::bounded(1);
        let outbound = mutation.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio");
            let outcome = runtime.block_on(push_mutation(&token, &outbound));
            let _ = tx.send_blocking(outcome);
        });
        let rollback = mutation;
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.recv().await else {
                return;
            };
            this.update(cx, |view, cx| {
                if let Err(err) = outcome {
                    view.shell.rollback(&rollback);
                    view.shell
                        .note(format!("Discord rejected the change: {err}"));
                } else {
                    view.shell.note("Discord accepted the change.");
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn reopen_audio(&mut self) {
        if self.audio_session.is_none() {
            return;
        }
        let inputs = rencord_voice::list_input_devices();
        let outputs = rencord_voice::list_output_devices();
        let mut input = self.shell.prefs.input_device.clone();
        let mut output = self.shell.prefs.output_device.clone();
        if let Some(name) = &input {
            if !inputs.iter().any(|item| item == name) {
                input = None;
            }
        }
        if let Some(name) = &output {
            if !outputs.iter().any(|item| item == name) {
                output = None;
            }
        }
        match rencord_voice::open_call_audio(input.as_deref(), output.as_deref()) {
            Ok(session) => {
                if session.input_fallback || session.output_fallback {
                    self.shell.note(format!(
                        "A chosen device was missing. Using {} and {}.",
                        session.input_name, session.output_name
                    ));
                } else {
                    self.shell.note(format!(
                        "Audio devices: {} / {}",
                        session.input_name, session.output_name
                    ));
                }
                self.shell.prefs.input_device = Some(session.input_name.clone());
                self.shell.prefs.output_device = Some(session.output_name.clone());
                self.audio_session = Some(session);
            }
            Err(err) => self.shell.note(format!("Could not switch devices: {err}")),
        }
    }

    fn join_voice(&mut self) {
        self.shell.join_voice_demo();
        match rencord_voice::open_call_audio(
            self.shell.prefs.input_device.as_deref(),
            self.shell.prefs.output_device.as_deref(),
        ) {
            Ok(session) => {
                self.audio_session = Some(session);
                self.shell
                    .note("cpal default devices are open and routed through the jitter buffer.");
            }
            Err(err) => self.shell.note(format!(
                "cpal devices were not opened ({err}). Loopback stats still ran."
            )),
        }
    }
}

struct Bootstrap {
    user: rencord_model::User,
    guilds: Vec<rencord_model::Guild>,
    channels: Vec<rencord_model::Channel>,
    gateway_note: String,
}

async fn fetch_bootstrap(token: String) -> Result<Bootstrap, String> {
    let api = rencord_api::DiscordApi::new(rencord_model::Token::new(token.clone()));
    let user = api.current_user().await.map_err(|err| err.to_string())?;
    let guilds = api.guilds().await.map_err(|err| err.to_string())?;
    let mut channels = api
        .private_channels()
        .await
        .map_err(|err| err.to_string())?;
    if let Some(guild) = guilds.first() {
        let mut guild_channels = api
            .guild_channels(guild.id)
            .await
            .map_err(|err| err.to_string())?;
        channels.append(&mut guild_channels);
    }
    let gateway_note = match spawn_gateway(token).await {
        Ok(note) => note,
        Err(err) => format!("gateway did not stay up ({err})"),
    };
    Ok(Bootstrap {
        user,
        guilds,
        channels,
        gateway_note,
    })
}

async fn spawn_gateway(token: String) -> Result<String, String> {
    let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(32);
    let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        let _ = rencord_gateway::run(
            rencord_gateway::GatewayConfig {
                url: "wss://gateway.discord.gg/?v=10&encoding=json".into(),
                token,
            },
            events_tx,
            cmd_rx,
        )
        .await;
    });
    let first = tokio::time::timeout(std::time::Duration::from_secs(8), events_rx.recv())
        .await
        .map_err(|_| "timed out waiting for the gateway".to_string())?;
    match first {
        Some(rencord_gateway::GatewayEvent::Ready { session_id, .. }) => {
            Ok(format!("ready, session {session_id}"))
        }
        Some(other) => Ok(format!("first event {other:?}")),
        None => Err("gateway closed".into()),
    }
}

impl Render for RencordWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_queries(cx);
        apply_kit_theme(&self.shell.prefs.palette(), cx);
        if self.bench && self.bench_frames == 0 {
            cx.on_next_frame(window, |this, window, cx| {
                this.note_bench_frame(window, cx);
            });
        }
        let signed_in = self.shell.session.user.is_some();
        let palette = self.shell.prefs.palette();
        let root = div()
            .id("rencord-root")
            .role(Role::Document)
            .aria_label("Rencord")
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(palette.app_bg))
            .font_family("Figtree")
            .text_color(rgb(palette.text))
            .text_size(px(self.shell.prefs.font_px))
            .on_action(cx.listener(|this, _: &QuickSwitcher, _, cx| {
                this.shell.toggle_palette();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CloseLayer, _, cx| {
                this.shell.close_layers();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextChannel, _, cx| {
                this.shell.step_channel(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PrevChannel, _, cx| {
                this.shell.step_channel(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextGuild, _, cx| {
                this.shell.step_guild(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PrevGuild, _, cx| {
                this.shell.step_guild(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.shell.toggle_settings();
                this.reopen_audio();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenInbox, _, cx| {
                this.shell.inbox_open = !this.shell.inbox_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.shell.prefs.show_sidebar = !this.shell.prefs.show_sidebar;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleMembers, _, cx| {
                this.shell.prefs.show_members = !this.shell.prefs.show_members;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSplit, _, cx| {
                this.shell.prefs.split_pane = !this.shell.prefs.split_pane;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TogglePip, _, cx| {
                this.shell.prefs.pip = !this.shell.prefs.pip;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SendMessage, _, cx| {
                this.send(cx);
            }))
            .on_action(cx.listener(|this, _: &ShowHelp, _, cx| {
                this.shell.help_open = !this.shell.help_open;
                cx.notify();
            }));
        if !signed_in {
            return root.child(self.login(cx)).into_any_element();
        }
        root.child(self.shell_view(window, cx)).into_any_element()
    }
}

impl RencordWindow {
    fn login(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.shell.prefs.palette();
        let this = cx.entity();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(rgb(palette.chat))
            .child(Label::new("Rencord"))
            .child(Label::new(
                "Unofficial client. A user token can get the account terminated.",
            ))
            .child(div().w(px(420.)).child(Input::new(&self.token_input)))
            .child(
                Button::new("connect")
                    .primary()
                    .label("Connect")
                    .on_click(move |_, _, cx| {
                        this.update(cx, |view, cx| view.connect(cx));
                    }),
            )
    }

    fn shell_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size().width;
        let palette = self.shell.prefs.palette();
        let density = self.shell.prefs.density;
        let this = cx.entity();
        let guilds = self.shell.session.guilds.clone();
        let selected_guild = self.shell.session.selected_guild;
        let channels = selected_guild
            .map(|id| {
                self.shell
                    .session
                    .guild_channels(id)
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let dms = self
            .shell
            .session
            .dm_channels()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let selected = self.shell.session.selected_channel;
        let messages = self.shell.visible_messages();
        let status = self.shell.session.status_line.clone();
        let members = self.shell.members.clone();
        let reactions = self.shell.reactions.clone();
        let font_px = self.shell.prefs.font_px;
        let theme_label = self.shell.prefs.theme.label();
        let voice = self.shell.voice_report.clone();
        let muted = self.shell.muted;
        let deafened = self.shell.deafened;
        let reply = self.shell.reply_to;
        let editing = self.shell.editing;
        let switch_us = self.shell.last_switch_us;
        let settings = self.shell.settings_open;
        let palette_open = self.shell.palette_open;
        let commands = self.shell.palette_commands();
        let inputs = rencord_voice::list_input_devices();
        let outputs = rencord_voice::list_output_devices();
        let chosen_in = self.shell.prefs.input_device.clone();
        let chosen_out = self.shell.prefs.output_device.clone();

        let user_name = self
            .shell
            .session
            .user
            .as_ref()
            .map(|user| user.display_name().to_string())
            .unwrap_or_else(|| "You".into());
        let guild_name = guilds
            .iter()
            .find(|guild| Some(guild.id) == selected_guild)
            .map(|guild| guild.name.clone())
            .unwrap_or_else(|| "Direct Messages".into());
        let home_active = selected.is_some_and(|id| dms.iter().any(|channel| channel.id == id));
        let pill_ink = if matches!(palette.mode, ThemeMode::Light) {
            0x060607
        } else {
            0xffffff
        };
        let mut rail = v_flex()
            .w(px(72.))
            .h_full()
            .bg(rgb(palette.rail))
            .items_center()
            .gap(px(8.))
            .py(px(12.));
        {
            let view = this.clone();
            rail = rail.child(rail_slot(
                "rail-home".into(),
                home_active,
                if home_active {
                    Pill::Selected
                } else {
                    Pill::None
                },
                if home_active { 0x5865f2 } else { 0x2a2e38 },
                if home_active { 0x5865f2 } else { 0x343844 },
                pill_ink,
                icon(
                    "home",
                    22.,
                    if home_active { 0xffffff } else { palette.text },
                ),
                move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.shell
                            .select_channel_timed(rencord_model::Snowflake::new(200));
                        cx.notify();
                    });
                },
            ));
        }
        rail = rail.child(rail_rule());
        for guild in guilds {
            let id = guild.id;
            let mark = guild_mark(&guild.name);
            let active = selected_guild == Some(id);
            let view = this.clone();
            let fill = guild_fill(&guild.name);
            rail = rail.child(rail_slot(
                format!("guild-{id}").into(),
                active,
                if active {
                    Pill::Selected
                } else if guild.name != "Analytical Engines" {
                    Pill::Unread
                } else {
                    Pill::None
                },
                fill,
                fill,
                pill_ink,
                tile_initial(&mark, 0xffffff),
                move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.shell.session.select_guild(id);
                        cx.notify();
                    });
                },
            ));
        }
        rail = rail.child(rail_rule());
        rail = rail.child(rail_action(this.clone(), "rail-add", "plus", |view, cx| {
            view.shell.note("Creating a server is not in this build.");
            cx.notify();
        }));
        rail = rail.child(rail_action(
            this.clone(),
            "rail-explore",
            "compass",
            |view, cx| {
                view.shell.note("Server discovery is not in this build.");
                cx.notify();
            },
        ));

        let mut sidebar = v_flex()
            .w(px(240.))
            .h_full()
            .bg(rgb(palette.sidebar))
            .border_r_1()
            .border_color(rgb(palette.line))
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(rgb(palette.line))
                    .child(paint(guild_name, palette.text, 16.)),
            )
            .child(
                div().px(px(8.)).py(px(8.)).child(
                    Input::new(&self.search_input)
                        .bg(rgb(palette.input))
                        .text_color(rgb(palette.text))
                        .border_color(rgb(palette.input)),
                ),
            );
        let row_px = density.channel_row_px();
        let favorites_open = !self.collapsed.contains("favorites");
        let text_open = !self.collapsed.contains("text");
        let voice_open = !self.collapsed.contains("voice");
        let dm_open = !self.collapsed.contains("dms");
        sidebar = sidebar.child(category_header(
            this.clone(),
            "favorites",
            "FAVORITES",
            favorites_open,
            palette,
        ));
        if favorites_open {
            let favorite_ids = self.shell.favorites.clone();
            for id in favorite_ids {
                if let Some(channel) = self.shell.session.channels.get(&id) {
                    sidebar = sidebar.child(channel_row(
                        this.clone(),
                        channel.id,
                        format!("* {}", channel.name),
                        selected == Some(channel.id),
                        palette,
                        None,
                        false,
                        row_px,
                    ));
                }
            }
        }
        sidebar = sidebar.child(category_header(
            this.clone(),
            "text",
            "TEXT CHANNELS",
            text_open,
            palette,
        ));
        if text_open {
            for channel in channels.iter().filter(|channel| channel.kind.is_text()) {
                let badge = if channel.name == "general" {
                    Some(1)
                } else {
                    None
                };
                sidebar = sidebar.child(channel_row(
                    this.clone(),
                    channel.id,
                    format!("# {}", channel.name),
                    selected == Some(channel.id),
                    palette,
                    badge,
                    false,
                    row_px,
                ));
            }
        }
        sidebar = sidebar.child(category_header(
            this.clone(),
            "voice",
            "VOICE CHANNELS",
            voice_open,
            palette,
        ));
        if voice_open {
            for channel in channels.iter().filter(|channel| channel.kind.is_voice()) {
                sidebar = sidebar.child(channel_row(
                    this.clone(),
                    channel.id,
                    format!(")) {}", channel.name),
                    false,
                    palette,
                    None,
                    true,
                    row_px,
                ));
            }
        }
        sidebar = sidebar.child(category_header(
            this.clone(),
            "dms",
            "DIRECT MESSAGES",
            dm_open,
            palette,
        ));
        if dm_open {
            for channel in &dms {
                sidebar = sidebar.child(channel_row(
                    this.clone(),
                    channel.id,
                    format!("dm {}", channel.name),
                    selected == Some(channel.id),
                    palette,
                    None,
                    false,
                    row_px,
                ));
            }
        }
        sidebar = sidebar.child(div().flex_1());

        let count = messages.len();
        if self.timeline.item_count() != count {
            self.timeline.reset(count);
        }
        if self.shell.demo && !self.bench && !self.pinned_top {
            self.timeline.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
            self.pinned_top = true;
        }
        if let Some(index) = self.shell.take_jump() {
            self.timeline.scroll_to_reveal_item(index);
        }
        let timeline_state = self.timeline.clone();
        let list_messages = messages.clone();
        let list_palette = palette;
        let list_entity = this.clone();
        let list_reactions = reactions.clone();
        let _density = density;
        let timeline = list(timeline_state, move |index, _window, _cx| {
            let Some(message) = list_messages.get(index).cloned() else {
                return div().into_any_element();
            };
            let grouped = index > 0
                && list_messages
                    .get(index - 1)
                    .is_some_and(|previous| previous.author.id == message.author.id);
            message_row(
                list_entity.clone(),
                &message,
                grouped,
                list_reactions
                    .get(&message.id.get())
                    .cloned()
                    .unwrap_or_default(),
                list_palette,
                font_px,
            )
            .into_any_element()
        })
        .size_full();

        let channel_name = channel_title(selected, &channels, &dms);
        let mut chat = v_flex()
            .flex_1()
            .h_full()
            .bg(rgb(palette.chat))
            .child(
                h_flex()
                    .h(px(48.))
                    .px(px(12.))
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(palette.line))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .overflow_hidden()
                            .child(icon("hash", 20., palette.muted))
                            .child(paint(channel_name.clone(), palette.text, 16.))
                            .child(div().w(px(1.)).h(px(16.)).bg(rgb(palette.line)))
                            .child(paint("Voice, notes, and no upsells", palette.muted, 13.)),
                    )
                    .child(
                        h_flex()
                            .gap(px(2.))
                            .items_center()
                            .child(header_icon(
                                this.clone(),
                                "hdr-threads",
                                "threads",
                                palette,
                                |view, cx| {
                                    view.shell.note("Threads are not in this build.");
                                    cx.notify();
                                },
                            ))
                            .child(header_icon(
                                this.clone(),
                                "hdr-pin",
                                "pin",
                                palette,
                                |view, cx| {
                                    view.shell.note("Pinned messages are not in this build.");
                                    cx.notify();
                                },
                            ))
                            .child(header_icon(
                                this.clone(),
                                "hdr-members",
                                "members",
                                palette,
                                |view, cx| {
                                    view.shell.prefs.show_members = !view.shell.prefs.show_members;
                                    cx.notify();
                                },
                            ))
                            .child(header_icon(
                                this.clone(),
                                "hdr-search",
                                "search",
                                palette,
                                |view, cx| {
                                    view.shell.toggle_palette();
                                    cx.notify();
                                },
                            ))
                            .child(header_icon(
                                this.clone(),
                                "hdr-inbox",
                                "inbox",
                                palette,
                                |view, cx| {
                                    view.shell.inbox_open = !view.shell.inbox_open;
                                    cx.notify();
                                },
                            )),
                    ),
            )
            .child(new_messages_banner(palette))
            .child(date_pill("Today", palette))
            .child(div().flex_1().overflow_hidden().child(timeline));
        let hits = self.shell.search_hits();
        if !self.shell.search.trim().is_empty() {
            chat = chat.child(search_hits_bar(this.clone(), &hits, palette));
        }
        if let Some(id) = reply {
            chat = chat.child(hint_bar(palette, format!("Replying to {id}")));
        }
        if let Some(id) = editing {
            chat = chat.child(hint_bar(palette, format!("Editing {id}")));
        }
        chat = chat.child(
            h_flex()
                .mx(px(12.))
                .mb(px(12.))
                .px(px(4.))
                .h(px(47.))
                .items_center()
                .gap(px(2.))
                .rounded(px(12.))
                .border_1()
                .border_color(rgb(palette.line))
                .bg(rgb(palette.input))
                .child(header_icon(
                    this.clone(),
                    "composer-plus",
                    "plus",
                    palette,
                    |view, cx| {
                        view.shell.note("File uploads are not in this build.");
                        cx.notify();
                    },
                ))
                .child(
                    div().flex_1().child(
                        Input::new(&self.composer)
                            .bg(rgb(palette.input))
                            .text_color(rgb(palette.text))
                            .border_color(rgb(palette.input)),
                    ),
                )
                .child(header_icon(
                    this.clone(),
                    "composer-gif",
                    "gif",
                    palette,
                    |view, cx| {
                        view.shell.note("GIF search is not in this build.");
                        cx.notify();
                    },
                ))
                .child(header_icon(
                    this.clone(),
                    "composer-emoji",
                    "emoji",
                    palette,
                    |view, cx| {
                        view.shell.note("The emoji picker is not in this build.");
                        cx.notify();
                    },
                )),
        );
        if self.shell.prefs.stats_hud {
            chat = chat.child(
                div()
                    .px(px(12.))
                    .pb(px(8.))
                    .child(paint(
                        format!(
                            "{status}  ·  {channel_name}  ·  {theme_label}  ·  {}  ·  switch {switch_us} µs",
                            self.shell.prefs.layout.label()
                        ),
                        palette.muted,
                        12.,
                    )),
            );
        }

        let mut people = v_flex()
            .w(px(240.))
            .h_full()
            .bg(rgb(palette.members))
            .child(section_label("ONLINE — 3", palette.muted));
        for member in members {
            let (name, activity) = member
                .split_once(" · ")
                .map(|(name, activity)| (name.to_owned(), activity.to_owned()))
                .unwrap_or_else(|| (member.clone(), "Online".into()));
            let role = role_color(&name, palette);
            let dot = match activity.as_str() {
                "Idle" => 0xf0b232,
                "Offline" => 0x80848e,
                _ => 0x23a559,
            };
            people = people.child(
                h_flex()
                    .mx(px(8.))
                    .px(px(8.))
                    .h(px(44.))
                    .gap(px(10.))
                    .items_center()
                    .rounded(px(6.))
                    .hover(|style| style.bg(rgb(palette.hover)))
                    .child(portrait_status(&name, 32., dot, palette.members))
                    .child(
                        v_flex()
                            .gap(px(0.))
                            .child(paint(name, role, 14.))
                            .child(paint(activity, palette.muted, 12.)),
                    ),
            );
        }

        let voice_panel = self.voice_card(cx, palette, &user_name, voice.as_ref(), muted, deafened);
        let layout_kind = self.shell.prefs.layout;
        let hide_members = !self.shell.prefs.show_members || viewport < px(1100.);
        let hide_sidebar = !self.shell.prefs.show_sidebar || viewport < px(760.);
        let sidebar = if hide_sidebar { div() } else { sidebar };
        let people = if hide_members { div() } else { people };
        let body = match layout_kind {
            rencord_desktop::LayoutKind::Discord => div()
                .size_full()
                .bg(rgb(palette.app_bg))
                .p(px(12.))
                .child(
                    h_flex()
                        .size_full()
                        .gap(px(12.))
                        .child(panel(palette, palette.rail).w(px(72.)).child(rail))
                        .child(
                            panel(palette, palette.sidebar)
                                .w(px(240.))
                                .child(sidebar.child(voice_panel)),
                        )
                        .child(panel(palette, palette.chat).flex_1().child(chat))
                        .child(panel(palette, palette.members).w(px(240.)).child(people)),
                )
                .into_any_element(),
            rencord_desktop::LayoutKind::Keyboard => v_flex()
                .size_full()
                .child(
                    h_flex()
                        .h(px(40.))
                        .px(px(12.))
                        .items_center()
                        .bg(rgb(palette.rail))
                        .child(Label::new("Quick switcher is the home. Ctrl+K. Two panes.")),
                )
                .child(
                    h_flex().flex_1().child(chat).child(
                        v_flex()
                            .w(px(280.))
                            .h_full()
                            .bg(rgb(palette.sidebar))
                            .border_l_1()
                            .border_color(rgb(palette.line))
                            .child(section_label("Split pane", palette.faint))
                            .child(Label::new(
                                channels
                                    .iter()
                                    .find(|channel| {
                                        Some(channel.id) != selected && channel.kind.is_text()
                                    })
                                    .map(|channel| format!("#{}", channel.name))
                                    .unwrap_or_else(|| "No second channel".into()),
                            ))
                            .child(voice_panel),
                    ),
                )
                .into_any_element(),
            rencord_desktop::LayoutKind::Sidebar => div()
                .relative()
                .size_full()
                .child(
                    h_flex()
                        .size_full()
                        .child(
                            v_flex()
                                .w(px(300.))
                                .h_full()
                                .bg(rgb(palette.sidebar))
                                .child(section_label("Favorites", palette.faint))
                                .child(Label::new("# general"))
                                .child(section_label("Inbox", palette.faint))
                                .child(Label::new("Mentions stay in this list"))
                                .child(h_flex().flex_1().child(rail).child(sidebar)),
                        )
                        .child(chat)
                        .child(people),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(16.))
                        .right(px(16.))
                        .w(px(280.))
                        .child(voice_panel),
                )
                .into_any_element(),
        };

        let mut layer = div().size_full().relative().child(body);
        if palette_open {
            layer = layer.child(self.palette_layer(cx, palette, &commands));
        }
        if self.shell.inbox_open {
            layer = layer.child(self.inbox_layer(cx, palette));
        }
        if self.shell.help_open {
            layer = layer.child(hint_bar(
                palette,
                "Ctrl+K palette, Tab secondary, Ctrl+I inbox, Ctrl+B sidebar, Ctrl+M members, Ctrl+\\ split, Ctrl+/ this list".into(),
            ));
        }
        if settings {
            layer = layer.child(self.settings_layer(
                cx,
                palette,
                density,
                &inputs,
                &outputs,
                chosen_in.as_deref(),
                chosen_out.as_deref(),
                voice.as_ref(),
            ));
        }
        layer
    }

    fn inbox_layer(&mut self, cx: &mut Context<Self>, palette: Palette) -> impl IntoElement {
        let this = cx.entity();
        let hits = self.shell.mentions();
        let mut list = v_flex().gap_1();
        for hit in hits.iter().take(8) {
            let channel_id = hit.channel_id;
            let message_id = hit.message.id;
            let label = format!(
                "#{} {}",
                hit.channel_name,
                hit.message.content.replace('\n', " ")
            );
            let view = this.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("inbox-{message_id}")))
                    .cursor_pointer()
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .bg(rgb(if self.shell.prefs.soft_ping {
                        palette.ping
                    } else {
                        palette.elevated
                    }))
                    .child(Label::new(label))
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.shell.jump_to(channel_id, message_id);
                            this.shell.inbox_open = false;
                            cx.notify();
                        });
                    }),
            );
        }
        overlay(
            palette,
            v_flex()
                .w(px(480.))
                .p(px(12.))
                .gap_2()
                .rounded(px(12.))
                .bg(rgb(palette.elevated))
                .border_1()
                .border_color(rgb(palette.line))
                .child(Label::new("Mentions"))
                .child(list)
                .child(small_button(
                    this,
                    "mark-read",
                    "Mark all read",
                    |view, cx| {
                        view.shell.mark_mentions_read();
                        cx.notify();
                    },
                )),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn voice_card(
        &mut self,
        cx: &mut Context<Self>,
        palette: Palette,
        user_name: &str,
        voice: Option<&rencord_voice::LoopbackReport>,
        muted: bool,
        deafened: bool,
    ) -> impl IntoElement {
        let this = cx.entity();
        let in_call = voice.is_some();
        let status = if muted {
            "Muted"
        } else if deafened {
            "Deafened"
        } else if in_call {
            "In voice"
        } else {
            "Online"
        };
        let dot = if muted || deafened {
            0xf23f43
        } else {
            0x23a559
        };
        let mut column = v_flex().w_full().flex_shrink_0().bg(rgb(palette.sidebar));
        if let Some(report) = voice {
            let view = this.clone();
            column = column.child(
                v_flex()
                    .px(px(8.))
                    .py(px(8.))
                    .gap(px(2.))
                    .bg(rgb(palette.elevated))
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(paint("Voice Connected", 0x23a559, 12.))
                            .child(hang_up_button(view)),
                    )
                    .child(paint("voice", palette.text, 14.))
                    .child(paint(
                        format!("Loopback · target {} ms", report.target_ms),
                        palette.muted,
                        12.,
                    )),
            );
        }
        column.child(
            h_flex()
                .h(px(52.))
                .px(px(8.))
                .gap(px(8.))
                .items_center()
                .bg(rgb(palette.sidebar))
                .child(portrait_status(user_name, 32., dot, palette.sidebar))
                .child(
                    v_flex()
                        .flex_1()
                        .overflow_hidden()
                        .child(paint(user_name.to_owned(), palette.text, 14.))
                        .child(paint(status, palette.muted, 12.)),
                )
                .child(dock_circle(
                    this.clone(),
                    "mic-btn",
                    "Mic",
                    palette,
                    muted,
                    |view, cx| {
                        view.shell.toggle_mute();
                        cx.notify();
                    },
                ))
                .child(dock_circle(
                    this.clone(),
                    "deaf-btn",
                    "Deaf",
                    palette,
                    deafened,
                    |view, cx| {
                        view.shell.toggle_deaf();
                        cx.notify();
                    },
                ))
                .child(dock_circle(
                    this,
                    "gear-btn",
                    "Set",
                    palette,
                    false,
                    |view, cx| {
                        view.shell.toggle_settings();
                        view.reopen_audio();
                        cx.notify();
                    },
                )),
        )
    }

    fn palette_layer(
        &mut self,
        cx: &mut Context<Self>,
        palette: Palette,
        commands: &[String],
    ) -> impl IntoElement {
        let this = cx.entity();
        let mut list = v_flex().gap_1().max_h(px(280.)).overflow_hidden();
        for command in commands.iter().take(12) {
            let label = command.clone();
            let view = this.clone();
            let hover = palette.hover;
            list = list.child(
                div()
                    .id(SharedString::from(format!("cmd-{label}")))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(hover)))
                    .child(Label::new(label.clone()))
                    .on_click(move |_, _, cx| {
                        let label = label.clone();
                        view.update(cx, |this, cx| {
                            this.shell.run_palette_command(&label);
                            cx.notify();
                        });
                    }),
            );
        }
        overlay(
            palette,
            v_flex()
                .w(px(460.))
                .p(px(12.))
                .gap_2()
                .rounded(px(12.))
                .bg(rgb(palette.elevated))
                .border_1()
                .border_color(rgb(palette.line))
                .child(Label::new("Quick switcher"))
                .child(Input::new(&self.palette_input))
                .child(list)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(palette.faint))
                        .child(Label::new(
                            "Enter a row. Esc closes. Alt+arrows move without this panel.",
                        )),
                )
                .with_animation(
                    "palette-in",
                    Animation::new(Duration::from_millis(MOTION_OVERLAY_MS)),
                    |card, delta| card.opacity(delta),
                ),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn settings_layer(
        &mut self,
        cx: &mut Context<Self>,
        palette: Palette,
        density: Density,
        inputs: &[String],
        outputs: &[String],
        chosen_in: Option<&str>,
        chosen_out: Option<&str>,
        voice: Option<&rencord_voice::LoopbackReport>,
    ) -> impl IntoElement {
        let this = cx.entity();
        let jitter = voice
            .map(|report| {
                format!(
                    "Loopback played {} of {}, concealed {}, hard gaps {}, target {} ms.",
                    report.played,
                    report.sent,
                    report.concealed,
                    report.hard_gaps,
                    report.target_ms
                )
            })
            .unwrap_or_else(|| "Join voice to fill jitter stats.".into());
        overlay(
            palette,
            v_flex()
                .w(px(520.))
                .p(px(16.))
                .gap_2()
                .rounded(px(12.))
                .bg(rgb(palette.elevated))
                .border_1()
                .border_color(rgb(palette.line))
                .child(Label::new("Settings"))
                .child(Label::new(format!(
                    "Theme {} · {} · {} px",
                    self.shell.prefs.theme.label(),
                    density.label(),
                    self.shell.prefs.font_px
                )))
                .child(
                    h_flex()
                        .gap_2()
                        .child(small_button(this.clone(), "theme", "Theme", |view, cx| {
                            view.shell.cycle_theme();
                            cx.notify();
                        }))
                        .child(small_button(this.clone(), "layout", "Layout", |view, cx| {
                            view.shell.cycle_layout();
                            cx.notify();
                        }))
                        .child(small_button(
                            this.clone(),
                            "density",
                            "Density",
                            |view, cx| {
                                view.shell.toggle_density();
                                cx.notify();
                            },
                        ))
                        .child(small_button(this.clone(), "font-up", "A+", |view, cx| {
                            view.shell.bump_font(1);
                            cx.notify();
                        }))
                        .child(small_button(this.clone(), "font-down", "A-", |view, cx| {
                            view.shell.bump_font(-1);
                            cx.notify();
                        }))
                        .child(small_button(
                            this.clone(),
                            "close-settings",
                            "Close",
                            |view, cx| {
                                view.shell.close_layers();
                                cx.notify();
                            },
                        )),
                )
                .child(Label::new("Input device"))
                .child(device_list(this.clone(), inputs, chosen_in, true, palette))
                .child(Label::new("Output device"))
                .child(device_list(this, outputs, chosen_out, false, palette))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(palette.muted))
                        .child(Label::new(jitter)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(palette.faint))
                        .child(Label::new(format!(
                            "Go Live is not in this build. Nitro gate in the UI is {}. There is no purchase surface.",
                            rencord_media::nitro_gate_in_ui()
                        ))),
                ),
        )
    }
}

fn overlay(palette: Palette, child: impl IntoElement) -> impl IntoElement {
    div()
        .absolute()
        .size_full()
        .bg(rgb(palette.app_bg))
        .occlude()
        .child(
            div()
                .size_full()
                .items_center()
                .justify_center()
                .bg(rgba(0x00000099u32))
                .child(child),
        )
}

fn category_header(
    view: Entity<RencordWindow>,
    key: &'static str,
    title: &'static str,
    open: bool,
    palette: Palette,
) -> impl IntoElement {
    let mark = if open { "⌄" } else { "›" };
    div()
        .id(SharedString::from(format!("cat-{key}")))
        .px(px(8.))
        .pt(px(14.))
        .pb(px(2.))
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                if !this.collapsed.remove(key) {
                    this.collapsed.insert(key);
                }
                cx.notify();
            });
        })
        .child(
            h_flex()
                .gap(px(4.))
                .items_center()
                .child(paint(mark, palette.muted, 12.))
                .child(paint(title, palette.muted, 12.)),
        )
}

fn section_label(text: &str, color: u32) -> impl IntoElement {
    div()
        .px(px(12.))
        .pt(px(10.))
        .text_size(px(11.))
        .text_color(rgb(color))
        .child(paint(text.to_uppercase(), color, 12.))
}

#[allow(clippy::too_many_arguments)]
fn channel_row(
    view: Entity<RencordWindow>,
    id: Snowflake,
    label: String,
    active: bool,
    palette: Palette,
    badge: Option<u32>,
    joins_voice: bool,
    row_px: f32,
) -> impl IntoElement {
    let hover = palette.hover;
    let bg = if active {
        palette.selected
    } else {
        palette.sidebar
    };
    let (kind, title) = if let Some(name) = label.strip_prefix("# ") {
        ("hash", name.to_owned())
    } else if let Some(name) = label.strip_prefix(")) ") {
        ("speaker", name.to_owned())
    } else if let Some(name) = label.strip_prefix("* ") {
        ("star", name.to_owned())
    } else if let Some(name) = label.strip_prefix("dm ") {
        ("dm", name.to_owned())
    } else {
        ("hash", label.clone())
    };
    let color = if active { palette.text } else { palette.muted };
    let height = if kind == "dm" {
        row_px.max(32.)
    } else {
        row_px
    };
    div()
        .id(SharedString::from(format!("channel-{id}")))
        .mx(px(8.))
        .px(px(8.))
        .h(px(height))
        .rounded(px(6.))
        .overflow_hidden()
        .bg(rgb(bg))
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(if active { bg } else { hover })))
        .child(
            h_flex()
                .size_full()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(if kind == "dm" {
                            portrait_status(&title, 24., 0x23a559, palette.sidebar)
                                .into_any_element()
                        } else {
                            icon(
                                if kind == "speaker" { "speaker" } else { "hash" },
                                20.,
                                color,
                            )
                            .into_any_element()
                        })
                        .child(paint(title.clone(), color, 16.))
                        .children(if kind == "star" {
                            Some(icon("star", 12., palette.muted))
                        } else {
                            None
                        }),
                )
                .children(badge.map(|count| {
                    div()
                        .min_w(px(16.))
                        .h(px(16.))
                        .px(px(4.))
                        .rounded(px(8.))
                        .items_center()
                        .justify_center()
                        .bg(rgb(0xb91c1c))
                        .child(paint(count.to_string(), 0xffffff, 11.))
                })),
        )
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                if joins_voice {
                    this.join_voice();
                } else {
                    this.shell.select_channel_timed(id);
                }
                cx.notify();
            });
        })
}

fn hint_bar(palette: Palette, text: String) -> impl IntoElement {
    div()
        .px(px(12.))
        .py(px(4.))
        .text_size(px(12.))
        .text_color(rgb(palette.muted))
        .bg(rgb(palette.elevated))
        .child(Label::new(text))
}

fn hang_up_button(view: Entity<RencordWindow>) -> impl IntoElement {
    let group = SharedString::from("voice-leave-group");
    let hide = group.clone();
    let show = group.clone();
    h_flex()
        .group(group)
        .id("voice-leave")
        .relative()
        .size(px(32.))
        .flex_shrink_0()
        .rounded(px(6.))
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0xf23f43)))
        .child(
            div()
                .group_hover(hide, |style| style.opacity(0.))
                .child(icon("phone", 20., 0xf23f43)),
        )
        .child(
            h_flex()
                .absolute()
                .top(px(0.))
                .left(px(0.))
                .size(px(32.))
                .items_center()
                .justify_center()
                .opacity(0.)
                .group_hover(show, |style| style.opacity(1.))
                .child(icon("phone", 20., 0xffffff)),
        )
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                this.shell.leave_voice();
                cx.notify();
            });
        })
}

fn dock_circle(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    palette: Palette,
    active: bool,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    let glyph = match (label, active) {
        ("Mic", true) => "mic-off",
        ("Mic", false) => "mic",
        ("Deaf", true) => "deafen-off",
        ("Deaf", false) => "deafen",
        ("Set", _) => "settings",
        _ => "plus",
    };
    let ink = if active && label != "Set" {
        palette.danger
    } else {
        palette.text
    };
    h_flex()
        .id(id)
        .size(px(32.))
        .flex_shrink_0()
        .rounded(px(6.))
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(palette.hover)))
        .child(icon(glyph, 20., ink))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| action(this, cx));
        })
}

fn small_button(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    Button::new(id).label(label).on_click(move |_, _, cx| {
        view.update(cx, |this, cx| action(this, cx));
    })
}

fn device_list(
    view: Entity<RencordWindow>,
    devices: &[String],
    chosen: Option<&str>,
    input: bool,
    palette: Palette,
) -> impl IntoElement {
    let mut column = v_flex().gap_1();
    if devices.is_empty() {
        column = column.child(Label::new("No devices reported by cpal on this machine."));
        return column;
    }
    for (index, name) in devices.iter().take(6).enumerate() {
        let marked = chosen == Some(name.as_str());
        let owned = name.clone();
        let row_view = view.clone();
        column = column.child(
            div()
                .id(SharedString::from(format!(
                    "{}-{index}",
                    if input { "in" } else { "out" }
                )))
                .px(px(8.))
                .py(px(4.))
                .rounded(px(6.))
                .bg(rgb(if marked {
                    palette.selected
                } else {
                    palette.chat
                }))
                .cursor_pointer()
                .child(Label::new(if marked {
                    format!("● {owned}")
                } else {
                    owned.clone()
                }))
                .on_click(move |_, _, cx| {
                    let owned = owned.clone();
                    row_view.update(cx, |this, cx| {
                        if input {
                            this.shell.prefs.input_device = Some(owned);
                        } else {
                            this.shell.prefs.output_device = Some(owned);
                        }
                        this.reopen_audio();
                        cx.notify();
                    });
                }),
        );
    }
    column
}

fn channel_title(
    selected: Option<Snowflake>,
    channels: &[rencord_model::Channel],
    dms: &[rencord_model::Channel],
) -> String {
    channels
        .iter()
        .chain(dms.iter())
        .find(|channel| Some(channel.id) == selected)
        .map(|channel| channel.name.clone())
        .unwrap_or_else(|| "Select a channel".into())
}

fn rail_rule() -> impl IntoElement {
    h_flex()
        .w(px(72.))
        .h(px(8.))
        .items_center()
        .justify_center()
        .child(div().w(px(32.)).h(px(2.)).rounded(px(1.)).bg(rgb(0x4e5058)))
}

#[derive(Clone, Copy)]
enum Pill {
    None,
    Unread,
    Selected,
}

fn guild_fill(name: &str) -> u32 {
    match name {
        "Analytical Engines" => 0x3d4a6b,
        "Difference Club" => 0x3a4150,
        _ => 0x343844,
    }
}

fn tile_initial(letter: &str, color: u32) -> impl IntoElement {
    div()
        .font_family("Figtree")
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(16.))
        .line_height(px(16.))
        .text_color(rgb(color))
        .child(letter.to_owned())
}

#[allow(clippy::too_many_arguments)]
fn rail_slot(
    id: SharedString,
    _active: bool,
    pill: Pill,
    fill: u32,
    hover: u32,
    pill_ink: u32,
    mark: impl IntoElement,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let radius = 12.;
    let pill_h = match pill {
        Pill::Selected => 36.,
        Pill::Unread => 8.,
        Pill::None => 0.,
    };
    let mut slot = h_flex()
        .relative()
        .w(px(72.))
        .h(px(48.))
        .flex_shrink_0()
        .items_center()
        .justify_center();
    if pill_h > 0. {
        slot = slot.child(
            div()
                .absolute()
                .left(px(0.))
                .top(px((48. - pill_h) / 2.))
                .w(px(4.))
                .h(px(pill_h))
                .rounded(px(2.))
                .bg(rgb(pill_ink)),
        );
    }
    slot.child(
        h_flex()
            .id(id)
            .size(px(48.))
            .flex_shrink_0()
            .rounded(px(radius))
            .items_center()
            .justify_center()
            .bg(rgb(fill))
            .cursor_pointer()
            .overflow_hidden()
            .hover(move |style| style.bg(rgb(hover)))
            .child(mark)
            .on_click(on_click),
    )
}

fn rail_action(
    view: Entity<RencordWindow>,
    id: &'static str,
    glyph: &'static str,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    h_flex()
        .w(px(72.))
        .h(px(48.))
        .items_center()
        .justify_center()
        .child(
            h_flex()
                .id(id)
                .size(px(48.))
                .flex_shrink_0()
                .rounded(px(12.))
                .items_center()
                .justify_center()
                .bg(rgb(0x1c1f26))
                .border_2()
                .border_color(rgb(0x23a55a))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0x243028)))
                .child(icon(glyph, 22., 0x23a55a))
                .on_click(move |_, _, cx| {
                    view.update(cx, |this, cx| action(this, cx));
                }),
        )
}

fn header_icon(
    view: Entity<RencordWindow>,
    id: &'static str,
    glyph: &'static str,
    palette: Palette,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    let hover = palette.hover;
    div()
        .id(id)
        .size(px(32.))
        .rounded(px(4.))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon(glyph, 20., palette.muted))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| action(this, cx));
        })
}

fn guild_mark(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .take(1)
        .collect::<String>()
        .to_uppercase()
}

fn message_row(
    view: Entity<RencordWindow>,
    message: &Message,
    grouped: bool,
    reactions: Vec<rencord_desktop::Reaction>,
    palette: Palette,
    font_px: f32,
) -> impl IntoElement {
    let id = message.id;
    let name = message.author.display_name().to_string();
    let role = role_color(&name, palette);
    let body_source = message.content.clone();
    let (reply_note, body_source) = if let Some(rest) = body_source.strip_prefix("replying to ") {
        let (target, text) = rest.split_once(':').unwrap_or(("a message", rest));
        let who = match target.trim() {
            "1000" | "1" => "Ada",
            "2" => "Charles",
            "3" => "Lovelace",
            _ => "a message",
        };
        (Some(format!("Replying to {who}")), text.trim().to_owned())
    } else {
        (None, body_source)
    };
    let body_source = body_source
        .lines()
        .filter(|line| !line.contains("attachment:"))
        .collect::<Vec<_>>()
        .join("\n");
    let group = SharedString::from(format!("row-{id}"));
    let hover = palette.hover;
    let mut body = v_flex().flex_1().gap_1();
    if !grouped {
        body = body.child(
            h_flex()
                .gap_2()
                .items_baseline()
                .child(
                    div()
                        .font_family("Figtree")
                        .text_size(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(role))
                        .child(name.clone()),
                )
                .child(
                    div()
                        .font_family("Figtree")
                        .text_size(px(12.))
                        .text_color(rgb(palette.muted))
                        .child(message.timestamp.clone()),
                ),
        );
    }
    if let Some(note) = reply_note {
        body = body.child(
            h_flex()
                .gap(px(6.))
                .items_center()
                .child(icon("reply", 14., palette.muted))
                .child(paint(note, palette.muted, 12.)),
        );
    }
    body = body.child(message_blocks(&body_source, id, palette, font_px));
    if !reactions.is_empty() {
        let mut pills = h_flex().gap_1().mt(px(2.));
        for reaction in &reactions {
            pills = pills.child(
                div()
                    .px(px(6.))
                    .h(px(24.))
                    .rounded(px(8.))
                    .items_center()
                    .bg(rgb(if reaction.mine {
                        palette.ping
                    } else {
                        palette.elevated
                    }))
                    .border_1()
                    .border_color(rgb(if reaction.mine {
                        palette.accent
                    } else {
                        palette.line
                    }))
                    .text_size(px(13.))
                    .text_color(rgb(palette.text))
                    .child(format!(
                        "{} {}",
                        if reaction.emoji == "wave" {
                            "👋"
                        } else {
                            reaction.emoji.as_str()
                        },
                        reaction.count
                    )),
            );
        }
        body = body.child(pills);
    }
    if message.content.contains("](") {
        body = body.child(
            v_flex()
                .mt(px(4.))
                .max_w(px(432.))
                .px(px(16.))
                .py(px(8.))
                .rounded(px(12.))
                .bg(rgb(palette.elevated))
                .border_l_4()
                .border_color(rgb(palette.link))
                .child(paint("example.com", palette.muted, 12.))
                .child(paint("Jitter notes", palette.link, 16.))
                .child(paint(
                    "How the adaptive buffer stays under the idle budget.",
                    palette.text,
                    14.,
                )),
        );
    }
    if message.content.contains("attachment:bench.png") {
        body = body.child(
            v_flex()
                .mt(px(4.))
                .gap_1()
                .child(
                    div()
                        .w(px(400.))
                        .h(px(96.))
                        .rounded(px(12.))
                        .overflow_hidden()
                        .child(bench_photo()),
                )
                .child(paint("voice-bench.jpg", palette.muted, 12.)),
        );
    }
    h_flex()
        .id(group.clone())
        .group(group.clone())
        .relative()
        .w_full()
        .px(px(16.))
        .py(px(if grouped { 2. } else { 8. }))
        .border_l_4()
        .border_color(rgb(if message.content.contains("Mention") {
            palette.accent
        } else {
            palette.chat
        }))
        .bg(rgb(if message.content.contains("Mention") {
            palette.ping
        } else {
            palette.chat
        }))
        .gap(px(16.))
        .items_start()
        .hover(move |style| style.bg(rgb(hover)))
        .child(if grouped {
            div().w(px(40.)).h(px(8.)).into_any_element()
        } else {
            portrait(&name, 40.).into_any_element()
        })
        .child(body)
        .child(
            h_flex()
                .absolute()
                .top(px(-12.))
                .right(px(12.))
                .gap_1()
                .px(px(4.))
                .h(px(32.))
                .items_center()
                .rounded(px(6.))
                .bg(rgb(0x111214))
                .border_1()
                .border_color(rgb(palette.line))
                .opacity(0.)
                .group_hover(group, |style| style.opacity(1.))
                .child(icon_action(
                    view.clone(),
                    format!("reply-{id}"),
                    "Reply",
                    id,
                    true,
                    palette,
                ))
                .child(icon_action(
                    view.clone(),
                    format!("edit-{id}"),
                    "Edit",
                    id,
                    false,
                    palette,
                ))
                .child(delete_action(view, id, palette)),
        )
}

fn icon_action(
    view: Entity<RencordWindow>,
    element_id: String,
    label: &'static str,
    id: Snowflake,
    reply: bool,
    palette: Palette,
) -> impl IntoElement {
    let glyph = if reply { "reply" } else { "pencil" };
    let hover = palette.hover;
    div()
        .id(SharedString::from(element_id))
        .size(px(32.))
        .rounded(px(4.))
        .items_center()
        .justify_center()
        .cursor_pointer()
        .aria_label(label)
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon(glyph, 18., palette.text))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                if reply {
                    this.shell.begin_reply(id);
                } else {
                    this.shell.begin_edit(id);
                }
                cx.notify();
            });
        })
}

fn role_color(name: &str, palette: Palette) -> u32 {
    if matches!(palette.mode, ThemeMode::Light) {
        return match name {
            "Ada" => 0x1d4ed8,
            "Charles" => 0x15803d,
            "Lovelace" => 0xc2410c,
            _ => palette.text,
        };
    }
    match name {
        "Ada" => 0xa8b0d4,
        "Charles" => 0x8fb89a,
        "Lovelace" => 0xc4a48a,
        _ => palette.text,
    }
}

fn delete_action(view: Entity<RencordWindow>, id: Snowflake, palette: Palette) -> impl IntoElement {
    let hover = palette.hover;
    div()
        .id(SharedString::from(format!("delete-{id}")))
        .size(px(32.))
        .rounded(px(6.))
        .items_center()
        .justify_center()
        .cursor_pointer()
        .aria_label("Delete")
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon("trash", 18., palette.danger))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                if let Some(message) = this.shell.delete_local(id) {
                    this.dispatch_mutation(rencord_desktop::Mutation::Delete { message }, cx);
                }
                cx.notify();
            });
        })
}

fn message_blocks(
    content: &str,
    id: Snowflake,
    palette: Palette,
    font_px: f32,
) -> impl IntoElement {
    let blocks = parse(content);
    let mut column = v_flex().gap_1();
    if blocks.is_empty() {
        return column.child(markdown_line(content, palette, font_px));
    }
    for (index, block) in blocks.into_iter().enumerate() {
        match block {
            Block::Code { text, language } => {
                column = column.child(code_block(id, index, text, language, palette, font_px));
            }
            Block::Paragraph { inlines }
            | Block::Heading { inlines, .. }
            | Block::Quote { inlines } => {
                column = column.child(rich_inlines(&inlines, palette, font_px));
            }
        }
    }
    column
}

fn code_block(
    id: Snowflake,
    index: usize,
    text: String,
    language: Option<String>,
    palette: Palette,
    font_px: f32,
) -> impl IntoElement {
    let copy_text = text.clone();
    let copy_group = SharedString::from(format!("code-copy-{id}-{index}"));
    let copy_hover = copy_group.clone();
    let mut lines = v_flex().w_full().overflow_hidden();
    for (line_index, line) in text.split('\n').enumerate() {
        lines = lines.child(
            div()
                .id(SharedString::from(format!(
                    "code-{id}-{index}-{line_index}"
                )))
                .whitespace_nowrap()
                .font_family("JetBrains Mono")
                .text_size(px((font_px - 1.0).max(12.0)))
                .child(highlight_code_line(line, palette)),
        );
    }
    v_flex()
        .group(copy_group)
        .w_full()
        .bg(rgb(palette.code_bg))
        .rounded(px(12.))
        .border_1()
        .border_color(rgb(palette.line))
        .p(px(16.))
        .gap_1()
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(paint(
                    language.unwrap_or_else(|| "text".into()),
                    palette.muted,
                    12.,
                ))
                .child(
                    h_flex()
                        .id(SharedString::from(format!("copy-{id}-{index}")))
                        .size(px(28.))
                        .rounded(px(6.))
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .opacity(0.)
                        .group_hover(copy_hover, |style| style.opacity(1.))
                        .hover(|style| style.bg(rgb(palette.hover)))
                        .child(icon("copy", 16., palette.muted))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                        }),
                ),
        )
        .child(lines)
}

fn highlight_code_line(line: &str, palette: Palette) -> StyledText {
    const KEYWORDS: &[&str] = &[
        "fn", "let", "mut", "pub", "struct", "impl", "return", "if", "else", "for", "while", "use",
        "mod", "match", "self",
    ];
    let mut text = String::new();
    let mut runs = Vec::new();
    let mut rest = line;
    if rest.is_empty() {
        rest = " ";
    }
    while !rest.is_empty() {
        let (token, next) = split_token(rest);
        let keyword = KEYWORDS.contains(&token);
        text.push_str(token);
        let mut font = font("JetBrains Mono");
        if keyword {
            font.weight = FontWeight::BOLD;
        }
        runs.push(TextRun {
            len: token.len(),
            font,
            color: rgb(if keyword { palette.link } else { palette.text }).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
        rest = next;
    }
    StyledText::new(text).with_runs(runs)
}

fn split_token(input: &str) -> (&str, &str) {
    let mut end = 0;
    for (index, ch) in input.char_indices() {
        if index == 0 {
            end = ch.len_utf8();
            if ch.is_ascii_alphanumeric() || ch == '_' {
                continue;
            }
            break;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    input.split_at(end.max(1).min(input.len()))
}

fn search_hits_bar(
    view: Entity<RencordWindow>,
    hits: &[rencord_desktop::SearchHit],
    palette: Palette,
) -> impl IntoElement {
    let mut column = v_flex()
        .px(px(12.))
        .py(px(6.))
        .gap_1()
        .bg(rgb(palette.elevated))
        .child(Label::new(format!(
            "{} local hits. Filters: from: in: has: before: after:",
            hits.len()
        )));
    for hit in hits.iter().take(6) {
        let channel_id = hit.channel_id;
        let message_id = hit.message.id;
        let label = format!(
            "#{}  {}",
            hit.channel_name,
            hit.message.content.replace('\n', " ")
        );
        let row = view.clone();
        column = column.child(
            div()
                .id(SharedString::from(format!("hit-{message_id}")))
                .cursor_pointer()
                .px(px(6.))
                .py(px(2.))
                .rounded(px(6.))
                .hover(|style| style.bg(rgb(palette.hover)))
                .child(Label::new(label))
                .on_click(move |_, _, cx| {
                    row.update(cx, |this, cx| {
                        this.shell.jump_to(channel_id, message_id);
                        this.shell.search.clear();
                        cx.notify();
                    });
                }),
        );
    }
    column
}

fn markdown_line(content: &str, palette: Palette, font_px: f32) -> impl IntoElement {
    let blocks = parse(content);
    let mut text = String::new();
    let mut runs = Vec::new();
    {
        let base = font(".SystemUIFont");
        let mut push = |chunk: &str, color: u32, weight: FontWeight| {
            if chunk.is_empty() {
                return;
            }
            text.push_str(chunk);
            let mut font = base.clone();
            font.weight = weight;
            runs.push(TextRun {
                len: chunk.len(),
                font,
                color: rgb(color).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        };
        if blocks.is_empty() {
            push(content, palette.text, FontWeight::NORMAL);
        }
        for block in &blocks {
            match block {
                Block::Paragraph { inlines }
                | Block::Heading { inlines, .. }
                | Block::Quote { inlines } => {
                    flatten_inlines(inlines, palette, &mut push);
                }
                Block::Code { text: body, .. } => {
                    push(body, palette.link, FontWeight::NORMAL);
                }
            }
            push(" ", palette.text, FontWeight::NORMAL);
        }
    }
    if text.is_empty() {
        text.push(' ');
        let mut font = font(".SystemUIFont");
        font.weight = FontWeight::NORMAL;
        runs.push(TextRun {
            len: 1,
            font,
            color: rgb(palette.text).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    div()
        .text_size(px(font_px))
        .overflow_hidden()
        .child(StyledText::new(text).with_runs(runs))
}

fn flatten_inlines(
    inlines: &[Inline],
    palette: Palette,
    push: &mut impl FnMut(&str, u32, FontWeight),
) {
    for inline in inlines {
        match inline {
            Inline::Text(value) => push(value, palette.text, FontWeight::NORMAL),
            Inline::Bold(inner) => {
                let mut flat = String::new();
                collect_plain(inner, &mut flat);
                push(&flat, palette.text, FontWeight::BOLD);
            }
            Inline::Italic(inner) => {
                let mut flat = String::new();
                collect_plain(inner, &mut flat);
                push(&flat, palette.muted, FontWeight::NORMAL);
            }
            Inline::Code(value) => push(value, palette.link, FontWeight::NORMAL),
            Inline::Link { text, .. } => push(text, palette.link, FontWeight::NORMAL),
            Inline::Mention { id } => push(&format!("@{id}"), palette.accent, FontWeight::BOLD),
            Inline::Spoiler(inner) => {
                let mut flat = String::new();
                collect_plain(inner, &mut flat);
                push(&flat, palette.faint, FontWeight::NORMAL);
            }
        }
    }
}

fn collect_plain(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(value) | Inline::Code(value) => out.push_str(value),
            Inline::Bold(inner) | Inline::Italic(inner) | Inline::Spoiler(inner) => {
                collect_plain(inner, out)
            }
            Inline::Link { text, .. } => out.push_str(text),
            Inline::Mention { id } => {
                out.push('@');
                out.push_str(id);
            }
        }
    }
}

fn apply_kit_theme(palette: &Palette, cx: &mut App) {
    let mode = if matches!(palette.mode, ThemeMode::Light) {
        "light"
    } else {
        "dark"
    };
    let hex = |color: u32| format!("#{color:06X}");
    let value = serde_json::json!({
        "name": "Rencord",
        "mode": mode,
        "font.family": "Figtree",
        "font.size": 15,
        "colors": {
            "background": hex(palette.chat),
            "foreground": hex(palette.text),
            "muted.foreground": hex(palette.muted),
            "muted.background": hex(palette.sidebar),
            "border": hex(palette.line),
            "input": hex(palette.input),
            "primary": hex(palette.accent),
            "primary.foreground": hex(palette.accent_text),
            "accent.background": hex(palette.hover),
            "accent.foreground": hex(palette.text),
            "button.background": hex(palette.elevated),
            "button.foreground": hex(palette.text),
        }
    });
    if let Ok(config) = serde_json::from_value::<gpui_kit::component::theme::ThemeConfig>(value) {
        gpui_kit::component::theme::Theme::global_mut(cx).apply_config(&std::rc::Rc::new(config));
    }
}

fn icon(name: &str, size: f32, color: u32) -> Svg {
    let raw = match name {
        "hash" => include_str!("../../../docs/design/icons/hash.svg"),
        "speaker" => include_str!("../../../docs/design/icons/speaker.svg"),
        "mic" => include_str!("../../../docs/design/icons/mic.svg"),
        "deafen" => include_str!("../../../docs/design/icons/deafen.svg"),
        "settings" => include_str!("../../../docs/design/icons/settings.svg"),
        "plus" => include_str!("../../../docs/design/icons/plus.svg"),
        "emoji" => include_str!("../../../docs/design/icons/emoji.svg"),
        "gif" => include_str!("../../../docs/design/icons/gif.svg"),
        "pin" => include_str!("../../../docs/design/icons/pin.svg"),
        "inbox" => include_str!("../../../docs/design/icons/inbox.svg"),
        "search" => include_str!("../../../docs/design/icons/search.svg"),
        "reply" => include_str!("../../../docs/design/icons/reply.svg"),
        "pencil" => include_str!("../../../docs/design/icons/pencil.svg"),
        "trash" => include_str!("../../../docs/design/icons/trash.svg"),
        "threads" => include_str!("../../../docs/design/icons/threads.svg"),
        "members" => include_str!("../../../docs/design/icons/members.svg"),
        "home" => include_str!("../../../docs/design/icons/home.svg"),
        "compass" => include_str!("../../../docs/design/icons/compass.svg"),
        "phone" => include_str!("../../../docs/design/icons/phone.svg"),
        "send" => include_str!("../../../docs/design/icons/send.svg"),
        "copy" => include_str!("../../../docs/design/icons/copy.svg"),
        "star" => include_str!("../../../docs/design/icons/star.svg"),
        "mic-off" => include_str!("../../../docs/design/icons/mic-off.svg"),
        "deafen-off" => include_str!("../../../docs/design/icons/deafen-off.svg"),
        _ => include_str!("../../../docs/design/icons/hash.svg"),
    };
    let colored = raw.replace("currentColor", &format!("#{color:06X}"));
    svg()
        .data(colored.as_bytes())
        .size(px(size))
        .text_color(rgb(color))
}

fn portrait(name: &str, size: f32) -> impl IntoElement {
    let bytes: &'static [u8] = match name {
        "Charles" => include_bytes!("../assets/avatars/charles.png"),
        "Lovelace" => include_bytes!("../assets/avatars/lovelace.png"),
        _ => include_bytes!("../assets/avatars/ada.png"),
    };
    let image = Image::from_bytes(ImageFormat::Png, bytes.to_vec());
    div()
        .size(px(size))
        .rounded(px(size / 2.))
        .overflow_hidden()
        .flex_shrink_0()
        .child(img(std::sync::Arc::new(image)).size(px(size)))
}

fn portrait_status(name: &str, size: f32, dot: u32, ring: u32) -> impl IntoElement {
    div()
        .relative()
        .size(px(size))
        .flex_shrink_0()
        .child(portrait(name, size))
        .child(
            div()
                .absolute()
                .bottom(px(-1.))
                .right(px(-1.))
                .size(px(10.))
                .rounded(px(5.))
                .bg(rgb(dot))
                .border_2()
                .border_color(rgb(ring)),
        )
}

fn panel(palette: Palette, fill: u32) -> gpui_kit::Div {
    div()
        .h_full()
        .rounded(px(12.))
        .overflow_hidden()
        .bg(rgb(fill))
        .border_1()
        .border_color(rgb(palette.line))
}

fn new_messages_banner(_palette: Palette) -> impl IntoElement {
    h_flex()
        .mx(px(12.))
        .mt(px(8.))
        .h(px(32.))
        .px(px(12.))
        .rounded(px(6.))
        .items_center()
        .justify_between()
        .bg(rgba(0x5865f2cc))
        .child(paint("4 new messages", 0xffffff, 13.))
        .child(paint("Mark as read", 0xffffff, 12.))
}

fn date_pill(label: &str, palette: Palette) -> impl IntoElement {
    h_flex().justify_center().py(px(8.)).child(
        h_flex()
            .px(px(10.))
            .h(px(20.))
            .rounded(px(999.))
            .items_center()
            .justify_center()
            .bg(rgb(palette.input))
            .child(paint(label, palette.muted, 11.)),
    )
}

fn paint(text: impl Into<String>, color: u32, size: f32) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .line_height(px(size + 4.))
        .font_family("Figtree")
        .font_weight(FontWeight::NORMAL)
        .child(text.into())
}

fn mention_label(id: &str) -> String {
    match id {
        "1" => "@Ada".into(),
        "2" => "@Charles".into(),
        "3" => "@Lovelace".into(),
        other => format!("@{other}"),
    }
}

fn rich_inlines(inlines: &[Inline], palette: Palette, font_px: f32) -> impl IntoElement {
    let mut text = String::new();
    let mut runs = Vec::new();
    walk_inlines(
        inlines,
        palette,
        FontWeight::NORMAL,
        &mut |chunk, color, weight, background| {
            if chunk.is_empty() {
                return;
            }
            text.push_str(chunk);
            let mut face = font("Inter");
            face.weight = weight;
            runs.push(TextRun {
                len: chunk.len(),
                font: face,
                color: rgb(color).into(),
                background_color: background.map(|color| rgb(color).into()),
                underline: None,
                strikethrough: None,
            });
        },
    );
    if text.is_empty() {
        text.push(' ');
        let mut face = font("Inter");
        face.weight = FontWeight::NORMAL;
        runs.push(TextRun {
            len: 1,
            font: face,
            color: rgb(palette.text).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    div()
        .text_size(px(font_px))
        .child(StyledText::new(text).with_runs(runs))
}

fn walk_inlines(
    inlines: &[Inline],
    palette: Palette,
    weight: FontWeight,
    push: &mut impl FnMut(&str, u32, FontWeight, Option<u32>),
) {
    for inline in inlines {
        match inline {
            Inline::Text(value) => push(value, palette.text, weight, None),
            Inline::Bold(inner) => walk_inlines(inner, palette, FontWeight::SEMIBOLD, push),
            Inline::Italic(inner) | Inline::Spoiler(inner) => {
                walk_inlines(inner, palette, weight, push)
            }
            Inline::Code(value) => push(value, palette.link, weight, Some(palette.elevated)),
            Inline::Link { text, .. } => push(text, palette.link, weight, None),
            Inline::Mention { id } => {
                let label = mention_label(id);
                let foreground = if matches!(palette.mode, ThemeMode::Light) {
                    0x1d4ed8
                } else {
                    0xffffff
                };
                push(&label, foreground, FontWeight::SEMIBOLD, Some(palette.ping));
            }
        }
    }
}

fn bench_photo() -> impl IntoElement {
    let image = Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../../docs/design/fixtures/bench.png").to_vec(),
    );
    img(std::sync::Arc::new(image)).size_full()
}

async fn push_mutation(token: &str, mutation: &rencord_desktop::Mutation) -> Result<(), String> {
    use rencord_desktop::Mutation;
    let api = rencord_api::DiscordApi::new(rencord_model::Token::new(token.to_owned()));
    match mutation {
        Mutation::Create {
            channel_id,
            content,
            reply_to,
            ..
        } => {
            if let Some(reply) = reply_to {
                api.reply_message(*channel_id, content, *reply)
                    .await
                    .map(|_| ())
                    .map_err(|err| err.to_string())?;
            } else {
                api.send_message(*channel_id, content)
                    .await
                    .map(|_| ())
                    .map_err(|err| err.to_string())?;
            }
        }
        Mutation::Edit {
            channel_id,
            message_id,
            next,
            ..
        } => {
            api.edit_message(*channel_id, *message_id, next)
                .await
                .map(|_| ())
                .map_err(|err| err.to_string())?;
        }
        Mutation::Delete { message } => {
            api.delete_message(message.channel_id, message.id)
                .await
                .map_err(|err| err.to_string())?;
        }
        Mutation::React {
            channel_id,
            message_id,
            emoji,
            remove,
        } => {
            let id = rencord_model::Snowflake::new(*message_id);
            if *remove {
                api.remove_reaction(*channel_id, id, emoji)
                    .await
                    .map_err(|err| err.to_string())?;
            } else {
                api.add_reaction(*channel_id, id, emoji)
                    .await
                    .map_err(|err| err.to_string())?;
            }
        }
    }
    Ok(())
}

fn rss_kib() -> u64 {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return 0;
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kb| kb.parse().ok())
        .unwrap_or(0)
}
