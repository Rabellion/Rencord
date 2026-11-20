//! GPUI shell. Run `rencord --demo` for the local fixture, or `rencord` and
//! paste a user token. Live Discord calls are best-effort and were not
//! exercised in CI.

use gpui_kit::assets::Assets;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::*;
use rencord_desktop::Shell;
use rencord_markdown::{parse, Block, Inline};
use rencord_model::Snowflake;

fn main() {
    let demo = std::env::args().any(|arg| arg == "--demo");
    let app = gpui_kit::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1120.), px(740.)), cx)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| RencordWindow::new(demo, window, cx))
        })
        .expect("open Rencord window");
    });
}

struct RencordWindow {
    shell: Shell,
    token_input: Entity<InputState>,
    composer: Entity<InputState>,
    audio_session: Option<rencord_voice::AudioSession>,
}

impl RencordWindow {
    fn new(demo: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shell = if demo { Shell::demo() } else { Shell::login() };
        let token_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("User token (not stored until you connect)")
        });
        let composer = cx.new(|cx| InputState::new(window, cx).placeholder("Message"));
        Self {
            shell,
            token_input,
            composer,
            audio_session: None,
        }
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
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(8);
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
        drop(cmd_tx);
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let signed_in = self.shell.session.user.is_some();
        if !signed_in {
            return self.login(cx).into_any_element();
        }
        self.shell_view(cx).into_any_element()
    }
}

