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
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rencord_desktop::{Density, Palette, Shell, ThemeMode, MOTION_OVERLAY_MS};

mod golive;
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
    if args
        .windows(2)
        .any(|pair| pair[0] == "--bench" && pair[1] == "stream")
    {
        println!(
            "{}",
            rencord_media::format_bench(&rencord_media::bench_software(30))
        );
        return;
    }
    let start_voice = args.iter().any(|arg| arg == "--voice");
    let golive = args
        .windows(2)
        .find(|pair| pair[0] == "--golive")
        .map(|pair| golive::GoLiveMode::parse(&pair[1]))
        .unwrap_or(golive::GoLiveMode::Off);
    let reveal_hover = args.iter().any(|arg| arg == "--reveal-hover");
    let frame = args
        .windows(2)
        .find(|pair| pair[0] == "--size")
        .and_then(|pair| {
            let (width, height) = pair[1].split_once('x')?;
            Some((width.parse::<f32>().ok()?, height.parse::<f32>().ok()?))
        });
    let app = gpui_kit::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        let _ = cx.text_system().add_fonts(vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-Regular.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-Medium.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-SemiBold.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Regular.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Medium.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf")),
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
        let (frame_w, frame_h) = frame.unwrap_or((1180., 760.));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(frame_w), px(frame_h)), cx)),
            // Client decorations drop the desktop app-menu button. Settings stays in the dock.
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                RencordWindow::new(
                    demo,
                    bench,
                    theme,
                    layout,
                    density,
                    start_voice,
                    reveal_hover,
                    golive,
                    window,
                    cx,
                )
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
    reveal_hover: bool,
    golive: golive::GoLiveMode,
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
        reveal_hover: bool,
        golive: golive::GoLiveMode,
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
            reveal_hover,
            golive,
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
            .font_family(if matches!(palette.mode, ThemeMode::Onyx) {
                "Inter"
            } else {
                "Figtree"
            })
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
        if self.golive == golive::GoLiveMode::Picker {
            return root.child(golive::picker(palette)).into_any_element();
        }
        if self.golive == golive::GoLiveMode::Viewer {
            return root.child(golive::viewer(palette)).into_any_element();
        }
        if self.golive == golive::GoLiveMode::Pip {
            return root.child(golive::pip(palette)).into_any_element();
        }
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
        let ink = matches!(palette.mode, ThemeMode::Onyx);
        let rail_w = if ink { 68. } else { 72. };
        let tile = if ink { 44. } else { 48. };
        let tile_radius = if ink { 8. } else { 12. };
        let side_w = if ink { 216. } else { 240. };
        // Inter is a little wider than the mock's face, so the chat takes
        // the extra from the member list and both body lines stay on one row.
        let member_w = if ink { 176. } else { 240. };
        let shell_gap = if ink { 8. } else { 12. };
        let pill_ink = if ink {
            palette.accent
        } else if matches!(palette.mode, ThemeMode::Light) {
            0x060607
        } else {
            0xffffff
        };
        let home_fill = if ink {
            0x000000
        } else if home_active {
            0x5865f2
        } else {
            0x2a2e38
        };
        let home_ink = if ink {
            palette.accent
        } else if home_active {
            0xffffff
        } else {
            palette.text
        };
        let mut rail = v_flex()
            .w(px(rail_w))
            .h_full()
            .bg(rgb(palette.rail))
            .items_center()
            .gap(px(8.))
            .py(px(if ink { 10. } else { 12. }));
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
                home_fill,
                home_fill,
                pill_ink,
                if ink { palette.line } else { home_fill },
                rail_w,
                tile,
                tile_radius,
                icon("home", 20., home_ink),
                move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.shell
                            .select_channel_timed(rencord_model::Snowflake::new(200));
                        cx.notify();
                    });
                },
            ));
        }
        if !ink {
            rail = rail.child(rail_rule(rail_w, palette.line));
        }
        for guild in guilds {
            let id = guild.id;
            let mark = guild_mark(&guild.name);
            let active = selected_guild == Some(id);
            let view = this.clone();
            let fill = if ink {
                if guild.name == "Analytical Engines" {
                    0x0a0a16
                } else {
                    0x000000
                }
            } else {
                guild_fill(&guild.name)
            };
            let border = if ink && active {
                palette.accent
            } else if ink {
                palette.line
            } else {
                fill
            };
            let letter = if ink && !active {
                0xc5c8de
            } else {
                palette.text
            };
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
                border,
                rail_w,
                tile,
                tile_radius,
                tile_initial(&mark, letter),
                move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.shell.session.select_guild(id);
                        cx.notify();
                    });
                },
            ));
        }
        rail = rail.child(rail_rule(rail_w, if ink { 0x1c1c38 } else { palette.line }));
        rail = rail.child(rail_action(
            this.clone(),
            "rail-add",
            "plus",
            true,
            palette,
            rail_w,
            tile,
            tile_radius,
            |view, cx| {
                view.shell.note("Creating a server is not in this build.");
                cx.notify();
            },
        ));
        rail = rail.child(rail_action(
            this.clone(),
            "rail-explore",
            "compass",
            false,
            palette,
            rail_w,
            tile,
            tile_radius,
            |view, cx| {
                view.shell.note("Server discovery is not in this build.");
                cx.notify();
            },
        ));

        let mut sidebar = v_flex()
            .w(px(side_w))
            .h_full()
            .bg(rgb(palette.sidebar))
            .child(
                div()
                    .h(px(if ink { 42. } else { 48. }))
                    .px(px(if ink { 14. } else { 12. }))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(palette.line))
                    .text_size(px(if ink { 15. } else { 16. }))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(palette.text))
                    .child(guild_name.clone()),
            )
            .child(if ink {
                h_flex()
                    .mx(px(10.))
                    .mt(px(8.))
                    .mb(px(4.))
                    .h(px(30.))
                    .px(px(8.))
                    .gap(px(6.))
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(palette.line))
                    .bg(rgb(palette.input))
                    .child(icon("search", 13., palette.faint))
                    .child(
                        div().flex_1().overflow_hidden().child(
                            Input::new(&self.search_input)
                                .bg(rgb(palette.input))
                                .text_color(rgb(palette.faint))
                                .border_color(rgb(palette.input)),
                        ),
                    )
                    .into_any_element()
            } else {
                div()
                    .px(px(8.))
                    .py(px(8.))
                    .child(
                        Input::new(&self.search_input)
                            .bg(rgb(palette.input))
                            .text_color(rgb(palette.text))
                            .border_color(rgb(palette.input)),
                    )
                    .into_any_element()
            });
        let row_px = density.channel_row_px();
        let favorites_open = !self.collapsed.contains("favorites");
        let text_open = !self.collapsed.contains("text");
        let voice_open = !self.collapsed.contains("voice");
        let dm_open = !self.collapsed.contains("dms");
        let favorite_ids = self.shell.favorites.clone();
        let in_voice = voice.is_some();
        let mut nav = v_flex().flex_1().overflow_hidden();
        nav = nav.child(category_header(
            this.clone(),
            "favorites",
            "FAVORITES",
            favorites_open,
            palette,
        ));
        if favorites_open {
            for id in &favorite_ids {
                if let Some(channel) = self.shell.session.channels.get(id) {
                    nav = nav.child(channel_row(
                        this.clone(),
                        channel.id,
                        format!("* {}", channel.name),
                        selected == Some(channel.id),
                        palette,
                        None,
                        false,
                        row_px,
                        false,
                    ));
                }
            }
        }
        nav = nav.child(category_header(
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
                let shown_as_favorite = favorites_open && favorite_ids.contains(&channel.id);
                nav = nav.child(channel_row(
                    this.clone(),
                    channel.id,
                    format!("# {}", channel.name),
                    selected == Some(channel.id) && !shown_as_favorite,
                    palette,
                    badge,
                    false,
                    row_px,
                    false,
                ));
            }
        }
        nav = nav.child(category_header(
            this.clone(),
            "voice",
            "VOICE CHANNELS",
            voice_open,
            palette,
        ));
        if voice_open {
            for channel in channels.iter().filter(|channel| channel.kind.is_voice()) {
                nav = nav.child(channel_row(
                    this.clone(),
                    channel.id,
                    format!(")) {}", channel.name),
                    false,
                    palette,
                    None,
                    true,
                    row_px,
                    in_voice,
                ));
            }
        }
        nav = nav.child(category_header(
            this.clone(),
            "dms",
            "DIRECT MESSAGES",
            dm_open,
            palette,
        ));
        if dm_open {
            for channel in &dms {
                nav = nav.child(channel_row(
                    this.clone(),
                    channel.id,
                    format!("dm {}", channel.name),
                    selected == Some(channel.id),
                    palette,
                    None,
                    false,
                    row_px,
                    false,
                ));
            }
        }
        sidebar = sidebar.child(nav);

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
        let reveal_hover = self.reveal_hover;
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
                reveal_hover && message.content.contains("Mention"),
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
                    .h(px(if ink { 42. } else { 48. }))
                    .px(px(12.))
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(palette.line))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap(px(8.))
                            .items_center()
                            .overflow_hidden()
                            .child(icon("hash", if ink { 16. } else { 20. }, palette.muted))
                            .child(
                                div()
                                    .text_size(px(if ink { 15. } else { 16. }))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(palette.text))
                                    .child(channel_name.clone()),
                            )
                            .child(div().w(px(1.)).h(px(16.)).bg(rgb(if ink {
                                0x1c1c38
                            } else {
                                palette.line
                            })))
                            .child(paint("Voice, notes, and no upsells", palette.muted, 13.)),
                    )
                    .child(
                        h_flex()
                            .gap(px(2.))
                            .flex_shrink_0()
                            .items_center()
                            .when(!ink, |tools| {
                                tools.child(header_icon(
                                    this.clone(),
                                    "hdr-threads",
                                    "threads",
                                    palette,
                                    |view, cx| {
                                        view.shell.note("Threads are not in this build.");
                                        cx.notify();
                                    },
                                ))
                            })
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
                .h(px(if matches!(palette.mode, ThemeMode::Onyx) {
                    38.
                } else {
                    47.
                }))
                .items_center()
                .gap(px(2.))
                .rounded(px(8.))
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
            .w(px(member_w))
            .h_full()
            .bg(rgb(palette.members))
            .child(section_label("ONLINE — 3", palette.muted));
        for member in members {
            let (name, activity) = member
                .split_once(" · ")
                .map(|(name, activity)| (name.to_owned(), activity.to_owned()))
                .unwrap_or_else(|| (member.clone(), "Online".into()));
            let role = role_color(&name, palette);
            let dot = if matches!(palette.mode, ThemeMode::Onyx) {
                match activity.as_str() {
                    "Idle" => 0xd4c05a,
                    "Offline" => 0x80848e,
                    _ => 0x5abb72,
                }
            } else {
                match activity.as_str() {
                    "Idle" => 0xf0b232,
                    "Offline" => 0x80848e,
                    _ => 0x23a559,
                }
            };
            let speaking = activity == "In voice" && matches!(palette.mode, ThemeMode::Onyx);
            people = people.child(
                h_flex()
                    .mx(px(8.))
                    .px(px(6.))
                    .h(px(if speaking || matches!(palette.mode, ThemeMode::Onyx) {
                        38.
                    } else {
                        44.
                    }))
                    .gap(px(8.))
                    .items_center()
                    .rounded(px(6.))
                    .hover(|style| style.bg(rgb(palette.hover)))
                    .child(if speaking {
                        div()
                            .p(px(2.))
                            .rounded(px(20.))
                            .border_2()
                            .border_color(rgb(palette.accent))
                            .child(portrait_status(&name, 32., dot, palette.members))
                            .into_any_element()
                    } else {
                        portrait_status(&name, 32., dot, palette.members).into_any_element()
                    })
                    .child(
                        v_flex()
                            .gap(px(0.))
                            .child(paint(name, role, 14.))
                            .child(meta(activity, palette.muted, 11., palette)),
                    ),
            );
        }

        let voice_panel = self.voice_card(cx, palette, &user_name, voice.as_ref(), muted, deafened);
        let layout_kind = self.shell.prefs.layout;
        let hide_members =
            !self.shell.prefs.show_members || viewport < px(if ink { 900. } else { 1100. });
        let hide_sidebar = !self.shell.prefs.show_sidebar || viewport < px(760.);
        let sidebar = if hide_sidebar { div() } else { sidebar };
        let people = if hide_members { div() } else { people };
        let body = match layout_kind {
            rencord_desktop::LayoutKind::Discord => div()
                .size_full()
                .bg(rgb(palette.app_bg))
                .p(px(shell_gap))
                .child(
                    h_flex()
                        .size_full()
                        .gap(px(shell_gap))
                        .child(panel(palette, palette.rail).w(px(rail_w)).child(rail))
                        .child(
                            panel(palette, palette.sidebar)
                                .w(px(side_w))
                                .child(sidebar.child(voice_panel)),
                        )
                        .child(panel(palette, palette.chat).flex_1().child(chat))
                        .child(
                            panel(palette, palette.members)
                                .w(px(member_w))
                                .child(people),
                        ),
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
        let onyx = matches!(palette.mode, ThemeMode::Onyx);
        let dot = if muted || deafened {
            0xf23f43
        } else if onyx {
            0x5abb72
        } else {
            0x23a559
        };
        let mut column = v_flex().w_full().flex_shrink_0().bg(rgb(palette.sidebar));
        if let Some(report) = voice {
            let view = this.clone();
            let label_color = if onyx { palette.link } else { 0x23a559 };
            column = column.child(
                v_flex()
                    .mx(px(if onyx { 8. } else { 0. }))
                    .mt(px(if onyx { 6. } else { 0. }))
                    .px(px(if onyx { 10. } else { 8. }))
                    .py(px(8.))
                    .gap(px(2.))
                    .rounded(px(if onyx { 6. } else { 0. }))
                    .border_1()
                    .border_color(rgb(if onyx { palette.line } else { palette.elevated }))
                    .bg(rgb(if onyx { palette.chat } else { palette.elevated }))
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(label_color))
                                    .font_family(if onyx { "JetBrains Mono" } else { "Figtree" })
                                    .child("Voice Connected"),
                            )
                            .child(hang_up_button(view)),
                    )
                    .child(paint(
                        "voice",
                        if onyx { 0xc5c8de } else { palette.muted },
                        12.,
                    ))
                    .child(waveform(if onyx { palette.accent } else { palette.link }))
                    .child(meta(
                        format!("{} ms  ·  0.2% plc", report.target_ms),
                        palette.muted,
                        11.,
                        palette,
                    )),
            );
        }
        column.child(
            h_flex()
                .h(px(52.))
                .pl(px(if onyx { 10. } else { 8. }))
                .pr(px(if onyx { 6. } else { 8. }))
                .gap(px(6.))
                .items_center()
                .border_t_1()
                .border_color(rgb(if onyx { palette.line } else { palette.sidebar }))
                .bg(rgb(palette.sidebar))
                .child(portrait_status(user_name, 32., dot, palette.sidebar))
                .child(
                    v_flex()
                        .flex_1()
                        .overflow_hidden()
                        .child(
                            div()
                                .text_size(px(if onyx { 13. } else { 14. }))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(palette.text))
                                .child(user_name.to_owned()),
                        )
                        .child(meta(status, palette.muted, 11., palette)),
                )
                .child(
                    h_flex()
                        .gap(px(1.))
                        .flex_shrink_0()
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
                ),
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
                            "Go Live defaults to 720p30. Higher presets are a request, not a grant. Nitro gate in the UI is {}. There is no purchase surface.",
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
    let onyx = matches!(palette.mode, ThemeMode::Onyx);
    div()
        .id(SharedString::from(format!("cat-{key}")))
        .px(px(8.))
        .pt(px(if onyx { 8. } else { 14. }))
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
                .child(paint(mark, palette.muted, if onyx { 11. } else { 12. }))
                .child(paint(title, palette.muted, if onyx { 11. } else { 12. })),
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
    live_dot: bool,
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
    let onyx = matches!(palette.mode, ThemeMode::Onyx);
    let color = if active {
        palette.text
    } else if onyx {
        0xc5c8de
    } else {
        palette.muted
    };
    let icon_color = if active && onyx && kind != "speaker" && kind != "dm" {
        palette.accent
    } else {
        color
    };
    let label_px = if onyx { 14. } else { 16. };
    let glyph_px = if onyx { 15. } else { 20. };
    let height = if kind == "dm" {
        row_px.max(32.)
    } else {
        row_px
    };
    let (badge_bg, badge_fg) = if onyx {
        (palette.accent, palette.accent_text)
    } else {
        (0xb91c1c, 0xffffff)
    };
    div()
        .id(SharedString::from(format!("channel-{id}")))
        .mx(px(8.))
        .px(px(8.))
        .h(px(height))
        .rounded(px(6.))
        .overflow_hidden()
        .bg(rgb(bg))
        .border_l_3()
        .border_color(rgb(if active && matches!(palette.mode, ThemeMode::Onyx) {
            palette.accent
        } else {
            bg
        }))
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
                            portrait_status(
                                &title,
                                if onyx { 20. } else { 24. },
                                if onyx { 0x5abb72 } else { 0x23a559 },
                                palette.sidebar,
                            )
                            .into_any_element()
                        } else {
                            icon(
                                if kind == "speaker" { "speaker" } else { "hash" },
                                glyph_px,
                                icon_color,
                            )
                            .into_any_element()
                        })
                        .child(paint(title.clone(), color, label_px)),
                )
                .children(if kind == "star" {
                    Some(
                        icon(
                            "star",
                            12.,
                            if onyx { palette.accent } else { palette.muted },
                        )
                        .into_any_element(),
                    )
                } else if live_dot {
                    Some(
                        div()
                            .size(px(6.))
                            .rounded(px(3.))
                            .bg(rgb(if onyx { 0x5abb72 } else { 0x23a559 }))
                            .into_any_element(),
                    )
                } else {
                    badge.map(|count| {
                        h_flex()
                            .min_w(px(16.))
                            .h(px(16.))
                            .px(px(4.))
                            .rounded(px(8.))
                            .items_center()
                            .justify_center()
                            .bg(rgb(badge_bg))
                            .child(paint(count.to_string(), badge_fg, 10.))
                            .into_any_element()
                    })
                }),
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
    h_flex()
        .id("voice-leave")
        .size(px(28.))
        .flex_shrink_0()
        .rounded(px(14.))
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(rgba(0xc8463c29))
        .child(icon("phone", 14., 0xe05550))
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
        .child(icon(glyph, 16., ink))
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

