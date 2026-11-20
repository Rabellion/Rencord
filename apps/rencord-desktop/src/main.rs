//! GPUI shell. `rencord --demo` uses local data. `--theme amoled|dark|light`
//! picks the starting theme. `--bench` prints startup, RSS, and frame gaps,
//! then quits.

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
    let app = gpui_kit::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_kit::init(cx);
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
        ]);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1180.), px(760.)), cx)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| RencordWindow::new(demo, bench, theme, window, cx))
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
    bench: bool,
    bench_frames: u32,
    bench_last: Instant,
    frame_us: Vec<u128>,
}

impl RencordWindow {
    fn new(
        demo: bool,
        bench: bool,
        theme: Option<ThemeMode>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut shell = if demo {
            Shell::demo_history(if bench { 800 } else { 240 })
        } else {
            Shell::login()
        };
        if let Some(mode) = theme {
            shell.set_theme(mode);
        }
        let token_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("User token (not stored until you connect)")
        });
        let composer = cx.new(|cx| InputState::new(window, cx).placeholder("Message"));
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
            bench,
            bench_frames: 0,
            bench_last: Instant::now(),
            frame_us: Vec::new(),
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
        self.shell.send_local(&text);
        cx.notify();
    }

    fn join_voice(&mut self) {
        self.shell.join_voice_demo();
        match rencord_voice::open_default_call_audio() {
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
        if self.bench && self.bench_frames == 0 {
            cx.on_next_frame(window, |this, window, cx| {
                this.note_bench_frame(window, cx);
            });
        }
        let signed_in = self.shell.session.user.is_some();
        let palette = self.shell.prefs.palette();
        let root = div()
            .id("rencord-root")
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(palette.app_bg))
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
                cx.notify();
            }));
        if !signed_in {
            return root.child(self.login(cx)).into_any_element();
        }
        root.child(self.shell_view(cx)).into_any_element()
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

    fn shell_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let gain = self.shell.speaker_gain;
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

        let mut rail = v_flex()
            .w(px(68.))
            .h_full()
            .bg(rgb(palette.rail))
            .items_center()
            .gap(px(8.))
            .py(px(12.))
            .border_r_1()
            .border_color(rgb(palette.line));
        for guild in guilds {
            let id = guild.id;
            let mark = guild_mark(&guild.name);
            let active = selected_guild == Some(id);
            let view = this.clone();
            let hover = palette.hover;
            let selected_bg = palette.accent;
            let text = if active {
                palette.accent_text
            } else {
                palette.text
            };
            rail = rail.child(
                div()
                    .id(SharedString::from(format!("guild-{id}")))
                    .size(px(42.))
                    .rounded(px(if active { 12. } else { 21. }))
                    .items_center()
                    .justify_center()
                    .bg(rgb(if active {
                        selected_bg
                    } else {
                        palette.elevated
                    }))
                    .text_color(rgb(text))
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(if active { selected_bg } else { hover })))
                    .child(Label::new(mark))
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.shell.step_guild(0);
                            this.shell.session.select_guild(id);
                            cx.notify();
                        });
                    }),
            );
        }

        let mut sidebar = v_flex()
            .w(px(232.))
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
                    .child(Label::new("Channels")),
            )
            .child(
                div()
                    .px(px(8.))
                    .py(px(8.))
                    .child(Input::new(&self.search_input)),
            );
        sidebar = sidebar.child(section_label("Text", palette.faint));
        for channel in channels.iter().filter(|channel| channel.kind.is_text()) {
            sidebar = sidebar.child(channel_row(
                this.clone(),
                channel.id,
                format!("# {}", channel.name),
                selected == Some(channel.id),
                palette,
            ));
        }
        sidebar = sidebar.child(section_label("Voice", palette.faint));
        for channel in channels.iter().filter(|channel| channel.kind.is_voice()) {
            sidebar = sidebar.child(channel_row(
                this.clone(),
                channel.id,
                format!(")) {}", channel.name),
                selected == Some(channel.id),
                palette,
            ));
        }
        sidebar = sidebar.child(section_label("Direct messages", palette.faint));
        for channel in &dms {
            sidebar = sidebar.child(channel_row(
                this.clone(),
                channel.id,
                channel.name.clone(),
                selected == Some(channel.id),
                palette,
            ));
        }
        sidebar = sidebar.child(div().flex_1());
        sidebar = sidebar.child(self.voice_card(
            cx,
            palette,
            &status,
            voice.as_ref(),
            muted,
            deafened,
            gain,
        ));

        let count = messages.len();
        let row_h = density.row_px();
        let list_messages = messages.clone();
        let list_palette = palette;
        let list_entity = this.clone();
        let list_reactions = reactions.clone();
        let timeline = uniform_list("timeline", count, move |range, _window, _cx| {
            range
                .filter_map(|index| {
                    let message = list_messages.get(index)?.clone();
                    let grouped = index > 0
                        && list_messages
                            .get(index - 1)
                            .is_some_and(|previous| previous.author.id == message.author.id);
                    Some(message_row(
                        list_entity.clone(),
                        &message,
                        grouped,
                        list_reactions
                            .get(&message.id.get())
                            .cloned()
                            .unwrap_or_default(),
                        list_palette,
                        row_h,
                        font_px,
                    ))
                })
                .collect()
        })
        .h(px(520.));

        let mut chat = v_flex()
            .flex_1()
            .h_full()
            .bg(rgb(palette.chat))
            .child(
                h_flex()
                    .px(px(16.))
                    .h(px(48.))
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(palette.line))
                    .child(Label::new(channel_title(selected, &channels, &dms)))
                    .child(
                        h_flex()
                            .gap_2()
                            .child({
                                let view = this.clone();
                                Button::new("settings").label("Settings").on_click(
                                    move |_, _, cx| {
                                        view.update(cx, |this, cx| {
                                            this.shell.toggle_settings();
                                            cx.notify();
                                        });
                                    },
                                )
                            })
                            .child({
                                let view = this.clone();
                                Button::new("palette")
                                    .label("Ctrl K")
                                    .on_click(move |_, _, cx| {
                                        view.update(cx, |this, cx| {
                                            this.shell.toggle_palette();
                                            cx.notify();
                                        });
                                    })
                            }),
                    ),
            )
            .child(div().flex_1().overflow_hidden().px(px(8.)).child(timeline));
        if let Some(id) = reply {
            chat = chat.child(hint_bar(palette, format!("Replying to {id}")));
        }
        if let Some(id) = editing {
            chat = chat.child(hint_bar(palette, format!("Editing {id}")));
        }
        chat = chat.child(
            h_flex()
                .p(px(12.))
                .gap_2()
                .items_center()
                .child(div().flex_1().child(Input::new(&self.composer)))
                .child({
                    let view = this.clone();
                    Button::new("send")
                        .primary()
                        .label("Send")
                        .on_click(move |_, _, cx| {
                            view.update(cx, |this, cx| this.send(cx));
                        })
                }),
        );
        chat = chat.child(
            div()
                .px(px(12.))
                .pb(px(8.))
                .text_color(rgb(palette.faint))
                .text_size(px(12.))
                .child(Label::new(format!(
                    "{status}  ·  {theme_label}  ·  switch {switch_us} µs  ·  Alt+Up/Down channels  ·  no upsells"
                ))),
        );

        let mut people = v_flex()
            .w(px(196.))
            .h_full()
            .bg(rgb(palette.members))
            .border_l_1()
            .border_color(rgb(palette.line))
            .child(section_label("Online", palette.faint));
        for member in members {
            let initial = member.chars().next().unwrap_or('?').to_string();
            people = people.child(
                h_flex()
                    .px(px(10.))
                    .py(px(6.))
                    .gap_2()
                    .items_center()
                    .hover(|style| style.bg(rgb(palette.hover)))
                    .child(avatar(palette, &initial))
                    .child(Label::new(member)),
            );
        }

        let body = h_flex()
            .size_full()
            .child(rail)
            .child(sidebar)
            .child(chat)
            .child(people);

        let mut layer = div().size_full().relative().child(body);
        if palette_open {
            layer = layer.child(self.palette_layer(cx, palette, &commands));
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

    #[allow(clippy::too_many_arguments)]
    fn voice_card(
        &mut self,
        cx: &mut Context<Self>,
        palette: Palette,
        status: &str,
        voice: Option<&rencord_voice::LoopbackReport>,
        muted: bool,
        deafened: bool,
        gain: f32,
    ) -> impl IntoElement {
        let this = cx.entity();
        let jitter = voice
            .map(|report| {
                format!(
                    "played {}  gaps {}  target {} ms",
                    report.played, report.hard_gaps, report.target_ms
                )
            })
            .unwrap_or_else(|| "Not in a call".into());
        v_flex()
            .m(px(8.))
            .p(px(8.))
            .gap_1()
            .rounded(px(12.))
            .bg(rgb(palette.elevated))
            .child(Label::new(status.to_owned()))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(palette.muted))
                    .child(Label::new(jitter)),
            )
            .child(Label::new(format!(
                "{} {} gain {:.0}%",
                if muted { "muted" } else { "mic on" },
                if deafened { "deaf" } else { "listening" },
                gain * 100.0
            )))
            .child(
                h_flex()
                    .gap_1()
                    .child(small_button(this.clone(), "mute", "Mute", |view, cx| {
                        view.shell.toggle_mute();
                        cx.notify();
                    }))
                    .child(small_button(
                        this.clone(),
                        "deafen",
                        "Deafen",
                        |view, cx| {
                            view.shell.toggle_deaf();
                            cx.notify();
                        },
                    ))
                    .child(small_button(this.clone(), "quieter", "Vol-", |view, cx| {
                        view.shell.set_speaker_gain(view.shell.speaker_gain - 0.1);
                        cx.notify();
                    }))
                    .child(small_button(this, "join-voice", "Join", |view, cx| {
                        view.join_voice();
                        cx.notify();
                    })),
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

fn section_label(text: &str, color: u32) -> impl IntoElement {
    div()
        .px(px(12.))
        .pt(px(10.))
        .text_size(px(11.))
        .text_color(rgb(color))
        .child(Label::new(text.to_owned()))
}

fn channel_row(
    view: Entity<RencordWindow>,
    id: Snowflake,
    label: String,
    active: bool,
    palette: Palette,
) -> impl IntoElement {
    let hover = palette.hover;
    let bg = if active {
        palette.selected
    } else {
        palette.sidebar
    };
    div()
        .id(SharedString::from(format!("channel-{id}")))
        .mx(px(8.))
        .px(px(8.))
        .py(px(6.))
        .rounded(px(6.))
        .bg(rgb(bg))
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(if active { bg } else { hover })))
        .child(Label::new(label))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                this.shell.select_channel_timed(id);
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

