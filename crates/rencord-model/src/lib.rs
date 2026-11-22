//! Toolkit-agnostic Discord domain types used by every Rencord crate.
//!
//! Snowflakes accept both the string form Discord sends on the wire and bare
//! integers used by tests. Secrets are held only as [`Token`], which redacts
//! itself in `Debug`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Discord snowflake identifier.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Snowflake(pub u64);

impl Snowflake {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Snowflake({})", self.0)
    }
}

impl Serialize for Snowflake {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Snowflake {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Snowflake;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a snowflake string or integer")
            }

            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Snowflake, E> {
                Ok(Snowflake(value))
            }

            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Snowflake, E> {
                u64::try_from(value)
                    .map(Snowflake)
                    .map_err(serde::de::Error::custom)
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Snowflake, E> {
                value
                    .parse::<u64>()
                    .map(Snowflake)
                    .map_err(serde::de::Error::custom)
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// User token. The `Debug` impl never prints the secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token([redacted])")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: Snowflake,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub discriminator: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub bot: bool,
}

impl User {
    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    GuildText,
    Dm,
    GuildVoice,
    GroupDm,
    GuildCategory,
    GuildAnnouncement,
    GuildForum,
    PublicThread,
    #[default]
    Other,
}

impl ChannelKind {
    pub fn from_discord(value: u8) -> Self {
        match value {
            0 => Self::GuildText,
            1 => Self::Dm,
            2 => Self::GuildVoice,
            3 => Self::GroupDm,
            4 => Self::GuildCategory,
            5 => Self::GuildAnnouncement,
            15 => Self::GuildForum,
            11 | 12 => Self::PublicThread,
            _ => Self::Other,
        }
    }

    pub fn to_discord(self) -> u8 {
        match self {
            Self::GuildText => 0,
            Self::Dm => 1,
            Self::GuildVoice => 2,
            Self::GroupDm => 3,
            Self::GuildCategory => 4,
            Self::GuildAnnouncement => 5,
            Self::GuildForum => 15,
            Self::PublicThread => 11,
            Self::Other => 0,
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(
            self,
            Self::GuildText
                | Self::Dm
                | Self::GroupDm
                | Self::GuildAnnouncement
                | Self::PublicThread
        )
    }

    pub fn is_voice(&self) -> bool {
        matches!(self, Self::GuildVoice)
    }
}

/// A role in a server. Field names are Discord's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Role {
    pub id: Snowflake,
    #[serde(default)]
    pub name: String,
    /// 0xRRGGBB, or 0 for a role with no colour.
    #[serde(default)]
    pub color: u32,
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub hoist: bool,
}

/// What a server knows about one of its people: a nickname and role ids.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    #[serde(default)]
    pub nick: Option<String>,
    #[serde(default)]
    pub roles: Vec<Snowflake>,
}

impl Member {
    /// The nickname, when there is a real one.
    pub fn nickname(&self) -> Option<&str> {
        self.nick.as_deref().filter(|nick| !nick.is_empty())
    }
}

/// Whether someone is online, as the presence dot shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PresenceStatus {
    Online,
    Idle,
    Dnd,
    /// Also what `invisible` and anything unknown becomes.
    #[default]
    Offline,
}

impl PresenceStatus {
    pub fn from_wire(status: &str) -> Self {
        match status {
            "online" => Self::Online,
            "idle" => Self::Idle,
            "dnd" => Self::Dnd,
            _ => Self::Offline,
        }
    }