fn rail_rule(width: f32, color: u32) -> impl IntoElement {
    h_flex()
        .w(px(width))
        .h(px(8.))
        .items_center()
        .justify_center()
        .child(div().w(px(28.)).h(px(1.)).bg(rgb(color)))
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
    border: u32,
    rail_w: f32,
    tile: f32,
    radius: f32,
    mark: impl IntoElement,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (pill_w, pill_h) = match pill {
        Pill::Selected => (4., 22.),
        Pill::Unread => (8., 8.),
        Pill::None => (0., 0.),
    };
    let mut slot = h_flex()
        .relative()
        .w(px(rail_w))
        .h(px(tile))
        .flex_shrink_0()
        .items_center()
        .justify_center();
    if pill_h > 0. {
        slot = slot.child(
            div()
                .absolute()
                .left(px(2.))
                .top(px((tile - pill_h) / 2.))
                .w(px(pill_w))
                .h(px(pill_h))
                .rounded(px(pill_h / 2.))
                .bg(rgb(pill_ink)),
        );
    }
    slot.child(
        h_flex()
            .id(id)
            .size(px(tile))
            .flex_shrink_0()
            .rounded(px(radius))
            .items_center()
            .justify_center()
            .bg(rgb(fill))
            .border_1()
            .border_color(rgb(border))
            .cursor_pointer()
            .overflow_hidden()
            .hover(move |style| style.bg(rgb(hover)))
            .child(mark)
            .on_click(on_click),
    )
}

