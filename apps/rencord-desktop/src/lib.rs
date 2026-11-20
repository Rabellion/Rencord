//! Desktop session controller. The GPUI binary renders this; tests drive it
//! without opening a window.

mod theme;

use rencord_model::{Message, Snowflake, User};
use rencord_platform::{FileSecretStore, SecretStore};
use rencord_store::Session;
use rencord_voice::{backend_name, loopback_call, LoopbackReport};
use std::collections::HashMap;
use std::time::Instant;

pub use theme::{token_summary, Density, Palette, Preferences, ThemeMode, MOTION_OVERLAY_MS};

#[derive(Clone, Debug)]
pub struct Reaction {
    pub emoji: String,
    pub count: u32,
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
        };
        shell.reactions.insert(
            1000,
            vec![Reaction {
                emoji: "wave".into(),
                count: 2,
            }],
        );
        shell
    }

    pub fn demo() -> Self {
        Self::fresh(
            Session::demo(),
            true,
            vec!["Demo data is local. Nothing is sent to Discord.".into()],
        )
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
        shell
    }

    pub fn login() -> Self {
        Self::fresh(
            Session::default(),
            false,
            vec!["Paste a user token. Official webview and QR login are not in this build.".into()],
        )
    }

    pub fn remember_token(token: &str) -> Result<(), String> {
        let store = FileSecretStore::in_data_dir();
        store
            .save_token(token.trim())
            .map_err(|err| err.to_string())
    }

    pub fn load_token() -> Option<String> {
        FileSecretStore::in_data_dir().load_token().ok().flatten()
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

    pub fn send_local(&mut self, content: &str) {
        let content = content.trim();
        if content.is_empty() {
            return;
        }
        let Some(channel_id) = self.session.selected_channel else {
            self.note("Select a channel before sending.");
            return;
        };
        if let Some(editing) = self.editing.take() {
            if let Some(list) = self.session.messages.get_mut(&channel_id) {
                if let Some(message) = list.iter_mut().find(|message| message.id == editing) {
                    message.content = content.to_owned();
                    self.note(format!("Edited message {editing}"));
                    return;
                }
            }
        }
        let prefix = if let Some(reply) = self.reply_to.take() {
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
    }

    pub fn cycle_theme(&mut self) {
        self.prefs.theme = self.prefs.theme.cycle();
    }

    pub fn set_theme(&mut self, mode: ThemeMode) {
        self.prefs.theme = mode;
    }

    pub fn toggle_density(&mut self) {
        self.prefs.density = self.prefs.density.toggle();
    }

    pub fn bump_font(&mut self, direction: i32) {
        self.prefs.bump_font(direction);
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
        let query = self.search.trim().to_ascii_lowercase();
        self.session
            .messages_for(channel_id)
            .iter()
            .filter(|message| {
                query.is_empty()
                    || message.content.to_ascii_lowercase().contains(&query)
                    || message
                        .author
                        .display_name()
                        .to_ascii_lowercase()
                        .contains(&query)
            })
            .cloned()
            .collect()
    }

    pub fn begin_reply(&mut self, id: Snowflake) {
        self.reply_to = Some(id);
        self.editing = None;
    }

    pub fn begin_edit(&mut self, id: Snowflake) {
        self.editing = Some(id);
        self.reply_to = None;
    }

    pub fn toggle_reaction(&mut self, message_id: u64, emoji: &str) {
        let list = self.reactions.entry(message_id).or_default();
        if let Some(existing) = list.iter_mut().find(|item| item.emoji == emoji) {
            existing.count = existing.count.saturating_add(1);
        } else {
            list.push(Reaction {
                emoji: emoji.to_owned(),
                count: 1,
            });
        }
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
            self.prefs.theme.label(),
            self.prefs.density.label(),
            self.prefs.font_px
        ));
        lines.push(token_summary());
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_outline_lists_guilds_channels_dms_and_markdown_source() {
        let shell = Shell::demo();
        let outline = shell.outline();
        assert!(outline.contains("Analytical Engines"));
        assert!(outline.contains("#general"));
        assert!(outline.contains("#voice"));
        assert!(outline.contains("Charles"));
        assert!(outline.contains("**Rencord**"));
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
        shell.search = "bold".into();
        assert!(shell.visible_messages().iter().all(|message| {
            message.content.to_ascii_lowercase().contains("bold")
                || message
                    .author
                    .display_name()
                    .to_ascii_lowercase()
                    .contains("bold")
        }));
        shell.cycle_theme();
        assert_eq!(shell.prefs.theme, ThemeMode::Light);
        shell.begin_reply(Snowflake::new(1000));
        shell.send_local("noted");
        assert!(shell.outline().contains("replying to 1000"));
    }
}
