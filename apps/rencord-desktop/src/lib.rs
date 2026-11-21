//! Desktop session controller. The GPUI binary renders this; tests drive it
//! without opening a window.

mod breakpoints;
mod icons;
pub mod layout;
mod search;
mod theme;

use rencord_model::{Message, Snowflake, User};
use rencord_store::Session;
use rencord_voice::{backend_name, loopback_call, LoopbackReport};
use std::collections::HashMap;
use std::time::Instant;

pub use breakpoints::{stacks_login, LOGIN_STACK, MEMBERS_HIDE, MEMBERS_HIDE_ONYX, SIDEBAR_HIDE};
pub use icons::{
    discord_install_icon_dir, hexwave_png, hexwave_svg_for_px, icon_pack_dir, rail_home_glyph_px,
};
pub use search::{guild_search_path, local_search, parse_message_query, MessageQuery, SearchHit};
pub use theme::{
    guild_tile, role_color, theme_file_from_path, token_summary, Density, HdrCurveChoice,
    LayoutKind, Palette, Preferences, Theme, ThemeMode, MOTION_OVERLAY_MS,
};

#[derive(Clone, Debug)]
pub enum Mutation {
    Create {
        local_id: Snowflake,
        channel_id: Snowflake,
        content: String,
        reply_to: Option<Snowflake>,
    },
    Edit {
        channel_id: Snowflake,
        message_id: Snowflake,
        previous: String,
        next: String,
    },
    Delete {
        message: Message,
    },
    React {
        channel_id: Snowflake,
        message_id: u64,
        emoji: String,
        remove: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Reaction {
    pub emoji: String,
    pub count: u32,
    pub mine: bool,
}

#[derive(Clone, Debug)]
pub struct Shell {
    pub session: Session,
    pub demo: bool,
    pub voice_report: Option<LoopbackReport>,
    pub muted: bool,
    pub deafened: bool,
    pub speaker_gain: f32,
    pub audio_backend: &'static str,
    pub log: Vec<String>,
    pub prefs: Preferences,
    pub palette_open: bool,
    pub settings_open: bool,
    pub search: String,
    pub palette_query: String,
    pub reply_to: Option<Snowflake>,
    pub editing: Option<Snowflake>,
    pub reactions: HashMap<u64, Vec<Reaction>>,
    pub members: Vec<String>,
    /// Last cached channel switch, in microseconds.
    pub last_switch_us: u128,
    pub jump_index: Option<usize>,
    pub favorites: Vec<Snowflake>,
    pub read_mentions: Vec<u64>,
    pub inbox_open: bool,
    pub help_open: bool,
    pub secondary_action: bool,
    /// Big-file sheet. The upload work itself runs off the UI thread.
    pub flow: rencord_upload::Flow,
}

impl Shell {
    fn fresh(session: Session, demo: bool, log: Vec<String>) -> Self {
        let mut shell = Self {
            session,
            demo,
            voice_report: None,
            muted: false,
            deafened: false,
            speaker_gain: 1.0,
            audio_backend: backend_name(),
            log,
            prefs: Preferences::default(),
            palette_open: false,
            settings_open: false,
            search: String::new(),
            palette_query: String::new(),
            reply_to: None,
            editing: None,
            reactions: HashMap::new(),
            members: vec!["Ada".into(), "Charles".into(), "Lovelace".into()],
            last_switch_us: 0,
            jump_index: None,
            favorites: vec![Snowflake::new(100)],
            read_mentions: Vec::new(),
            inbox_open: false,
            help_open: false,
            secondary_action: false,
            flow: rencord_upload::Flow::default(),
        };
        shell.reactions.insert(
            1000,
            vec![Reaction {
                emoji: "wave".into(),
                count: 2,
                mine: false,
            }],
        );
        shell
    }

    pub fn demo() -> Self {
        let mut shell = Self::fresh(
            Session::demo(),
            true,
            vec!["Demo data is local. Nothing is sent to Discord.".into()],
        );
        shell.seed_showcase();
        shell
    }

    fn seed_showcase(&mut self) {
        let channel = Snowflake::new(100);
        let ada = self.session.user.clone().unwrap();
        let charles = User {
            id: Snowflake::new(2),
            username: "charles".into(),
            global_name: Some("Charles".into()),
            discriminator: None,
            bot: false,
            avatar: None,
        };
        let lovelace = User {
            id: Snowflake::new(3),
            username: "lovelace".into(),
            global_name: Some("Lovelace".into()),
            discriminator: None,
            bot: false,
            avatar: None,
        };
        self.reactions.remove(&1000);
        self.session.messages.insert(
            channel,
            vec![
                Message {
                    id: Snowflake::new(1000),
                    channel_id: channel,
                    author: ada.clone(),
                    content: "```rust\nfn main() {\n    let answer = 42;\n}\n```".into(),
                    timestamp: "Today at 3:38 PM".into(),
                },
                Message {
                    id: Snowflake::new(1002),
                    channel_id: channel,
                    author: charles.clone(),
                    content: "replying to 1000: That buffer is the whole point. Mention <@1> if the call drops.".into(),
                    timestamp: "Today at 3:44 PM".into(),
                },
                Message {
                    id: Snowflake::new(1003),
                    channel_id: channel,
                    author: lovelace,
                    content: "Write-up is here: [jitter notes](https://example.com/jitter). The preview can sit under the link.\nattachment:bench.png".into(),
                    timestamp: "Today at 3:51 PM".into(),
                },
                Message {
                    id: Snowflake::new(1004),
                    channel_id: channel,
                    author: charles,
                    content: "Shipping the voice dock next. No shop, no quests.".into(),
                    timestamp: "Today at 4:02 PM".into(),
                },
            ],
        );
        self.members = vec![
            "Ada · Online".into(),
            "Charles · In voice".into(),
            "Lovelace · Idle".into(),
        ];
    }

    /// Demo session plus a long cached history so the virtual list has work.
    pub fn demo_history(count: usize) -> Self {
        let mut shell = Self::demo();
        let ada = shell.session.user.clone().unwrap();
        let charles = User {
            id: Snowflake::new(2),
            username: "charles".into(),
            global_name: Some("Charles".into()),
            discriminator: None,
            bot: false,
            avatar: None,
        };
        let channel = Snowflake::new(100);
        for index in 0..count {
            let author = if index % 3 == 0 {
                charles.clone()
            } else {
                ada.clone()
            };
            let content = if index % 11 == 0 {
                format!("note {index}: **bold** and `code`")
            } else if index % 17 == 0 {
                format!("see [the log](https://example.com/{index})")
            } else {
                format!("cached line {index}")
            };
            shell.session.insert_message(Message {
                id: Snowflake::new(10_000 + index as u64),
                channel_id: channel,
                author,
                content,
                timestamp: format!("2026-11-20T00:{:02}:{:02}Z", (index / 60) % 60, index % 60),
            });
        }
        shell.session.insert_message(Message {
            id: Snowflake::new(90_000),
            channel_id: Snowflake::new(100),
            author: ada,
            content: "```rust\nfn main() {\n    let answer = 42;\n}\n```".into(),
            timestamp: "2026-11-20T01:00:00Z".into(),
        });
        shell
    }

    pub fn login() -> Self {
        Self::fresh(
            Session::default(),
            false,
            vec!["Scan the code with the Discord app, or use Log in with Discord.".into()],
        )
    }

    pub fn note(&mut self, line: impl Into<String>) {
        self.log.push(line.into());
        if self.log.len() > 40 {
            let overflow = self.log.len() - 40;
            self.log.drain(0..overflow);
        }
    }

    pub fn set_user(&mut self, user: User) {
        self.session.user = Some(user);
        self.session.status_line = "Signed in".into();
    }

    pub fn send_local(&mut self, content: &str) -> Option<Mutation> {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        let Some(channel_id) = self.session.selected_channel else {
            self.note("Select a channel before sending.");
            return None;
        };
        if let Some(editing) = self.editing.take() {
            if let Some(list) = self.session.messages.get_mut(&channel_id) {
                if let Some(message) = list.iter_mut().find(|message| message.id == editing) {
                    let previous = message.content.clone();
                    message.content = content.to_owned();
                    self.note(format!("Edited message {editing}"));
                    return Some(Mutation::Edit {
                        channel_id,
                        message_id: editing,
                        previous,
                        next: content.to_owned(),
                    });
                }
            }
        }
        let reply_to = self.reply_to.take();
        let prefix = if let Some(reply) = reply_to {
            format!("replying to {reply}: ")
        } else {
            String::new()
        };
        let author = self.session.user.clone().unwrap_or(User {
            id: Snowflake::new(0),
            username: "you".into(),
            global_name: None,
            discriminator: None,
            bot: false,
            avatar: None,
        });
        let id = Snowflake::new(
            self.session
                .messages_for(channel_id)
                .last()
                .map(|message| message.id.get().saturating_add(1))
                .unwrap_or(1),
        );
        self.session.insert_message(Message {
            id,
            channel_id,
            author,
            content: format!("{prefix}{content}"),
            timestamp: "local".into(),
        });
        self.session.composer.clear();
        self.note(format!("Queued message in channel {channel_id}"));
        Some(Mutation::Create {
            local_id: id,
            channel_id,
            content: content.to_owned(),
            reply_to,
        })
    }

    pub fn rollback(&mut self, mutation: &Mutation) {
        match mutation {
            Mutation::Create {
                local_id,
                channel_id,
                ..
            } => {
                if let Some(list) = self.session.messages.get_mut(channel_id) {
                    list.retain(|message| message.id != *local_id);
                }
                self.note("Send rolled back");
            }
            Mutation::Edit {
                channel_id,
                message_id,
                previous,
                ..
            } => {
                if let Some(list) = self.session.messages.get_mut(channel_id) {
                    if let Some(message) = list.iter_mut().find(|message| message.id == *message_id)
                    {
                        message.content = previous.clone();
                    }
                }
                self.note("Edit rolled back");
            }
            Mutation::Delete { message } => {
                self.session.insert_message(message.clone());
                self.note("Delete rolled back");
            }
            Mutation::React {
                message_id, emoji, ..
            } => {
                let _ = self.toggle_reaction(*message_id, emoji);
                self.note("Reaction rolled back");
            }
        }
    }

    pub fn join_voice_demo(&mut self) {
        let channel = self
            .session
            .channels
            .values()
            .find(|channel| channel.kind.is_voice())
            .map(|channel| channel.id);
        self.session.voice_channel = channel;
        match loopback_call(180, 0.015, 11) {
            Ok(report) => {
                self.session.status_line = format!(
                    "Loopback voice: played {} of {} sent, {} concealed, {} hard gaps, target {} ms",
                    report.played, report.sent, report.concealed, report.hard_gaps, report.target_ms
                );
                self.voice_report = Some(report);
            }
            Err(err) => self.note(format!("Voice loopback failed: {err}")),
        }
    }

    pub fn leave_voice(&mut self) {
        self.session.voice_channel = None;
        self.voice_report = None;
        self.session.status_line = "Left voice".into();
    }

    pub fn toggle_palette(&mut self) {
        self.palette_open = !self.palette_open;
        if self.palette_open {
            self.settings_open = false;
        }
    }

    pub fn toggle_settings(&mut self) {
        self.settings_open = !self.settings_open;
        if self.settings_open {
            self.palette_open = false;
        }
    }

    pub fn close_layers(&mut self) {
        self.palette_open = false;
        self.settings_open = false;
        self.flow.close();
    }

    pub fn cycle_theme(&mut self) {
        self.prefs.custom = None;
        self.prefs.theme = self.prefs.theme.cycle();
    }

    pub fn cycle_layout(&mut self) {
        self.prefs.layout = self.prefs.layout.cycle();
    }

    pub fn toggle_favorite(&mut self, id: Snowflake) {
        if let Some(index) = self.favorites.iter().position(|item| *item == id) {
            self.favorites.remove(index);
        } else {
            self.favorites.push(id);
        }
    }

    pub fn mentions(&self) -> Vec<SearchHit> {
        let mut hits = Vec::new();
        for channel in self.session.channels.values() {
            for message in self.session.messages_for(channel.id) {
                if self.read_mentions.contains(&message.id.get()) {
                    continue;
                }
                let text = message.content.to_ascii_lowercase();
                if text.contains("<@") || text.contains('@') || text.contains("mention") {
                    hits.push(SearchHit {
                        channel_id: channel.id,
                        channel_name: channel.name.clone(),
                        message: message.clone(),
                    });
                }
            }
        }
        hits
    }

    pub fn mark_mentions_read(&mut self) {
        for message in self.session.messages.values().flatten() {
            let text = message.content.to_ascii_lowercase();
            if text.contains("<@") || text.contains('@') || text.contains("mention") {
                self.read_mentions.push(message.id.get());
            }
        }
        self.inbox_open = false;
        self.note("Marked mentions read");
    }

    pub fn set_theme(&mut self, mode: ThemeMode) {
        self.prefs.custom = None;
        self.prefs.theme = mode;
    }

    pub fn appearance_account(&self) -> String {
        self.session
            .user
            .as_ref()
            .map(|user| user.id.get().to_string())
            .unwrap_or_else(|| "local".into())
    }

    pub fn load_appearance(&mut self) -> bool {
        let path = rencord_platform::appearance_path(&self.appearance_account());
        if let Some(file) = rencord_theme::read_appearance(&path) {
            self.prefs.apply_file(file);
            true
        } else {
            false
        }
    }

    pub fn save_appearance(&self) {
        let path = rencord_platform::appearance_path(&self.appearance_account());
        let _ = rencord_theme::write_appearance(&path, &self.prefs.to_file());
    }

    /// Use Discord's synced light/dark/client theme as a starting point.
    /// Nothing here writes appearance back to Discord. A later launch keeps
    /// the local file unless the user opted in to follow Discord.
    pub fn consider_discord_appearance(&mut self, theme: Option<&str>, client: Option<&str>) {
        let path = rencord_platform::appearance_path(&self.appearance_account());
        let saved = path.is_file();
        if !(self.prefs.sync_discord || (!saved && !self.prefs.seeded_from_discord)) {
            return;
        }
        if let Some(preset) = rencord_theme::map_discord_appearance(theme, client) {
            self.prefs.theme = preset;
            self.prefs.custom = None;
            self.prefs.seeded_from_discord = true;
            self.save_appearance();
        }
    }

    pub fn toggle_density(&mut self) {
        self.prefs.density = self.prefs.density.cycle();
    }

    pub fn bump_font(&mut self, direction: i32) {
        self.prefs.bump_font(direction);
    }

    pub fn toggle_hdr_send(&mut self) {
        self.prefs.hdr_send = !self.prefs.hdr_send;
    }

    pub fn toggle_hdr_receive(&mut self) {
        self.prefs.hdr_receive = !self.prefs.hdr_receive;
    }

    pub fn cycle_hdr_curve(&mut self) {
        self.prefs.hdr_curve = self.prefs.hdr_curve.cycle();
    }

    /// What the toggles would do for a 1 000 cd/m² master and an unknown audience.
    /// The Go Live worker still tone-maps, so the line says so.
    pub fn hdr_settings_summary(&self) -> String {
        let session = rencord_hdr::HdrSession::from_settings(
            self.prefs.hdr_send,
            self.prefs.hdr_receive,
            self.prefs.hdr_curve.tone_curve(),
        );
        let plan =
            rencord_hdr::HdrSignal::plan(&session, &rencord_hdr::SourceColorimetry::hdr10(1000.0));
        format!(
            "{} · {} · {}. {} The Go Live worker still tone-maps HDR to 8-bit.",
            if self.prefs.hdr_send {
                "Send on"
            } else {
                "Send off"
            },
            if self.prefs.hdr_receive {
                "Receive on"
            } else {
                "Receive off"
            },
            self.prefs.hdr_curve.label(),
            plan.summary()
        )
    }

    pub fn select_channel_timed(&mut self, channel_id: Snowflake) {
        let started = Instant::now();
        self.session.select_channel(channel_id);
        let _ = self.session.messages_for(channel_id);
        self.last_switch_us = started.elapsed().as_micros();
        self.search.clear();
    }

    pub fn step_channel(&mut self, delta: i32) {
        let mut ids: Vec<Snowflake> = self
            .session
            .channels
            .values()
            .filter(|channel| channel.kind.is_text() || channel.kind.is_voice())
            .map(|channel| channel.id)
            .collect();
        ids.sort_by_key(|id| id.get());
        if ids.is_empty() {
            return;
        }
        let current = self
            .session
            .selected_channel
            .and_then(|id| ids.iter().position(|item| *item == id))
            .unwrap_or(0);
        let next = (current as i32 + delta).rem_euclid(ids.len() as i32) as usize;
        self.select_channel_timed(ids[next]);
    }

    pub fn step_guild(&mut self, delta: i32) {
        if self.session.guilds.is_empty() {
            return;
        }
        let current = self
            .session
            .selected_guild
            .and_then(|id| self.session.guilds.iter().position(|guild| guild.id == id))
            .unwrap_or(0);
        let next = (current as i32 + delta).rem_euclid(self.session.guilds.len() as i32) as usize;
        let id = self.session.guilds[next].id;
        let started = Instant::now();
        self.session.select_guild(id);
        self.last_switch_us = started.elapsed().as_micros();
    }

    pub fn visible_messages(&self) -> Vec<Message> {
        let Some(channel_id) = self.session.selected_channel else {
            return Vec::new();
        };
        self.session.messages_for(channel_id).to_vec()
    }

    pub fn begin_reply(&mut self, id: Snowflake) {
        self.reply_to = Some(id);
        self.editing = None;
    }

    pub fn begin_edit(&mut self, id: Snowflake) {
        self.editing = Some(id);
        self.reply_to = None;
    }

    pub fn toggle_reaction(&mut self, message_id: u64, emoji: &str) -> Mutation {
        let channel_id = self.session.selected_channel.unwrap_or(Snowflake::new(0));
        let list = self.reactions.entry(message_id).or_default();
        if let Some(index) = list
            .iter()
            .position(|item| item.emoji == emoji && item.mine)
        {
            let existing = &mut list[index];
            existing.count = existing.count.saturating_sub(1);
            existing.mine = false;
            if existing.count == 0 {
                list.remove(index);
            }
            Mutation::React {
                channel_id,
                message_id,
                emoji: emoji.to_owned(),
                remove: true,
            }
        } else if let Some(existing) = list.iter_mut().find(|item| item.emoji == emoji) {
            existing.count = existing.count.saturating_add(1);
            existing.mine = true;
            Mutation::React {
                channel_id,
                message_id,
                emoji: emoji.to_owned(),
                remove: false,
            }
        } else {
            list.push(Reaction {
                emoji: emoji.to_owned(),
                count: 1,
                mine: true,
            });
            Mutation::React {
                channel_id,
                message_id,
                emoji: emoji.to_owned(),
                remove: false,
            }
        }
    }

    pub fn delete_local(&mut self, id: Snowflake) -> Option<Message> {
        let channel_id = self.session.selected_channel?;
        let list = self.session.messages.get_mut(&channel_id)?;
        let index = list.iter().position(|message| message.id == id)?;
        let message = list.remove(index);
        self.reactions.remove(&id.get());
        Some(message)
    }

    pub fn restore_message(&mut self, message: Message) {
        self.session.insert_message(message);
    }

    pub fn replace_message_content(&mut self, id: Snowflake, content: &str) -> Option<String> {
        let channel_id = self.session.selected_channel?;
        let list = self.session.messages.get_mut(&channel_id)?;
        let message = list.iter_mut().find(|message| message.id == id)?;
        let previous = message.content.clone();
        message.content = content.to_owned();
        Some(previous)
    }

    pub fn search_hits(&self) -> Vec<SearchHit> {
        local_search(&self.session, &self.search)
    }

    pub fn jump_to(&mut self, channel_id: Snowflake, message_id: Snowflake) {
        self.select_channel_timed(channel_id);
        self.jump_index = self
            .session
            .messages_for(channel_id)
            .iter()
            .position(|message| message.id == message_id);
    }

    pub fn take_jump(&mut self) -> Option<usize> {
        self.jump_index.take()
    }

    /// Guild search path for the signed-in account. `from:` and `in:` names are
    /// resolved against the cache when possible.
    pub fn remote_search_path(&self) -> Option<String> {
        let guild = self.session.selected_guild?;
        let query = parse_message_query(&self.search);
        let author_id = query.from.as_ref().and_then(|name| {
            self.session
                .messages
                .values()
                .flatten()
                .find_map(|message| {
                    let matches = message.author.display_name().eq_ignore_ascii_case(name)
                        || message.author.username.eq_ignore_ascii_case(name)
                        || message.author.id.get().to_string() == *name;
                    matches.then_some(message.author.id)
                })
        });
        let channel_id = query.in_channel.as_ref().and_then(|name| {
            self.session
                .channels
                .values()
                .find(|channel| channel.name.eq_ignore_ascii_case(name))
                .map(|channel| channel.id)
        });
        Some(guild_search_path(guild, &query, author_id, channel_id))
    }

    pub fn palette_commands(&self) -> Vec<String> {
        let mut commands = vec![
            "Toggle theme".into(),
            "Toggle density".into(),
            "Open settings".into(),
            "Join voice".into(),
            "Toggle mute".into(),
        ];
        for channel in self.session.channels.values() {
            commands.push(format!("#{}", channel.name));
        }
        if self.palette_query.is_empty() {
            commands
        } else {
            let query = self.palette_query.to_ascii_lowercase();
            commands
                .into_iter()
                .filter(|command| command.to_ascii_lowercase().contains(&query))
                .collect()
        }
    }

    pub fn run_palette_command(&mut self, command: &str) {
        self.palette_open = false;
        match command {
            "Toggle theme" => self.cycle_theme(),
            "Toggle density" => self.toggle_density(),
            "Open settings" => self.settings_open = true,
            "Join voice" => self.join_voice_demo(),
            "Toggle mute" => self.toggle_mute(),
            other if other.starts_with('#') => {
                let name = &other[1..];
                if let Some(channel) = self
                    .session
                    .channels
                    .values()
                    .find(|channel| channel.name == name)
                    .map(|channel| channel.id)
                {
                    self.select_channel_timed(channel);
                }
            }
            _ => {}
        }
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.note(if self.muted { "Muted" } else { "Unmuted" });
    }

    pub fn toggle_deaf(&mut self) {
        self.deafened = !self.deafened;
        if self.deafened {
            self.muted = true;
        }
        self.note(if self.deafened {
            "Deafened"
        } else {
            "Undeafened"
        });
    }

    pub fn set_speaker_gain(&mut self, gain: f32) {
        self.speaker_gain = gain.clamp(0.0, 2.0);
    }

    /// Text snapshot of the shell, used by tests in place of a screenshot.
    pub fn outline(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("status: {}", self.session.status_line));
        if let Some(user) = &self.session.user {
            lines.push(format!("user: {}", user.display_name()));
        }
        for guild in &self.session.guilds {
            let mark = if self.session.selected_guild == Some(guild.id) {
                "*"
            } else {
                " "
            };
            lines.push(format!("{mark} guild {}", guild.name));
            for channel in self.session.guild_channels(guild.id) {
                let current = if self.session.selected_channel == Some(channel.id) {
                    ">"
                } else {
                    " "
                };
                lines.push(format!("  {current} #{}", channel.name));
            }
        }
        lines.push("dms:".into());
        for channel in self.session.dm_channels() {
            lines.push(format!("  {}", channel.name));
        }
        if let Some(channel_id) = self.session.selected_channel {
            lines.push(format!("timeline {channel_id}:"));
            for message in self.session.messages_for(channel_id) {
                lines.push(format!(
                    "  {}: {}",
                    message.author.display_name(),
                    message.content.replace('\n', " ")
                ));
            }
        }
        if let Some(report) = &self.voice_report {
            lines.push(format!(
                "voice played={} hard_gaps={} target={}",
                report.played, report.hard_gaps, report.target_ms
            ));
        }
        lines.push(format!(
            "mute={} deaf={} gain={:.2}",
            self.muted, self.deafened, self.speaker_gain
        ));
        lines.push(format!(
            "theme={} density={} font={}",
            self.prefs.theme_label(),
            self.prefs.density.label(),
            self.prefs.font_px
        ));
        lines.push(format!(
            "hdr send={} receive={} curve={}",
            self.prefs.hdr_send,
            self.prefs.hdr_receive,
            self.prefs.hdr_curve.label()
        ));
        lines.push(token_summary());
        lines.push(format!(
            "icon-pack {}",
            discord_install_icon_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| icon_pack_dir().display().to_string())
        ));
        lines.join("\n")
    }
}