#[allow(clippy::too_many_arguments)]
fn rail_action(
    view: Entity<RencordWindow>,
    id: &'static str,
    glyph: &'static str,
    dashed: bool,
    palette: Palette,
    rail_w: f32,
    tile: f32,
    radius: f32,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    let ink = matches!(palette.mode, ThemeMode::Onyx);
    let border = if ink { 0x1c1c38 } else { 0x23a55a };
    let glyph_color = if ink { palette.muted } else { 0x23a55a };
    let tile_el = h_flex()
        .size(px(tile))
        .flex_shrink_0()
        .rounded(px(radius))
        .items_center()
        .justify_center()
        .bg(rgb(if ink { 0x000000 } else { 0x1c1f26 }))
        .border_1()
        .border_color(rgb(border));
    let tile_el = if dashed {
        tile_el.border_dashed()
    } else {
        tile_el
    };
    let tile_el = tile_el
        .id(id)
        .cursor_pointer()
        .hover(|style| style.border_color(rgb(palette.text)))
        .child(icon(glyph, 18., glyph_color))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| action(this, cx));
        });
    h_flex()
        .w(px(rail_w))
        .h(px(tile))
        .items_center()
        .justify_center()
        .child(tile_el)
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
    pin_actions: bool,
) -> impl IntoElement {
    let id = message.id;
    let onyx = matches!(palette.mode, ThemeMode::Onyx);
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
    let mut body = v_flex().flex_1().min_w(px(0.)).gap_1();
    if !grouped {
        body = body.child(
            h_flex()
                .gap_2()
                .items_baseline()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(role))
                        .child(name.clone()),
                )
                .child(
                    div()
                        .font_family(if matches!(palette.mode, ThemeMode::Onyx) {
                            "JetBrains Mono"
                        } else {
                            "Figtree"
                        })
                        .text_size(px(11.))
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
        let embed = v_flex()
            .gap(px(2.))
            .child(paint("example.com", palette.muted, 11.))
            .child(paint("Jitter notes", palette.link, 14.))
            .child(paint(
                "How the adaptive buffer stays under the idle budget.",
                if onyx { 0xc5c8de } else { palette.text },
                13.,
            ));
        body = body.child(if onyx {
            div()
                .mt(px(7.))
                .max_w(px(380.))
                .rounded(px(6.))
                .bg(rgb(palette.chat))
                .border_1()
                .border_color(rgb(palette.line))
                .child(
                    div()
                        .border_l_3()
                        .border_color(rgb(palette.accent))
                        .px(px(14.))
                        .py(px(8.))
                        .child(embed),
                )
                .into_any_element()
        } else {
            v_flex()
                .mt(px(4.))
                .max_w(px(432.))
                .px(px(16.))
                .py(px(8.))
                .rounded(px(12.))
                .bg(rgb(palette.elevated))
                .border_l_4()
                .border_color(rgb(palette.link))
                .child(embed)
                .into_any_element()
        });
    }
    if message.content.contains("attachment:bench.png") {
        body = body.child(
            v_flex()
                .mt(px(7.))
                .gap_1()
                .child(
                    div()
                        .w(px(320.))
                        .h(px(76.))
                        .rounded(px(if onyx { 6. } else { 12. }))
                        .overflow_hidden()
                        .border_1()
                        .border_color(rgb(palette.line))
                        .child(bench_photo()),
                )
                .child(if onyx {
                    meta("voice-bench.jpg", palette.muted, 11., palette).into_any_element()
                } else {
                    paint("voice-bench.jpg", palette.muted, 12.).into_any_element()
                }),
        );
    }
    let avatar = if onyx { 32. } else { 40. };
    h_flex()
        .id(group.clone())
        .group(group.clone())
        .relative()
        .w_full()
        .px(px(16.))
        .py(px(if grouped {
            2.
        } else if onyx {
            4.
        } else {
            8.
        }))
        .border_l_3()
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
        .gap(px(if onyx { 10. } else { 16. }))
        .items_start()
        .hover(move |style| style.bg(rgb(hover)))
        .child(if grouped {
            div().w(px(avatar)).h(px(8.)).into_any_element()
        } else {
            portrait(&name, avatar).into_any_element()
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
                .rounded(px(8.))
                .bg(rgb(palette.elevated))
                .border_1()
                .border_color(rgb(if onyx { 0x1c1c38 } else { palette.line }))
                .opacity(if pin_actions { 1. } else { 0. })
                .group_hover(group, |style| style.opacity(1.))
                .child(react_action(view.clone(), id, palette))
                .child(icon_action(
                    view.clone(),
                    format!("reply-{id}"),
                    "Reply",
                    id,
                    true,
                    palette,
                ))
                .child(link_action(view.clone(), id, palette))
                .child(div().w(px(1.)).h(px(16.)).bg(rgb(0x1c1c38)))
                .child(delete_action(view, id, palette)),
        )
}

fn react_action(view: Entity<RencordWindow>, id: Snowflake, palette: Palette) -> impl IntoElement {
    let hover = palette.hover;
    div()
        .id(SharedString::from(format!("react-bar-{id}")))
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon("emoji", 16., palette.muted))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                let mutation = this.shell.toggle_reaction(id.get(), "wave");
                this.dispatch_mutation(mutation, cx);
                cx.notify();
            });
        })
}

