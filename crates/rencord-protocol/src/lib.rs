//! Gateway and voice websocket payloads.
//!
//! Rencord identifies itself as Rencord. It does not imitate an official
//! browser fingerprint.

use base64::Engine;
use rencord_model::{ChannelKind, Message, Snowflake, User};
use serde::{Deserialize, Serialize};
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
    pub unavailable: bool,
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
#[derive(Clone, Debug, Deserialize)]
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
            recipient_ids: self.recipients.into_iter().map(|user| user.id).collect(),
        }
    }
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
}

impl From<WireMessage> for Message {
    fn from(value: WireMessage) -> Self {
        Message {
            id: value.id,
            channel_id: value.channel_id,
            author: value.author,
            content: value.content,
            timestamp: value.timestamp,
        }
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
}
