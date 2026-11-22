//! Gateway and voice websocket payloads.
//!
//! Rencord identifies itself as Rencord. It does not imitate an official
//! browser fingerprint.

use base64::Engine;
use rencord_model::{
    Attachment, ChannelKind, Embed, Member, Message, MessageReaction, PresenceStatus, Reply, Role,
    Snowflake, User,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

pub const GATEWAY_VERSION: u8 = 10;
pub const VOICE_VERSION: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayOpcode {
    Dispatch = 0,
    Heartbeat = 1,
    Identify = 2,
    PresenceUpdate = 3,
    VoiceStateUpdate = 4,
    Resume = 6,
    Reconnect = 7,
    RequestGuildMembers = 8,
    InvalidSession = 9,
    Hello = 10,
    HeartbeatAck = 11,
}

impl GatewayOpcode {
    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Dispatch,
            1 => Self::Heartbeat,
            2 => Self::Identify,
            3 => Self::PresenceUpdate,
            4 => Self::VoiceStateUpdate,
            6 => Self::Resume,
            7 => Self::Reconnect,
            8 => Self::RequestGuildMembers,
            9 => Self::InvalidSession,
            10 => Self::Hello,
            11 => Self::HeartbeatAck,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GatewayFrame {
    pub op: GatewayOpcode,
    pub data: Value,
    pub sequence: Option<u64>,
    pub event_name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("gateway frame is not a JSON object")]
    NotObject,
    #[error("missing or unknown opcode")]
    Opcode,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl GatewayFrame {
    pub fn parse(bytes: &[u8]) -> Result<Self, FrameError> {
        let value: Value = serde_json::from_slice(bytes)?;
        let object = value.as_object().ok_or(FrameError::NotObject)?;
        let op = object
            .get("op")
            .and_then(|v| v.as_u64())
            .and_then(|v| u8::try_from(v).ok())
            .and_then(GatewayOpcode::from_u8)
            .ok_or(FrameError::Opcode)?;
        Ok(Self {
            op,
            data: object.get("d").cloned().unwrap_or(Value::Null),
            sequence: object.get("s").and_then(|v| v.as_u64()),
            event_name: object.get("t").and_then(|v| v.as_str()).map(str::to_owned),
        })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "op": self.op as u8,
            "d": self.data,
            "s": self.sequence,
            "t": self.event_name,
        })
    }
}

pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Same agent string the REST client sends. Login and the gateway share it.
pub const USER_AGENT: &str = concat!(
    "Rencord (https://github.com/Rabellion/Rencord, ",
    env!("CARGO_PKG_VERSION"),
    ")"
);

/// Locale in the `en-US` shape Discord expects. `LANG` is the only source.
pub fn discord_locale() -> String {
    let raw = std::env::var("LANG").unwrap_or_else(|_| "en-US".into());
    let code = raw
        .split(['.', '@'])
        .next()
        .unwrap_or("en-US")
        .replace('_', "-");
    let safe: String = code
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .take(16)
        .collect();
    if safe.len() < 2 {
        "en-US".into()
    } else {
        safe
    }
}

/// IANA zone for `X-Discord-Timezone`. `TZ` wins, then the OS zone database.
/// The result is a zone name, never an offset string we invented.
pub fn discord_timezone() -> String {
    if let Ok(value) = std::env::var("TZ") {
        if let Some(name) = sanitize_timezone(&value) {
            return name;
        }
    }
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| sanitize_timezone(&name))
        .unwrap_or_else(|| "UTC".into())
}

fn sanitize_timezone(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches(':');
    if (2..64).contains(&value.len())
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-' | '+'))
    {
        Some(value.to_owned())
    } else {
        None
    }
}

/// Properties on gateway Identify and, base64-encoded, on `X-Super-Properties`.
/// The browser and device stay `Rencord`. This does not impersonate the official client.
pub fn client_properties() -> Value {
    json!({
        "os": std::env::consts::OS,
        "browser": "Rencord",
        "device": "Rencord",
        "system_locale": discord_locale(),
        "browser_user_agent": USER_AGENT,
        "browser_version": CLIENT_VERSION,
        "os_version": std::env::consts::OS,
        "os_arch": std::env::consts::ARCH,
        "app_arch": std::env::consts::ARCH,
        "client_version": CLIENT_VERSION,
        "release_channel": "stable",
    })
}

