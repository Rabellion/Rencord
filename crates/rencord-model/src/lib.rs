//! Toolkit-agnostic Discord domain types used by every Rencord crate.
//!
//! Snowflakes accept both the string form Discord sends on the wire and bare
//! integers used by tests. Secrets are held only as [`Token`], which redacts
//! itself in `Debug`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Discord snowflake identifier.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guild {
    pub id: Snowflake,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Avatar hash for a direct message, when the recipient has one.
    #[serde(default)]
    pub avatar: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: Snowflake,
    pub channel_id: Snowflake,
    pub author: User,
    pub content: String,
    #[serde(default)]
    pub timestamp: String,
    /// Discord message type. `0` is a normal message. `7` is a member join.
    #[serde(default)]
    pub kind: u8,
}

/// One-line system copy. Normal messages (`0`) return nothing so the body is shown.
pub fn system_line(kind: u8, name: &str) -> Option<String> {
    match kind {
        3 => Some(format!("{name} started a call.")),
        6 => Some(format!("{name} pinned a message to this channel.")),
        7 => Some(format!("{name} joined the server.")),
        8..=11 => Some(format!("{name} boosted the server.")),
        _ => None,
    }
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
    fn a_join_is_a_system_line_and_a_normal_message_is_not() {
        assert_eq!(system_line(7, "Ada"), Some("Ada joined the server.".into()));
        assert_eq!(
            system_line(6, "Ada"),
            Some("Ada pinned a message to this channel.".into())
        );
        assert_eq!(
            system_line(3, "Munch"),
            Some("Munch started a call.".into())
        );
        assert!(system_line(0, "Ada").is_none());
    }

    #[test]
    fn channel_kind_mapping() {
        assert!(ChannelKind::from_discord(2).is_voice());
        assert!(ChannelKind::from_discord(0).is_text());
        assert_eq!(ChannelKind::from_discord(99), ChannelKind::Other);
    }
}
