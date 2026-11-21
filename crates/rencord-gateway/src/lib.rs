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
    GatewayOpcode, HelloData, ReadyData, VoiceServerUpdate, VoiceStateDispatch, WireMessage,
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
        guild_ids: Vec<Snowflake>,
        discord_theme: Option<String>,
        discord_client_theme: Option<String>,
    },
    MessageCreate(Message),
    MessageUpdate(Message),
    MessageDelete {
        channel_id: Snowflake,
        message_id: Snowflake,
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

pub async fn run(
    config: GatewayConfig,
    events: mpsc::Sender<GatewayEvent>,
    mut commands: mpsc::Receiver<GatewayCommand>,
) -> Result<(), GatewayError> {
    let mut session_id: Option<String> = None;
    let mut sequence: Option<u64> = None;
    let mut resumes = 0u32;
    loop {
        match run_socket(
            &config,
            &events,
            &mut commands,
            &mut session_id,
            &mut sequence,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(GatewayError::Closed) if session_id.is_some() && resumes < 5 => {
                resumes += 1;
                let _ = events.send(GatewayEvent::Reconnect).await;
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
            Err(err) => return Err(err),
        }
    }
}

async fn run_socket(
    config: &GatewayConfig,
    events: &mpsc::Sender<GatewayEvent>,
    commands: &mut mpsc::Receiver<GatewayCommand>,
    session_id: &mut Option<String>,
    sequence: &mut Option<u64>,
) -> Result<(), GatewayError> {
    let (socket, _) = connect_async(&config.url).await?;
    let (mut write, mut read) = socket.split();
    let mut identified = false;
    let mut inflater = ZlibStream::new();
    let heartbeat_generation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    let (heartbeat_tx, mut heartbeat_rx) = mpsc::channel::<Option<u64>>(4);

    loop {
        tokio::select! {
            incoming = read.next() => {
                let Some(incoming) = incoming else {
                    return Err(GatewayError::Closed);
                };
                let message = incoming?;
                let bytes = match message {
                    WsMessage::Text(text) => text.as_bytes().to_vec(),
                    WsMessage::Binary(bin) => inflater.feed(&bin)?,
                    WsMessage::Close(_) => return Err(GatewayError::Closed),
                    WsMessage::Ping(payload) => {
                        write.send(WsMessage::Pong(payload)).await?;
                        continue;
                    }
                    _ => continue,
                };
                if bytes.is_empty() {
                    continue;
                }
                let frame = GatewayFrame::parse(&bytes)?;
                if let Some(seq) = frame.sequence {
                    *sequence = Some(seq);
                }
                match frame.op {
                    GatewayOpcode::Hello => {
                        let hello: HelloData = serde_json::from_value(frame.data)?;
                        let generation = heartbeat_generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        let flag = heartbeat_generation.clone();
                        let tx = heartbeat_tx.clone();
                        let interval = Duration::from_millis(hello.heartbeat_interval.max(1));
                        tokio::spawn(async move {
                            let mut ticker = tokio::time::interval(interval);
                            ticker.tick().await;
                            loop {
                                ticker.tick().await;
                                if flag.load(std::sync::atomic::Ordering::Relaxed) != generation {
                                    break;
                                }
                                if tx.send(None).await.is_err() {
                                    break;
                                }
                            }
                        });
                        if let Some(id) = session_id.clone() {
                            let payload = resume_payload(&config.token, &id, sequence.unwrap_or(0));
                            write.send(WsMessage::Text(payload.to_string().into())).await?;
                            identified = true;
                        } else if !identified {
                            let payload = identify_payload(&config.token);
                            write.send(WsMessage::Text(payload.to_string().into())).await?;
                            identified = true;
                        }
                    }
                    GatewayOpcode::HeartbeatAck => {}
                    GatewayOpcode::Heartbeat => {
                        let _ = heartbeat_tx.send(*sequence).await;
                    }
                    GatewayOpcode::Reconnect => {
                        let _ = events.send(GatewayEvent::Reconnect).await;
                        return Err(GatewayError::Closed);
                    }
                    GatewayOpcode::InvalidSession => {
                        let resumable = frame.data.as_bool().unwrap_or(false);
                        if !resumable {
                            *session_id = None;
                        }
                        return Err(GatewayError::Closed);
                    }
                    GatewayOpcode::Dispatch => {
                        let name = frame.event_name.clone().unwrap_or_default();
                        if name == "RESUMED" {
                            let _ = events.send(GatewayEvent::Resumed).await;
                            continue;
                        }
                        if let Some(event) = decode_dispatch(&name, frame.data)? {
                            if let GatewayEvent::Ready { session_id: id, .. } = &event {
                                *session_id = Some(id.clone());
                            }
                            let _ = events.send(event).await;
                        }
                    }
                    GatewayOpcode::Identify
                    | GatewayOpcode::PresenceUpdate
                    | GatewayOpcode::VoiceStateUpdate
                    | GatewayOpcode::Resume
                    | GatewayOpcode::RequestGuildMembers => {}
                }
            }
            beat = heartbeat_rx.recv() => {
                if beat.is_some() {
                    let payload = heartbeat_payload(*sequence);
                    write.send(WsMessage::Text(payload.to_string().into())).await?;
                }
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
                guild_ids: ready.guilds.into_iter().map(|guild| guild.id).collect(),
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
                guild_ids,
                discord_theme,
                discord_client_theme,
            } => {
                assert!(discord_theme.is_none());
                assert!(discord_client_theme.is_none());
                assert_eq!(session_id, "sess");
                assert_eq!(user.username, "ada");
                assert_eq!(guild_ids[0].get(), 100);
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
}