/// Base64 JSON of [`client_properties`] for the `X-Super-Properties` header.
pub fn super_properties() -> String {
    let bytes = serde_json::to_vec(&client_properties()).expect("client properties serialize");
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn identify_payload(token: &str) -> Value {
    json!({
        "op": GatewayOpcode::Identify as u8,
        "d": {
            "token": token,
            "properties": client_properties(),
            "compress": false,
            "large_threshold": 50
        }
    })
}

pub fn heartbeat_payload(sequence: Option<u64>) -> Value {
    json!({
        "op": GatewayOpcode::Heartbeat as u8,
        "d": sequence,
    })
}

pub fn resume_payload(token: &str, session_id: &str, sequence: u64) -> Value {
    json!({
        "op": GatewayOpcode::Resume as u8,
        "d": {
            "token": token,
            "session_id": session_id,
            "seq": sequence,
        }
    })
}

pub fn voice_state_update(
    guild_id: Option<Snowflake>,
    channel_id: Option<Snowflake>,
    self_mute: bool,
    self_deaf: bool,
) -> Value {
    json!({
        "op": GatewayOpcode::VoiceStateUpdate as u8,
        "d": {
            "guild_id": guild_id.map(|id| id.to_string()),
            "channel_id": channel_id.map(|id| id.to_string()),
            "self_mute": self_mute,
            "self_deaf": self_deaf,
        }
    })
}

/// Voice-state update that asks to publish a Go Live stream.
///
/// `self_stream` is the gateway flag. It is not a promise that Discord will
/// accept the quality. The video samples travel on a separate voice connection.
pub fn voice_state_go_live(
    guild_id: Option<Snowflake>,
    channel_id: Option<Snowflake>,
    streaming: bool,
) -> Value {
    json!({
        "op": GatewayOpcode::VoiceStateUpdate as u8,
        "d": {
            "guild_id": guild_id.map(|id| id.to_string()),
            "channel_id": channel_id.map(|id| id.to_string()),
            "self_mute": false,
            "self_deaf": false,
            "self_video": false,
            "self_stream": streaming,
        }
    })
}

/// Ask the video connection to subscribe to another member's stream.
pub fn stream_watch(user_id: &str) -> Value {
    json!({
        "op": "stream_watch",
        "d": { "user_id": user_id }
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct HelloData {
    pub heartbeat_interval: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ReadyData {
    pub session_id: String,
    pub user: User,
    #[serde(default)]
    pub guilds: Vec<ReadyGuild>,
    /// Presences of friends and the people you have DMs with.
    #[serde(default, deserialize_with = "lenient_vec")]
    pub presences: Vec<WirePresence>,
    /// Legacy JSON settings. Newer clients send a protobuf instead. When this
    /// object is present, Rencord may use it once as a local starting theme.
    #[serde(default)]
    pub user_settings: Option<ReadyUserSettings>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct ReadyUserSettings {
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub client_theme: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ReadyGuild {
    pub id: Snowflake,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub roles: Vec<Role>,
    /// Limited for a user account: people you share a relationship or a voice call with.
    #[serde(default, deserialize_with = "lenient_vec")]
    pub members: Vec<WireGuildMember>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub presences: Vec<WirePresence>,
    #[serde(default)]
    pub unavailable: bool,
    #[serde(default)]
    pub channels: Vec<WireChannel>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct VoiceServerUpdate {
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    pub token: String,
    #[serde(default)]
    pub endpoint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct VoiceStateDispatch {
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub channel_id: Option<Snowflake>,
    pub user_id: Snowflake,
    pub session_id: String,
    #[serde(default)]
    pub self_mute: bool,
    #[serde(default)]
    pub self_deaf: bool,
}

/// Discord channel JSON is wider than [`rencord_model::Channel`]. This is the
/// subset Milestone 1 reads.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct WireChannel {
    pub id: Snowflake,
    #[serde(default, rename = "type")]
    pub kind: u8,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub parent_id: Option<Snowflake>,
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub recipients: Vec<User>,
}

impl WireChannel {
    pub fn into_channel(self) -> rencord_model::Channel {
        let avatar = self.recipients.first().and_then(|user| user.avatar.clone());
        let name = self.name.unwrap_or_else(|| {
            if self.recipients.is_empty() {
                "Direct Message".to_owned()
            } else {
                self.recipients
                    .iter()
                    .map(User::display_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        });
        rencord_model::Channel {
            id: self.id,
            name,
            kind: ChannelKind::from_discord(self.kind),
            guild_id: self.guild_id,
            parent_id: self.parent_id,
            position: self.position,
            recipient_ids: self.recipients.iter().map(|user| user.id).collect(),
            avatar,
        }
    }
}

/// A list where an element that does not decode is dropped. One odd attachment
/// or embed must not cost the whole message, and nothing from another user may
/// stop a page of history from loading.
fn lenient_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let values: Option<Vec<Value>> = Option::deserialize(deserializer)?;
    Ok(values
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect())
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct WireUserId {
    #[serde(default)]
    pub id: Snowflake,
}

/// A server member as READY, GUILD_CREATE and the member events carry it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct WireGuildMember {
    #[serde(default)]
    pub user: Option<WireUserId>,
    #[serde(default)]
    pub nick: Option<String>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub roles: Vec<Snowflake>,
}

impl WireGuildMember {
    /// The user's id and what the server knows about them. `None` when the
    /// entry does not say whose it is.
    pub fn into_entry(self) -> Option<(Snowflake, Member)> {
        let id = self.user?.id;
        (id.get() != 0).then_some((
            id,
            Member {
                nick: self.nick,
                roles: self.roles,
            },
        ))
    }
}

/// A presence as READY, GUILD_CREATE and PRESENCE_UPDATE carry it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct WirePresence {
    #[serde(default)]
    pub user: Option<WireUserId>,
    #[serde(default)]
    pub status: Option<String>,
}

impl WirePresence {
    pub fn into_entry(self) -> Option<(Snowflake, PresenceStatus)> {
        let id = self.user?.id;
        (id.get() != 0).then(|| {
            (
                id,
                PresenceStatus::from_wire(self.status.as_deref().unwrap_or("")),
            )
        })
    }
}

/// The elements of a JSON array that decode, dropping the ones that do not.
pub fn lenient_list<T: serde::de::DeserializeOwned>(value: Option<&Value>) -> Vec<T> {
    value
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| serde_json::from_value(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Members from a list, dropping any entry that does not say whose it is.
pub fn member_entries(members: Vec<WireGuildMember>) -> Vec<(Snowflake, Member)> {
    members
        .into_iter()
        .filter_map(WireGuildMember::into_entry)
        .collect()
}

pub fn presence_entries(presences: Vec<WirePresence>) -> Vec<(Snowflake, PresenceStatus)> {
    presences
        .into_iter()
        .filter_map(WirePresence::into_entry)
        .collect()
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct WireReference {
    #[serde(default)]
    pub message_id: Option<Snowflake>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WireMessage {
    pub id: Snowflake,
    pub channel_id: Snowflake,
    pub author: User,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default, rename = "type")]
    pub kind: u64,
    #[serde(default)]
    pub edited_timestamp: Option<String>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub attachments: Vec<Attachment>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub embeds: Vec<Embed>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub reactions: Vec<MessageReaction>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub mentions: Vec<User>,
    #[serde(default)]
    pub message_reference: Option<WireReference>,
    /// Kept raw: it is the replied-to message, and it may be null, deleted,
    /// or missing an author. Only its author and text are read.
    #[serde(default)]
    pub referenced_message: Option<Value>,
    /// The author's server nickname and roles. Kept raw so an odd one cannot cost the message.
    #[serde(default)]
    pub member: Option<Value>,
}

impl From<WireMessage> for Message {
    fn from(value: WireMessage) -> Self {
        let reply_to = value
            .message_reference
            .and_then(|reference| reference.message_id)
            .map(|message_id| {
                let original = value.referenced_message.as_ref();
                Reply {
                    message_id,
                    author: original
                        .and_then(|message| message.get("author"))
                        .and_then(|author| serde_json::from_value(author.clone()).ok()),
                    content: original
                        .and_then(|message| message.get("content"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                }
            });
        Message {
            id: value.id,
            channel_id: value.channel_id,
            author: value.author,
            content: value.content,
            timestamp: value.timestamp,
            kind: u8::try_from(value.kind).unwrap_or(u8::MAX),
            edited_timestamp: value.edited_timestamp,
            attachments: value.attachments,
            embeds: value.embeds,
            reactions: value.reactions,
            mentions: value.mentions,
            reply_to,
            member: value
                .member
                .and_then(|member| serde_json::from_value(member).ok()),
        }
    }
}

/// What a `MESSAGE_UPDATE` changed. Discord often sends only the changed
/// fields, such as the embeds once a link has been fetched, with no author.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessagePatch {
    pub id: Snowflake,
    pub channel_id: Snowflake,
    pub content: Option<String>,
    pub edited_timestamp: Option<String>,
    pub embeds: Option<Vec<Embed>>,
    pub attachments: Option<Vec<Attachment>>,
    pub mentions: Option<Vec<User>>,
    /// Set when the update carried the whole message (it had an author).
    pub full: Option<Message>,
}

impl MessagePatch {
    pub fn from_value(data: &Value) -> Result<Self, serde_json::Error> {
        let id: Snowflake = serde_json::from_value(data.get("id").cloned().unwrap_or(Value::Null))?;
        let channel_id: Snowflake =
            serde_json::from_value(data.get("channel_id").cloned().unwrap_or(Value::Null))?;
        let list = |key: &str| -> Option<Value> { data.get(key).filter(|v| v.is_array()).cloned() };
        let decode = |key: &str| -> Option<Vec<Value>> {
            list(key).and_then(|value| serde_json::from_value(value).ok())
        };
        Ok(Self {
            id,
            channel_id,
            content: data
                .get("content")
                .and_then(Value::as_str)
                .map(str::to_owned),
            edited_timestamp: data
                .get("edited_timestamp")
                .and_then(Value::as_str)
                .map(str::to_owned),
            embeds: decode("embeds").map(|items| {
                items
                    .into_iter()
                    .filter_map(|item| serde_json::from_value(item).ok())
                    .collect()
            }),
            attachments: decode("attachments").map(|items| {
                items
                    .into_iter()
                    .filter_map(|item| serde_json::from_value(item).ok())
                    .collect()
            }),
            mentions: decode("mentions").map(|items| {
                items
                    .into_iter()
                    .filter_map(|item| serde_json::from_value(item).ok())
                    .collect()
            }),
            full: if data.get("author").is_some() {
                serde_json::from_value::<WireMessage>(data.clone())
                    .ok()
                    .map(Message::from)
            } else {
                None
            },
        })
    }
}

/// Voice websocket opcodes. DAVE numbers follow the published protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceOpcode {
    Identify = 0,
    SelectProtocol = 1,
    Ready = 2,
    Heartbeat = 3,
    SessionDescription = 4,
    Speaking = 5,
    HeartbeatAck = 6,
    Resume = 7,
    Hello = 8,
    Resumed = 9,
    ClientsConnect = 11,
    ClientDisconnect = 13,
    DavePrepareTransition = 21,
    DaveExecuteTransition = 22,
    DaveTransitionReady = 23,
    DavePrepareEpoch = 24,
    MlsExternalSender = 25,
    MlsKeyPackage = 26,
    MlsProposals = 27,
    MlsCommitWelcome = 28,
    MlsAnnounceCommitTransition = 29,
    MlsWelcome = 30,
    MlsInvalidCommitWelcome = 31,
}

impl VoiceOpcode {
    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Identify,
            1 => Self::SelectProtocol,
            2 => Self::Ready,
            3 => Self::Heartbeat,
            4 => Self::SessionDescription,
            5 => Self::Speaking,
            6 => Self::HeartbeatAck,
            7 => Self::Resume,
            8 => Self::Hello,
            9 => Self::Resumed,
            11 => Self::ClientsConnect,
            13 => Self::ClientDisconnect,
            21 => Self::DavePrepareTransition,
            22 => Self::DaveExecuteTransition,
            23 => Self::DaveTransitionReady,
            24 => Self::DavePrepareEpoch,
            25 => Self::MlsExternalSender,
            26 => Self::MlsKeyPackage,
            27 => Self::MlsProposals,
            28 => Self::MlsCommitWelcome,
            29 => Self::MlsAnnounceCommitTransition,
            30 => Self::MlsWelcome,
            31 => Self::MlsInvalidCommitWelcome,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VoiceFrame {
    pub op: VoiceOpcode,
    pub data: Value,
    /// Server sequence number (voice v8), echoed back as seq_ack.
    pub seq: Option<u64>,
}

impl VoiceFrame {
    pub fn parse(bytes: &[u8]) -> Result<Self, FrameError> {
        let value: Value = serde_json::from_slice(bytes)?;
        let object = value.as_object().ok_or(FrameError::NotObject)?;
        let op = object
            .get("op")
            .and_then(|v| v.as_u64())
            .and_then(|v| u8::try_from(v).ok())
            .and_then(VoiceOpcode::from_u8)
            .ok_or(FrameError::Opcode)?;
        Ok(Self {
            op,
            data: object.get("d").cloned().unwrap_or(Value::Null),
            seq: object.get("seq").and_then(|v| v.as_u64()),
        })
    }

    /// A binary DAVE frame from the server: `[seq u16 BE][op u8][payload]`.
    /// The payload comes back as `{"data": [bytes]}`. Only the MLS opcodes are
    /// binary; anything else is `FrameError::Opcode` so callers can skip it.
    pub fn parse_binary(bytes: &[u8]) -> Result<Self, FrameError> {
        let [s0, s1, op, payload @ ..] = bytes else {
            return Err(FrameError::Opcode);
        };
        let op = match VoiceOpcode::from_u8(*op) {
            Some(
                op @ (VoiceOpcode::MlsExternalSender
                | VoiceOpcode::MlsProposals
                | VoiceOpcode::MlsAnnounceCommitTransition
                | VoiceOpcode::MlsWelcome),
            ) => op,
            _ => return Err(FrameError::Opcode),
        };
        Ok(Self {
            op,
            data: json!({ "data": payload }),
            seq: Some(u64::from(u16::from_be_bytes([*s0, *s1]))),
        })
    }

    pub fn to_json(&self) -> Value {
        json!({ "op": self.op as u8, "d": self.data })
    }
}

pub fn voice_identify(server_id: &str, user_id: &str, session_id: &str, token: &str) -> Value {
    json!({
        "op": VoiceOpcode::Identify as u8,
        "d": {
            "server_id": server_id,
            "user_id": user_id,
            "session_id": session_id,
            "token": token,
            "max_dave_protocol_version": 1,
        }
    })
}

pub fn voice_select_protocol(address: &str, port: u16, mode: &str) -> Value {
    json!({
        "op": VoiceOpcode::SelectProtocol as u8,
        "d": {
            "protocol": "udp",
            "data": {
                "address": address,
                "port": port,
                "mode": mode,
            }
        }
    })
}

/// Voice v8 heartbeat: `t` is a nonce, `seq_ack` the last server sequence seen.
pub fn voice_heartbeat(nonce: u64, seq_ack: u64) -> Value {
    json!({
        "op": VoiceOpcode::Heartbeat as u8,
        "d": { "t": nonce, "seq_ack": seq_ack }
    })
}

/// A binary frame to the server (DAVE key package, commit and welcome): `[op u8][payload]`.
pub fn voice_binary(op: VoiceOpcode, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(op as u8);
    frame.extend_from_slice(payload);
    frame
}

pub fn voice_speaking(ssrc: u32, speaking: bool) -> Value {
    json!({
        "op": VoiceOpcode::Speaking as u8,
        "d": {
            "speaking": if speaking { 1 } else { 0 },
            "delay": 0,
            "ssrc": ssrc,
        }
    })
}

pub fn voice_resume(server_id: &str, session_id: &str, token: &str, seq_ack: u64) -> Value {
    json!({
        "op": VoiceOpcode::Resume as u8,
        "d": {
            "server_id": server_id,
            "session_id": session_id,
            "token": token,
            "seq_ack": seq_ack,
        }
    })
}

/// Preferred transport mode. DAVE sits on top of this AEAD mode.
pub const TRANSPORT_MODE: &str = "aead_xchacha20_poly1305_rtpsize";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct VoiceHello {
    pub heartbeat_interval: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct VoiceReady {
    pub ssrc: u32,
    pub ip: String,
    pub port: u16,
    pub modes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionDescription {
    pub mode: String,
    pub secret_key: Vec<u8>,
    #[serde(default)]
    pub dave_protocol_version: Option<u16>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MessageReference {
    pub message_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutgoingMessage {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_reference: Option<MessageReference>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_keeps_mentions_edits_replies_and_attachments() {
        let raw = r#"{
            "id":"9","channel_id":"100","timestamp":"t","type":0,
            "content":"hello <@2> in <#100>",
            "edited_timestamp":"later",
            "author":{"id":"1","username":"a"},
            "mentions":[{"id":"2","username":"munch","global_name":"Munch"}],
            "attachments":[{"id":"5","url":"https://cdn.example/a.png","proxy_url":"https://media.example/a.png","content_type":"image/png"}],
            "message_reference":{"message_id":"8"}
        }"#;
        let message = Message::from(serde_json::from_str::<WireMessage>(raw).unwrap());
        // The text stays as sent; the UI turns mentions into names.
        assert_eq!(message.content, "hello <@2> in <#100>");
        assert_eq!(message.edited_timestamp.as_deref(), Some("later"));
        assert_eq!(message.mentions[0].display_name(), "Munch");
        assert_eq!(message.attachments[0].url, "https://cdn.example/a.png");
        assert!(message.attachments[0].is_image());
        assert_eq!(message.reply_to.unwrap().message_id.get(), 8);

        let embed = r#"{"id":"1","channel_id":"1","content":"","author":{"id":"1","username":"a"},"embeds":[{"title":"Welcome card"}]}"#;
        let message = Message::from(serde_json::from_str::<WireMessage>(embed).unwrap());
        assert_eq!(message.embeds[0].title.as_deref(), Some("Welcome card"));
        assert_eq!(message.content, "");

        let both = r#"{"id":"1","channel_id":"1","content":"<a:wave:11> <:smile:22>","author":{"id":"1","username":"a"}}"#;
        let message = Message::from(serde_json::from_str::<WireMessage>(both).unwrap());
        assert_eq!(message.content, "<a:wave:11> <:smile:22>");
    }
    #[test]
    fn identify_names_rencord() {
        let payload = identify_payload("token-value");
        let browser = payload["d"]["properties"]["browser"].as_str().unwrap();
        assert_eq!(browser, "Rencord");
        assert_eq!(payload["d"]["properties"]["device"], "Rencord");
        assert_eq!(payload["d"]["properties"], client_properties());
        assert_eq!(payload["op"], 2);
        assert!(!payload.to_string().contains("Chrome"));
        let header = super_properties();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(header.as_bytes())
            .expect("super properties");
        let value: Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(value["browser"], "Rencord");
        assert_eq!(value["browser_user_agent"], USER_AGENT);
        assert!(!header.contains("token-value"));
    }

    #[test]
    fn gateway_frame_roundtrip() {
        let raw = br#"{"op":10,"d":{"heartbeat_interval":41250},"s":null,"t":null}"#;
        let frame = GatewayFrame::parse(raw).unwrap();
        assert_eq!(frame.op, GatewayOpcode::Hello);
        let hello: HelloData = serde_json::from_value(frame.data.clone()).unwrap();
        assert_eq!(hello.heartbeat_interval, 41250);
        let encoded = serde_json::to_vec(&frame.to_json()).unwrap();
        let again = GatewayFrame::parse(&encoded).unwrap();
        assert_eq!(again.op, GatewayOpcode::Hello);
    }

    #[test]
    fn voice_opcodes_cover_dave() {
        assert_eq!(
            VoiceOpcode::from_u8(21),
            Some(VoiceOpcode::DavePrepareTransition)
        );
        assert_eq!(VoiceOpcode::from_u8(30), Some(VoiceOpcode::MlsWelcome));
        let frame = VoiceFrame {
            seq: None,
            op: VoiceOpcode::Speaking,
            data: json!({"speaking": 1, "ssrc": 9}),
        };
        let parsed = VoiceFrame::parse(&serde_json::to_vec(&frame.to_json()).unwrap()).unwrap();
        assert_eq!(parsed.op, VoiceOpcode::Speaking);
    }

    #[test]
    fn voice_binary_dave_frames_carry_seq_op_and_payload() {
        // [seq u16 BE][op u8][payload]: seq 0x0102, op 27 (proposals).
        let frame = VoiceFrame::parse_binary(&[1, 2, 27, 9, 8, 7]).unwrap();
        assert_eq!(frame.op, VoiceOpcode::MlsProposals);
        assert_eq!(frame.seq, Some(0x0102));
        assert_eq!(frame.data, json!({ "data": [9, 8, 7] }));
        // Too short, or a non-binary opcode, is skipped by callers, not fatal.
        assert!(matches!(
            VoiceFrame::parse_binary(&[1, 2]),
            Err(FrameError::Opcode)
        ));
        assert!(matches!(
            VoiceFrame::parse_binary(&[0, 1, VoiceOpcode::Speaking as u8]),
            Err(FrameError::Opcode)
        ));
        assert_eq!(
            voice_binary(VoiceOpcode::MlsKeyPackage, &[5, 6]),
            vec![26, 5, 6]
        );
    }

    #[test]
    fn voice_clients_connect_and_unknown_ops_and_seq() {
        let connect = VoiceFrame::parse(br#"{"op":11,"d":{"user_ids":["1"]},"seq":4}"#).unwrap();
        assert_eq!(connect.op, VoiceOpcode::ClientsConnect);
        assert_eq!(connect.seq, Some(4));
        // An opcode Rencord does not know is a distinct, skippable error.
        assert!(matches!(
            VoiceFrame::parse(br#"{"op":99,"d":{}}"#),
            Err(FrameError::Opcode)
        ));
    }

    #[test]
    fn voice_heartbeat_acks_the_last_seq_not_the_nonce() {
        let beat = voice_heartbeat(1_700_000_000_000, 12);
        assert_eq!(beat["d"]["t"], 1_700_000_000_000u64);
        assert_eq!(beat["d"]["seq_ack"], 12);
    }

    fn wire(value: serde_json::Value) -> Message {
        Message::from(serde_json::from_value::<WireMessage>(value).unwrap())
    }

    fn author() -> serde_json::Value {
        json!({ "id": "42", "username": "ada", "global_name": "Ada", "avatar": "abc123", "bot": false })
    }

    #[test]
    fn a_join_message_has_a_type_and_no_text() {
        let message = wire(json!({
            "id": "100", "channel_id": "7", "type": 7, "content": "",
            "timestamp": "2026-11-07T10:00:00.000000+00:00", "author": author(),
        }));
        assert_eq!(message.kind, 7);
        assert!(message.is_system());
        assert!(message.content.is_empty());
        let ordinary = wire(json!({
            "id": "101", "channel_id": "7", "type": 0, "content": "hi", "author": author(),
        }));
        assert!(!ordinary.is_system());
        let reply = wire(json!({
            "id": "102", "channel_id": "7", "type": 19, "content": "yes", "author": author(),
        }));
        assert!(!reply.is_system());
    }

    #[test]
    fn a_message_carries_files_embeds_reactions_mentions_a_reply_and_an_edit() {
        let message = wire(json!({
            "id": "200", "channel_id": "7", "type": 19, "content": "look <@43>",
            "timestamp": "2026-11-19T12:00:00.000000+00:00",
            "edited_timestamp": "2026-11-19T12:05:00.000000+00:00",
            "author": author(),
            "mentions": [{ "id": "43", "username": "charles", "global_name": "Charles" }],
            "attachments": [{
                "id": "300", "filename": "clip.png", "size": 2048,
                "url": "https://cdn.discordapp.com/attachments/1/300/clip.png?ex=1&is=2&hm=3&",
                "proxy_url": "https://media.discordapp.net/attachments/1/300/clip.png?ex=1&is=2&hm=3&",
                "content_type": "image/png", "width": 640, "height": 360
            }],
            "embeds": [{
                "type": "link", "title": "Release notes", "description": "What changed",
                "url": "https://example.test/notes", "color": 5198940,
                "provider": { "name": "Example" },
                "thumbnail": { "url": "https://example.test/t.png",
                               "proxy_url": "https://images-ext-1.discordapp.net/external/t.png",
                               "width": 100, "height": 50 }
            }],
            "reactions": [
                { "count": 2, "me": true, "emoji": { "id": null, "name": "\u{1F44D}" } },
                { "count": 1, "me": false, "emoji": { "id": "900", "name": "pepe" } }
            ],
            "message_reference": { "message_id": "150", "channel_id": "7" },
            "referenced_message": { "id": "150", "content": "any news?", "author": author() },
        }));
        assert_eq!(
            message.edited_timestamp.as_deref(),
            Some("2026-11-19T12:05:00.000000+00:00")
        );
        assert_eq!(message.mentions.len(), 1);
        assert!(message.mentions_user(Snowflake::new(43)));
        assert!(!message.mentions_user(Snowflake::new(99)));
        let file = &message.attachments[0];
        assert_eq!(
            (file.filename.as_str(), file.size, file.width, file.height),
            ("clip.png", 2048, Some(640), Some(360))
        );
        assert!(file.is_image() && !file.is_video());
        let embed = &message.embeds[0];
        assert_eq!(embed.kind.as_deref(), Some("link"));
        assert_eq!(embed.title.as_deref(), Some("Release notes"));
        assert_eq!(
            embed.provider.as_ref().and_then(|p| p.name.as_deref()),
            Some("Example")
        );
        assert_eq!(embed.color, Some(5198940));
        assert_eq!(
            embed.thumbnail.as_ref().map(|t| t.best_url()),
            Some("https://images-ext-1.discordapp.net/external/t.png")
        );
        assert_eq!(message.reactions.len(), 2);
        assert!(message.reactions[0].me);
        assert_eq!(message.reactions[1].emoji.id, Some(Snowflake::new(900)));
        let reply = message.reply_to.as_ref().unwrap();
        assert_eq!(reply.message_id, Snowflake::new(150));
        assert_eq!(reply.content.as_deref(), Some("any news?"));
        assert_eq!(reply.author.as_ref().map(|u| u.display_name()), Some("Ada"));
    }

    #[test]
    fn one_bad_attachment_or_embed_does_not_cost_the_message() {
        let message = wire(json!({
            "id": "1", "channel_id": "2", "content": "x", "author": author(),
            "attachments": [
                { "id": "5", "filename": "ok.png", "url": "https://cdn.discordapp.com/a/ok.png", "size": 3 },
                { "id": { "nested": true }, "filename": 4 },
                7
            ],
            "embeds": [ "not an object", { "title": "kept" } ],
            "reactions": null,
            "mentions": [ { "no_id_here": true } ],
        }));
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].filename, "ok.png");
        assert_eq!(message.embeds.len(), 1);
        assert_eq!(message.embeds[0].title.as_deref(), Some("kept"));
        assert!(message.reactions.is_empty());
        assert!(message.mentions.is_empty());
        assert_eq!(message.content, "x");
    }

    #[test]
    fn null_sizes_and_a_deleted_reply_decode() {
        let message = wire(json!({
            "id": "1", "channel_id": "2", "content": "", "author": author(),
            "attachments": [{ "id": "5", "filename": "f.bin", "url": "u", "width": null, "height": null, "content_type": null }],
            "message_reference": { "message_id": "9" },
            "referenced_message": null,
        }));
        assert_eq!(message.attachments[0].width, None);
        assert!(!message.attachments[0].is_image());
        let reply = message.reply_to.unwrap();
        assert_eq!(reply.message_id, Snowflake::new(9));
        assert!(reply.author.is_none() && reply.content.is_none());
        // A type larger than a byte maps to a system message, not a crash.
        let odd = wire(json!({ "id": "1", "channel_id": "2", "type": 4000, "author": author() }));
        assert!(odd.is_system());
    }

    #[test]
    fn a_partial_update_carries_only_what_changed() {
        let patch = MessagePatch::from_value(&json!({
            "id": "10", "channel_id": "2",
            "embeds": [{ "type": "link", "title": "Resolved", "url": "https://example.test" }],
        }))
        .unwrap();
        assert_eq!(patch.id, Snowflake::new(10));
        assert_eq!(patch.embeds.as_ref().map(|e| e.len()), Some(1));
        assert_eq!(patch.content, None);
        assert_eq!(patch.attachments, None);
        assert!(patch.full.is_none());
        let full = MessagePatch::from_value(&json!({
            "id": "10", "channel_id": "2", "content": "edited", "author": author(),
            "edited_timestamp": "2026-11-20T01:00:00.000000+00:00",
        }))
        .unwrap();
        assert_eq!(full.content.as_deref(), Some("edited"));
        assert_eq!(
            full.full.as_ref().map(|m| m.content.as_str()),
            Some("edited")
        );
        assert!(MessagePatch::from_value(&json!({ "embeds": [] })).is_err());
    }

    #[test]
    fn a_message_carries_its_authors_server_profile() {
        let message = wire(json!({
            "id": "1", "channel_id": "2", "content": "hi", "author": author(),
            "member": { "nick": "The Boss", "roles": ["900", "901"], "joined_at": "2026-01-01T00:00:00Z" },
        }));
        let member = message.member.unwrap();
        assert_eq!(member.nickname(), Some("The Boss"));
        assert_eq!(member.roles, vec![Snowflake::new(900), Snowflake::new(901)]);
        // A member that is not an object cannot cost the message.
        let odd = wire(json!({
            "id": "1", "channel_id": "2", "content": "hi", "author": author(), "member": 7,
        }));
        assert!(odd.member.is_none());
        assert_eq!(odd.content, "hi");
        // An empty nickname is not a nickname.
        let blank = wire(json!({
            "id": "1", "channel_id": "2", "content": "hi", "author": author(),
            "member": { "nick": "", "roles": [] },
        }));
        assert_eq!(blank.member.unwrap().nickname(), None);
    }
}
