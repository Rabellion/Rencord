//! Discord remote auth (QR login), gateway version 2.
//!
//! The socket speaks `wss://remote-auth-gateway.discord.gg/?v=2`. The desktop
//! generates a 2048-bit RSA key, proves it can decrypt a nonce, and shows
//! `https://discord.com/ra/<fingerprint>`. After the phone confirms, the
//! ticket from `pending_login` is exchanged at `/users/@me/remote-auth/login`
//! and the returned token is decrypted. The gateway requires an `Origin` of
//! `https://discord.com`; the user agent on that socket is still Rencord's.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rencord_model::Token;
use rencord_protocol::USER_AGENT;
use serde_json::{json, Value};
use tokio::time::{sleep_until, Instant, MissedTickBehavior};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use crate::crypto::{encode_nonce, RemoteKey};
use crate::login_api::AuthClient;
use crate::qr::render_qr_png;
use crate::AuthError;

pub const REMOTE_AUTH_GATEWAY: &str = "wss://remote-auth-gateway.discord.gg/?v=2";
pub const AUTH_API_ROOT: &str = "https://discord.com/api/v9";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserPreview {
    pub user_id: String,
    pub username: String,
    pub discriminator: String,
    pub avatar: Option<String>,
    pub global_name: Option<String>,
}

impl UserPreview {
    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }

    pub fn avatar_url(&self) -> String {
        if let Some(hash) = &self.avatar {
            let ext = if hash.starts_with("a_") { "gif" } else { "png" };
            return format!(
                "https://cdn.discordapp.com/avatars/{}/{hash}.{ext}?size=128",
                self.user_id
            );
        }
        let index = default_avatar_index(&self.user_id, &self.discriminator);
        format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
    }
}

fn default_avatar_index(user_id: &str, discriminator: &str) -> u64 {
    if let Ok(value) = discriminator.parse::<u64>() {
        if value > 0 {
            return value % 5;
        }
    }
    user_id.parse::<u64>().map(|id| (id >> 22) % 6).unwrap_or(0)
}

/// Colon-separated user payload from `pending_ticket`, or a JSON object.
pub fn parse_user_payload(text: &str) -> Result<UserPreview, AuthError> {
    let text = text.trim().trim_matches('\0');
    if text.starts_with('{') {
        let value: Value = serde_json::from_str(text)
            .map_err(|_| AuthError::Message("The account preview was not readable.".into()))?;
        let user_id = value
            .get("id")
            .or_else(|| value.get("user_id"))
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_owned();
        let username = value
            .get("username")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_owned();
        if user_id.is_empty() || username.is_empty() {
            return Err(AuthError::Message(
                "The account preview was incomplete.".into(),
            ));
        }
        let avatar = value
            .get("avatar")
            .and_then(|item| item.as_str())
            .filter(|hash| !hash.is_empty() && *hash != "0")
            .map(str::to_owned);
        return Ok(UserPreview {
            user_id,
            username,
            discriminator: value
                .get("discriminator")
                .and_then(|item| item.as_str())
                .unwrap_or("0")
                .to_owned(),
            avatar,
            global_name: value
                .get("global_name")
                .and_then(|item| item.as_str())
                .map(str::to_owned),
        });
    }
    let mut parts = text.splitn(4, ':');
    let user_id = parts.next().unwrap_or("").to_owned();
    let discriminator = parts.next().unwrap_or("0").to_owned();
    let avatar_raw = parts.next().unwrap_or("0");
    let username = parts.next().unwrap_or("").to_owned();
    if user_id.is_empty() || username.is_empty() {
        return Err(AuthError::Message(
            "The account preview was incomplete.".into(),
        ));
    }
    let avatar = if avatar_raw.is_empty() || avatar_raw == "0" {
        None
    } else {
        Some(avatar_raw.to_owned())
    };
    Ok(UserPreview {
        user_id,
        username,
        discriminator,
        avatar,
        global_name: None,
    })
}

/// One thing the gateway asked the client to do. `Debug` does not print frames.
#[derive(Clone, PartialEq, Eq)]
pub enum Step {
    Send(String),
    Qr { url: String },
    User(UserPreview),
    Ticket(String),
    Ack,
    Cancelled,
}

impl std::fmt::Debug for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Send(_) => f.write_str("Send([redacted frame])"),
            Self::Qr { url } => f.debug_struct("Qr").field("url", url).finish(),
            Self::User(user) => f.debug_tuple("User").field(user).finish(),
            Self::Ticket(_) => f.write_str("Ticket([redacted])"),
            Self::Ack => f.write_str("Ack"),
            Self::Cancelled => f.write_str("Cancelled"),
        }
    }
}

