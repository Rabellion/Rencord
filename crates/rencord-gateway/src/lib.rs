//! Gateway session: identify, heartbeat, resume, and dispatch fan-out.
//!
//! The socket speaks JSON. `zlib-stream` inflate is available for servers that
//! still compress, and is covered by a unit test. Live Discord traffic is not
//! contacted from tests; a local websocket stands in for the gateway.

use flate2::{Decompress, FlushDecompress};
use futures_util::{SinkExt, StreamExt};
use rencord_model::{Message, Snowflake, User};
use rencord_protocol::{
    heartbeat_payload, identify_payload, resume_payload, voice_state_update, GatewayFrame,
    GatewayOpcode, HelloData, ReadyData, VoiceServerUpdate, VoiceStateDispatch, WireChannel,
    WireMessage,
};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("websocket error: {0}")]
    Socket(String),
    #[error("gateway closed before ready")]
    Closed,
    #[error("gateway stopped acknowledging heartbeats")]
    NoHeartbeatAck,
    #[error("gateway closed with code {0}; reconnecting cannot help")]
    Fatal(u16),
    #[error("frame error: {0}")]
    Frame(#[from] rencord_protocol::FrameError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("zlib inflate failed")]
    Inflate,
}

impl From<tokio_tungstenite::tungstenite::Error> for GatewayError {
    fn from(value: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Socket(value.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatewayEvent {
    Ready {
        session_id: String,
        user: User,
        guilds: Vec<rencord_protocol::ReadyGuild>,
        discord_theme: Option<String>,
        discord_client_theme: Option<String>,
    },
    MessageCreate(Message),
    MessageUpdate(Message),
    MessageDelete {
        channel_id: Snowflake,
        message_id: Snowflake,
    },
    GuildUpsert {
        id: Snowflake,
        name: Option<String>,
        icon: Option<String>,
        channels: Vec<rencord_model::Channel>,
    },
    GuildDelete(Snowflake),
    ChannelUpsert(rencord_model::Channel),
    ChannelDelete(Snowflake),
    Reaction {
        message_id: Snowflake,
        emoji: String,
        remove: bool,
    },
    VoiceState(VoiceStateDispatch),
    VoiceServer(VoiceServerUpdate),
    Reconnect,
    Resumed,
    Other {
        name: String,
    },
}

#[derive(Clone, Debug)]
pub enum GatewayCommand {
    UpdateVoiceState {
        guild_id: Option<Snowflake>,
        channel_id: Option<Snowflake>,
        self_mute: bool,
        self_deaf: bool,
    },
    Shutdown,
}

#[derive(Clone, Debug)]
pub struct GatewayConfig {
    pub url: String,
    pub token: String,
}

/// Close codes after which a new connection would fail the same way:
/// authentication failed, invalid shard, sharding required, invalid API
/// version, invalid or disallowed intents.
const FATAL_CLOSE_CODES: [u16; 6] = [4004, 4010, 4011, 4012, 4013, 4014];

/// What survives a dropped socket so the next one can resume.
#[derive(Default)]
struct ResumeState {
    session_id: Option<String>,
    sequence: Option<u64>,
    url: Option<String>,
}

/// Runs until a `Shutdown` command, the command or event channel closing, or
/// a fatal close code. Every other failure reconnects with backoff and resumes
/// when Discord still has the session.
pub async fn run(
    config: GatewayConfig,
    events: mpsc::Sender<GatewayEvent>,
    mut commands: mpsc::Receiver<GatewayCommand>,
) -> Result<(), GatewayError> {
    let mut resume = ResumeState::default();
    let mut failures = 0u32;
    loop {
        match run_socket(&config, &events, &mut commands, &mut resume, &mut failures).await {
            Ok(()) => return Ok(()),
            Err(err @ GatewayError::Fatal(_)) => return Err(err),
            Err(err) => {
                tracing::warn!("gateway dropped ({err}); reconnecting");
                if events.send(GatewayEvent::Reconnect).await.is_err() {
                    return Ok(());
                }
                tokio::time::sleep(backoff(failures)).await;
                failures = failures.saturating_add(1);
            }
        }
    }
}

/// 250 ms, doubling per failure in a row, capped at 30 s. Reset by READY or RESUMED.
fn backoff(failures: u32) -> Duration {
    Duration::from_millis(250u64 << failures.min(7)).min(Duration::from_secs(30))
}

/// `resume_gateway_url` from READY carries no query, so reuse the configured one.
fn resume_url(base: &str, configured: &str) -> String {
    let query = configured
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or("v=10&encoding=json");
    format!("{}/?{query}", base.trim_end_matches('/'))
}

async fn next_beat(ticker: &mut Option<tokio::time::Interval>) {
    match ticker {
        Some(ticker) => {
            ticker.tick().await;
        }
        None => std::future::pending().await,
    }
}

async fn run_socket(
    config: &GatewayConfig,
    events: &mpsc::Sender<GatewayEvent>,
    commands: &mut mpsc::Receiver<GatewayCommand>,
    resume: &mut ResumeState,
    failures: &mut u32,
) -> Result<(), GatewayError> {
    let url = match (&resume.session_id, &resume.url) {
        (Some(_), Some(base)) => resume_url(base, &config.url),
        _ => config.url.clone(),
    };
    let (socket, _) = connect_async(url.as_str()).await?;
    let (mut write, mut read) = socket.split();
    let mut inflater = ZlibStream::new();
    let mut ticker: Option<tokio::time::Interval> = None;
    let mut awaiting_ack = false;

    loop {
        tokio::select! {
            incoming = read.next() => {
                let Some(incoming) = incoming else {
                    return Err(GatewayError::Closed);
                };
                let bytes = match incoming? {
                    WsMessage::Text(text) => text.as_bytes().to_vec(),
                    WsMessage::Binary(bin) => inflater.feed(&bin)?,
                    WsMessage::Close(frame) => {
                        let code = frame.map(|frame| u16::from(frame.code));
                        return Err(match code {
                            Some(code) if FATAL_CLOSE_CODES.contains(&code) => GatewayError::Fatal(code),
                            _ => GatewayError::Closed,
                        });
                    }
                    WsMessage::Ping(payload) => {
                        write.send(WsMessage::Pong(payload)).await?;
                        continue;
                    }
                    _ => continue,
                };
                if bytes.is_empty() {
                    continue;
                }
                let frame = match GatewayFrame::parse(&bytes) {
                    Ok(frame) => frame,
                    Err(err) => {
                        tracing::warn!("skipped a gateway frame: {err}");
                        continue;
                    }
                };
                if let Some(seq) = frame.sequence {
                    resume.sequence = Some(seq);
                }
                match frame.op {
                    GatewayOpcode::Hello => {
                        let hello: HelloData = serde_json::from_value(frame.data)?;
                        let interval = Duration::from_millis(hello.heartbeat_interval.max(1));
                        let mut beats = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
                        beats.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        ticker = Some(beats);
                        awaiting_ack = false;
                        let payload = match &resume.session_id {
                            Some(id) => resume_payload(&config.token, id, resume.sequence.unwrap_or(0)),
                            None => identify_payload(&config.token),
                        };
                        write.send(WsMessage::Text(payload.to_string().into())).await?;
                    }
                    GatewayOpcode::HeartbeatAck => awaiting_ack = false,
                    GatewayOpcode::Heartbeat => {
                        let payload = heartbeat_payload(resume.sequence);
                        write.send(WsMessage::Text(payload.to_string().into())).await?;
                    }
                    GatewayOpcode::Reconnect => return Err(GatewayError::Closed),
                    GatewayOpcode::InvalidSession => {
                        if !frame.data.as_bool().unwrap_or(false) {
                            *resume = ResumeState::default();
                        }
                        return Err(GatewayError::Closed);
                    }
                    GatewayOpcode::Dispatch => {
                        let name = frame.event_name.clone().unwrap_or_default();
                        if name == "RESUMED" {
                            *failures = 0;
                            if events.send(GatewayEvent::Resumed).await.is_err() {
                                return Ok(());
                            }
                            continue;
                        }
                        if name == "READY" {
                            resume.url = frame
                                .data
                                .get("resume_gateway_url")
                                .and_then(Value::as_str)
                                .map(str::to_owned);
                        }
                        match decode_dispatch(&name, frame.data) {
                            Ok(Some(event)) => {
                                if let GatewayEvent::Ready { session_id: id, .. } = &event {
                                    resume.session_id = Some(id.clone());
                                    *failures = 0;
                                }
                                if events.send(event).await.is_err() {
                                    return Ok(());
                                }
                            }
                            Ok(None) => {}
                            // One odd payload must not take the whole connection down.
                            Err(err) => tracing::warn!("skipped {name}: {err}"),
                        }
                    }
                    GatewayOpcode::Identify
                    | GatewayOpcode::PresenceUpdate
                    | GatewayOpcode::VoiceStateUpdate
                    | GatewayOpcode::Resume
                    | GatewayOpcode::RequestGuildMembers => {}
                }
            }
            _ = next_beat(&mut ticker) => {
                // No ack since the last beat: the socket is half-open (sleep, NAT timeout).
                if awaiting_ack {
                    return Err(GatewayError::NoHeartbeatAck);
                }
                let payload = heartbeat_payload(resume.sequence);
                write.send(WsMessage::Text(payload.to_string().into())).await?;
                awaiting_ack = true;
            }
            command = commands.recv() => {
                match command {
                    Some(GatewayCommand::Shutdown) | None => return Ok(()),
                    Some(GatewayCommand::UpdateVoiceState { guild_id, channel_id, self_mute, self_deaf }) => {
                        let payload = voice_state_update(guild_id, channel_id, self_mute, self_deaf);
                        write.send(WsMessage::Text(payload.to_string().into())).await?;
                    }
                }
            }
        }
    }
}

fn decode_dispatch(name: &str, data: Value) -> Result<Option<GatewayEvent>, GatewayError> {
    let event = match name {
        "READY" => {
            let ready: ReadyData = serde_json::from_value(data)?;
            Some(GatewayEvent::Ready {
                session_id: ready.session_id,
                user: ready.user,
                guilds: ready.guilds,
                discord_theme: ready
                    .user_settings
                    .as_ref()
                    .and_then(|settings| settings.theme.clone()),
                discord_client_theme: ready
                    .user_settings
                    .as_ref()
                    .and_then(|settings| settings.client_theme.clone()),
            })
        }
        "GUILD_CREATE" | "GUILD_UPDATE" => guild_upsert(&data)?,
        "GUILD_DELETE" => snowflake_field(&data, "id")?.map(GatewayEvent::GuildDelete),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" => channel_upsert(&data)?,
        "CHANNEL_DELETE" => snowflake_field(&data, "id")?.map(GatewayEvent::ChannelDelete),
        "MESSAGE_REACTION_ADD" | "MESSAGE_REACTION_REMOVE" => reaction(name, &data)?,
        "MESSAGE_DELETE" => {
            let channel_id = data.get("channel_id").cloned().unwrap_or(Value::Null);
            let message_id = data.get("id").cloned().unwrap_or(Value::Null);
            let channel_id: Snowflake = serde_json::from_value(channel_id)?;
            let message_id: Snowflake = serde_json::from_value(message_id)?;
            Some(GatewayEvent::MessageDelete {
                channel_id,
                message_id,
            })
        }
        "MESSAGE_CREATE" | "MESSAGE_UPDATE" => {
            let wire: WireMessage = serde_json::from_value(data)?;
            let message = Message::from(wire);
            Some(if name == "MESSAGE_CREATE" {
                GatewayEvent::MessageCreate(message)
            } else {
                GatewayEvent::MessageUpdate(message)
            })
        }
        "VOICE_STATE_UPDATE" => Some(GatewayEvent::VoiceState(serde_json::from_value(data)?)),
        "VOICE_SERVER_UPDATE" => Some(GatewayEvent::VoiceServer(serde_json::from_value(data)?)),
        other if !other.is_empty() => Some(GatewayEvent::Other {
            name: other.to_owned(),
        }),
        _ => None,
    };
    Ok(event)
}

fn snowflake_field(data: &Value, key: &str) -> Result<Option<Snowflake>, GatewayError> {
    match data.get(key) {
        Some(value) => Ok(Some(serde_json::from_value(value.clone())?)),
        None => Ok(None),
    }
}

fn guild_upsert(data: &Value) -> Result<Option<GatewayEvent>, GatewayError> {
    let Some(id) = snowflake_field(data, "id")? else {
        return Ok(None);
    };
    let name = data
        .get("name")
        .and_then(|value| value.as_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    let icon = data
        .get("icon")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let mut channels = Vec::new();
    if let Some(list) = data.get("channels").and_then(|value| value.as_array()) {
        for item in list {
            let Ok(wire) = serde_json::from_value::<WireChannel>(item.clone()) else {
                continue;
            };
            let mut channel = wire.into_channel();
            if channel.guild_id.is_none() {
                channel.guild_id = Some(id);
            }
            channels.push(channel);
        }
    }
    Ok(Some(GatewayEvent::GuildUpsert {
        id,
        name,
        icon,
        channels,
    }))
}

fn channel_upsert(data: &Value) -> Result<Option<GatewayEvent>, GatewayError> {
    let Ok(wire) = serde_json::from_value::<WireChannel>(data.clone()) else {
        return Ok(None);
    };
    Ok(Some(GatewayEvent::ChannelUpsert(wire.into_channel())))
}

fn reaction(name: &str, data: &Value) -> Result<Option<GatewayEvent>, GatewayError> {
    let Some(message_id) = snowflake_field(data, "message_id")? else {
        return Ok(None);
    };
    let emoji = data
        .get("emoji")
        .and_then(|emoji| emoji.get("name"))
        .and_then(|name| name.as_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    let Some(emoji) = emoji else {
        return Ok(None);
    };
    Ok(Some(GatewayEvent::Reaction {
        message_id,
        emoji,
        remove: name == "MESSAGE_REACTION_REMOVE",
    }))
}

/// Discord's `zlib-stream` transport appends `00 00 ff ff` between messages.
pub struct ZlibStream {
    decoder: Decompress,
    buffer: Vec<u8>,
}

impl ZlibStream {
    pub fn new() -> Self {
        Self {
            decoder: Decompress::new(true),
            buffer: Vec::new(),
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<u8>, GatewayError> {
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() < 4 || self.buffer[self.buffer.len() - 4..] != [0, 0, 0xff, 0xff] {
            return Ok(Vec::new());
        }
        let input = std::mem::take(&mut self.buffer);
        let mut output = vec![0u8; input.len().saturating_mul(8).max(256)];
        let before_out = self.decoder.total_out();
        let before_in = self.decoder.total_in();
        self.decoder
            .decompress(&input, &mut output, FlushDecompress::Sync)
            .map_err(|_| GatewayError::Inflate)?;
        let consumed = (self.decoder.total_in() - before_in) as usize;
        let produced = (self.decoder.total_out() - before_out) as usize;
        if consumed < input.len() {
            self.buffer.extend_from_slice(&input[consumed..]);
        }
        output.truncate(produced);
        Ok(output)
    }
}

impl Default for ZlibStream {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    #[test]
    fn zlib_stream_roundtrip() {
        let payload = br#"{"op":10,"d":{"heartbeat_interval":1000},"s":null,"t":null}"#;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write;
        encoder.write_all(payload).unwrap();
        encoder.flush().unwrap();
        let compressed = encoder.get_ref().clone();
        assert!(
            compressed.ends_with(&[0, 0, 0xff, 0xff]),
            "missing sync marker"
        );
        let mut stream = ZlibStream::new();
        let inflated = stream.feed(&compressed).unwrap();
        assert_eq!(inflated, payload);
    }

    #[tokio::test]
    async fn live_dispatch_over_local_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket
                .send(WsMessage::Text(
                    r#"{"op":10,"d":{"heartbeat_interval":60000},"s":null,"t":null}"#.into(),
                ))
                .await
                .unwrap();
            let identify = socket.next().await.unwrap().unwrap();
            let text = identify.into_text().unwrap();
            assert!(text.contains("\"op\":2"), "{text}");
            assert!(text.contains("Rencord"));
            socket
                .send(WsMessage::Text(
                    r#"{"op":0,"t":"READY","s":1,"d":{"session_id":"sess","user":{"id":"9","username":"ada"},"guilds":[{"id":"100"}]}}"#.into(),
                ))
                .await
                .unwrap();
            socket
                .send(WsMessage::Text(
                    r#"{"op":0,"t":"MESSAGE_CREATE","s":2,"d":{"id":"5","channel_id":"8","content":"hi **there**","timestamp":"t","author":{"id":"9","username":"ada"}}}"#.into(),
                ))
                .await
                .unwrap();
            // Keep the socket open until the client finishes reading.
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let (events_tx, mut events_rx) = mpsc::channel(8);
        let (_cmd_tx, cmd_rx) = mpsc::channel(4);
        let url = format!("ws://{addr}");
        let runner = tokio::spawn(run(
            GatewayConfig {
                url,
                token: "token".into(),
            },
            events_tx,
            cmd_rx,
        ));

        let ready = tokio::time::timeout(Duration::from_secs(3), events_rx.recv())
            .await
            .expect("ready timeout")
            .unwrap();
        match ready {
            GatewayEvent::Ready {
                session_id,
                user,
                guilds,
                discord_theme,
                discord_client_theme,
            } => {
                assert!(discord_theme.is_none());
                assert!(discord_client_theme.is_none());
                assert_eq!(session_id, "sess");
                assert_eq!(user.username, "ada");
                assert_eq!(guilds[0].id.get(), 100);
            }
            other => panic!("unexpected {other:?}"),
        }
        let created = tokio::time::timeout(Duration::from_secs(3), events_rx.recv())
            .await
            .expect("message timeout")
            .unwrap();
        match created {
            GatewayEvent::MessageCreate(message) => {
                assert_eq!(message.content, "hi **there**");
                assert_eq!(message.channel_id.get(), 8);
            }
            other => panic!("unexpected {other:?}"),
        }
        runner.abort();
    }

    #[tokio::test]
    async fn resume_after_the_socket_drops() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket
                .send(WsMessage::Text(
                    r#"{"op":10,"d":{"heartbeat_interval":60000},"s":null,"t":null}"#.into(),
                ))
                .await
                .unwrap();
            let identify = socket.next().await.unwrap().unwrap().into_text().unwrap();
            assert!(identify.contains("\"op\":2"), "{identify}");
            socket
                .send(WsMessage::Text(
                    r#"{"op":0,"t":"READY","s":4,"d":{"session_id":"sess-2","user":{"id":"9","username":"ada"},"guilds":[]}}"#.into(),
                ))
                .await
                .unwrap();
            socket.close(None).await.ok();

            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket
                .send(WsMessage::Text(
                    r#"{"op":10,"d":{"heartbeat_interval":60000},"s":null,"t":null}"#.into(),
                ))
                .await
                .unwrap();
            let resume = socket.next().await.unwrap().unwrap().into_text().unwrap();
            assert!(resume.contains("\"op\":6"), "{resume}");
            assert!(resume.contains("sess-2"), "{resume}");
            socket
                .send(WsMessage::Text(
                    r#"{"op":0,"t":"RESUMED","s":5,"d":{}}"#.into(),
                ))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let (events_tx, mut events_rx) = mpsc::channel(8);
        let (_cmd_tx, cmd_rx) = mpsc::channel(4);
        let runner = tokio::spawn(run(
            GatewayConfig {
                url: format!("ws://{addr}"),
                token: "token".into(),
            },
            events_tx,
            cmd_rx,
        ));
        let ready = tokio::time::timeout(Duration::from_secs(3), events_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(ready, GatewayEvent::Ready { .. }));
        let mut saw_resumed = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Some(event)) =
                tokio::time::timeout(Duration::from_millis(500), events_rx.recv()).await
            {
                if matches!(event, GatewayEvent::Resumed) {
                    saw_resumed = true;
                    break;
                }
            }
        }
        assert!(saw_resumed, "gateway did not resume");
        runner.abort();
    }

    fn text(body: &str) -> WsMessage {
        WsMessage::Text(body.to_owned().into())
    }

    fn hello(ms: u64) -> WsMessage {
        text(&format!(
            r#"{{"op":10,"d":{{"heartbeat_interval":{ms}}},"s":null,"t":null}}"#
        ))
    }

    fn ready(session: &str, extra: &str) -> WsMessage {
        text(&format!(
            r#"{{"op":0,"t":"READY","s":1,"d":{{"session_id":"{session}","user":{{"id":"9","username":"ada"}},"guilds":[]{extra}}}}}"#
        ))
    }

    async fn first_op(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    ) -> String {
        socket
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_text()
            .unwrap()
            .to_string()
    }

    fn start(
        url: String,
    ) -> (
        mpsc::Receiver<GatewayEvent>,
        tokio::task::JoinHandle<Result<(), GatewayError>>,
        mpsc::Sender<GatewayCommand>,
    ) {
        let (events_tx, events_rx) = mpsc::channel(16);
        let (cmd_tx, cmd_rx) = mpsc::channel(4);
        let runner = tokio::spawn(run(
            GatewayConfig {
                url,
                token: "token".into(),
            },
            events_tx,
            cmd_rx,
        ));
        (events_rx, runner, cmd_tx)
    }

    async fn next_event(events: &mut mpsc::Receiver<GatewayEvent>) -> GatewayEvent {
        tokio::time::timeout(Duration::from_secs(3), events.recv())
            .await
            .expect("gateway event timeout")
            .expect("gateway stopped")
    }

    #[tokio::test]
    #[allow(clippy::result_large_err)] // tungstenite fixes the handshake callback's error type
    async fn reconnects_after_a_reset_and_resumes_on_the_resume_url() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket.send(hello(60000)).await.unwrap();
            first_op(&mut socket).await;
            let extra = format!(r#","resume_gateway_url":"ws://{addr}/resume""#);
            socket.send(ready("sess", &extra)).await.unwrap();
            drop(socket); // no close frame: the client sees a reset, not a clean close

            let (stream, _) = listener.accept().await.unwrap();
            let path = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
            let seen = path.clone();
            let mut socket = tokio_tungstenite::accept_hdr_async(
                stream,
                move |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                      response| {
                    *seen.lock().unwrap() = request.uri().path().to_owned();
                    Ok(response)
                },
            )
            .await
            .unwrap();
            socket.send(hello(60000)).await.unwrap();
            let resume = first_op(&mut socket).await;
            assert!(
                resume.contains("\"op\":6") && resume.contains("sess"),
                "{resume}"
            );
            assert!(path.lock().unwrap().starts_with("/resume"));
            socket
                .send(text(r#"{"op":0,"t":"RESUMED","s":2,"d":{}}"#))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let (mut events, runner, _commands) = start(format!("ws://{addr}/?v=10&encoding=json"));
        assert!(matches!(
            next_event(&mut events).await,
            GatewayEvent::Ready { .. }
        ));
        assert!(matches!(
            next_event(&mut events).await,
            GatewayEvent::Reconnect
        ));
        assert!(matches!(
            next_event(&mut events).await,
            GatewayEvent::Resumed
        ));
        runner.abort();
    }

    #[tokio::test]
    async fn a_socket_that_stops_acking_heartbeats_is_replaced() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket.send(hello(100)).await.unwrap();
            first_op(&mut socket).await;
            socket.send(ready("sess", "")).await.unwrap();
            // Keep the socket open and read heartbeats without ever acking them.
            tokio::spawn(async move { while socket.next().await.is_some() {} });

            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket.send(hello(60000)).await.unwrap();
            let resume = first_op(&mut socket).await;
            assert!(resume.contains("\"op\":6"), "{resume}");
            socket
                .send(text(r#"{"op":0,"t":"RESUMED","s":2,"d":{}}"#))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let (mut events, runner, _commands) = start(format!("ws://{addr}"));
        assert!(matches!(
            next_event(&mut events).await,
            GatewayEvent::Ready { .. }
        ));
        assert!(matches!(
            next_event(&mut events).await,
            GatewayEvent::Reconnect
        ));
        assert!(matches!(
            next_event(&mut events).await,
            GatewayEvent::Resumed
        ));
        runner.abort();
    }

    #[tokio::test]
    async fn a_dispatch_that_does_not_decode_is_skipped() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket.send(hello(60000)).await.unwrap();
            first_op(&mut socket).await;
            socket.send(ready("sess", "")).await.unwrap();
            // A partial update with no author, then a normal message.
            socket.send(text(r#"{"op":0,"t":"MESSAGE_UPDATE","s":2,"d":{"id":"5","channel_id":"8","embeds":[]}}"#)).await.unwrap();
            socket.send(text(r#"{"op":0,"t":"MESSAGE_CREATE","s":3,"d":{"id":"6","channel_id":"8","content":"still here","timestamp":"t","author":{"id":"9","username":"ada"}}}"#)).await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let (mut events, runner, _commands) = start(format!("ws://{addr}"));
        assert!(matches!(
            next_event(&mut events).await,
            GatewayEvent::Ready { .. }
        ));
        match next_event(&mut events).await {
            GatewayEvent::MessageCreate(message) => assert_eq!(message.content, "still here"),
            other => panic!("unexpected {other:?}"),
        }
        runner.abort();
    }

    #[tokio::test]
    async fn a_session_that_cannot_resume_identifies_again() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket.send(hello(60000)).await.unwrap();
            first_op(&mut socket).await;
            socket.send(ready("old", "")).await.unwrap();
            socket
                .send(text(r#"{"op":9,"d":false,"s":null,"t":null}"#))
                .await
                .unwrap();

            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket.send(hello(60000)).await.unwrap();
            let identify = first_op(&mut socket).await;
            assert!(identify.contains("\"op\":2"), "{identify}");
            socket.send(ready("new", "")).await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let (mut events, runner, _commands) = start(format!("ws://{addr}"));
        assert!(matches!(
            next_event(&mut events).await,
            GatewayEvent::Ready { .. }
        ));
        assert!(matches!(
            next_event(&mut events).await,
            GatewayEvent::Reconnect
        ));
        match next_event(&mut events).await {
            GatewayEvent::Ready { session_id, .. } => assert_eq!(session_id, "new"),
            other => panic!("unexpected {other:?}"),
        }
        runner.abort();
    }

    #[tokio::test]
    async fn authentication_failed_stops_without_retrying() {
        use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket.send(hello(60000)).await.unwrap();
            first_op(&mut socket).await;
            let frame = CloseFrame {
                code: CloseCode::from(4004),
                reason: "Authentication failed.".into(),
            };
            socket.close(Some(frame)).await.ok();
        });
        let (_events, runner, _commands) = start(format!("ws://{addr}"));
        let outcome = tokio::time::timeout(Duration::from_secs(3), runner)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(outcome, Err(GatewayError::Fatal(4004))),
            "{outcome:?}"
        );
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(0), Duration::from_millis(250));
        assert_eq!(backoff(2), Duration::from_secs(1));
        assert_eq!(backoff(50), Duration::from_secs(30));
    }

    #[test]
    fn message_delete_dispatch_carries_ids() {
        let event = decode_dispatch(
            "MESSAGE_DELETE",
            serde_json::json!({"id": "9", "channel_id": "3"}),
        )
        .unwrap();
        match event {
            Some(GatewayEvent::MessageDelete {
                channel_id,
                message_id,
            }) => {
                assert_eq!(channel_id.get(), 3);
                assert_eq!(message_id.get(), 9);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn guild_create_keeps_the_name_icon_and_channels() {
        let event = decode_dispatch(
            "GUILD_CREATE",
            serde_json::json!({
                "id": "10",
                "name": "Analytical Engines",
                "icon": "abc",
                "channels": [{"id": "100", "type": 0, "name": "general"}]
            }),
        )
        .unwrap();
        match event {
            Some(GatewayEvent::GuildUpsert {
                id,
                name,
                icon,
                channels,
            }) => {
                assert_eq!(id.get(), 10);
                assert_eq!(name.as_deref(), Some("Analytical Engines"));
                assert_eq!(icon.as_deref(), Some("abc"));
                assert_eq!(channels[0].name, "general");
                assert_eq!(channels[0].guild_id, Some(Snowflake::new(10)));
            }
            other => panic!("unexpected {other:?}"),
        }
        let gone = decode_dispatch("GUILD_DELETE", serde_json::json!({"id": "10"})).unwrap();
        assert!(matches!(gone, Some(GatewayEvent::GuildDelete(id)) if id.get() == 10));
        let reaction = decode_dispatch(
            "MESSAGE_REACTION_ADD",
            serde_json::json!({"message_id": "5", "emoji": {"name": "wave"}}),
        )
        .unwrap();
        assert!(matches!(
            reaction,
            Some(GatewayEvent::Reaction { remove: false, .. })
        ));
    }
}
