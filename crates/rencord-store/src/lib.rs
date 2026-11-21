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
    /// Reaction counts keyed by message id. Gateway events maintain this.
    pub reactions: HashMap<u64, Vec<(String, u32)>>,
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
            reactions: HashMap::new(),
        }
    }
}

impl Session {
    pub fn apply_gateway(&mut self, event: GatewayEvent) {
        match event {
            GatewayEvent::Ready {
                session_id,
                user,
                guilds,
                discord_theme,
                discord_client_theme,
            } => {
                self.discord_theme = discord_theme;
                self.discord_client_theme = discord_client_theme;
                self.session_id = Some(session_id);
                self.user = Some(user);
                for guild in guilds {
                    self.upsert_ready_guild(guild);
                }
                self.status_line = "Gateway ready".to_owned();
            }
            GatewayEvent::GuildUpsert {
                id,
                name,
                icon,
                channels,
            } => {
                self.apply_guild(id, name, icon);
                for channel in channels {
                    self.upsert_channel(channel);
                }
            }
            GatewayEvent::GuildDelete(id) => self.remove_guild(id),
            GatewayEvent::ChannelUpsert(channel) => self.upsert_channel(channel),
            GatewayEvent::ChannelDelete(id) => {
                self.channels.remove(&id);
                if self.selected_channel == Some(id) {
                    self.selected_channel = self.selected_guild.and_then(|guild| {
                        self.guild_channels(guild)
                            .into_iter()
                            .next()
                            .map(|channel| channel.id)
                    });
                }
            }
            GatewayEvent::Reaction {
                message_id,
                emoji,
                remove,
            } => self.apply_reaction(message_id.get(), emoji, !remove),
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

    fn upsert_ready_guild(&mut self, guild: rencord_protocol::ReadyGuild) {
        let channels = guild.channels;
        let id = guild.id;
        self.apply_guild(id, guild.name, guild.icon);
        for wire in channels {
            let mut channel = wire.into_channel();
            if channel.guild_id.is_none() {
                channel.guild_id = Some(id);
            }
            self.upsert_channel(channel);
        }
    }

    fn apply_guild(&mut self, id: Snowflake, name: Option<String>, icon: Option<String>) {
        if let Some(existing) = self.guilds.iter_mut().find(|guild| guild.id == id) {
            if let Some(name) = name {
                existing.name = name;
            }
            if icon.is_some() {
                existing.icon = icon;
            }
            return;
        }
        self.guilds.push(Guild {
            id,
            name: name.unwrap_or_else(|| format!("Guild {id}")),
            icon,
        });
    }

    pub fn remove_guild(&mut self, id: Snowflake) {
        self.guilds.retain(|guild| guild.id != id);
        self.channels
            .retain(|_, channel| channel.guild_id != Some(id));
        if self.selected_guild == Some(id) {
            self.selected_guild = self.guilds.first().map(|guild| guild.id);
            self.selected_channel = self
                .selected_guild
                .and_then(|guild| self.guild_channels(guild).into_iter().next().map(|c| c.id));
        }
    }

    fn apply_reaction(&mut self, message_id: u64, emoji: String, add: bool) {
        let list = self.reactions.entry(message_id).or_default();
        if let Some(existing) = list.iter_mut().find(|item| item.0 == emoji) {
            if add {
                existing.1 = existing.1.saturating_add(1);
            } else {
                existing.1 = existing.1.saturating_sub(1);
            }
        } else if add {
            list.push((emoji, 1));
        }
        list.retain(|item| item.1 > 0);
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

    /// Adds fetched or confirmed messages. Snowflakes grow with time, so the
    /// channel stays in id order with no duplicates however they arrive.
    pub fn merge_history(&mut self, channel_id: Snowflake, messages: Vec<Message>) {
        let list = self.messages.entry(channel_id).or_default();
        list.extend(messages);
        list.sort_by_key(|message| message.id.get());
        list.dedup_by_key(|message| message.id.get());
        if list.len() > MESSAGE_CAP {
            let overflow = list.len() - MESSAGE_CAP;
            list.drain(0..overflow);
        }
    }

    /// Older pages stay at the front. Past 4000 messages, the newest (the far
    /// end of a scroll-back) are dropped so the page just loaded remains.
    pub fn merge_older(&mut self, channel_id: Snowflake, messages: Vec<Message>) -> usize {
        let list = self.messages.entry(channel_id).or_default();
        let known: std::collections::HashSet<u64> =
            list.iter().map(|message| message.id.get()).collect();
        list.extend(messages);
        list.sort_by_key(|message| message.id.get());
        list.dedup_by_key(|message| message.id.get());
        let prepended = list
            .iter()
            .take_while(|message| !known.contains(&message.id.get()))
            .count();
        if list.len() > MESSAGE_CAP {
            let overflow = list.len() - MESSAGE_CAP;
            let end = list.len();
            list.drain(end - overflow..);
        }
        prepended
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
    fn history_merges_in_order_without_duplicates() {
        let mut session = Session::demo();
        let channel = Snowflake::new(500);
        let author = session.user.clone().unwrap();
        let at = |id: u64| Message {
            id: Snowflake::new(id),
            channel_id: channel,
            author: author.clone(),
            content: id.to_string(),
            timestamp: "t".into(),
        };
        // A live message arrives first; history comes back newest-first.
        session.insert_message(at(30));
        session.merge_history(channel, vec![at(30), at(20), at(10)]);
        let ids: Vec<u64> = session
            .messages_for(channel)
            .iter()
            .map(|m| m.id.get())
            .collect();
        assert_eq!(ids, [10, 20, 30]);
    }

    #[test]
    fn older_history_keeps_the_new_page_and_trims_the_newest() {
        let mut session = Session::default();
        let channel = Snowflake::new(7);
        let author = User {
            id: Snowflake::new(1),
            username: "ada".into(),
            global_name: None,
            discriminator: None,
            bot: false,
            avatar: None,
        };
        let at = |id: u64| Message {
            id: Snowflake::new(id),
            channel_id: channel,
            author: author.clone(),
            content: id.to_string(),
            timestamp: "t".into(),
        };
        for id in 50..4050 {
            session.insert_message(at(id));
        }
        let prepended = session.merge_older(channel, vec![at(1), at(2), at(3)]);
        assert_eq!(prepended, 3);
        let ids: Vec<u64> = session
            .messages_for(channel)
            .iter()
            .map(|message| message.id.get())
            .collect();
        assert_eq!(ids.len(), 4000);
        assert_eq!(ids[0], 1);
        assert_eq!(*ids.last().unwrap(), 4046);
    }

    #[test]
    fn ready_placeholder_is_replaced_by_guild_create_and_delete_removes_it() {
        let mut session = Session::default();
        session.apply_gateway(GatewayEvent::Ready {
            session_id: "s".into(),
            user: User {
                id: Snowflake::new(1),
                username: "ada".into(),
                global_name: None,
                discriminator: None,
                bot: false,
                avatar: None,
            },
            guilds: vec![rencord_protocol::ReadyGuild {
                id: Snowflake::new(10),
                name: None,
                icon: None,
                unavailable: true,
                channels: Vec::new(),
            }],
            discord_theme: None,
            discord_client_theme: None,
        });
        assert_eq!(session.guilds[0].name, "Guild 10");
        session.apply_gateway(GatewayEvent::GuildUpsert {
            id: Snowflake::new(10),
            name: Some("Analytical Engines".into()),
            icon: Some("abc".into()),
            channels: vec![Channel {
                id: Snowflake::new(100),
                name: "general".into(),
                kind: ChannelKind::GuildText,
                guild_id: None,
                parent_id: None,
                position: 0,
                recipient_ids: Vec::new(),
            }],
        });
        assert_eq!(session.guilds[0].name, "Analytical Engines");
        assert_eq!(session.guilds[0].icon.as_deref(), Some("abc"));
        assert_eq!(session.channels[&Snowflake::new(100)].guild_id, None);
        session.apply_gateway(GatewayEvent::ChannelUpsert(Channel {
            id: Snowflake::new(100),
            name: "lounge".into(),
            kind: ChannelKind::GuildText,
            guild_id: Some(Snowflake::new(10)),
            parent_id: None,
            position: 1,
            recipient_ids: Vec::new(),
        }));
        assert_eq!(session.channels[&Snowflake::new(100)].name, "lounge");
        session.apply_gateway(GatewayEvent::Reaction {
            message_id: Snowflake::new(5),
            emoji: "wave".into(),
            remove: false,
        });
        session.apply_gateway(GatewayEvent::Reaction {
            message_id: Snowflake::new(5),
            emoji: "wave".into(),
            remove: true,
        });
        assert!(session
            .reactions
            .get(&5)
            .map(|list| list.is_empty())
            .unwrap_or(true));
        session.apply_gateway(GatewayEvent::GuildDelete(Snowflake::new(10)));
        assert!(session.guilds.is_empty());
        assert!(session.channels.is_empty());
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