impl Step {
    pub fn outbound(&self) -> Option<&str> {
        match self {
            Self::Send(frame) => Some(frame),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Hello,
    Nonce,
    Scan,
    Confirm,
}

pub struct RemoteMachine {
    key: RemoteKey,
    phase: Phase,
    heartbeat_ms: u64,
    timeout_ms: u64,
}

impl RemoteMachine {
    pub fn generate() -> Result<Self, AuthError> {
        Ok(Self {
            key: RemoteKey::generate()?,
            phase: Phase::Hello,
            heartbeat_ms: 41_250,
            timeout_ms: 120_000,
        })
    }

    pub fn heartbeat_interval(&self) -> Duration {
        Duration::from_millis(self.heartbeat_ms.max(1))
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.max(1))
    }

    pub fn fingerprint(&self) -> String {
        self.key.fingerprint()
    }

    pub fn decrypt_token(&self, encrypted_b64: &str) -> Result<Token, AuthError> {
        let bytes = self.key.decrypt_b64(encrypted_b64)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| AuthError::Message("The session from Discord was not text.".into()))?;
        let text = text.trim().trim_matches('"').trim().to_owned();
        if text.is_empty() {
            return Err(AuthError::Message(
                "Discord returned an empty session.".into(),
            ));
        }
        Ok(Token::new(text))
    }

    pub fn handle(&mut self, text: &str) -> Result<Vec<Step>, AuthError> {
        let value: Value = serde_json::from_str(text).map_err(|_| {
            AuthError::Message("Discord sent a login frame that was not JSON.".into())
        })?;
        let op = value.get("op").and_then(|item| item.as_str()).unwrap_or("");
        match op {
            "hello" => self.on_hello(&value),
            "nonce_proof" => self.on_nonce(&value),
            "pending_remote_init" => self.on_fingerprint(&value),
            "pending_ticket" => self.on_ticket(&value),
            "pending_login" => self.on_login(&value),
            "heartbeat_ack" => Ok(vec![Step::Ack]),
            "cancel" => {
                self.phase = Phase::Confirm;
                Ok(vec![Step::Cancelled])
            }
            _ => Err(AuthError::Handshake),
        }
    }

    fn on_hello(&mut self, value: &Value) -> Result<Vec<Step>, AuthError> {
        if self.phase != Phase::Hello {
            return Err(AuthError::Handshake);
        }
        if let Some(interval) = value
            .get("heartbeat_interval")
            .and_then(|item| item.as_u64())
        {
            self.heartbeat_ms = interval.max(1);
        }
        if let Some(timeout) = value.get("timeout_ms").and_then(|item| item.as_u64()) {
            self.timeout_ms = timeout.max(1);
        }
        self.phase = Phase::Nonce;
        let frame = json!({
            "op": "init",
            "encoded_public_key": self.key.encoded_public_key(),
        });
        Ok(vec![Step::Send(frame.to_string())])
    }

    fn on_nonce(&mut self, value: &Value) -> Result<Vec<Step>, AuthError> {
        if self.phase != Phase::Nonce {
            return Err(AuthError::Handshake);
        }
        let encrypted = value
            .get("encrypted_nonce")
            .and_then(|item| item.as_str())
            .ok_or(AuthError::Handshake)?;
        let nonce = self.key.decrypt_b64(encrypted)?;
        self.phase = Phase::Scan;
        let frame = json!({
            "op": "nonce_proof",
            "nonce": encode_nonce(&nonce),
        });
        Ok(vec![Step::Send(frame.to_string())])
    }

    fn on_fingerprint(&mut self, value: &Value) -> Result<Vec<Step>, AuthError> {
        if self.phase != Phase::Scan {
            return Err(AuthError::Handshake);
        }
        let fingerprint = value
            .get("fingerprint")
            .and_then(|item| item.as_str())
            .ok_or(AuthError::Handshake)?;
        if !fingerprint_chars_ok(fingerprint) || fingerprint != self.key.fingerprint() {
            return Err(AuthError::Handshake);
        }
        self.phase = Phase::Confirm;
        let url = format!("https://discord.com/ra/{fingerprint}");
        tracing::info!("remote auth code is ready");
        Ok(vec![Step::Qr { url }])
    }

    fn on_ticket(&mut self, value: &Value) -> Result<Vec<Step>, AuthError> {
        if self.phase != Phase::Confirm {
            return Err(AuthError::Handshake);
        }
        let encrypted = value
            .get("encrypted_user_payload")
            .and_then(|item| item.as_str())
            .ok_or(AuthError::Handshake)?;
        let plain = self.key.decrypt_b64(encrypted)?;
        let text = String::from_utf8(plain)
            .map_err(|_| AuthError::Message("The account preview was not text.".into()))?;
        let user = parse_user_payload(&text)?;
        tracing::info!(user_id = %user.user_id, "remote auth is waiting for confirmation");
        Ok(vec![Step::User(user)])
    }

