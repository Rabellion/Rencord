//! Sign-in for Rencord.
//!
//! The primary path is Discord's remote-auth gateway (the QR code). The other
//! path opens Discord's own login page in a system webview, so the password,
//! captcha, and passkey stay in that page. Email and password against the
//! login endpoints are the fallback when the webview cannot start. Tokens are
//! stored in the OS keychain. Nothing in this crate prints a token, a
//! password, or a ticket.

mod crypto;
mod login_api;
mod qr;
mod redact;
mod remote;
mod session;
mod vault;
mod webview;

pub use crypto::RemoteKey;
pub use login_api::fetch_avatar;
pub use login_api::{AuthClient, CaptchaChallenge, LoginOutcome, MfaChallenge, MfaMethod};
pub use qr::{hexwave_png, render_qr_image, render_qr_png, QrImage};
pub use redact::redact;
pub use remote::{
    complete_remote_login, parse_user_payload, RemoteMachine, RemoteProgress, Step, UserPreview,
    AUTH_API_ROOT, REMOTE_AUTH_GATEWAY,
};
pub use session::{EndReason, SessionMachine, SessionPhase};
pub use vault::{AccountRecord, AccountStore};
pub use webview::{
    discord_login_window_main, open_discord_login_window, read_login_frame, webview_backend,
    write_login_frame, LoginFrame, LOGIN_PROBE_SCRIPT,
};

use std::fmt;

/// Failures a person can be shown. The display text never includes a token,
/// password, ticket, or ciphertext.
#[derive(Debug)]
pub enum AuthError {
    Cancelled,
    Declined,
    Expired,
    Handshake,
    Disconnected,
    Unauthorized,
    Suspended,
    WebviewUnavailable(String),
    Keychain(String),
    /// Discord challenged the ticket exchange. The webview solves it.
    Captcha(crate::login_api::CaptchaChallenge),
    Message(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.public_message())
    }
}

impl std::error::Error for AuthError {}

impl AuthError {
    pub fn public_message(&self) -> String {
        match self {
            Self::Cancelled => "Login cancelled.".into(),
            Self::Declined => "The login was cancelled on your phone.".into(),
            Self::Expired => "This login code expired.".into(),
            Self::Handshake => "Discord rejected the login handshake.".into(),
            Self::Disconnected => "The connection to Discord dropped.".into(),
            Self::Unauthorized => "This session is no longer valid.".into(),
            Self::Suspended => "This account is suspended.".into(),
            Self::WebviewUnavailable(reason) => {
                format!("Discord's login page could not be opened. {reason}")
            }
            Self::Keychain(reason) => format!(
                "The OS keychain is unavailable ({reason}). Rencord will not write a token to a file."
            ),
            Self::Captcha(_) => {
                "Discord asked for a captcha. Continue on Discord's own page. Rencord does not solve captchas.".into()
            }
            Self::Message(text) => text.clone(),
        }
    }

    /// Short line under the login card. A server error is not "the code stopped".
    pub fn caption(&self) -> &'static str {
        match self {
            Self::Expired => "The code expired",
            Self::Declined | Self::Cancelled => "The login was cancelled",
            Self::Handshake | Self::Disconnected => "The code stopped",
            Self::Captcha(_) => "Discord asked for a captcha",
            Self::Unauthorized
            | Self::Suspended
            | Self::WebviewUnavailable(_)
            | Self::Keychain(_)
            | Self::Message(_) => "Discord could not finish signing in",
        }
    }
}