fn avatar_slot(grouped: bool, palette: Palette, initial: &str) -> gpui_kit::Div {
    if grouped {
        return div().w(px(28.));
    }
    avatar(palette, initial)
}

fn avatar(palette: Palette, initial: &str) -> gpui_kit::Div {
    div()
        .size(px(28.))
        .rounded(px(14.))
        .bg(rgb(palette.accent))
        .text_color(rgb(palette.accent_text))
        .items_center()
        .justify_center()
        .text_size(px(12.))
        .child(Label::new(initial.to_owned()))
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
        .map(|channel| format!("#{}", channel.name))
        .unwrap_or_else(|| "Select a channel".into())
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
    row_h: f32,
    font_px: f32,
) -> impl IntoElement {
    let id = message.id;
    let initial = message
        .author
        .display_name()
        .chars()
        .next()
        .unwrap_or('?')
        .to_string();
    let header = if grouped {
        String::new()
    } else {
        format!("{}  {}", message.author.display_name(), message.timestamp)
    };
    let hover = palette.hover;
    h_flex()
        .id(SharedString::from(format!("msg-{id}")))
        .h(px(row_h))
        .w_full()
        .px(px(8.))
        .gap_2()
        .items_start()
        .rounded(px(6.))
        .hover(move |style| style.bg(rgb(hover)))
        .child(avatar_slot(grouped, palette, &initial))
        .child(
            v_flex()
                .flex_1()
                .overflow_hidden()
                .child(
                    div()
                        .text_size(px((font_px - 2.0).max(11.0)))
                        .text_color(rgb(palette.muted))
                        .child(Label::new(header)),
                )
                .child(markdown_line(&message.content, palette, font_px))
                .child(reaction_line(view.clone(), id.get(), &reactions, palette)),
        )
        .child(
            h_flex()
                .gap_1()
                .child(text_action(
                    view.clone(),
                    format!("reply-{id}"),
                    "Reply",
                    id,
                    true,
                ))
                .child(text_action(view, format!("edit-{id}"), "Edit", id, false)),
        )
}

fn text_action(
    view: Entity<RencordWindow>,
    element_id: String,
    label: &'static str,
    id: Snowflake,
    reply: bool,
) -> impl IntoElement {
    div()
        .id(SharedString::from(element_id))
        .px(px(6.))
        .cursor_pointer()
        .text_size(px(12.))
        .child(Label::new(label))
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

fn reaction_line(
    view: Entity<RencordWindow>,
    message_id: u64,
    reactions: &[rencord_desktop::Reaction],
    palette: Palette,
) -> impl IntoElement {
    let mut row = h_flex().gap_1();
    for reaction in reactions {
        row = row.child(Label::new(format!("{} {}", reaction.emoji, reaction.count)));
    }
    let view = view.clone();
    row.child(
        div()
            .id(SharedString::from(format!("react-{message_id}")))
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(rgb(palette.accent))
            .child(Label::new("+"))
            .on_click(move |_, _, cx| {
                view.update(cx, |this, cx| {
                    this.shell.toggle_reaction(message_id, "plus");
                    cx.notify();
                });
            }),
    )
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
