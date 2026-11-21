//! Login, MFA, logout, and the remote-auth ticket exchange.
//!
//! These are the same routes the official client calls. Passwords and codes
//! are sent once and are not logged. A captcha response is returned to the
//! caller so it can open Discord's page. This crate does not solve captchas.

use rencord_model::{Token, User};
use rencord_protocol::{discord_locale, super_properties, USER_AGENT};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::redact::redact;
use crate::AuthError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MfaMethod {
    Totp,
    Sms,
    Backup,
    WebAuthn,
}

impl MfaMethod {
    fn path(self) -> &'static str {
        match self {
            Self::Totp => "/auth/mfa/totp",
            Self::Sms => "/auth/mfa/sms",
            Self::Backup => "/auth/mfa/backup",
            Self::WebAuthn => "/auth/mfa/webauthn",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MfaChallenge {
    pub ticket: String,
    pub login_instance_id: Option<String>,
    pub totp: bool,
    pub sms: bool,
    pub backup: bool,
    /// Stringified WebAuthn request options, when Discord offered a passkey.
    pub webauthn: Option<String>,
    pub user_id: Option<String>,
}

impl std::fmt::Debug for MfaChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MfaChallenge")
            .field("ticket", &"[redacted]")
            .field("totp", &self.totp)
            .field("sms", &self.sms)
            .field("backup", &self.backup)
            .field("webauthn", &self.webauthn.is_some())
            .field("user_id", &self.user_id)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptchaChallenge {
    pub sitekey: String,
    pub service: String,
    pub session_id: Option<String>,
    pub rqdata: Option<String>,
    pub rqtoken: Option<String>,
}

/// Result of a login or MFA submit. `Debug` redacts the session.
pub enum LoginOutcome {
    Session {
        token: Token,
        user_id: Option<String>,
    },
    Mfa(MfaChallenge),
    Captcha(CaptchaChallenge),
}

impl std::fmt::Debug for LoginOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session { user_id, .. } => f
                .debug_struct("Session")
                .field("token", &"Token([redacted])")
                .field("user_id", user_id)
                .finish(),
            Self::Mfa(challenge) => f.debug_tuple("Mfa").field(challenge).finish(),
            Self::Captcha(challenge) => f.debug_tuple("Captcha").field(challenge).finish(),
        }
    }
}

pub struct AuthClient {
    http: reqwest::Client,
    root: String,
}

impl AuthClient {
    pub fn new() -> Self {
        Self::with_root(crate::AUTH_API_ROOT.to_owned())
    }