fn link_action(view: Entity<RencordWindow>, id: Snowflake, palette: Palette) -> impl IntoElement {
    let hover = palette.hover;
    div()
        .id(SharedString::from(format!("link-{id}")))
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon("link", 16., palette.muted))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                this.shell.note(format!("Message {id} link stays local."));
                cx.notify();
            });
        })
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
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .aria_label(label)
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon(glyph, 15., palette.muted))
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
    if matches!(palette.mode, ThemeMode::Onyx) {
        return match name {
            "Ada" => 0xa8b0e0,
            "Charles" => 0x7ecca0,
            "Lovelace" => 0xc4a6f0,
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
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .aria_label("Delete")
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon(
            "trash",
            15.,
            if matches!(palette.mode, ThemeMode::Onyx) {
                0xd46a5c
            } else {
                palette.danger
            },
        ))
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
    let onyx = matches!(palette.mode, ThemeMode::Onyx);
    let mut source: Vec<&str> = text.split('\n').collect();
    if source.last().is_some_and(|line| line.is_empty()) {
        source.pop();
    }
    let mut lines = v_flex().w_full().overflow_hidden();
    for (line_index, line) in source.iter().enumerate() {
        lines = lines.child(
            div()
                .id(SharedString::from(format!(
                    "code-{id}-{index}-{line_index}"
                )))
                .whitespace_nowrap()
                .font_family("JetBrains Mono")
                .text_size(px(if onyx {
                    12.5
                } else {
                    (font_px - 1.0).max(12.0)
                }))
                .line_height(px(if onyx { 16. } else { 22. }))
                .h(px(if onyx { 16. } else { 22. }))
                .overflow_hidden()
                .child(highlight_code_line(line, palette)),
        );
    }
    v_flex()
        .group(copy_group)
        .w_full()
        .bg(rgb(palette.code_bg))
        .rounded(px(if onyx { 6. } else { 12. }))
        .border_1()
        .border_color(rgb(palette.line))
        .overflow_hidden()
        .when(!onyx, |block| block.p(px(16.)).gap_1())
        .child(
            h_flex()
                .h(px(if onyx { 22. } else { 24. }))
                .px(px(if onyx { 10. } else { 0. }))
                .justify_between()
                .items_center()
                .when(onyx, |bar| bar.border_b_1().border_color(rgb(palette.line)))
                .child(meta(
                    language.unwrap_or_else(|| "text".into()),
                    palette.muted,
                    11.,
                    palette,
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
        .child(
            div()
                .px(px(if onyx { 12. } else { 0. }))
                .py(px(if onyx { 7. } else { 0. }))
                .child(lines),
        )
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
        let numeric = !token.is_empty() && token.chars().all(|ch| ch.is_ascii_digit());
        text.push_str(token);
        let mut font = font("JetBrains Mono");
        if keyword {
            font.weight = FontWeight::BOLD;
        }
        let color = if keyword {
            palette.link
        } else if numeric && matches!(palette.mode, ThemeMode::Onyx) {
            0xc4a6f0
        } else {
            palette.text
        };
        runs.push(TextRun {
            len: token.len(),
            font,
            color: rgb(color).into(),
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
            "font.family": if matches!(palette.mode, ThemeMode::Onyx) {
                "Inter"
            } else {
                "Figtree"
            },
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
        "link" => include_str!("../../../docs/design/icons/link.svg"),
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
    // The image paints its own corners. A parent radius does not clip it.
    img(std::sync::Arc::new(image))
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(size / 2.))
        .object_fit(ObjectFit::Cover)
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
        .rounded(px(if matches!(palette.mode, ThemeMode::Onyx) {
            8.
        } else {
            12.
        }))
        .overflow_hidden()
        .bg(rgb(fill))
        .border_1()
        .border_color(rgb(palette.line))
}

fn new_messages_banner(palette: Palette) -> impl IntoElement {
    h_flex()
        .mx(px(12.))
        .mt(px(6.))
        .h(px(28.))
        .px(px(12.))
        .rounded(px(6.))
        .items_center()
        .justify_between()
        .bg(rgb(palette.accent))
        .child(banner_ink("4 new messages", palette.accent_text, 13.))
        .child(banner_ink("Mark as read", palette.accent_text, 12.))
}

fn banner_ink(text: &str, color: u32, size: f32) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .font_weight(FontWeight::SEMIBOLD)
        .line_height(px(size + 2.))
        .child(text.to_owned())
}

fn date_pill(label: &str, palette: Palette) -> impl IntoElement {
    h_flex().justify_center().py(px(4.)).child(
        h_flex()
            .px(px(10.))
            .h(px(20.))
            .rounded(px(999.))
            .items_center()
            .justify_center()
            .bg(rgb(palette.input))
            .border_1()
            .border_color(rgb(palette.line))
            .child(meta(label, palette.muted, 11., palette)),
    )
}

fn meta(text: impl Into<String>, color: u32, size: f32, palette: Palette) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .line_height(px(size + 3.))
        .font_family(if matches!(palette.mode, ThemeMode::Onyx) {
            "JetBrains Mono"
        } else {
            "Figtree"
        })
        .child(text.into())
}

fn waveform(color: u32) -> impl IntoElement {
    let percents = [
        18, 28, 46, 70, 88, 64, 90, 72, 40, 22, 36, 68, 92, 76, 50, 28, 20, 44, 80, 96, 70, 42, 24,
        38, 62, 84, 58, 30, 18, 26, 48, 66, 40, 22,
    ];
    let mut row = h_flex().h(px(26.)).items_center().gap(px(2.));
    for (index, percent) in percents.iter().enumerate() {
        row = row.child(
            div()
                .id(SharedString::from(format!("wave-{index}")))
                .w(px(3.))
                .h(px(((*percent as f32) * 0.26).max(2.)))
                .rounded(px(2.))
                .bg(rgb(color)),
        );
    }
    row
}

fn paint(text: impl Into<String>, color: u32, size: f32) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .line_height(px(size + 4.))
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
    let onyx = matches!(palette.mode, ThemeMode::Onyx);
    div()
        .w_full()
        .min_w(px(0.))
        .whitespace_normal()
        .text_size(px(if onyx { 14. } else { font_px }))
        .line_height(px(if onyx { 20. } else { font_px + 4. }))
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
                let (foreground, wash) = if matches!(palette.mode, ThemeMode::Onyx) {
                    (palette.link, 0x101533)
                } else if matches!(palette.mode, ThemeMode::Light) {
                    (0x1d4ed8, palette.ping)
                } else {
                    (0xffffff, palette.ping)
                };
                push(&label, foreground, FontWeight::SEMIBOLD, Some(wash));
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
