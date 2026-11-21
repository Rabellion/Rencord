//! Session state for the desktop shell.
//!
//! The in-memory session is what the UI reads. SQLite keeps a bounded copy of
//! recent messages and the composer draft so a restart can refill the timeline.
//! Neither store is encrypted. Tokens live in `rencord-platform`, not here.

use rencord_gateway::GatewayEvent;
use rencord_model::{Channel, ChannelKind, Guild, Message, Snowflake, User};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;

const MESSAGE_CAP: usize = 4000;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub user: Option<User>,
    pub session_id: Option<String>,
    pub guilds: Vec<Guild>,
    pub channels: HashMap<Snowflake, Channel>,
    pub messages: HashMap<Snowflake, Vec<Message>>,
    pub selected_guild: Option<Snowflake>,
    pub selected_channel: Option<Snowflake>,
    pub composer: String,
    pub voice_channel: Option<Snowflake>,
    pub status_line: String,
    pub discord_theme: Option<String>,
    pub discord_client_theme: Option<String>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            user: None,
            session_id: None,
            guilds: Vec::new(),
            channels: HashMap::new(),
            messages: HashMap::new(),
            selected_guild: None,
            selected_channel: None,
            composer: String::new(),
            voice_channel: None,
            status_line: "Not connected".to_owned(),
            discord_theme: None,
            discord_client_theme: None,
        }
    }
}

impl Session {
    pub fn apply_gateway(&mut self, event: GatewayEvent) {
        match event {
            GatewayEvent::Ready {
                session_id,
                user,
                guild_ids,
                discord_theme,
                discord_client_theme,
            } => {
                self.discord_theme = discord_theme;
                self.discord_client_theme = discord_client_theme;
                self.session_id = Some(session_id);
                self.user = Some(user);
                for id in guild_ids {
                    if !self.guilds.iter().any(|guild| guild.id == id) {
                        self.guilds.push(Guild {
                            id,
                            name: format!("Guild {id}"),
                            icon: None,
                        });
                    }
                }
                self.status_line = "Gateway ready".to_owned();
            }
            GatewayEvent::MessageCreate(message) => self.insert_message(message),
            GatewayEvent::MessageDelete {
                channel_id,
                message_id,
            } => {
                if let Some(list) = self.messages.get_mut(&channel_id) {
                    list.retain(|message| message.id != message_id);
                }
            }
            GatewayEvent::MessageUpdate(message) => {
                if let Some(list) = self.messages.get_mut(&message.channel_id) {
                    if let Some(existing) = list.iter_mut().find(|item| item.id == message.id) {
                        *existing = message;
                        return;
                    }
                }
                let channel_id = message.channel_id;
                self.messages.entry(channel_id).or_default().push(message);
            }
            GatewayEvent::VoiceState(state) => {
                if self
                    .user
                    .as_ref()
                    .is_some_and(|user| user.id == state.user_id)
                {
                    self.voice_channel = state.channel_id;
                    self.status_line = match state.channel_id {
                        Some(channel) => format!("Voice channel {channel}"),
                        None => "Left voice".to_owned(),
                    };
                }
            }
            GatewayEvent::VoiceServer(update) => {
                if update.endpoint.is_some() {
                    self.status_line = "Voice server assigned".to_owned();
                }
            }
            GatewayEvent::Reconnect => {
                self.status_line = "Gateway asked to reconnect".to_owned();
            }
            GatewayEvent::Resumed => {
                self.status_line = "Gateway resumed".to_owned();
            }
            GatewayEvent::Other { name } => {
                self.status_line = format!("Event {name}");
            }
        }
    }

    pub fn upsert_guild(&mut self, guild: Guild) {
        if let Some(existing) = self.guilds.iter_mut().find(|item| item.id == guild.id) {
            *existing = guild;
        } else {
            self.guilds.push(guild);
        }
    }

    pub fn upsert_channel(&mut self, channel: Channel) {
        self.channels.insert(channel.id, channel);
    }

    pub fn insert_message(&mut self, message: Message) {
        let list = self.messages.entry(message.channel_id).or_default();
        if list.iter().any(|item| item.id == message.id) {
            return;
        }
        list.push(message);
        if list.len() > MESSAGE_CAP {
            let overflow = list.len() - MESSAGE_CAP;
            list.drain(0..overflow);
        }
    }

    pub fn guild_channels(&self, guild_id: Snowflake) -> Vec<&Channel> {
        let mut channels: Vec<_> = self
            .channels
            .values()
            .filter(|channel| channel.guild_id == Some(guild_id))
            .collect();
        channels.sort_by_key(|channel| (channel.position, channel.id.get()));
        channels
    }

    pub fn dm_channels(&self) -> Vec<&Channel> {
        let mut channels: Vec<_> = self
            .channels
            .values()
            .filter(|channel| {
                matches!(channel.kind, ChannelKind::Dm | ChannelKind::GroupDm)
                    || channel.guild_id.is_none() && channel.kind.is_text()
            })
            .collect();
        channels.sort_by(|a, b| a.name.cmp(&b.name));
        channels
    }