    fn on_login(&mut self, value: &Value) -> Result<Vec<Step>, AuthError> {
        let ticket = value
            .get("ticket")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .trim()
            .to_owned();
        if ticket.is_empty() {
            return Err(AuthError::Handshake);
        }
        tracing::info!("remote auth ticket is ready to exchange");
        Ok(vec![Step::Ticket(ticket)])
    }
}

fn fingerprint_chars_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

#[derive(Clone)]
pub enum RemoteProgress {
    Qr { url: String, png: Vec<u8> },
    Confirm(UserPreview),
    Refreshing,
}

/// Run the gateway until a token is decrypted, retrying expired or declined
/// attempts a few times. `cancel` stops the loop without logging anything.
pub async fn complete_remote_login(
    gateway_url: &str,
    api_root: &str,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(RemoteProgress),
) -> Result<Token, AuthError> {
    let mut last = AuthError::Disconnected;
    for attempt in 0..3 {
        if cancel.load(Ordering::Relaxed) {
            return Err(AuthError::Cancelled);
        }
        if attempt > 0 {
            on_progress(RemoteProgress::Refreshing);
        }
        match remote_once(gateway_url, api_root, cancel, &mut on_progress).await {
            Ok(token) => return Ok(token),
            Err(AuthError::Cancelled) => return Err(AuthError::Cancelled),
            Err(err) => {
                let retry = matches!(
                    err,
                    AuthError::Expired
                        | AuthError::Declined
                        | AuthError::Handshake
                        | AuthError::Disconnected
                );
                if !retry || attempt == 2 {
                    return Err(err);
                }
                last = err;
            }
        }
    }
    Err(last)
}