impl RencordWindow {
    fn login(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        v_flex()
            .size_full()
            .bg(rgb(0x1e1f22))
            .items_center()
            .justify_center()
            .gap_3()
            .text_color(rgb(0xf2f3f5))
            .child(Label::new("Rencord"))
            .child(Label::new(
                "Unofficial client. A user token can get the account terminated.",
            ))
            .child(Label::new(
                "Official login needs an ephemeral discord.com webview (WebView2 / WKWebView, or GTK4 WebKit on Linux). Milestone 1 accepts a token.",
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
        let messages = selected
            .map(|id| self.shell.session.messages_for(id).to_vec())
            .unwrap_or_default();
        let status = self.shell.session.status_line.clone();
        let voice = self.shell.session.voice_channel;

        h_flex()
            .size_full()
            .bg(rgb(0x1e1f22))
            .text_color(rgb(0xf2f3f5))
            .child(self.guild_rail(cx, &guilds, selected_guild))
            .child(self.channel_column(cx, &channels, &dms, selected))
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .child(
                        div()
                            .id("timeline")
                            .flex_1()
                            .p_4()
                            .gap_2()
                            .flex()
                            .flex_col()
                            .overflow_y_scroll()
                            .children(messages.into_iter().map(render_message)),
                    )
                    .child(
                        h_flex()
                            .p_3()
                            .gap_2()
                            .items_center()
                            .child(div().flex_1().child(Input::new(&self.composer)))
                            .child({
                                let this = this.clone();
                                Button::new("send").label("Send").on_click(move |_, _, cx| {
                                    this.update(cx, |view, cx| view.send(cx));
                                })
                            }),
                    )
                    .child(self.voice_bar(cx, &status, voice)),
            )
    }

    fn guild_rail(
        &mut self,
        cx: &mut Context<Self>,
        guilds: &[rencord_model::Guild],
        selected: Option<Snowflake>,
    ) -> impl IntoElement {
        let mut column = v_flex().w(px(72.)).h_full().p_2().gap_2().bg(rgb(0x111214));
        for guild in guilds {
            let id = guild.id;
            let label: SharedString = guild.name.chars().next().unwrap_or('?').to_string().into();
            let this = cx.entity();
            let mut button = Button::new(SharedString::from(format!("guild-{}", id.get())))
                .label(label)
                .on_click(move |_, _, cx| {
                    this.update(cx, |view, cx| {
                        view.shell.session.select_guild(id);
                        cx.notify();
                    });
                });
            if selected == Some(id) {
                button = button.primary();
            }
            column = column.child(button);
        }
        column
    }

    fn channel_column(
        &mut self,
        cx: &mut Context<Self>,
        channels: &[rencord_model::Channel],
        dms: &[rencord_model::Channel],
        selected: Option<Snowflake>,
    ) -> impl IntoElement {
        let mut column = v_flex()
            .w(px(240.))
            .h_full()
            .p_2()
            .gap_1()
            .bg(rgb(0x2b2d31));
        column = column.child(Label::new("Channels"));
        for channel in channels {
            column = column.child(self.channel_button(cx, channel, selected));
        }
        column = column.child(Label::new("Direct messages"));
        for channel in dms {
            column = column.child(self.channel_button(cx, channel, selected));
        }
        column
    }

    fn channel_button(
        &mut self,
        cx: &mut Context<Self>,
        channel: &rencord_model::Channel,
        selected: Option<Snowflake>,
    ) -> impl IntoElement {
        let id = channel.id;
        let this = cx.entity();
        let mut button = Button::new(SharedString::from(format!("channel-{}", id.get())))
            .label(SharedString::from(channel.name.clone()))
            .on_click(move |_, _, cx| {
                this.update(cx, |view, cx| {
                    view.shell.session.select_channel(id);
                    cx.notify();
                });
            });
        if selected == Some(id) {
            button = button.primary();
        }
        button
    }

    fn voice_bar(
        &mut self,
        cx: &mut Context<Self>,
        status: &str,
        voice: Option<Snowflake>,
    ) -> impl IntoElement {
        let this = cx.entity();
        h_flex()
            .p_3()
            .gap_2()
            .items_center()
            .bg(rgb(0x232428))
            .child(Label::new(SharedString::from(status.to_owned())))
            .child(Label::new(if rencord_media::nitro_gate_in_ui() {
                "Nitro gate is on"
            } else {
                "Stream presets up to 4K/120 are defined. Capture is not in this build."
            }))
            .child({
                let this = this.clone();
                Button::new("mute").label("Mute").on_click(move |_, _, cx| {
                    this.update(cx, |view, cx| {
                        view.shell.toggle_mute();
                        cx.notify();
                    });
                })
            })
            .child({
                let this = this.clone();
                Button::new("deafen").label("Deafen").on_click(move |_, _, cx| {
                    this.update(cx, |view, cx| {
                        view.shell.toggle_deaf();
                        cx.notify();
                    });
                })
            })
            .child({
                let this = this.clone();
                Button::new("quieter").label("Volume -").on_click(move |_, _, cx| {
                    this.update(cx, |view, cx| {
                        view.shell
                            .set_speaker_gain(view.shell.speaker_gain - 0.1);
                        cx.notify();
                    });
                })
            })
            .child({
                let this = this.clone();
                Button::new("join-voice").label("Join voice").on_click(move |_, _, cx| {
                    this.update(cx, |view, cx| {
                        view.shell.join_voice_demo();
                        match rencord_voice::open_default_call_audio() {
                            Ok(session) => {
                                view.audio_session = Some(session);
                                view.shell.note(
                                    "cpal default devices are open and routed through the jitter buffer.",
                                );
                            }
                            Err(err) => view.shell.note(format!(
                                "cpal devices were not opened ({err}). Loopback stats still ran."
                            )),
                        }
                        cx.notify();
                    });
                })
            })
            .child({
                let this = this.clone();
                Button::new("leave-voice")
                    .label(SharedString::from(match voice {
                        Some(id) => format!("Leave {id}"),
                        None => "Leave voice".to_owned(),
                    }))
                    .on_click(move |_, _, cx| {
                        this.update(cx, |view, cx| {
                            view.shell.leave_voice();
                            cx.notify();
                        });
                    })
            })
    }
}

fn render_message(message: rencord_model::Message) -> impl IntoElement {
    let blocks = parse(&message.content);
    v_flex()
        .gap_1()
        .p_2()
        .child(Label::new(SharedString::from(format!(
            "{}  {}",
            message.author.display_name(),
            message.timestamp
        ))))
        .children(blocks.into_iter().map(render_block))
}

fn render_block(block: Block) -> impl IntoElement {
    match block {
        Block::Heading { inlines, .. } => div()
            .text_lg()
            .child(plain_inlines(&inlines))
            .into_any_element(),
        Block::Quote { inlines } => div()
            .border_l_2()
            .border_color(rgb(0x4e5058))
            .pl_2()
            .child(plain_inlines(&inlines))
            .into_any_element(),
        Block::Code { language, text } => div()
            .p_2()
            .bg(rgb(0x111214))
            .rounded_md()
            .child(Label::new(SharedString::from(match language {
                Some(language) => format!("{language}\n{text}"),
                None => text,
            })))
            .into_any_element(),
        Block::Paragraph { inlines } => div().child(plain_inlines(&inlines)).into_any_element(),
    }
}

fn plain_inlines(inlines: &[Inline]) -> SharedString {
    fn walk(items: &[Inline], out: &mut String) {
        for item in items {
            match item {
                Inline::Text(text) => out.push_str(text),
                Inline::Bold(inner) => {
                    out.push_str("**");
                    walk(inner, out);
                    out.push_str("**");
                }
                Inline::Italic(inner) => {
                    out.push('*');
                    walk(inner, out);
                    out.push('*');
                }
                Inline::Code(text) => {
                    out.push('`');
                    out.push_str(text);
                    out.push('`');
                }
                Inline::Link { text, href } => {
                    out.push_str(text);
                    out.push_str(" (");
                    out.push_str(href);
                    out.push(')');
                }
                Inline::Mention { id } => {
                    out.push('@');
                    out.push_str(id);
                }
                Inline::Spoiler(inner) => {
                    out.push_str("||");
                    walk(inner, out);
                    out.push_str("||");
                }
            }
        }
    }
    let mut out = String::new();
    walk(inlines, &mut out);
    out.into()
}