    pub fn with_root(root: String) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("reqwest client builds with rustls");
        Self { http, root }
    }

    pub async fn exchange_remote_ticket(&self, ticket: &str) -> Result<String, AuthError> {
        let value = self
            .post_json(
                "/users/@me/remote-auth/login",
                json!({ "ticket": ticket }),
                None,
            )
            .await?;
        value
            .get("encrypted_token")
            .and_then(|item| item.as_str())
            .map(str::to_owned)
            .ok_or_else(|| {
                AuthError::Message("Discord did not return an encrypted session.".into())
            })
    }

    pub async fn login_with_password(
        &self,
        login: &str,
        password: &str,
    ) -> Result<LoginOutcome, AuthError> {
        let value = self
            .post_json(
                "/auth/login",
                json!({
                    "login": login,
                    "password": password,
                    "undelete": false,
                    "login_source": Value::Null,
                    "gift_code_sku_id": Value::Null,
                }),
                None,
            )
            .await?;
        parse_login_value(&value)
    }

    /// `code` is a TOTP code, an SMS code, a backup code, or the stringified
    /// WebAuthn assertion JSON. The caller supplies the assertion. This
    /// function does not talk to an authenticator.
    pub async fn submit_mfa(
        &self,
        method: MfaMethod,
        ticket: &str,
        login_instance_id: Option<&str>,
        code: &str,
    ) -> Result<LoginOutcome, AuthError> {
        let mut body = json!({
            "ticket": ticket,
            "code": code,
            "login_source": Value::Null,
            "gift_code_sku_id": Value::Null,
        });
        if let Some(instance) = login_instance_id {
            body["login_instance_id"] = json!(instance);
        }
        let value = self.post_json(method.path(), body, None).await?;
        parse_login_value(&value)
    }

    /// Ask Discord to text a code. Returns the already-redacted phone string.
    pub async fn send_sms(&self, ticket: &str) -> Result<String, AuthError> {
        let value = self
            .post_json("/auth/mfa/sms/send", json!({ "ticket": ticket }), None)
            .await?;
        let phone = value
            .get("phone")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        Ok(redact_phone(phone))
    }

    pub async fn logout(&self, token: &Token) -> Result<(), AuthError> {
        let url = format!("{}/auth/logout", self.root.trim_end_matches('/'));
        let response = self
            .http
            .post(url)
            .headers(auth_headers(Some(token.expose())))
            .json(&json!({}))
            .send()
            .await
            .map_err(|_| AuthError::Message("Could not reach Discord.".into()))?;
        let status = response.status();
        if status.is_success() || status.as_u16() == 401 {
            return Ok(());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| AuthError::Message("Could not reach Discord.".into()))?;
        Err(message_error(status.as_u16(), &bytes))
    }

    pub async fn current_user(&self, token: &Token) -> Result<User, AuthError> {
        let url = format!("{}/users/@me", self.root.trim_end_matches('/'));
        let response = self
            .http
            .get(url)
            .headers(auth_headers(Some(token.expose())))
            .send()
            .await
            .map_err(|_| AuthError::Message("Could not reach Discord.".into()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| AuthError::Message("Could not reach Discord.".into()))?;
        if status.as_u16() == 401 {
            return Err(AuthError::Unauthorized);
        }
        if !status.is_success() {
            return Err(message_error(status.as_u16(), &bytes));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| AuthError::Message("Discord's profile response was not usable.".into()))
    }

    async fn post_json(
        &self,
        path: &str,
        body: Value,
        token: Option<&str>,
    ) -> Result<Value, AuthError> {
        let url = format!("{}{path}", self.root.trim_end_matches('/'));
        let response = self
            .http
            .post(url)
            .headers(auth_headers(token))
            .json(&body)
            .send()
            .await
            .map_err(|_| AuthError::Message("Could not reach Discord.".into()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| AuthError::Message("Could not reach Discord.".into()))?;
        if status.as_u16() == 401 && !path.contains("auth/login") && !path.contains("auth/mfa") {
            return Err(AuthError::Unauthorized);
        }
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if value.get("suspended_user_token").is_some() && value.get("token").is_none() {
            return Err(AuthError::Suspended);
        }
        // A captcha is a 400. Hand the parsed body back so the caller can open
        // Discord's page instead of treating it as a generic failure.
        if is_captcha(&value) {
            return Ok(value);
        }
        if !status.is_success() {
            return Err(message_error(status.as_u16(), &bytes));
        }
        Ok(value)
    }
}

impl Default for AuthClient {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse_login_value(value: &Value) -> Result<LoginOutcome, AuthError> {
    if let Some(captcha) = captcha_from(value) {
        return Ok(LoginOutcome::Captcha(captcha));
    }
    if value.get("mfa").and_then(|item| item.as_bool()) == Some(true) {
        let ticket = value
            .get("ticket")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_owned();
        if ticket.is_empty() {
            return Err(AuthError::Message(
                "Discord asked for a code but did not send a ticket.".into(),
            ));
        }
        return Ok(LoginOutcome::Mfa(MfaChallenge {
            ticket,
            login_instance_id: value
                .get("login_instance_id")
                .and_then(|item| item.as_str())
                .map(str::to_owned),
            totp: value
                .get("totp")
                .and_then(|item| item.as_bool())
                .unwrap_or(false),
            sms: value
                .get("sms")
                .and_then(|item| item.as_bool())
                .unwrap_or(false),
            backup: value
                .get("backup")
                .and_then(|item| item.as_bool())
                .unwrap_or(false),
            webauthn: value
                .get("webauthn")
                .and_then(|item| item.as_str())
                .map(str::to_owned),
            user_id: value
                .get("user_id")
                .and_then(|item| item.as_str())
                .map(str::to_owned),
        }));
    }
    if let Some(token) = value.get("token").and_then(|item| item.as_str()) {
        if !token.is_empty() {
            return Ok(LoginOutcome::Session {
                token: Token::new(token.to_owned()),
                user_id: value
                    .get("user_id")
                    .and_then(|item| item.as_str())
                    .map(str::to_owned),
            });
        }
    }
    Err(AuthError::Message(
        "Discord did not complete the login.".into(),
    ))
}

fn is_captcha(value: &Value) -> bool {
    captcha_from(value).is_some()
}

fn captcha_from(value: &Value) -> Option<CaptchaChallenge> {
    let sitekey = value
        .get("captcha_sitekey")
        .and_then(|item| item.as_str())?;
    Some(CaptchaChallenge {
        sitekey: sitekey.to_owned(),
        service: value
            .get("captcha_service")
            .and_then(|item| item.as_str())
            .unwrap_or("hcaptcha")
            .to_owned(),
        session_id: value
            .get("captcha_session_id")
            .and_then(|item| item.as_str())
            .map(str::to_owned),
        rqdata: value
            .get("captcha_rqdata")
            .and_then(|item| item.as_str())
            .map(str::to_owned),
        rqtoken: value
            .get("captcha_rqtoken")
            .and_then(|item| item.as_str())
            .map(str::to_owned),
    })
}

fn message_error(status: u16, bytes: &[u8]) -> AuthError {
    if status == 401 {
        return AuthError::Unauthorized;
    }
    #[derive(Deserialize)]
    struct Body {
        message: Option<String>,
    }
    let message = serde_json::from_slice::<Body>(bytes)
        .ok()
        .and_then(|body| body.message)
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| format!("Discord returned {status}."));
    AuthError::Message(redact(&message))
}

fn auth_headers(token: Option<&str>) -> reqwest::header::HeaderMap {
    use reqwest::header::{
        HeaderMap, HeaderValue, AUTHORIZATION, ORIGIN, REFERER, USER_AGENT as UA,
    };
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(USER_AGENT) {
        headers.insert(UA, value);
    }
    if let Ok(value) = HeaderValue::from_str(&super_properties()) {
        headers.insert("X-Super-Properties", value);
    }
    if let Ok(value) = HeaderValue::from_str(&discord_locale()) {
        headers.insert("X-Discord-Locale", value);
    }
    headers.insert(ORIGIN, HeaderValue::from_static("https://discord.com"));
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://discord.com/login"),
    );
    if let Some(token) = token {
        if let Ok(value) = HeaderValue::from_str(token) {
            headers.insert(AUTHORIZATION, value);
        }
    }
    headers
}

