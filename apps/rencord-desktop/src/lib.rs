//! Desktop session controller. The GPUI binary renders this; tests drive it
//! without opening a window.

use rencord_model::{Message, Snowflake, User};
use rencord_platform::{FileSecretStore, SecretStore};
use rencord_store::Session;
use rencord_voice::{backend_name, loopback_call, LoopbackReport};

#[derive(Clone, Debug)]
pub struct Shell {
    pub session: Session,
    pub demo: bool,
    pub voice_report: Option<LoopbackReport>,
    pub audio_backend: &'static str,
    pub log: Vec<String>,
}

impl Shell {
    pub fn demo() -> Self {
        Self {
            session: Session::demo(),
            demo: true,
            voice_report: None,
            audio_backend: backend_name(),
            log: vec!["Demo data is local. Nothing is sent to Discord.".into()],
        }
    }

    pub fn login() -> Self {
        Self {
            session: Session::default(),
            demo: false,
            voice_report: None,
            audio_backend: backend_name(),
            log: vec![
                "Paste a user token. Official webview and QR login are not in this build.".into(),
            ],
        }
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
            content: content.to_owned(),
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
    }
}