#[cfg(test)]
fn visit_rs(dir: &std::path::Path, offenders: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_rs(&path, offenders);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            if line.contains("hex-lint-allow") {
                continue;
            }
            if line_has_hex_color(line) {
                offenders.push(format!("{}:{}", path.display(), index + 1));
            }
        }
    }
}

#[cfg(test)]
fn line_has_hex_color(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#' && exact_hex(&bytes[index + 1..]) {
            return true;
        }
        if index + 1 < bytes.len()
            && bytes[index] == b'0'
            && (bytes[index + 1] == b'x' || bytes[index + 1] == b'X')
            && exact_hex(&bytes[index + 2..])
        {
            return true;
        }
        index += 1;
    }
    false
}

#[cfg(test)]
fn exact_hex(bytes: &[u8]) -> bool {
    for length in [6, 8] {
        if bytes.len() >= length
            && bytes[..length].iter().all(|byte| byte.is_ascii_hexdigit())
            && bytes
                .get(length)
                .map(|byte| !byte.is_ascii_hexdigit())
                .unwrap_or(true)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_shell_leads_with_the_code_not_a_pasted_token() {
        let shell = Shell::login();
        let text = shell.log.join(" ").to_lowercase();
        assert!(text.contains("scan"));
        assert!(!text.contains("paste a user token"));
    }

    #[test]
    fn demo_outline_lists_guilds_channels_dms_and_markdown_source() {
        let shell = Shell::demo();
        let outline = shell.outline();
        assert!(outline.contains("Analytical Engines"));
        assert!(outline.contains("#general"));
        assert!(outline.contains("#voice"));
        assert!(outline.contains("Charles"));
        assert!(outline.contains("fn main()"));
    }

    #[test]
    fn sending_appends_a_message_and_voice_loopback_reports_audio() {
        let mut shell = Shell::demo();
        shell.send_local("ping from the composer");
        assert!(shell.outline().contains("ping from the composer"));
        shell.join_voice_demo();
        let report = shell.voice_report.as_ref().expect("report");
        assert!(report.played > 70, "{report:?}");
        assert!(report.hard_gaps < 25, "{report:?}");
        assert!(shell.outline().contains("voice played="));
        shell.toggle_mute();
        shell.set_speaker_gain(0.5);
        shell.toggle_deaf();
        let outline = shell.outline();
        assert!(outline.contains("mute=true"));
        assert!(outline.contains("deaf=true"));
        assert!(outline.contains("gain=0.50"));
    }

    #[test]
    fn cached_channel_switch_stays_under_the_budget() {
        let mut shell = Shell::demo_history(1500);
        for _ in 0..40 {
            shell.step_channel(1);
        }
        assert!(
            shell.last_switch_us < 50_000,
            "switch took {} us",
            shell.last_switch_us
        );
        assert!(shell.visible_messages().len() > 100);
        let mut sheet = Shell::demo();
        sheet.flow = rencord_upload::Flow::preview_sheet();
        assert!(sheet
            .flow
            .lines()
            .iter()
            .any(|line| line.contains("lecture.mp4")));
        sheet.close_layers();
        assert!(!sheet.flow.open);
        shell.search = "from:charles has:code".into();
        assert!(!shell.search_hits().is_empty());
        let hit = shell.search_hits()[0].clone();
        shell.jump_to(hit.channel_id, hit.message.id);
        assert_eq!(shell.session.selected_channel, Some(hit.channel_id));
        shell.begin_edit(Snowflake::new(1000));
        let edit = shell.send_local("changed").expect("edit");
        shell.rollback(&edit);
        assert!(shell
            .session
            .messages_for(Snowflake::new(100))
            .iter()
            .any(
                |message| message.id == Snowflake::new(1000) && message.content.contains("fn main")
            ));
        shell.cycle_theme();
        assert_eq!(shell.prefs.theme, ThemeMode::Dark);
        assert!(!shell.prefs.hdr_send);
        assert!(shell.hdr_settings_summary().contains("Send off"));
        assert!(shell.hdr_settings_summary().contains("tone-maps"));
        shell.toggle_hdr_send();
        shell.toggle_hdr_receive();
        shell.cycle_hdr_curve();
        assert!(shell.prefs.hdr_send && shell.prefs.hdr_receive);
        assert_eq!(shell.prefs.hdr_curve, HdrCurveChoice::Hable);
        assert!(shell.outline().contains("hdr send=true"));
        shell.begin_reply(Snowflake::new(1000));
        shell.send_local("noted");
        assert!(shell.outline().contains("replying to 1000"));
    }

    #[test]
    fn ui_sources_do_not_embed_hex_colors() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        super::visit_rs(&root, &mut offenders);
        assert!(
            offenders.is_empty(),
            "raw hex colors belong in rencord-theme:\n{}",
            offenders.join("\n")
        );
    }
}