    /// The word Discord uses for this status.
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Idle => "idle",
            Self::Dnd => "dnd",
            Self::Offline => "offline",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guild {
    pub id: Snowflake,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// The picture behind the server name in the channel list.
    #[serde(default)]
    pub banner: Option<String>,
    /// Client-side folder name. Empty guilds stay loose on the rail.
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub roles: Vec<Role>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub id: Snowflake,
    pub name: String,
    pub kind: ChannelKind,
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub parent_id: Option<Snowflake>,
    #[serde(default)]
    pub position: i32,
    /// DM / group DM recipients. Empty for guild channels.
    #[serde(default)]
    pub recipient_ids: Vec<Snowflake>,
    /// The same people with their avatars, for the DM list.
    #[serde(default)]
    pub recipients: Vec<User>,
    /// Avatar hash for a direct message, when the recipient has one.
    #[serde(default)]
    pub avatar: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: Snowflake,
    pub channel_id: Snowflake,
    pub author: User,
    pub content: String,
    #[serde(default)]
    pub timestamp: String,
    /// Discord's message type: 0 and 19 are ordinary, 7 is a join, 3 a call.
    #[serde(default)]
    pub kind: u8,
    /// Set when the message has been edited.
    #[serde(default)]
    pub edited_timestamp: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub embeds: Vec<Embed>,
    #[serde(default)]
    pub reactions: Vec<MessageReaction>,
    /// Users the message mentions, so `<@id>` can be shown as a name.
    #[serde(default)]
    pub mentions: Vec<User>,
    #[serde(default)]
    pub reply_to: Option<Reply>,
    /// The author's nickname and roles in the server, when Discord sent them.
    #[serde(default)]
    pub member: Option<Member>,
}

impl Message {
    /// A system message such as "joined the server" has no text of its own.
    pub fn is_system(&self) -> bool {
        !matches!(self.kind, 0 | 19 | 20 | 21)
    }

    /// Whether `user` is named in the message.
    pub fn mentions_user(&self, user: Snowflake) -> bool {
        self.mentions.iter().any(|mentioned| mentioned.id == user)
    }
}

/// A file on a message. Field names are Discord's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(default)]
    pub id: Snowflake,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl Attachment {
    pub fn is_image(&self) -> bool {
        self.content_type
            .as_deref()
            .is_some_and(|kind| kind.starts_with("image/"))
    }

    pub fn is_video(&self) -> bool {
        self.content_type
            .as_deref()
            .is_some_and(|kind| kind.starts_with("video/"))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedImage {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl EmbedImage {
    /// Discord's proxy copy when there is one, so the original host is not contacted.
    pub fn best_url(&self) -> &str {
        self.proxy_url.as_deref().unwrap_or(&self.url)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedProvider {
    #[serde(default)]
    pub name: Option<String>,
}

/// A link preview or rich embed. Field names are Discord's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Embed {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub color: Option<u32>,
    #[serde(default)]
    pub provider: Option<EmbedProvider>,
    #[serde(default)]
    pub thumbnail: Option<EmbedImage>,
    #[serde(default)]
    pub image: Option<EmbedImage>,
    #[serde(default)]
    pub video: Option<EmbedImage>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReactionEmoji {
    #[serde(default)]
    pub id: Option<Snowflake>,
    #[serde(default)]
    pub name: Option<String>,
}

/// A reaction group on a message, as Discord reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReaction {
    #[serde(default)]
    pub count: u32,
    /// True when the signed-in user reacted.
    #[serde(default)]
    pub me: bool,
    #[serde(default)]
    pub emoji: ReactionEmoji,
}

/// What a reply points at. The text and author are there when Discord sent
/// the original message along, and absent when it was deleted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub message_id: Snowflake,
    #[serde(default)]
    pub author: Option<User>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceEndpoint {
    pub guild_id: Option<Snowflake>,
    pub channel_id: Snowflake,
    pub endpoint: String,
    pub token: String,
    pub session_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceJoinRequest {
    pub guild_id: Option<Snowflake>,
    pub channel_id: Snowflake,
    pub self_mute: bool,
    pub self_deaf: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snowflake_roundtrip_string_and_int() {
        let id = Snowflake::new(123456789012345678);
        let as_string = serde_json::to_string(&id).unwrap();
        assert_eq!(as_string, "\"123456789012345678\"");
        let back: Snowflake = serde_json::from_str(&as_string).unwrap();
        assert_eq!(back, id);
        let from_int: Snowflake = serde_json::from_str("42").unwrap();
        assert_eq!(from_int.get(), 42);
    }

    #[test]
    fn token_debug_is_redacted() {
        let token = Token::new("super-secret-value");
        let rendered = format!("{token:?}");
        assert!(!rendered.contains("super-secret"));
        assert_eq!(token.expose(), "super-secret-value");
    }

    #[test]
    fn channel_kind_mapping() {
        assert!(ChannelKind::from_discord(2).is_voice());
        assert!(ChannelKind::from_discord(0).is_text());
        assert_eq!(ChannelKind::from_discord(99), ChannelKind::Other);
    }
}