    pub fn messages_for(&self, channel_id: Snowflake) -> &[Message] {
        self.messages
            .get(&channel_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn select_guild(&mut self, guild_id: Snowflake) {
        self.selected_guild = Some(guild_id);
        self.selected_channel = self
            .guild_channels(guild_id)
            .into_iter()
            .find(|channel| channel.kind.is_text())
            .map(|channel| channel.id);
    }

    pub fn select_channel(&mut self, channel_id: Snowflake) {
        if let Some(channel) = self.channels.get(&channel_id) {
            self.selected_guild = channel.guild_id;
        }
        self.selected_channel = Some(channel_id);
    }

    /// Demo fixture used by `--demo` and by tests. No network.
    pub fn demo() -> Self {
        let mut session = Self::default();
        let user = User {
            id: Snowflake::new(1),
            username: "ada".into(),
            global_name: Some("Ada".into()),
            discriminator: None,
            bot: false,
            avatar: None,
        };
        session.user = Some(user.clone());
        session.status_line = "Demo session".into();
        session.upsert_guild(Guild {
            id: Snowflake::new(10),
            name: "Analytical Engines".into(),
            icon: None,
        });
        session.upsert_guild(Guild {
            id: Snowflake::new(11),
            name: "Difference Club".into(),
            icon: None,
        });
        session.upsert_channel(Channel {
            id: Snowflake::new(100),
            name: "general".into(),
            kind: ChannelKind::GuildText,
            guild_id: Some(Snowflake::new(10)),
            parent_id: None,
            position: 0,
            recipient_ids: Vec::new(),
        });
        session.upsert_channel(Channel {
            id: Snowflake::new(101),
            name: "voice".into(),
            kind: ChannelKind::GuildVoice,
            guild_id: Some(Snowflake::new(10)),
            parent_id: None,
            position: 1,
            recipient_ids: Vec::new(),
        });
        session.upsert_channel(Channel {
            id: Snowflake::new(110),
            name: "notes".into(),
            kind: ChannelKind::GuildText,
            guild_id: Some(Snowflake::new(11)),
            parent_id: None,
            position: 0,
            recipient_ids: Vec::new(),
        });
        session.upsert_channel(Channel {
            id: Snowflake::new(200),
            name: "Charles".into(),
            kind: ChannelKind::Dm,
            guild_id: None,
            parent_id: None,
            position: 0,
            recipient_ids: vec![Snowflake::new(2)],
        });
        session.insert_message(Message {
            id: Snowflake::new(1000),
            channel_id: Snowflake::new(100),
            author: user,
            content: "Hello **Rencord**. Mention <@2>, a [link](https://example.com), and `code`.\n\n```\nfn main() {}\n```"
                .into(),
            timestamp: "2026-11-20T00:00:00Z".into(),
        });
        session.insert_message(Message {
            id: Snowflake::new(2000),
            channel_id: Snowflake::new(200),
            author: User {
                id: Snowflake::new(2),
                username: "charles".into(),
                global_name: Some("Charles".into()),
                discriminator: None,
                bot: false,
                avatar: None,
            },
            content: "The voice channel is the one named voice.".into(),
            timestamp: "2026-11-20T00:01:00Z".into(),
        });
        session.select_guild(Snowflake::new(10));
        session
    }
}

pub struct LocalStore {
    connection: Connection,
}

impl LocalStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                channel_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                body TEXT NOT NULL,
                PRIMARY KEY (channel_id, message_id)
            );
            CREATE TABLE IF NOT EXISTS drafts (
                channel_id TEXT PRIMARY KEY,
                body TEXT NOT NULL
            );",
        )?;
        Ok(Self { connection })
    }

    pub fn save_message(&self, message: &Message) -> Result<(), StoreError> {
        let body = serde_json::to_string(message)?;
        self.connection.execute(
            "INSERT OR REPLACE INTO messages (channel_id, message_id, body) VALUES (?1, ?2, ?3)",
            params![message.channel_id.to_string(), message.id.to_string(), body],
        )?;
        Ok(())
    }

    pub fn load_messages(&self, channel_id: Snowflake) -> Result<Vec<Message>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT body FROM messages WHERE channel_id = ?1 ORDER BY message_id ASC")?;
        let rows = statement.query_map(params![channel_id.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(serde_json::from_str(&row?)?);
        }
        Ok(messages)
    }

    pub fn save_draft(&self, channel_id: Snowflake, body: &str) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO drafts (channel_id, body) VALUES (?1, ?2)",
            params![channel_id.to_string(), body],
        )?;
        Ok(())
    }

    pub fn load_draft(&self, channel_id: Snowflake) -> Result<Option<String>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT body FROM drafts WHERE channel_id = ?1")?;
        let mut rows = statement.query(params![channel_id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rencord_gateway::GatewayEvent;

    #[test]
    fn demo_session_has_guilds_dms_and_messages() {
        let session = Session::demo();
        assert_eq!(session.guilds.len(), 2);
        assert_eq!(session.guild_channels(Snowflake::new(10)).len(), 2);
        assert_eq!(session.dm_channels().len(), 1);
        assert_eq!(session.messages_for(Snowflake::new(100)).len(), 1);
        assert_eq!(session.selected_channel, Some(Snowflake::new(100)));
    }

    #[test]
    fn gateway_message_lands_in_the_timeline() {
        let mut session = Session::demo();
        session.apply_gateway(GatewayEvent::MessageCreate(Message {
            id: Snowflake::new(1001),
            channel_id: Snowflake::new(100),
            author: session.user.clone().unwrap(),
            content: "live".into(),
            timestamp: "t".into(),
        }));
        let messages = session.messages_for(Snowflake::new(100));
        assert_eq!(messages.last().unwrap().content, "live");
    }

    #[test]
    fn sqlite_roundtrip() {
        let path =
            std::env::temp_dir().join(format!("rencord-store-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = LocalStore::open(&path).unwrap();
        let message = Session::demo().messages_for(Snowflake::new(100))[0].clone();
        store.save_message(&message).unwrap();
        store.save_draft(message.channel_id, "draft").unwrap();
        let loaded = store.load_messages(message.channel_id).unwrap();
        assert_eq!(loaded[0].content, message.content);
        assert_eq!(
            store.load_draft(message.channel_id).unwrap().as_deref(),
            Some("draft")
        );
        let _ = std::fs::remove_file(&path);
    }
}