/// Download a Discord CDN avatar. Other hosts are refused.
pub async fn fetch_avatar(url: &str) -> Result<Vec<u8>, AuthError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| AuthError::Message("The avatar address was not usable.".into()))?;
    let host = parsed.host_str().unwrap_or("");
    if host != "cdn.discordapp.com" {
        return Err(AuthError::Message(
            "The avatar address was not usable.".into(),
        ));
    }
    let bytes = reqwest::get(parsed)
        .await
        .map_err(|_| AuthError::Message("Could not load the avatar.".into()))?
        .bytes()
        .await
        .map_err(|_| AuthError::Message("Could not load the avatar.".into()))?;
    if bytes.len() > 2_000_000 {
        return Err(AuthError::Message("The avatar was too large.".into()));
    }
    Ok(bytes.to_vec())
}

pub fn redact_phone(phone: &str) -> String {
    let digits: String = phone.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() <= 2 {
        return "a phone on the account".into();
    }
    let tail = &digits[digits.len() - 2..];
    format!("a number ending in {tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn parsers_cover_session_mfa_and_captcha_without_leaking_the_token() {
        let session = parse_login_value(&json!({
            "user_id": "42",
            "token": "user.session.secret-token"
        }))
        .unwrap();
        let rendered = format!("{session:?}");
        assert!(rendered.contains("42"));
        assert!(!rendered.contains("secret-token"));
        match session {
            LoginOutcome::Session { token, .. } => {
                assert_eq!(token.expose(), "user.session.secret-token")
            }
            other => panic!("expected a session, got {other:?}"),
        }

        let mfa = parse_login_value(&json!({
            "mfa": true,
            "totp": true,
            "sms": true,
            "backup": true,
            "ticket": "ticket-value",
            "webauthn": "{\"publicKey\":{}}"
        }))
        .unwrap();
        match mfa {
            LoginOutcome::Mfa(challenge) => {
                assert!(challenge.totp && challenge.sms && challenge.backup);
                assert!(challenge.webauthn.is_some());
            }
            other => panic!("expected mfa, got {other:?}"),
        }

        let captcha = parse_login_value(&json!({
            "captcha_key": ["captcha-required"],
            "captcha_sitekey": "site-key",
            "captcha_service": "hcaptcha"
        }))
        .unwrap();
        match captcha {
            LoginOutcome::Captcha(challenge) => {
                assert_eq!(challenge.service, "hcaptcha");
                assert_eq!(challenge.sitekey, "site-key");
            }
            other => panic!("expected captcha, got {other:?}"),
        }
    }

    #[test]
    fn error_bodies_are_redacted() {
        let err = message_error(400, br#"{"message":"nope user.session.secret-token"}"#);
        let text = err.public_message();
        assert!(!text.contains("secret-token"));
        assert!(text.contains("[redacted]"));
    }

    #[tokio::test]
    async fn password_mfa_logout_and_remote_exchange_hit_the_documented_routes() {
        let (base, request) = serve(
            r#"{"token":"user.session.secret-token","user_id":"9"}"#,
            200,
        )
        .await;
        let client = AuthClient::with_root(base);
        let outcome = client
            .login_with_password("ada@example.com", "correct horse battery")
            .await
            .unwrap();
        let captured = request.await.unwrap();
        assert!(captured.starts_with("POST /auth/login "));
        assert!(captured.contains("\"login\":\"ada@example.com\""));
        assert!(captured.contains("\"password\":\"correct horse battery\""));
        let lower = captured.to_ascii_lowercase();
        assert!(lower.contains("x-super-properties:"));
        assert!(lower.contains("origin: https://discord.com"));
        assert!(lower.contains("user-agent: rencord"));
        let encoded = captured
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("x-super-properties:"))
            .unwrap()
            .split_once(':')
            .unwrap()
            .1
            .trim();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let properties: Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(properties["browser"], "Rencord");
        assert!(!format!("{outcome:?}").contains("secret-token"));

        let (base, request) = serve(
            r#"{"mfa":true,"totp":true,"sms":false,"backup":true,"ticket":"mfa-ticket"}"#,
            200,
        )
        .await;
        let client = AuthClient::with_root(base);
        let outcome = client
            .submit_mfa(MfaMethod::Totp, "mfa-ticket", None, "123456")
            .await
            .unwrap();
        assert!(matches!(outcome, LoginOutcome::Mfa(_)));
        assert!(request.await.unwrap().contains("POST /auth/mfa/totp "));

        let (base, request) = serve(r#"{"phone":"+*******0085"}"#, 200).await;
        let client = AuthClient::with_root(base);
        let phone = client.send_sms("mfa-ticket").await.unwrap();
        assert_eq!(phone, "a number ending in 85");
        assert!(!phone.contains("0085"));
        assert!(request.await.unwrap().contains("POST /auth/mfa/sms/send "));

        let (base, request) = serve(r#"{"encrypted_token":"Y2lwaGVy"}"#, 200).await;
        let client = AuthClient::with_root(base);
        let encrypted = client.exchange_remote_ticket("plain-ticket").await.unwrap();
        assert_eq!(encrypted, "Y2lwaGVy");
        let body = request.await.unwrap();
        assert!(body.contains("POST /users/@me/remote-auth/login "));
        assert!(body.contains("\"ticket\":\"plain-ticket\""));
        assert!(!body.to_ascii_lowercase().contains("authorization:"));

        let (base, request) = serve("{}", 204).await;
        let client = AuthClient::with_root(base);
        let token = Token::new("user.session.secret-token");
        client.logout(&token).await.unwrap();
        let body = request.await.unwrap();
        assert!(body.starts_with("POST /auth/logout "));
        assert!(body
            .to_ascii_lowercase()
            .contains("authorization: user.session.secret-token"));
        assert!(!format!("{token:?}").contains("secret"));

        let (base, _) = serve(r#"{"message":"401: Unauthorized","code":0}"#, 401).await;
        let client = AuthClient::with_root(base);
        let err = client
            .current_user(&Token::new("stale.session.value"))
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::Unauthorized));
        assert!(!err.public_message().contains("stale"));
    }

    async fn serve(
        body: &'static str,
        status: u16,
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = vec![0u8; 16384];
            let n = socket.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            let _ = tx.send(request);
            let reason = if status == 204 {
                "No Content"
            } else if status == 401 {
                "Unauthorized"
            } else {
                "OK"
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        (format!("http://{addr}"), rx)
    }
}