async fn remote_once(
    gateway_url: &str,
    api_root: &str,
    cancel: &AtomicBool,
    on_progress: &mut impl FnMut(RemoteProgress),
) -> Result<Token, AuthError> {
    let mut machine = RemoteMachine::generate()?;
    let request = gateway_request(gateway_url)?;
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|_| AuthError::Disconnected)?;
    let mut heartbeat: Option<tokio::time::Interval> = None;
    let mut awaiting_ack = false;
    let mut deadline = Instant::now() + Duration::from_secs(20);

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AuthError::Cancelled);
        }
        let heartbeat_tick = async {
            if let Some(timer) = heartbeat.as_mut() {
                timer.tick().await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        tokio::select! {
            _ = sleep_until(deadline) => return Err(AuthError::Expired),
            _ = heartbeat_tick => {
                if awaiting_ack {
                    return Err(AuthError::Disconnected);
                }
                socket
                    .send(Message::Text(r#"{"op":"heartbeat"}"#.into()))
                    .await
                    .map_err(|_| AuthError::Disconnected)?;
                awaiting_ack = true;
            }
            incoming = socket.next() => {
                let message = incoming
                    .ok_or(AuthError::Disconnected)?
                    .map_err(|_| AuthError::Disconnected)?;
                match message {
                    Message::Ping(payload) => {
                        socket
                            .send(Message::Pong(payload))
                            .await
                            .map_err(|_| AuthError::Disconnected)?;
                    }
                    Message::Close(frame) => {
                        let code = frame.as_ref().map(|frame| u16::from(frame.code)).unwrap_or(1000);
                        return Err(close_error(code));
                    }
                    Message::Text(text) => {
                        let steps = machine.handle(text.as_str())?;
                        for step in steps {
                            match step {
                                Step::Send(frame) => {
                                    socket
                                        .send(Message::Text(frame.into()))
                                        .await
                                        .map_err(|_| AuthError::Disconnected)?;
                                }
                                Step::Qr { url } => {
                                    let png = render_qr_png(&url).unwrap_or_default();
                                    on_progress(RemoteProgress::Qr { url, png });
                                }
                                Step::User(user) => on_progress(RemoteProgress::Confirm(user)),
                                Step::Ticket(ticket) => {
                                    let client = AuthClient::with_root(api_root.to_owned());
                                    let encrypted = client.exchange_remote_ticket(&ticket).await?;
                                    return machine.decrypt_token(&encrypted);
                                }
                                Step::Ack => awaiting_ack = false,
                                Step::Cancelled => return Err(AuthError::Declined),
                            }
                        }
                        if heartbeat.is_none() && machine.phase == Phase::Nonce {
                            deadline = Instant::now() + machine.timeout();
                            let mut timer = tokio::time::interval(machine.heartbeat_interval());
                            timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
                            timer.reset();
                            heartbeat = Some(timer);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn close_error(code: u16) -> AuthError {
    match code {
        4003 => AuthError::Expired,
        4000..=4002 => AuthError::Handshake,
        _ => AuthError::Disconnected,
    }
}

fn gateway_request(
    url: &str,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, AuthError> {
    let mut request = url
        .into_client_request()
        .map_err(|_| AuthError::Disconnected)?;
    let headers = request.headers_mut();
    headers.insert(
        "Origin",
        http::HeaderValue::from_static("https://discord.com"),
    );
    if let Ok(agent) = http::HeaderValue::from_str(USER_AGENT) {
        headers.insert(http::header::USER_AGENT, agent);
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{encode_nonce, fingerprint_of_spki};
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use futures_util::{SinkExt, StreamExt};
    use rand::rngs::OsRng;
    use rsa::pkcs8::DecodePublicKey;
    use rsa::{Oaep, RsaPublicKey};
    use sha2::Sha256;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_hdr_async;

    #[test]
    fn user_payload_parses_the_colon_form_and_a_null_avatar() {
        let user = parse_user_payload("852892297661906993:0:0:dolfies").unwrap();
        assert_eq!(user.username, "dolfies");
        assert!(user.avatar.is_none());
        assert!(user.avatar_url().contains("embed/avatars/"));
        let with_hash = parse_user_payload("42:0:abc123:ada:extra").unwrap();
        assert_eq!(with_hash.username, "ada:extra");
        assert_eq!(with_hash.avatar.as_deref(), Some("abc123"));
        let json = parse_user_payload(
            r#"{"id":"7","username":"ada","discriminator":"0","avatar":null,"global_name":"Ada"}"#,
        )
        .unwrap();
        assert_eq!(json.display_name(), "Ada");
        assert!(parse_user_payload("nope").is_err());
    }

    #[test]
    fn machine_handshake_proof_fingerprint_user_and_cancel() {
        let mut machine = RemoteMachine::generate().unwrap();
        let hello = machine
            .handle(r#"{"op":"hello","heartbeat_interval":1000,"timeout_ms":5000}"#)
            .unwrap();
        let init: Value = serde_json::from_str(hello[0].outbound().unwrap()).unwrap();
        assert_eq!(init["op"], "init");
        let encoded = init["encoded_public_key"].as_str().unwrap();
        let spki = STANDARD.decode(encoded).unwrap();
        assert_eq!(machine.fingerprint(), fingerprint_of_spki(&spki));
        let public = RsaPublicKey::from_public_key_der(&spki).unwrap();
        let nonce = b"nonce-bytes-for-the-proof";
        let cipher = public
            .encrypt(&mut OsRng, Oaep::new::<Sha256>(), nonce)
            .unwrap();
        let frame = json!({"op": "nonce_proof", "encrypted_nonce": STANDARD.encode(cipher)});
        let proof = machine.handle(&frame.to_string()).unwrap();
        let sent: Value = serde_json::from_str(proof[0].outbound().unwrap()).unwrap();
        assert_eq!(sent["op"], "nonce_proof");
        assert_eq!(sent["nonce"], encode_nonce(nonce));
        assert!(machine
            .handle(r#"{"op":"pending_remote_init","fingerprint":"not-this-key"}"#)
            .is_err());
        let good = json!({"op": "pending_remote_init", "fingerprint": machine.fingerprint()});
        let qr = machine.handle(&good.to_string()).unwrap();
        match &qr[0] {
            Step::Qr { url } => assert_eq!(
                url,
                &format!("https://discord.com/ra/{}", machine.fingerprint())
            ),
            other => panic!("expected a code, got {other:?}"),
        }
        let payload = b"42:0:0:ada";
        let encrypted = public
            .encrypt(&mut OsRng, Oaep::new::<Sha256>(), payload)
            .unwrap();
        let ticket =
            json!({"op": "pending_ticket", "encrypted_user_payload": STANDARD.encode(encrypted)});
        let preview = machine.handle(&ticket.to_string()).unwrap();
        match &preview[0] {
            Step::User(user) => assert_eq!(user.username, "ada"),
            other => panic!("expected a user, got {other:?}"),
        }
        let login = machine
            .handle(r#"{"op":"pending_login","ticket":"plain-ticket"}"#)
            .unwrap();
        match &login[0] {
            Step::Ticket(value) => assert_eq!(value, "plain-ticket"),
            other => panic!("expected a ticket, got {other:?}"),
        }
        assert!(!format!("{login:?}").contains("plain-ticket"));
        let cancel = machine.handle(r#"{"op":"cancel"}"#).unwrap();
        assert!(matches!(cancel[0], Step::Cancelled));
        let token_cipher = public
            .encrypt(
                &mut OsRng,
                Oaep::new::<Sha256>(),
                b"user.session.secret-token",
            )
            .unwrap();
        let token = machine
            .decrypt_token(&STANDARD.encode(token_cipher))
            .unwrap();
        assert_eq!(token.expose(), "user.session.secret-token");
        assert!(!format!("{token:?}").contains("secret"));
    }

    #[tokio::test]
    #[allow(clippy::result_large_err)]
    async fn scripted_gateway_exchanges_a_ticket_and_decrypts_the_token() {
        let gateway_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway_addr = gateway_listener.local_addr().unwrap();
        let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_addr = http_listener.local_addr().unwrap();
        let (key_tx, key_rx) = tokio::sync::oneshot::channel::<Vec<u8>>();

        tokio::spawn(async move {
            let (stream, _) = gateway_listener.accept().await.unwrap();
            let mut socket = accept_hdr_async(stream, |request: &http::Request<()>, response| {
                let origin = request
                    .headers()
                    .get("Origin")
                    .and_then(|value| value.to_str().ok());
                assert_eq!(origin, Some("https://discord.com"));
                let agent = request
                    .headers()
                    .get(http::header::USER_AGENT)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("");
                assert!(agent.contains("Rencord"));
                Ok(response)
            })
            .await
            .unwrap();
            socket
                .send(Message::Text(
                    r#"{"op":"hello","timeout_ms":60000,"heartbeat_interval":60000}"#.into(),
                ))
                .await
                .unwrap();
            let init = next_text(&mut socket).await;
            let init: Value = serde_json::from_str(&init).unwrap();
            let spki = STANDARD
                .decode(init["encoded_public_key"].as_str().unwrap())
                .unwrap();
            let _ = key_tx.send(spki.clone());
            let public = RsaPublicKey::from_public_key_der(&spki).unwrap();
            let nonce = b"gateway-nonce";
            let cipher = public
                .encrypt(&mut OsRng, Oaep::new::<Sha256>(), nonce)
                .unwrap();
            let frame = json!({"op": "nonce_proof", "encrypted_nonce": STANDARD.encode(cipher)});
            socket
                .send(Message::Text(frame.to_string().into()))
                .await
                .unwrap();
            let proof = next_text(&mut socket).await;
            let proof: Value = serde_json::from_str(&proof).unwrap();
            assert_eq!(proof["nonce"], encode_nonce(nonce));
            let fingerprint = fingerprint_of_spki(&spki);
            socket
                .send(Message::Text(
                    json!({"op": "pending_remote_init", "fingerprint": fingerprint})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let user_cipher = public
                .encrypt(&mut OsRng, Oaep::new::<Sha256>(), b"99:0:0:ada")
                .unwrap();
            socket
                .send(Message::Text(
                    json!({"op": "pending_ticket", "encrypted_user_payload": STANDARD.encode(user_cipher)})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Text(
                    r#"{"op":"pending_login","ticket":"plain-ticket"}"#.into(),
                ))
                .await
                .unwrap();
        });

        tokio::spawn(async move {
            let spki = key_rx.await.unwrap();
            let public = RsaPublicKey::from_public_key_der(&spki).unwrap();
            let (mut stream, _) = http_listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]);
            assert!(request.contains("POST /users/@me/remote-auth/login"));
            assert!(request.contains("\"ticket\":\"plain-ticket\""));
            let cipher = public
                .encrypt(
                    &mut OsRng,
                    Oaep::new::<Sha256>(),
                    b"user.session.secret-token",
                )
                .unwrap();
            let body = json!({"encrypted_token": STANDARD.encode(cipher)}).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let mut saw_user = false;
        let token = complete_remote_login(
            &format!("ws://{gateway_addr}"),
            &format!("http://{http_addr}"),
            &AtomicBool::new(false),
            |progress| {
                if let RemoteProgress::Confirm(user) = progress {
                    assert_eq!(user.username, "ada");
                    saw_user = true;
                }
            },
        )
        .await
        .unwrap();
        assert!(saw_user);
        assert_eq!(token.expose(), "user.session.secret-token");
        assert!(!format!("{token:?}").contains("secret-token"));
    }

    async fn next_text<S>(socket: &mut S) -> String
    where
        S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            if let Message::Text(text) = message {
                return text.to_string();
            }
        }
    }
}
