//! Session state for the desktop shell.
//!
//! The in-memory session is what the UI reads. SQLite keeps a bounded copy of
//! recent messages and the composer draft so a restart can refill the timeline.
//! Neither store is encrypted. Tokens live in `rencord-platform`, not here.

use rencord_gateway::{GatewayEvent, MessagePatch};
use rencord_model::{
    Channel, ChannelKind, Guild, Member, Message, PresenceStatus, Role, Snowflake, User,
};
mod members;

pub use members::{apply_member_list, RosterGroup, RosterMember};
use rusqlite::{params, Connection};

use std::collections::{HashMap, HashSet};
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
    /// Channels with a message that arrived while they were not on screen.
    pub unread: HashSet<Snowflake>,
    /// What each server knows about its people, by server and then user: the
    /// nickname and role ids that decide the name shown and its colour.
    pub members: HashMap<Snowflake, HashMap<Snowflake, Member>>,
    /// Role groups for the guild currently subscribed with opcode 14.
    pub roster: Vec<RosterGroup>,
    /// Last read message id per channel, from READY `read_state`.
    pub read_marker: HashMap<u64, u64>,
    pub mention_counts: HashMap<u64, u32>,
    /// Presence line per user id: status, then activity or custom status.
    pub presences: HashMap<u64, (String, String)>,
    pub custom_status: String,
    /// Voice channel id to the people sitting in it.
    pub voice_seats: HashMap<u64, Vec<VoiceSeat>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceSeat {
    pub user_id: u64,
    pub name: String,
    pub avatar: Option<String>,
    pub mute: bool,
    pub deaf: bool,
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
            unread: HashSet::new(),
            members: HashMap::new(),
            roster: Vec::new(),
            read_marker: HashMap::new(),
            mention_counts: HashMap::new(),
            presences: HashMap::new(),
            custom_status: String::new(),
            voice_seats: HashMap::new(),
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
                read_state,
                presences,
                voice_states,
            } => {
                self.discord_theme = discord_theme;
                self.discord_client_theme = discord_client_theme;
                self.session_id = Some(session_id);
                let self_id = user.id.get();
                self.user = Some(user);
                for guild in guilds {
                    self.upsert_ready_guild(guild);
                }
                self.apply_ready_extras(self_id, read_state, presences, voice_states);
                self.status_line = "Gateway ready".to_owned();
            }
            GatewayEvent::GuildUpsert {
                id,
                name,
                icon,
                banner,
                roles,
                members,
                presences,
                channels,
            } => {
                self.apply_guild(id, name, icon, banner);
                if let Some(roles) = roles {
                    self.set_roles(id, roles);
                }
                self.members.entry(id).or_default().extend(members);
                for (user, status) in presences {
                    self.set_presence(user, status);
                }
                for channel in channels {
                    self.upsert_channel(channel);
                }
            }
            GatewayEvent::Presence { user_id, status } => {
                self.set_presence(user_id, status);
            }
            GatewayEvent::MemberUpdate {
                guild_id,
                user_id,
                member,
            } => {
                self.members
                    .entry(guild_id)
                    .or_default()
                    .insert(user_id, member);
            }
            GatewayEvent::MembersChunk { guild_id, members } => {
                self.members.entry(guild_id).or_default().extend(members);
            }
            GatewayEvent::RoleUpsert { guild_id, role } => {
                if let Some(guild) = self.guilds.iter_mut().find(|guild| guild.id == guild_id) {
                    match guild.roles.iter_mut().find(|item| item.id == role.id) {
                        Some(existing) => *existing = role,
                        None => guild.roles.push(role),
                    }
                }
            }
            GatewayEvent::RoleDelete { guild_id, role_id } => {
                if let Some(guild) = self.guilds.iter_mut().find(|guild| guild.id == guild_id) {
                    guild.roles.retain(|role| role.id != role_id);
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
            GatewayEvent::MessageCreate(message) => {
                let channel_id = message.channel_id;
                let viewing = self.selected_channel == Some(channel_id);
                // A new message says who the author is in this server. Keep it, so
                // their older messages show the same name and colour.
                if let (Some(member), Some(guild)) = (
                    message.member.clone(),
                    self.channels
                        .get(&channel_id)
                        .and_then(|channel| channel.guild_id),
                ) {
                    self.members
                        .entry(guild)
                        .or_default()
                        .insert(message.author.id, member);
                }
                self.insert_message(message);
                if !viewing {
                    self.unread.insert(channel_id);
                }
            }
            GatewayEvent::MessageDelete {
                channel_id,
                message_id,
            } => {
                if let Some(list) = self.messages.get_mut(&channel_id) {
                    list.retain(|message| message.id != message_id);
                }
            }
            GatewayEvent::MessageUpdate(patch) => self.apply_patch(patch),

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
                let user_id = state.user_id.get();
                for seats in self.voice_seats.values_mut() {
                    seats.retain(|seat| seat.user_id != user_id);
                }
                if let Some(channel) = state.channel_id {
                    self.voice_seats
                        .entry(channel.get())
                        .or_default()
                        .push(VoiceSeat {
                            user_id,
                            name: format!("User {user_id}"),
                            avatar: None,
                            mute: state.self_mute,
                            deaf: state.self_deaf,
                        });
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
            GatewayEvent::MemberList(data) => {
                apply_member_list(&mut self.roster, &data);
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
        self.apply_guild(id, guild.name, guild.icon, guild.banner.map(Some));
        if !guild.roles.is_empty() {
            self.set_roles(id, guild.roles);
        }
        self.members
            .entry(id)
            .or_default()
            .extend(rencord_protocol::member_entries(guild.members));
        for (user, status) in rencord_protocol::presence_entries(guild.presences) {
            self.set_presence(user, status);
        }
        for wire in channels {
            let mut channel = wire.into_channel();
            if channel.guild_id.is_none() {
                channel.guild_id = Some(id);
            }
            self.upsert_channel(channel);
        }
    }

    fn apply_guild(
        &mut self,
        id: Snowflake,
        name: Option<String>,
        icon: Option<String>,
        banner: Option<Option<String>>,
    ) {
        if let Some(existing) = self.guilds.iter_mut().find(|guild| guild.id == id) {
            if let Some(name) = name {
                existing.name = name;
            }
            if icon.is_some() {
                existing.icon = icon;
            }
            if let Some(banner) = banner {
                existing.banner = banner;
            }
            return;
        }
        self.guilds.push(Guild {
            id,
            name: name.unwrap_or_else(|| format!("Guild {id}")),
            icon,
            banner: banner.flatten(),
            ..Default::default()
        });
    }

    fn set_roles(&mut self, guild_id: Snowflake, roles: Vec<Role>) {
        if let Some(guild) = self.guilds.iter_mut().find(|guild| guild.id == guild_id) {
            guild.roles = roles;
        }
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

    /// A `MESSAGE_UPDATE`. Discord often sends only the fields that changed, so
    /// those are merged into the stored message instead of replacing it.
    fn apply_patch(&mut self, patch: MessagePatch) {
        let existing = self
            .messages
            .get_mut(&patch.channel_id)
            .and_then(|list| list.iter_mut().find(|item| item.id == patch.id));
        match (existing, patch.full) {
            (Some(existing), Some(full)) => *existing = full,
            (Some(existing), None) => {
                if let Some(content) = patch.content {
                    existing.content = content;
                }
                if let Some(edited) = patch.edited_timestamp {
                    existing.edited_timestamp = Some(edited);
                }
                if let Some(embeds) = patch.embeds {
                    existing.embeds = embeds;
                }
                if let Some(attachments) = patch.attachments {
                    existing.attachments = attachments;
                }
                if let Some(mentions) = patch.mentions {
                    existing.mentions = mentions;
                }
            }
            // A message that was never loaded is only worth keeping whole.
            (None, Some(full)) => self.insert_message(full),
            (None, None) => {}
        }
    }

    /// Reaction counts a fetched message already carries. Gateway events keep
    /// them current from there.
    fn seed_reactions(&mut self, message: &Message) {
        if message.reactions.is_empty() || self.reactions.contains_key(&message.id.get()) {
            return;
        }
        let counts: Vec<(String, u32)> = message
            .reactions
            .iter()
            .filter(|reaction| reaction.count > 0)
            .filter_map(|reaction| {
                let name = reaction
                    .emoji
                    .name
                    .clone()
                    .filter(|name| !name.is_empty())?;
                Some((name, reaction.count))
            })
            .collect();
        if !counts.is_empty() {
            self.reactions.insert(message.id.get(), counts);
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
        self.seed_reactions(&message);
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
        for message in &messages {
            self.seed_reactions(message);
        }
        let list = self.messages.entry(channel_id).or_default();
        list.extend(
            messages
                .into_iter()
                .filter(|message| message.channel_id == channel_id),
        );
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
        list.extend(
            messages
                .into_iter()
                .filter(|message| message.channel_id == channel_id),
        );
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

    /// A presence update changes the status and keeps the activity line READY gave.
    fn set_presence(&mut self, user: Snowflake, status: PresenceStatus) {
        let entry = self.presences.entry(user.get()).or_default();
        entry.0 = status.as_wire().to_owned();
    }

    /// Home is the direct-message list. A guild's sidebar does not include it.
    pub fn apply_ready_extras(
        &mut self,
        self_id: u64,
        read_state: Vec<rencord_gateway::ReadyRead>,
        presences: Vec<rencord_gateway::ReadyPresence>,
        voice_states: Vec<rencord_gateway::ReadyVoice>,
    ) {
        self.read_marker.clear();
        self.mention_counts.clear();
        for state in read_state {
            if state.last_message_id > 0 {
                self.read_marker
                    .insert(state.channel_id, state.last_message_id);
            }
            if state.mention_count > 0 {
                self.mention_counts
                    .insert(state.channel_id, state.mention_count);
            }
        }
        self.presences.clear();
        self.custom_status.clear();
        for presence in presences {
            if presence.user_id == self_id && !presence.activity.is_empty() {
                self.custom_status = presence.activity.clone();
            }
            self.presences
                .insert(presence.user_id, (presence.status, presence.activity));
        }
        self.voice_seats.clear();
        for state in voice_states {
            self.sit_voice(state);
        }
    }

    fn sit_voice(&mut self, state: rencord_gateway::ReadyVoice) {
        let Some(channel) = state.channel_id else {
            return;
        };
        self.voice_seats
            .entry(channel)
            .or_default()
            .push(VoiceSeat {
                user_id: state.user_id,
                name: state.name,
                avatar: state.avatar,
                mute: state.mute,
                deaf: state.deaf,
            });
    }

    pub fn open_home(&mut self) {
        self.selected_guild = None;
        self.selected_channel = self
            .dm_channels()
            .into_iter()
            .next()
            .map(|channel| channel.id);
        if let Some(channel) = self.selected_channel {
            self.unread.remove(&channel);
        }
    }

    pub fn select_guild(&mut self, guild_id: Snowflake) {
        self.selected_guild = Some(guild_id);
        self.selected_channel = self
            .guild_channels(guild_id)
            .into_iter()
            .find(|channel| channel.kind.is_text())
            .map(|channel| channel.id);
        if let Some(channel) = self.selected_channel {
            self.unread.remove(&channel);
        }
    }

    pub fn select_channel(&mut self, channel_id: Snowflake) {
        if let Some(channel) = self.channels.get(&channel_id) {
            self.selected_guild = channel.guild_id;
        }
        self.selected_channel = Some(channel_id);
        self.unread.remove(&channel_id);
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
            // A made-up hash; the desktop demo seeds a picture for it.
            banner: Some("demo".into()),
            ..Default::default()
        });
        session.upsert_guild(Guild {
            id: Snowflake::new(11),
            name: "Difference Club".into(),
            icon: None,
            ..Default::default()
        });
        session.upsert_channel(Channel {
            id: Snowflake::new(100),
            name: "general".into(),
            kind: ChannelKind::GuildText,
            guild_id: Some(Snowflake::new(10)),
            parent_id: None,
            position: 0,
            recipient_ids: Vec::new(),
            ..Default::default()
        });
        session.upsert_channel(Channel {
            id: Snowflake::new(101),
            name: "voice".into(),
            kind: ChannelKind::GuildVoice,
            guild_id: Some(Snowflake::new(10)),
            parent_id: None,
            position: 1,
            recipient_ids: Vec::new(),
            ..Default::default()
        });
        session.upsert_channel(Channel {
            id: Snowflake::new(110),
            name: "notes".into(),
            kind: ChannelKind::GuildText,
            guild_id: Some(Snowflake::new(11)),
            parent_id: None,
            position: 0,
            recipient_ids: Vec::new(),
            ..Default::default()
        });
        session.upsert_channel(Channel {
            id: Snowflake::new(200),
            name: "Charles".into(),
            kind: ChannelKind::Dm,
            guild_id: None,
            parent_id: None,
            position: 0,
            recipient_ids: vec![Snowflake::new(2)],
            ..Default::default()
        });
        session.insert_message(Message {
            id: Snowflake::new(1000),
            channel_id: Snowflake::new(100),
            author: user,
            content: "Hello **Rencord**. Mention <@2>, a [link](https://example.com), and `code`.\n\n```\nfn main() {}\n```"
                .into(),
            kind: 0,
            timestamp: "2026-11-20T00:00:00Z".into(),
            ..Default::default()
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
            kind: 0,
            timestamp: "2026-11-20T00:01:00Z".into(),
            ..Default::default()
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

    fn sample_message(id: u64, channel: u64, content: &str) -> Message {
        Message {
            id: Snowflake::new(id),
            channel_id: Snowflake::new(channel),
            author: User {
                id: Snowflake::new(2),
                username: "munch".into(),
                global_name: Some("Munch".into()),
                discriminator: None,
                bot: false,
                avatar: Some("u2".into()),
            },
            content: content.into(),
            timestamp: "2026-11-21T18:00:00Z".into(),
            kind: 0,
            ..Default::default()
        }
    }

    #[test]
    fn a_dm_keeps_only_that_channels_messages() {
        let mut session = Session::default();
        session.upsert_channel(Channel {
            id: Snowflake::new(200),
            name: "Munch".into(),
            kind: ChannelKind::Dm,
            guild_id: None,
            parent_id: None,
            position: 0,
            recipient_ids: vec![Snowflake::new(2)],
            avatar: Some("u2".into()),
        });
        session.merge_history(
            Snowflake::new(200),
            vec![
                sample_message(6, 100, "Munch joined the server."),
                sample_message(7, 200, "Want to queue?"),
            ],
        );
        session.select_channel(Snowflake::new(200));
        let shown = session.messages_for(session.selected_channel.unwrap());
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].content, "Want to queue?");
        assert_eq!(shown[0].channel_id.get(), 200);
        assert!(session.messages_for(Snowflake::new(100)).is_empty());
    }

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
            kind: 0,
            timestamp: "t".into(),
            ..Default::default()
        }));
        let messages = session.messages_for(Snowflake::new(100));
        assert_eq!(messages.last().unwrap().content, "live");
        assert!(!session.unread.contains(&Snowflake::new(100)));
    }

    #[test]
    fn a_message_elsewhere_is_unread_until_that_channel_is_open() {
        let mut session = Session::demo();
        let author = session.user.clone().unwrap();
        session.apply_gateway(GatewayEvent::MessageCreate(Message {
            id: Snowflake::new(3001),
            channel_id: Snowflake::new(110),
            author: author.clone(),
            content: "notes".into(),
            kind: 0,
            timestamp: "t".into(),
            ..Default::default()
        }));
        assert!(session.unread.contains(&Snowflake::new(110)));
        session.select_channel(Snowflake::new(110));
        assert!(!session.unread.contains(&Snowflake::new(110)));
        session.apply_gateway(GatewayEvent::MessageCreate(Message {
            id: Snowflake::new(3002),
            channel_id: Snowflake::new(100),
            author,
            content: "back in general".into(),
            kind: 0,
            timestamp: "t".into(),
            ..Default::default()
        }));
        assert!(session.unread.contains(&Snowflake::new(100)));
        session.select_guild(Snowflake::new(10));
        assert!(!session.unread.contains(&Snowflake::new(100)));
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
            kind: 0,
            timestamp: "t".into(),
            ..Default::default()
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
            kind: 0,
            timestamp: "t".into(),
            ..Default::default()
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
                banner: None,
                roles: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
                unavailable: true,
                channels: Vec::new(),
            }],
            discord_theme: None,
            discord_client_theme: None,
            read_state: Vec::new(),
            presences: Vec::new(),
            voice_states: Vec::new(),
        });
        assert_eq!(session.guilds[0].name, "Guild 10");
        session.apply_gateway(GatewayEvent::GuildUpsert {
            id: Snowflake::new(10),
            name: Some("Analytical Engines".into()),
            icon: Some("abc".into()),
            banner: Some(Some("bnr".into())),
            roles: None,
            members: Vec::new(),
            presences: Vec::new(),
            channels: vec![Channel {
                id: Snowflake::new(100),
                name: "general".into(),
                kind: ChannelKind::GuildText,
                guild_id: None,
                parent_id: None,
                position: 0,
                recipient_ids: Vec::new(),
                ..Default::default()
            }],
        });
        assert_eq!(session.guilds[0].name, "Analytical Engines");
        assert_eq!(session.guilds[0].icon.as_deref(), Some("abc"));
        assert_eq!(session.guilds[0].banner.as_deref(), Some("bnr"));
        session.apply_gateway(GatewayEvent::GuildUpsert {
            id: Snowflake::new(10),
            name: None,
            icon: None,
            banner: Some(None),
            roles: None,
            members: Vec::new(),
            presences: Vec::new(),
            channels: Vec::new(),
        });
        assert!(session.guilds[0].banner.is_none());
        assert_eq!(session.channels[&Snowflake::new(100)].guild_id, None);
        session.apply_gateway(GatewayEvent::ChannelUpsert(Channel {
            id: Snowflake::new(100),
            name: "lounge".into(),
            kind: ChannelKind::GuildText,
            guild_id: Some(Snowflake::new(10)),
            parent_id: None,
            position: 1,
            recipient_ids: Vec::new(),
            ..Default::default()
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

    fn stored(id: u64, content: &str) -> Message {
        Message {
            id: Snowflake::new(id),
            channel_id: Snowflake::new(8),
            author: User {
                id: Snowflake::new(1),
                username: "ada".into(),
                ..Default::default()
            },
            content: content.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_partial_update_merges_into_the_message_and_keeps_the_rest() {
        let mut session = Session::default();
        session.insert_message(stored(5, "see this https://example.test"));
        session.apply_gateway(GatewayEvent::MessageUpdate(MessagePatch {
            id: Snowflake::new(5),
            channel_id: Snowflake::new(8),
            embeds: Some(vec![rencord_model::Embed {
                title: Some("Resolved".into()),
                ..Default::default()
            }]),
            ..Default::default()
        }));
        let message = &session.messages[&Snowflake::new(8)][0];
        // The embed arrived, and the text and author were not lost.
        assert_eq!(message.embeds[0].title.as_deref(), Some("Resolved"));
        assert_eq!(message.content, "see this https://example.test");
        assert_eq!(message.author.username, "ada");
        session.apply_gateway(GatewayEvent::MessageUpdate(MessagePatch {
            id: Snowflake::new(5),
            channel_id: Snowflake::new(8),
            content: Some("edited".into()),
            edited_timestamp: Some("2026-11-20T01:00:00+00:00".into()),
            ..Default::default()
        }));
        let message = &session.messages[&Snowflake::new(8)][0];
        assert_eq!(message.content, "edited");
        assert!(message.edited_timestamp.is_some());
        assert_eq!(message.embeds.len(), 1);
    }

    #[test]
    fn an_update_for_a_message_not_loaded_is_kept_only_when_whole() {
        let mut session = Session::default();
        session.apply_gateway(GatewayEvent::MessageUpdate(MessagePatch {
            id: Snowflake::new(9),
            channel_id: Snowflake::new(8),
            embeds: Some(Vec::new()),
            ..Default::default()
        }));
        assert!(session
            .messages
            .get(&Snowflake::new(8))
            .is_none_or(Vec::is_empty));
        session.apply_gateway(GatewayEvent::MessageUpdate(MessagePatch {
            id: Snowflake::new(9),
            channel_id: Snowflake::new(8),
            full: Some(stored(9, "whole")),
            ..Default::default()
        }));
        assert_eq!(session.messages[&Snowflake::new(8)][0].content, "whole");
    }

    #[test]
    fn reactions_on_fetched_history_are_counted_and_stay_live() {
        let mut session = Session::default();
        let mut message = stored(5, "popular");
        message.reactions = vec![
            rencord_model::MessageReaction {
                count: 2,
                me: true,
                emoji: rencord_model::ReactionEmoji {
                    id: None,
                    name: Some("\u{1F44D}".into()),
                },
            },
            rencord_model::MessageReaction {
                count: 1,
                me: false,
                emoji: rencord_model::ReactionEmoji {
                    id: Some(Snowflake::new(900)),
                    name: Some("pepe".into()),
                },
            },
            rencord_model::MessageReaction {
                count: 0,
                me: false,
                emoji: rencord_model::ReactionEmoji {
                    id: None,
                    name: Some("gone".into()),
                },
            },
        ];
        session.merge_history(Snowflake::new(8), vec![message]);
        assert_eq!(
            session.reactions[&5],
            vec![("\u{1F44D}".to_owned(), 2), ("pepe".to_owned(), 1)]
        );
        session.apply_gateway(GatewayEvent::Reaction {
            message_id: Snowflake::new(5),
            emoji: "\u{1F44D}".into(),
            remove: false,
        });
        assert_eq!(session.reactions[&5][0].1, 3);
    }

    fn session_with_a_server() -> Session {
        let mut session = Session::default();
        session.upsert_guild(Guild {
            id: Snowflake::new(10),
            name: "Engines".into(),
            ..Default::default()
        });
        session.upsert_channel(Channel {
            id: Snowflake::new(8),
            name: "general".into(),
            kind: ChannelKind::GuildText,
            guild_id: Some(Snowflake::new(10)),
            ..Default::default()
        });
        session
    }

    #[test]
    fn roles_members_and_presence_follow_the_gateway() {
        use rencord_model::{Member, PresenceStatus, Role};
        let mut session = session_with_a_server();
        let role = |id: u64, color: u32| Role {
            id: Snowflake::new(id),
            name: format!("role-{id}"),
            color,
            position: id as i32,
            hoist: false,
        };
        session.apply_gateway(GatewayEvent::RoleUpsert {
            guild_id: Snowflake::new(10),
            role: role(900, 255),
        });
        session.apply_gateway(GatewayEvent::RoleUpsert {
            guild_id: Snowflake::new(10),
            role: role(900, 65280),
        });
        session.apply_gateway(GatewayEvent::RoleUpsert {
            guild_id: Snowflake::new(10),
            role: role(901, 1),
        });
        assert_eq!(session.guilds[0].roles.len(), 2);
        assert_eq!(session.guilds[0].roles[0].color, 65280);
        session.apply_gateway(GatewayEvent::RoleDelete {
            guild_id: Snowflake::new(10),
            role_id: Snowflake::new(901),
        });
        assert_eq!(session.guilds[0].roles.len(), 1);

        let boss = Member {
            nick: Some("Boss".into()),
            roles: vec![Snowflake::new(900)],
        };
        session.apply_gateway(GatewayEvent::MemberUpdate {
            guild_id: Snowflake::new(10),
            user_id: Snowflake::new(42),
            member: boss.clone(),
        });
        assert_eq!(
            session.members[&Snowflake::new(10)][&Snowflake::new(42)],
            boss
        );
        session.apply_gateway(GatewayEvent::MembersChunk {
            guild_id: Snowflake::new(10),
            members: vec![(Snowflake::new(43), Member::default())],
        });
        assert_eq!(session.members[&Snowflake::new(10)].len(), 2);

        session.apply_gateway(GatewayEvent::Presence {
            user_id: Snowflake::new(42),
            status: PresenceStatus::Idle,
        });
        assert_eq!(session.presences[&42].0, "idle");
        session.apply_gateway(GatewayEvent::Presence {
            user_id: Snowflake::new(42),
            status: PresenceStatus::Offline,
        });
        assert_eq!(session.presences[&42].0, "offline");
    }

    #[test]
    fn a_new_message_teaches_the_store_who_its_author_is_in_the_server() {
        use rencord_model::Member;
        let mut session = session_with_a_server();
        let mut message = stored(5, "hello");
        message.member = Some(Member {
            nick: Some("Boss".into()),
            roles: vec![Snowflake::new(900)],
        });
        session.apply_gateway(GatewayEvent::MessageCreate(message));
        // Their older messages can now show the same name and colour.
        let known = &session.members[&Snowflake::new(10)][&Snowflake::new(1)];
        assert_eq!(known.nickname(), Some("Boss"));
        // A message in a channel that belongs to no server learns nothing.
        let mut dm = stored(6, "dm");
        dm.channel_id = Snowflake::new(999);
        dm.member = Some(Member::default());
        session.apply_gateway(GatewayEvent::MessageCreate(dm));
        assert_eq!(session.members.len(), 1);
    }
}
