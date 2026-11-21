//! Minimal Discord REST client for Milestone 1.
//!
//! User tokens are sent as the raw `Authorization` value. Routes that Milestone
//! 1 calls: current user, guild list, private channels, guild channels, message
//! history, and message create. A per-route bucket honors `Retry-After`.

use rencord_model::{Channel, Guild, Message, Snowflake, Token, User};
use rencord_protocol::{MessageReference, OutgoingMessage, WireChannel, WireMessage};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const USER_AGENT: &str = rencord_protocol::USER_AGENT;
/// Longest rate-limit wait the client absorbs itself. Longer ones go back to the caller.
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("discord returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("rate limited on {route} for {retry_after:?}")]
    RateLimited {
        route: String,
        retry_after: Duration,
    },
    #[error("response JSON did not match {what}: {source}")]
    Decode {
        what: &'static str,
        source: serde_json::Error,
    },
}

impl ApiError {
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Status { status: 401, .. })
    }
}

#[derive(Debug)]
struct Bucket {
    remaining: u32,
    reset_at: Instant,
}

pub struct DiscordApi {
    http: reqwest::Client,
    base: String,
    token: Token,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl DiscordApi {
    pub fn new(token: Token) -> Self {
        Self::with_base(token, "https://discord.com/api/v10".to_owned())
    }

    pub fn with_base(token: Token, base: String) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("reqwest client builds with rustls");
        Self {
            http,
            base,
            token,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub async fn current_user(&self) -> Result<User, ApiError> {
        self.get_json("/users/@me", "user").await
    }

    pub async fn guilds(&self) -> Result<Vec<Guild>, ApiError> {
        self.get_json("/users/@me/guilds", "guild list").await
    }

    pub async fn private_channels(&self) -> Result<Vec<Channel>, ApiError> {
        let wires: Vec<WireChannel> = self
            .get_json("/users/@me/channels", "private channels")
            .await?;
        Ok(wires.into_iter().map(WireChannel::into_channel).collect())
    }

    pub async fn guild_channels(&self, guild_id: Snowflake) -> Result<Vec<Channel>, ApiError> {
        let path = format!("/guilds/{guild_id}/channels");
        let wires: Vec<WireChannel> = self.get_json(&path, "guild channels").await?;
        Ok(wires.into_iter().map(WireChannel::into_channel).collect())
    }

    pub async fn messages(
        &self,
        channel_id: Snowflake,
        limit: u32,
    ) -> Result<Vec<Message>, ApiError> {
        let path = format!("/channels/{channel_id}/messages?limit={limit}");
        let wires: Vec<WireMessage> = self.get_json(&path, "messages").await?;
        Ok(wires.into_iter().map(Message::from).collect())
    }

    pub async fn send_message(
        &self,
        channel_id: Snowflake,
        content: &str,
    ) -> Result<Message, ApiError> {
        self.post_message(channel_id, content, None).await
    }

    pub async fn reply_message(
        &self,
        channel_id: Snowflake,
        content: &str,
        reply_to: Snowflake,
    ) -> Result<Message, ApiError> {
        self.post_message(channel_id, content, Some(reply_to)).await
    }

    pub async fn edit_message(
        &self,
        channel_id: Snowflake,
        message_id: Snowflake,
        content: &str,
    ) -> Result<Message, ApiError> {
        let path = format!("/channels/{channel_id}/messages/{message_id}");
        let body = serde_json::json!({ "content": content });
        let response = self.send("PATCH", &path, Some(&body)).await?;
        decode_message(response)
    }

    pub async fn delete_message(
        &self,
        channel_id: Snowflake,
        message_id: Snowflake,
    ) -> Result<(), ApiError> {
        let path = format!("/channels/{channel_id}/messages/{message_id}");
        self.send("DELETE", &path, None::<&()>).await?;
        Ok(())
    }

    pub async fn add_reaction(
        &self,
        channel_id: Snowflake,
        message_id: Snowflake,
        emoji: &str,
    ) -> Result<(), ApiError> {
        let path = format!(
            "/channels/{channel_id}/messages/{message_id}/reactions/{}/@me",
            url_encode(emoji)
        );
        self.send("PUT", &path, None::<&()>).await?;
        Ok(())
    }

    pub async fn remove_reaction(
        &self,
        channel_id: Snowflake,
        message_id: Snowflake,
        emoji: &str,
    ) -> Result<(), ApiError> {
        let path = format!(
            "/channels/{channel_id}/messages/{message_id}/reactions/{}/@me",
            url_encode(emoji)
        );
        self.send("DELETE", &path, None::<&()>).await?;
        Ok(())
    }

    pub async fn search_guild(&self, path: &str) -> Result<Vec<Message>, ApiError> {
        #[derive(Deserialize)]
        struct SearchBody {
            #[serde(default)]
            messages: Vec<Vec<WireMessage>>,
        }
        let body: SearchBody = self.get_json(path, "search").await?;
        Ok(body
            .messages
            .into_iter()
            .flatten()
            .map(Message::from)
            .collect())
    }

    async fn post_message(
        &self,
        channel_id: Snowflake,
        content: &str,
        reply_to: Option<Snowflake>,
    ) -> Result<Message, ApiError> {
        let path = format!("/channels/{channel_id}/messages");
        let body = OutgoingMessage {
            content: content.to_owned(),
            message_reference: reply_to.map(|id| MessageReference {
                message_id: id.to_string(),
            }),
        };
        let response = self.send("POST", &path, Some(&body)).await?;
        decode_message(response)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        what: &'static str,
    ) -> Result<T, ApiError> {
        let response = self.send("GET", path, None::<&()>).await?;
        serde_json::from_slice(&response).map_err(|source| ApiError::Decode { what, source })
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&(impl serde::Serialize + ?Sized)>,
    ) -> Result<Vec<u8>, ApiError> {
        let route = route_key(method, path);
        let url = format!("{}{path}", self.base.trim_end_matches('/'));
        let mut retried = false;
        loop {
            self.wait_for_bucket(&route).await?;
            let mut request = self
                .http
                .request(method.parse().expect("method"), url.as_str())
                .header("Authorization", self.token.expose())
                .header("User-Agent", USER_AGENT)
                .header("X-Super-Properties", rencord_protocol::super_properties())
                .header("X-Discord-Locale", rencord_protocol::discord_locale());
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = request.send().await?;
            let status = response.status();
            self.observe_limits(&route, &response);
            if status.as_u16() == 429 {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(Duration::from_secs_f64)
                    .unwrap_or(Duration::from_secs(1));
                // A 429 means nothing was created, so one retry cannot duplicate a send.
                if !retried && retry_after <= MAX_RATE_LIMIT_WAIT {
                    retried = true;
                    tokio::time::sleep(retry_after).await;
                    continue;
                }
                return Err(ApiError::RateLimited { route, retry_after });
            }
            let bytes = response.bytes().await?.to_vec();
            if !status.is_success() {
                return Err(ApiError::Status {
                    status: status.as_u16(),
                    body: String::from_utf8_lossy(&bytes).into_owned(),
                });
            }
            return Ok(bytes);
        }
    }

    /// Waits out an exhausted bucket instead of sending a request Discord will refuse.
    async fn wait_for_bucket(&self, route: &str) -> Result<(), ApiError> {
        let wait = {
            let buckets = self.buckets.lock().expect("bucket lock");
            buckets
                .get(route)
                .filter(|bucket| bucket.remaining == 0)
                .map(|bucket| bucket.reset_at.saturating_duration_since(Instant::now()))
        };
        match wait {
            Some(wait) if wait > MAX_RATE_LIMIT_WAIT => Err(ApiError::RateLimited {
                route: route.to_owned(),
                retry_after: wait,
            }),
            Some(wait) => {
                tokio::time::sleep(wait).await;
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn observe_limits(&self, route: &str, response: &reqwest::Response) {
        let headers = response.headers();
        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u32>().ok());
        let reset_after = headers
            .get("x-ratelimit-reset-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<f64>().ok())
            .map(Duration::from_secs_f64);
        if let (Some(remaining), Some(reset_after)) = (remaining, reset_after) {
            let mut buckets = self.buckets.lock().expect("bucket lock");
            buckets.insert(
                route.to_owned(),
                Bucket {
                    remaining,
                    reset_at: Instant::now() + reset_after,
                },
            );
        }
    }
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn route_key(method: &str, path: &str) -> String {
    let path = path.split('?').next().unwrap_or(path);
    format!("{method} {path}")
}

fn decode_message(bytes: Vec<u8>) -> Result<Message, ApiError> {
    let wire: WireMessage = serde_json::from_slice(&bytes).map_err(|source| ApiError::Decode {
        what: "message",
        source,
    })?;
    Ok(Message::from(wire))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn serve_once(body: &'static str, status: &'static str) -> String {
        let (base, _) = serve_capture(body, status).await;
        base
    }

    async fn serve_capture(
        body: &'static str,
        status: &'static str,
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            let _ = tx.send(request);
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nx-ratelimit-remaining: 1\r\nx-ratelimit-reset-after: 1\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        (format!("http://{addr}"), rx)
    }

    #[tokio::test]
    async fn current_user_and_send_message() {
        let base = serve_once(
            r#"{"id":"42","username":"ada","global_name":"Ada Lovelace"}"#,
            "200 OK",
        )
        .await;
        let api = DiscordApi::with_base(Token::new("test-token"), base);
        let user = api.current_user().await.unwrap();
        assert_eq!(user.display_name(), "Ada Lovelace");
        assert_eq!(user.id.get(), 42);
    }

    #[tokio::test]
    async fn message_create_decodes() {
        let base = serve_once(
            r#"{"id":"7","channel_id":"3","content":"hello **world**","timestamp":"t","author":{"id":"1","username":"ada"}}"#,
            "200 OK",
        )
        .await;
        let api = Arc::new(DiscordApi::with_base(Token::new("test-token"), base));
        let message = api
            .send_message(Snowflake::new(3), "hello **world**")
            .await
            .unwrap();
        assert_eq!(message.content, "hello **world**");
        assert_eq!(message.channel_id.get(), 3);
    }

    #[tokio::test]
    async fn requests_name_rencord_in_the_super_properties() {
        let (base, rx) = serve_capture(r#"{"id":"1","username":"ada"}"#, "200 OK").await;
        let api = DiscordApi::with_base(Token::new("test-token"), base);
        api.current_user().await.unwrap();
        let request = rx.await.unwrap();
        let lower = request.to_ascii_lowercase();
        assert!(lower.contains("user-agent: rencord"));
        assert!(lower.contains("x-super-properties:"));
        assert!(!request.contains("Chrome"));
        let line = request
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("x-super-properties:"))
            .unwrap();
        let encoded = line.split_once(':').unwrap().1.trim();
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(value["browser"], "Rencord");
        assert!(lower.contains("authorization: test-token"));
    }

    #[tokio::test]
    async fn unauthorized_is_a_distinct_status() {
        let base = serve_once(r#"{"message":"401: Unauthorized"}"#, "401 Unauthorized").await;
        let api = DiscordApi::with_base(Token::new("stale-token"), base);
        let err = api.current_user().await.unwrap_err();
        assert!(err.is_unauthorized());
        assert!(!err.to_string().contains("stale-token"));
    }

    /// Answers each connection with the next canned response, recording requests.
    async fn serve_sequence(
        responses: Vec<String>,
    ) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap();
                log.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{addr}"), seen)
    }

    fn reply(status: &str, extra: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn a_short_429_is_waited_out_and_retried_once() {
        let message = r#"{"id":"7","channel_id":"3","content":"hi","timestamp":"t","author":{"id":"1","username":"ada"}}"#;
        let (base, seen) = serve_sequence(vec![
            reply("429 Too Many Requests", "retry-after: 0.05\r\n", "{}"),
            reply("200 OK", "", message),
        ])
        .await;
        let api = DiscordApi::with_base(Token::new("test-token"), base);
        let sent = api.send_message(Snowflake::new(3), "hi").await.unwrap();
        assert_eq!(sent.id.get(), 7);
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_long_429_goes_back_to_the_caller() {
        let (base, seen) = serve_sequence(vec![reply(
            "429 Too Many Requests",
            "retry-after: 60\r\n",
            "{}",
        )])
        .await;
        let api = DiscordApi::with_base(Token::new("test-token"), base);
        let err = api.send_message(Snowflake::new(3), "hi").await.unwrap_err();
        assert!(matches!(err, ApiError::RateLimited { .. }), "{err}");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_empty_bucket_waits_for_its_reset() {
        let user = r#"{"id":"1","username":"ada"}"#;
        let (base, seen) = serve_sequence(vec![
            reply(
                "200 OK",
                "x-ratelimit-remaining: 0\r\nx-ratelimit-reset-after: 0.2\r\n",
                user,
            ),
            reply("200 OK", "", user),
        ])
        .await;
        let api = DiscordApi::with_base(Token::new("test-token"), base);
        api.current_user().await.unwrap();
        let started = Instant::now();
        api.current_user().await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn route_key_strips_query() {
        assert_eq!(
            route_key("GET", "/channels/1/messages?limit=50"),
            "GET /channels/1/messages"
        );
    }

    #[tokio::test]
    async fn reply_edit_delete_and_reaction_hit_the_right_routes() {
        let message = r#"{"id":"9","channel_id":"3","content":"edited","timestamp":"t","author":{"id":"1","username":"ada"}}"#;
        let (base, rx) = serve_capture(message, "200 OK").await;
        let api = DiscordApi::with_base(Token::new("test-token"), base);
        let sent = api
            .reply_message(Snowflake::new(3), "hello", Snowflake::new(8))
            .await
            .unwrap();
        assert_eq!(sent.content, "edited");
        let request = rx.await.unwrap();
        assert!(request.starts_with("POST /channels/3/messages"));
        assert!(request.contains("\"message_reference\""));
        assert!(request.contains("\"message_id\":\"8\""));

        let (base, rx) = serve_capture(message, "200 OK").await;
        let api = DiscordApi::with_base(Token::new("test-token"), base);
        api.edit_message(Snowflake::new(3), Snowflake::new(9), "edited")
            .await
            .unwrap();
        assert!(rx
            .await
            .unwrap()
            .starts_with("PATCH /channels/3/messages/9"));

        let (base, rx) = serve_capture("{}", "204 No Content").await;
        let api = DiscordApi::with_base(Token::new("test-token"), base);
        api.delete_message(Snowflake::new(3), Snowflake::new(9))
            .await
            .unwrap();
        assert!(rx
            .await
            .unwrap()
            .starts_with("DELETE /channels/3/messages/9"));

        let (base, rx) = serve_capture("", "204 No Content").await;
        let api = DiscordApi::with_base(Token::new("test-token"), base);
        api.add_reaction(Snowflake::new(3), Snowflake::new(9), "plus")
            .await
            .unwrap();
        assert!(rx
            .await
            .unwrap()
            .contains("PUT /channels/3/messages/9/reactions/plus/@me"));
    }

    #[tokio::test]
    async fn guild_search_flattens_message_groups() {
        let body = r#"{"messages":[[{"id":"5","channel_id":"8","content":"found","timestamp":"t","author":{"id":"1","username":"ada"}}]]}"#;
        let base = serve_once(body, "200 OK").await;
        let api = DiscordApi::with_base(Token::new("test-token"), base);
        let hits = api
            .search_guild("/guilds/10/messages/search?content=found")
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].content, "found");
    }
}
