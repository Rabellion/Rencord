//! When to open, move, or leave a voice connection.
//!
//! The desktop sends gateway opcode 4, then waits for our own voice state
//! (the session id) and a voice server update (the token and endpoint). A
//! null endpoint means the server is not ready yet. A later endpoint is a
//! move. This type does not open a socket.

use rencord_model::Snowflake;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceGatewayUpdate {
    pub guild_id: Option<Snowflake>,
    pub channel_id: Option<Snowflake>,
    pub self_mute: bool,
    pub self_deaf: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceStatus {
    Idle,
    Connecting { server: String },
    WaitingForServer,
    RtcConnected { server: String },
    DaveReady { server: String },
    Error { server: String, message: String },
}

impl VoiceStatus {
    pub fn text(&self) -> String {
        match self {
            Self::Idle => "Not in voice".into(),
            Self::Connecting { server } => format!("Connecting to {server}"),
            Self::WaitingForServer => "Waiting for the voice server".into(),
            Self::RtcConnected { server } => format!("RTC connected to {server}"),
            Self::DaveReady { server } => format!("DAVE ready ({server})"),
            Self::Error { server, message } => format!("Voice error ({server}): {message}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceConnect {
    pub ws_url: String,
    pub server_id: String,
    pub user_id: String,
    pub session_id: String,
    pub token: String,
    pub server_name: String,
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub struct VoiceJoin {
    pub user_id: Snowflake,
    guild_id: Option<Snowflake>,
    channel_id: Option<Snowflake>,
    session_id: Option<String>,
    token: Option<String>,
    endpoint: Option<String>,
    self_mute: bool,
    self_deaf: bool,
    generation: u64,
    pub status: VoiceStatus,
}

impl VoiceJoin {
    pub fn new(user_id: Snowflake) -> Self {
        Self {
            user_id,
            guild_id: None,
            channel_id: None,
            session_id: None,
            token: None,
            endpoint: None,
            self_mute: false,
            self_deaf: false,
            generation: 0,
            status: VoiceStatus::Idle,
        }
    }

    pub fn join(
        &mut self,
        guild_id: Option<Snowflake>,
        channel_id: Snowflake,
    ) -> VoiceGatewayUpdate {
        self.guild_id = guild_id;
        self.channel_id = Some(channel_id);
        self.session_id = None;
        self.token = None;
        self.endpoint = None;
        self.generation = self.generation.wrapping_add(1);
        self.status = VoiceStatus::Connecting {
            server: "voice".into(),
        };
        self.gateway_update()
    }

    pub fn leave(&mut self) -> VoiceGatewayUpdate {
        self.channel_id = None;
        self.session_id = None;
        self.token = None;
        self.endpoint = None;
        self.generation = self.generation.wrapping_add(1);
        self.status = VoiceStatus::Idle;
        self.gateway_update()
    }

    pub fn set_self_state(&mut self, mute: bool, deaf: bool) -> VoiceGatewayUpdate {
        self.self_mute = mute;
        self.self_deaf = deaf;
        self.gateway_update()
    }

    /// Our own voice state. Other users are ignored. Returns a connect request
    /// when the session id arrives after the server endpoint.
    pub fn on_voice_state(
        &mut self,
        user_id: Snowflake,
        channel_id: Option<Snowflake>,
        session_id: &str,
    ) -> Option<VoiceConnect> {
        if user_id != self.user_id {
            return None;
        }
        self.channel_id = channel_id;
        if channel_id.is_none() {
            self.session_id = None;
            self.status = VoiceStatus::Idle;
            return None;
        }
        self.session_id = Some(session_id.to_owned());
        self.connect_if_ready()
    }

    /// `None` endpoint means wait. A new endpoint while we are up is a move.
    pub fn on_voice_server(&mut self, token: &str, endpoint: Option<&str>) -> Option<VoiceConnect> {
        self.token = Some(token.to_owned());
        match endpoint.map(str::trim).filter(|value| !value.is_empty()) {
            None => {
                self.endpoint = None;
                self.status = VoiceStatus::WaitingForServer;
                None
            }
            Some(endpoint) => {
                let moved = self.endpoint.as_deref() != Some(endpoint);
                self.endpoint = Some(endpoint.to_owned());
                if moved {
                    self.generation = self.generation.wrapping_add(1);
                }
                self.connect_if_ready()
            }
        }
    }

    pub fn connected(&mut self, dave: bool) {
        let server = self.server_name();
        self.status = if dave {
            VoiceStatus::DaveReady { server }
        } else {
            VoiceStatus::RtcConnected { server }
        };
    }

    pub fn failed(&mut self, message: impl Into<String>) {
        self.status = VoiceStatus::Error {
            server: self.server_name(),
            message: message.into(),
        };
    }

    pub fn channel_id(&self) -> Option<u64> {
        self.channel_id.map(|id| id.get())
    }

    pub fn server_name(&self) -> String {
        self.endpoint.clone().unwrap_or_else(|| "voice".into())
    }

    fn connect_if_ready(&mut self) -> Option<VoiceConnect> {
        let (Some(session_id), Some(token), Some(endpoint), Some(_)) = (
            self.session_id.clone(),
            self.token.clone(),
            self.endpoint.clone(),
            self.channel_id,
        ) else {
            return None;
        };
        let server_name = endpoint.clone();
        self.status = VoiceStatus::Connecting {
            server: server_name.clone(),
        };
        Some(VoiceConnect {
            ws_url: voice_ws_url(&endpoint),
            server_id: self.guild_id.map(|id| id.to_string()).unwrap_or_default(),
            user_id: self.user_id.to_string(),
            session_id,
            token,
            server_name,
            generation: self.generation,
        })
    }

    fn gateway_update(&self) -> VoiceGatewayUpdate {
        VoiceGatewayUpdate {
            guild_id: self.guild_id,
            channel_id: self.channel_id,
            self_mute: self.self_mute,
            self_deaf: self.self_deaf,
        }
    }
}

/// Discord's voice endpoint is `host:port` with no scheme.
pub fn voice_ws_url(endpoint: &str) -> String {
    let host = endpoint
        .trim()
        .trim_start_matches("wss://")
        .trim_end_matches('/');
    format!("wss://{host}/?v=8")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> Snowflake {
        Snowflake::new(7)
    }

    #[test]
    fn null_endpoint_waits_and_a_later_one_connects() {
        let mut join = VoiceJoin::new(user());
        let update = join.join(Some(Snowflake::new(1)), Snowflake::new(2));
        assert_eq!(update.channel_id, Some(Snowflake::new(2)));
        assert!(join
            .on_voice_state(user(), Some(Snowflake::new(2)), "sess")
            .is_none());
        assert!(join.on_voice_server("token", None).is_none());
        assert_eq!(join.status, VoiceStatus::WaitingForServer);
        let connect = join
            .on_voice_server("token", Some("example.discord.media:443"))
            .expect("both halves arrived");
        assert_eq!(connect.ws_url, "wss://example.discord.media:443/?v=8");
        assert_eq!(connect.session_id, "sess");
        assert_eq!(connect.server_name, "example.discord.media:443");
        assert!(connect
            .ws_url
            .starts_with("wss://example.discord.media:443"));
    }

    #[test]
    fn a_moved_endpoint_is_a_new_connection_and_leave_clears_the_channel() {
        let mut join = VoiceJoin::new(user());
        join.join(Some(Snowflake::new(1)), Snowflake::new(2));
        join.on_voice_state(user(), Some(Snowflake::new(2)), "sess");
        let first = join.on_voice_server("token", Some("a.media:443")).unwrap();
        let second = join
            .on_voice_server("token-2", Some("b.media:443"))
            .unwrap();
        assert_ne!(first.generation, second.generation);
        assert_eq!(second.server_name, "b.media:443");
        let leave = join.leave();
        assert_eq!(leave.channel_id, None);
        assert_eq!(join.status, VoiceStatus::Idle);
        assert!(join
            .on_voice_state(Snowflake::new(9), Some(Snowflake::new(2)), "other")
            .is_none());
    }

    #[test]
    fn mute_and_deafen_are_gateway_updates() {
        let mut join = VoiceJoin::new(user());
        join.join(None, Snowflake::new(4));
        let update = join.set_self_state(true, true);
        assert!(update.self_mute && update.self_deaf);
        assert_eq!(update.channel_id, Some(Snowflake::new(4)));
    }
}
