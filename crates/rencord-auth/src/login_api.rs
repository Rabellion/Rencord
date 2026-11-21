//! Login, MFA, logout, and the remote-auth ticket exchange.
//!
//! These are the same routes the official client calls. Passwords and codes
//! are sent once and are not logged. A captcha response is returned to the
//! caller so it can open Discord's page. This crate does not solve captchas.

use rencord_model::{Token, User};
use rencord_protocol::{discord_locale, discord_timezone, super_properties, USER_AGENT};
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

/// hCaptcha solution headers. Rencord does not produce these. Discord's page does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptchaSolution {
    pub key: String,
    pub rqtoken: Option<String>,
    pub session_id: Option<String>,
}

/// Ticket exchange result. The ciphertext is still encrypted with our RSA key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteExchange {
    EncryptedToken(String),
    Captcha(CaptchaChallenge),
}

pub struct AuthClient {
    http: reqwest::Client,
    root: String,
    fingerprint: Option<String>,
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
        Self {
            http,
            root,
            fingerprint: None,
        }
    }

    pub fn with_fingerprint(mut self, fingerprint: impl Into<String>) -> Self {
        self.fingerprint = Some(fingerprint.into());
        self
    }

    /// `GET /experiments` mints the `X-Fingerprint` Discord expects before login.
    /// A failure leaves the header off. The ticket exchange still runs.
    pub async fn prepare(&mut self) {
        if self.fingerprint.is_some() {
            return;
        }
        let url = format!("{}/experiments", self.root.trim_end_matches('/'));
        let Ok(response) = self
            .http
            .get(url)
            .headers(auth_headers(None, None))
            .send()
            .await
        else {
            return;
        };
        if !response.status().is_success() {
            return;
        }
        let Ok(bytes) = response.bytes().await else {
            return;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            return;
        };
        let Some(fingerprint) = value.get("fingerprint").and_then(|item| item.as_str()) else {
            return;
        };
        if (8..128).contains(&fingerprint.len())
            && fingerprint
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        {
            self.fingerprint = Some(fingerprint.to_owned());
        }
    }

    /// Exchange the plaintext `pending_login` ticket.
    ///
    /// The path is still `POST /api/v9/users/@me/remote-auth/login` and the
    /// success field is still `encrypted_token` (docs.discord.food, late 2026).
    /// A captcha is a 400 with `captcha_key`. Any other body keeps Discord's
    /// `code` and `message`. The ticket is never copied into the error.
    pub async fn exchange_remote_ticket(&self, ticket: &str) -> Result<RemoteExchange, AuthError> {
        self.exchange_remote_ticket_solved(ticket, None).await
    }

    /// Retry after Discord's page has solved the captcha. The solution is sent
    /// as `X-Captcha-Key`, `X-Captcha-Rqtoken`, and `X-Captcha-Session-Id`.
    pub async fn exchange_remote_ticket_solved(
        &self,
        ticket: &str,
        solution: Option<&CaptchaSolution>,
    ) -> Result<RemoteExchange, AuthError> {
        let value = self
            .post_json(
                "/users/@me/remote-auth/login",
                json!({ "ticket": ticket }),
                None,
                solution,
            )
            .await?;
        if let Some(captcha) = captcha_from(&value) {
            tracing::debug!(
                body = %redact(&value.to_string()),
                "remote auth ticket exchange asked for a captcha"
            );
            return Ok(RemoteExchange::Captcha(captcha));
        }
        if let Some(token) = value
            .get("encrypted_token")
            .and_then(|item| item.as_str())
            .filter(|token| !token.is_empty())
        {
            return Ok(RemoteExchange::EncryptedToken(token.to_owned()));
        }
        let bytes = serde_json::to_vec(&value).unwrap_or_default();
        Err(exchange_failure(200, &bytes))
    }

    pub async fn login_with_password(
        &self,
        login: &str,
        password: &str,
    ) -> Result<LoginOutcome, AuthError> {
        self.login_with_password_solved(login, password, None).await
    }

    pub async fn login_with_password_solved(
        &self,
        login: &str,
        password: &str,
        solution: Option<&CaptchaSolution>,
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
                solution,
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
        self.submit_mfa_solved(method, ticket, login_instance_id, code, None)
            .await
    }

    pub async fn submit_mfa_solved(
        &self,
        method: MfaMethod,
        ticket: &str,
        login_instance_id: Option<&str>,
        code: &str,
        solution: Option<&CaptchaSolution>,
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
        let value = self.post_json(method.path(), body, None, solution).await?;
        parse_login_value(&value)
    }

    /// Ask Discord to text a code. Returns the already-redacted phone string.
    pub async fn send_sms(&self, ticket: &str) -> Result<String, AuthError> {
        let value = self
            .post_json(
                "/auth/mfa/sms/send",
                json!({ "ticket": ticket }),
                None,
                None,
            )
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
            .headers(auth_headers(
                Some(token.expose()),
                self.fingerprint.as_deref(),
            ))
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
            .headers(auth_headers(
                Some(token.expose()),
                self.fingerprint.as_deref(),
            ))
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
        captcha: Option<&CaptchaSolution>,
    ) -> Result<Value, AuthError> {
        let url = format!("{}{path}", self.root.trim_end_matches('/'));
        let mut headers = auth_headers(token, self.fingerprint.as_deref());
        if let Some(captcha) = captcha {
            insert_captcha_headers(&mut headers, captcha);
        }
        let response = self
            .http
            .post(url)
            .headers(headers)
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
    let challenged = value.get("captcha_key").is_some()
        || value
            .get("captcha_sitekey")
            .and_then(|item| item.as_str())
            .is_some();
    if !challenged {
        return None;
    }
    Some(CaptchaChallenge {
        sitekey: value
            .get("captcha_sitekey")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_owned(),
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

fn exchange_failure(status: u16, bytes: &[u8]) -> AuthError {
    log_discord_body(status, bytes);
    AuthError::Message(discord_error_text(status, bytes))
}

fn message_error(status: u16, bytes: &[u8]) -> AuthError {
    log_discord_body(status, bytes);
    if status == 401 {
        return AuthError::Unauthorized;
    }
    AuthError::Message(discord_error_text(status, bytes))
}

fn log_discord_body(status: u16, bytes: &[u8]) {
    let body = String::from_utf8_lossy(bytes);
    tracing::debug!(status, body = %redact(&body), "discord auth response");
}

fn discord_error_text(status: u16, bytes: &[u8]) -> String {
    #[derive(Deserialize)]
    struct Body {
        message: Option<String>,
        code: Option<Value>,
    }
    let parsed = serde_json::from_slice::<Body>(bytes).ok();
    let code = parsed.as_ref().and_then(|body| {
        body.code.as_ref().and_then(|code| {
            code.as_i64()
                .map(|value| value.to_string())
                .or_else(|| code.as_str().map(str::to_owned))
        })
    });
    let message = parsed
        .and_then(|body| body.message)
        .filter(|message| !message.is_empty());
    let text = match (code, message) {
        (Some(code), Some(message)) => format!("Discord returned {status} ({code}): {message}"),
        (None, Some(message)) => format!("Discord returned {status}: {message}"),
        (Some(code), None) => format!("Discord returned {status} ({code})."),
        (None, None) => format!("Discord returned {status} and no session."),
    };
    redact(&text)
}

fn auth_headers(token: Option<&str>, fingerprint: Option<&str>) -> reqwest::header::HeaderMap {
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
    if let Ok(value) = HeaderValue::from_str(&discord_timezone()) {
        headers.insert("X-Discord-Timezone", value);
    }
    if let Some(fingerprint) = fingerprint {
        if let Ok(value) = HeaderValue::from_str(fingerprint) {
            headers.insert("X-Fingerprint", value);
        }
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

fn insert_captcha_headers(headers: &mut reqwest::header::HeaderMap, solution: &CaptchaSolution) {
    use reqwest::header::HeaderValue;
    if let Ok(value) = HeaderValue::from_str(&solution.key) {
        headers.insert("X-Captcha-Key", value);
    }
    if let Some(token) = &solution.rqtoken {
        if let Ok(value) = HeaderValue::from_str(token) {
            headers.insert("X-Captcha-Rqtoken", value);
        }
    }
    if let Some(id) = &solution.session_id {
        if let Ok(value) = HeaderValue::from_str(id) {
            headers.insert("X-Captcha-Session-Id", value);
        }
    }
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
        let client = AuthClient::with_root(base).with_fingerprint("1084179945133187083.abc");
        let encrypted = client.exchange_remote_ticket("plain-ticket").await.unwrap();
        assert!(matches!(
            encrypted,
            RemoteExchange::EncryptedToken(token) if token == "Y2lwaGVy"
        ));
        let body = request.await.unwrap();
        assert!(body.contains("POST /users/@me/remote-auth/login "));
        assert!(body.contains("\"ticket\":\"plain-ticket\""));
        let lower = body.to_ascii_lowercase();
        assert!(!lower.contains("authorization:"));
        assert!(lower.contains("x-discord-timezone:"));
        assert!(lower.contains("x-discord-locale:"));
        assert!(lower.contains("x-fingerprint: 1084179945133187083.abc"));
        assert!(lower.contains("origin: https://discord.com"));
        assert!(lower.contains("user-agent: rencord"));

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

    #[tokio::test]
    async fn remote_exchange_reports_captcha_and_retries_with_solved_headers() {
        let (base, mut requests) = scripted(vec![
            (
                400,
                r#"{"captcha_key":["captcha-required"],"captcha_sitekey":"site-key","captcha_service":"hcaptcha","captcha_rqtoken":"rq-token","captcha_session_id":"sess-1"}"#,
            ),
            (200, r#"{"encrypted_token":"Y2lwaGVy"}"#),
        ])
        .await;
        let client = AuthClient::with_root(base);
        let first = client.exchange_remote_ticket("plain-ticket").await.unwrap();
        match first {
            RemoteExchange::Captcha(challenge) => {
                assert_eq!(challenge.sitekey, "site-key");
                assert_eq!(challenge.rqtoken.as_deref(), Some("rq-token"));
                assert!(!format!("{challenge:?}").contains("plain-ticket"));
            }
            other => panic!("expected a captcha, got {other:?}"),
        }
        let first_request = requests.recv().await.unwrap();
        assert!(!first_request.to_ascii_lowercase().contains("x-captcha-key"));

        let solved = client
            .exchange_remote_ticket_solved(
                "plain-ticket",
                Some(&CaptchaSolution {
                    key: "solved-token".into(),
                    rqtoken: Some("rq-token".into()),
                    session_id: Some("sess-1".into()),
                }),
            )
            .await
            .unwrap();
        assert!(matches!(
            solved,
            RemoteExchange::EncryptedToken(token) if token == "Y2lwaGVy"
        ));
        let retry = requests.recv().await.unwrap();
        let lower = retry.to_ascii_lowercase();
        assert!(lower.contains("x-captcha-key: solved-token"));
        assert!(lower.contains("x-captcha-rqtoken: rq-token"));
        assert!(lower.contains("x-captcha-session-id: sess-1"));
        assert!(retry.contains("POST /users/@me/remote-auth/login "));
    }

    #[tokio::test]
    async fn password_login_retries_with_the_captcha_headers() {
        let (base, mut requests) = scripted(vec![
            (
                400,
                r#"{"captcha_key":["captcha-required"],"captcha_sitekey":"site-key","captcha_service":"hcaptcha","captcha_rqtoken":"rq-token","captcha_session_id":"sess-9","captcha_rqdata":"rq-data"}"#,
            ),
            (200, r#"{"token":"user.session.secret-token","user_id":"9"}"#),
        ])
        .await;
        let client = AuthClient::with_root(base);
        let first = client
            .login_with_password("ada@example.com", "test-password")
            .await
            .unwrap();
        let LoginOutcome::Captcha(challenge) = first else {
            panic!("expected a captcha");
        };
        assert_eq!(challenge.sitekey, "site-key");
        assert_eq!(challenge.rqdata.as_deref(), Some("rq-data"));
        assert_eq!(challenge.session_id.as_deref(), Some("sess-9"));
        let _ = requests.recv().await.unwrap();
        let solved = client
            .login_with_password_solved(
                "ada@example.com",
                "test-password",
                Some(&CaptchaSolution {
                    key: "solved-token".into(),
                    rqtoken: challenge.rqtoken.clone(),
                    session_id: challenge.session_id.clone(),
                }),
            )
            .await
            .unwrap();
        assert!(matches!(solved, LoginOutcome::Session { .. }));
        let retry = requests.recv().await.unwrap();
        let lower = retry.to_ascii_lowercase();
        assert!(lower.contains("post /auth/login "));
        assert!(lower.contains("x-captcha-key: solved-token"));
        assert!(lower.contains("x-captcha-rqtoken: rq-token"));
        assert!(lower.contains("x-captcha-session-id: sess-9"));
        assert!(!format!("{solved:?}").contains("secret-token"));
    }

    #[tokio::test]
    async fn remote_exchange_keeps_discord_code_and_message_and_redacts_the_log() {
        let secret = "ODUyODky.GX5Xdp.22jsdSqEiHLUYEJSsjeq_vJKLp";
        let body = format!(
            r#"{{"code":50035,"message":"Invalid Form Body {secret}","ticket":"{secret}"}}"#
        );
        let (base, _request) = serve_owned(body, 400).await;
        let client = AuthClient::with_root(base);
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let _guard = tracing::subscriber::set_default(Capture(recorded.clone()));
        let err = client
            .exchange_remote_ticket("plain-ticket")
            .await
            .unwrap_err();
        drop(_guard);
        let text = err.public_message();
        assert!(text.contains("50035"), "{text}");
        assert!(text.contains("Invalid Form Body"), "{text}");
        assert!(text.contains("[redacted]"), "{text}");
        assert!(!text.contains("GX5Xdp"), "{text}");
        assert!(!text.contains("plain-ticket"), "{text}");
        assert_eq!(err.caption(), "Discord could not finish signing in");
        let log = recorded.lock().unwrap().clone();
        assert!(log.contains("50035"), "{log}");
        assert!(log.contains("[redacted]"), "{log}");
        assert!(!log.contains("GX5Xdp"), "{log}");
    }

    #[tokio::test]
    async fn prepare_reads_the_experiment_fingerprint() {
        let (base, request) = serve(
            r#"{"fingerprint":"1084179945133187083.JQddgNMmwJPghoBtFmaH7jTmdsw","assignments":[]}"#,
            200,
        )
        .await;
        let mut client = AuthClient::with_root(base);
        client.prepare().await;
        let seen = request.await.unwrap();
        assert!(seen.starts_with("GET /experiments "));
        assert_eq!(
            client.fingerprint.as_deref(),
            Some("1084179945133187083.JQddgNMmwJPghoBtFmaH7jTmdsw")
        );
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

    async fn serve_owned(
        body: String,
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
            let response = format!(
                "HTTP/1.1 {status} Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        (format!("http://{addr}"), rx)
    }

    async fn scripted(
        steps: Vec<(u16, &'static str)>,
    ) -> (String, tokio::sync::mpsc::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            for (status, body) in steps {
                let (mut socket, _) = listener.accept().await.unwrap();
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 16384];
                let n = socket.read(&mut buf).await.unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let _ = tx.send(request).await;
                let reason = if status == 400 { "Bad Request" } else { "OK" };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{addr}"), rx)
    }

    struct Capture(std::sync::Arc<std::sync::Mutex<String>>);

    impl tracing::Subscriber for Capture {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            *metadata.level() <= tracing::Level::DEBUG
        }

        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let mut visitor = Visit(&self.0);
            event.record(&mut visitor);
        }

        fn enter(&self, _span: &tracing::span::Id) {}

        fn exit(&self, _span: &tracing::span::Id) {}
    }

    struct Visit<'a>(&'a std::sync::Mutex<String>);

    impl tracing::field::Visit for Visit<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            let mut guard = self.0.lock().unwrap();
            guard.push_str(field.name());
            guard.push('=');
            guard.push_str(&format!("{value:?}"));
            guard.push('\n');
        }
    }
}
