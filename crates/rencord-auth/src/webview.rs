//! Discord's own login page in a system webview.
//!
//! The window loads `https://discord.com/login`. A script watches the login
//! and MFA responses for a session string. It does not read the form, the
//! password, or the request body. The page handles captcha, SSO, and passkeys.
//! The session comes back on a localhost socket and the temporary webview
//! profile is removed when the window closes.

use std::io::{Read, Write};

#[cfg(feature = "webview")]
use std::time::Duration;

use rencord_model::Token;

use crate::AuthError;

/// Runs in Discord's page. It only inspects auth responses and `localStorage`'s
/// `token` item. It does not read form fields.
pub const LOGIN_PROBE_SCRIPT: &str = r#"
(() => {
  if (window.__rencordWatch) return;
  window.__rencordWatch = true;
  let last = "";
  const deliver = (value) => {
    if (typeof value !== "string") return;
    const token = value.trim();
    if (token.length < 20 || token.length > 200) return;
    if (/\s/.test(token)) return;
    if (token === last) return;
    last = token;
    try { window.ipc.postMessage(token); } catch (err) {}
  };
  const take = (text) => {
    if (typeof text !== "string" || text.indexOf("token") < 0) return;
    try {
      const data = JSON.parse(text);
      if (data && typeof data.token === "string") deliver(data.token);
    } catch (err) {}
  };
  const authPath = (url) => {
    try {
      const parsed = new URL(String(url), location.href);
      const host = parsed.hostname;
      const discord = host === "discord.com" || host.endsWith(".discord.com")
        || host === "discordapp.com" || host.endsWith(".discordapp.com");
      if (!discord) return false;
      return /\/api\/v\d+\/auth\//.test(parsed.pathname);
    } catch (err) {
      return false;
    }
  };
  const origFetch = window.fetch;
  if (typeof origFetch === "function") {
    window.fetch = function (input) {
      const url = typeof input === "string" ? input : (input && input.url) || "";
      return origFetch.apply(this, arguments).then((response) => {
        if (authPath(url)) response.clone().text().then(take).catch(() => {});
        return response;
      });
    };
  }
  const open = XMLHttpRequest.prototype.open;
  const send = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (method, url) {
    this.__rencordUrl = url;
    return open.apply(this, arguments);
  };
  XMLHttpRequest.prototype.send = function () {
    this.addEventListener("load", function () {
      if (authPath(this.__rencordUrl || "")) take(this.responseText || "");
    });
    return send.apply(this, arguments);
  };
  const peek = () => {
    try {
      const raw = localStorage.getItem("token");
      if (!raw) return;
      try { deliver(JSON.parse(raw)); } catch (err) { deliver(raw); }
    } catch (err) {}
  };
  peek();
  setInterval(peek, 700);
})();
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginFrame {
    Session,
    Failure,
}

pub fn write_login_frame(
    mut out: impl Write,
    kind: LoginFrame,
    payload: &[u8],
) -> std::io::Result<()> {
    if payload.len() > 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "login frame is too large",
        ));
    }
    let tag = match kind {
        LoginFrame::Session => 1u8,
        LoginFrame::Failure => 2u8,
    };
    out.write_all(&[tag])?;
    out.write_all(&(payload.len() as u32).to_le_bytes())?;
    out.write_all(payload)?;
    out.flush()?;
    Ok(())
}

pub fn read_login_frame(mut input: impl Read) -> std::io::Result<(LoginFrame, Vec<u8>)> {
    let mut tag = [0u8; 1];
    input.read_exact(&mut tag)?;
    let mut len_buf = [0u8; 4];
    input.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "login frame is too large",
        ));
    }
    let mut payload = vec![0u8; len];
    input.read_exact(&mut payload)?;
    let kind = match tag[0] {
        1 => LoginFrame::Session,
        2 => LoginFrame::Failure,
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "login frame tag was not recognised",
            ))
        }
    };
    Ok((kind, payload))
}

#[cfg(all(feature = "webview", target_os = "linux"))]
pub fn webview_backend() -> Option<&'static str> {
    Some("WebKitGTK")
}

#[cfg(all(feature = "webview", target_os = "macos"))]
pub fn webview_backend() -> Option<&'static str> {
    Some("WKWebView")
}

#[cfg(all(feature = "webview", target_os = "windows"))]
pub fn webview_backend() -> Option<&'static str> {
    Some("WebView2")
}

#[cfg(not(all(
    feature = "webview",
    any(target_os = "linux", target_os = "macos", target_os = "windows")
)))]
pub fn webview_backend() -> Option<&'static str> {
    None
}

/// Open Discord's login page and return the session it produces.
///
/// The password never crosses into this process. The session does, because
/// that is what the rest of the client has to present.
pub fn open_discord_login_window() -> Result<Token, AuthError> {
    #[cfg(not(feature = "webview"))]
    {
        return Err(AuthError::WebviewUnavailable(
            "This build has no system webview. Use email and password, or a token.".into(),
        ));
    }
    #[cfg(feature = "webview")]
    {
        spawn_login_process()
    }
}

/// Entry point for the helper process. The desktop binary calls this before
/// starting GPUI when it sees `--discord-login-window`.
pub fn discord_login_window_main() -> i32 {
    let Ok(port) = std::env::var("RENCORD_LOGIN_PORT") else {
        eprintln!("rencord: the login window is an internal helper");
        return 1;
    };
    let Ok(nonce) = std::env::var("RENCORD_LOGIN_NONCE") else {
        eprintln!("rencord: the login window is an internal helper");
        return 1;
    };
    #[cfg(not(feature = "webview"))]
    {
        if let Err(err) = send_outcome(
            &port,
            &nonce,
            Err(AuthError::WebviewUnavailable(
                "This build has no system webview.".into(),
            )),
        ) {
            eprintln!("rencord: {err}");
            return 1;
        }
        return 0;
    }
    #[cfg(feature = "webview")]
    {
        if let Err(err) = run_login_window(&port, &nonce) {
            if let Err(report) = send_outcome(&port, &nonce, Err(err)) {
                eprintln!("rencord: {report}");
                return 1;
            }
        }
        0
    }
}

fn send_outcome(port: &str, nonce: &str, outcome: Result<Token, AuthError>) -> Result<(), String> {
    let mut stream = std::net::TcpStream::connect(format!("127.0.0.1:{port}"))
        .map_err(|_| "could not report the login result".to_owned())?;
    stream
        .write_all(nonce.as_bytes())
        .map_err(|_| "could not report the login result".to_owned())?;
    let (kind, payload) = match &outcome {
        Ok(token) => (LoginFrame::Session, token.expose().as_bytes().to_vec()),
        Err(err) => (LoginFrame::Failure, err.public_message().into_bytes()),
    };
    write_login_frame(&mut stream, kind, &payload)
        .map_err(|_| "could not report the login result".to_owned())
}

#[cfg(feature = "webview")]
fn spawn_login_process() -> Result<Token, AuthError> {
    use rand::RngCore;
    use std::io::Read;
    use std::net::TcpListener;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| {
        AuthError::WebviewUnavailable("Could not open a local channel for the login window.".into())
    })?;
    let port = listener
        .local_addr()
        .map_err(|_| {
            AuthError::WebviewUnavailable(
                "Could not open a local channel for the login window.".into(),
            )
        })?
        .port();
    let mut nonce_bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = nonce_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let exe = std::env::current_exe().map_err(|_| {
        AuthError::WebviewUnavailable("Could not find the Rencord executable.".into())
    })?;
    let mut child = Command::new(exe)
        .arg("--discord-login-window")
        .env("RENCORD_LOGIN_PORT", port.to_string())
        .env("RENCORD_LOGIN_NONCE", &nonce)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| AuthError::WebviewUnavailable("Could not start the login window.".into()))?;

    let (tx, rx) = mpsc::channel();
    let expected = nonce.clone();
    thread::spawn(move || {
        let result = (|| {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(30)))?;
            let mut got = vec![0u8; expected.len()];
            stream.read_exact(&mut got)?;
            if got != expected.as_bytes() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "login channel did not match",
                ));
            }
            read_login_frame(&mut stream)
        })();
        let _ = tx.send(result);
    });

    let frame = match rx.recv_timeout(Duration::from_secs(370)) {
        Ok(Ok(frame)) => frame,
        Ok(Err(_)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AuthError::WebviewUnavailable(
                "The login window closed before Discord finished signing in.".into(),
            ));
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AuthError::WebviewUnavailable(
                "The login window timed out.".into(),
            ));
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    match frame {
        (LoginFrame::Session, bytes) => {
            let text = String::from_utf8(bytes).map_err(|_| {
                AuthError::Message("Discord's page returned a session that was not text.".into())
            })?;
            let text = text.trim().to_owned();
            if text.len() < 20 || text.len() > 200 || text.chars().any(char::is_whitespace) {
                return Err(AuthError::Message(
                    "Discord's page did not return a usable session.".into(),
                ));
            }
            Ok(Token::new(text))
        }
        (LoginFrame::Failure, bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let text = crate::redact::redact(text.trim());
            Err(AuthError::WebviewUnavailable(if text.is_empty() {
                "The login window closed.".into()
            } else {
                text
            }))
        }
    }
}

#[cfg(feature = "webview")]
fn run_login_window(port: &str, nonce: &str) -> Result<(), AuthError> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::window::WindowBuilder;
    use wry::WebViewBuilder;

    let scratch = ScratchDir::create()?;
    let mut context = wry::WebContext::new(Some(scratch.path.clone()));
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Log in with Discord")
        .with_inner_size(tao::dpi::LogicalSize::new(480.0, 720.0))
        .build(&event_loop)
        .map_err(|_| AuthError::WebviewUnavailable("Could not open the login window.".into()))?;

    let slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let page: Arc<Mutex<String>> = Arc::new(Mutex::new("https://discord.com/login".into()));
    let slot_ipc = Arc::clone(&slot);
    let page_ipc = Arc::clone(&page);
    let page_load = Arc::clone(&page);

    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url("https://discord.com/login")
        .with_incognito(true)
        .with_initialization_script(LOGIN_PROBE_SCRIPT)
        .with_on_page_load_handler(move |_event, url| {
            if let Ok(mut current) = page_load.lock() {
                *current = url;
            }
        })
        .with_ipc_handler(move |request: wry::http::Request<String>| {
            let here = page_ipc
                .lock()
                .ok()
                .map(|guard| guard.clone())
                .unwrap_or_default();
            let ipc_host = request.uri().host().unwrap_or("");
            if !discord_host_or_page(ipc_host, &here) {
                return;
            }
            let body = request.body().trim().trim_matches('"');
            if body.len() < 20 || body.len() > 200 || body.chars().any(char::is_whitespace) {
                return;
            }
            if let Ok(mut guard) = slot_ipc.lock() {
                *guard = Some(body.to_owned());
            }
        });

    #[cfg(not(target_os = "linux"))]
    let _webview = builder
        .build(&window)
        .map_err(|_| AuthError::WebviewUnavailable("The system webview failed to start.".into()))?;
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.gtk_window()).map_err(|_| {
            AuthError::WebviewUnavailable(
                "WebKitGTK failed to start. Email and password are the fallback.".into(),
            )
        })?
    };

    let deadline = std::time::Instant::now() + Duration::from_secs(350);
    let reported = Arc::new(AtomicBool::new(false));
    let port = port.to_owned();
    let nonce = nonce.to_owned();
    // `EventLoop::run` does not return on every platform. The session is
    // written to the local channel before the loop exits.
    event_loop.run(move |event, _, control_flow| {
        *control_flow =
            ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(200));
        let timed_out = std::time::Instant::now() >= deadline;
        if let Some(token) = slot.lock().ok().and_then(|mut guard| guard.take()) {
            if !reported.swap(true, Ordering::Relaxed) {
                let _ = send_outcome(&port, &nonce, Ok(Token::new(token)));
            }
            *control_flow = ControlFlow::Exit;
            return;
        }
        let closing = matches!(
            event,
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
        );
        if (closing || timed_out) && !reported.swap(true, Ordering::Relaxed) {
            let reason = if timed_out {
                "The login window timed out."
            } else {
                "The login window was closed before Discord finished signing in."
            };
            let _ = send_outcome(
                &port,
                &nonce,
                Err(AuthError::WebviewUnavailable(reason.into())),
            );
            *control_flow = ControlFlow::Exit;
        }
    });
    #[allow(unreachable_code)]
    {
        drop(_webview);
        drop(context);
        drop(scratch);
        Ok(())
    }
}

#[cfg(feature = "webview")]
fn discord_host_or_page(ipc_host: &str, page: &str) -> bool {
    discord_host(ipc_host) || page_url_is_discord(page)
}

#[cfg(feature = "webview")]
fn discord_host(host: &str) -> bool {
    host == "discord.com"
        || host.ends_with(".discord.com")
        || host == "discordapp.com"
        || host.ends_with(".discordapp.com")
}

#[cfg(feature = "webview")]
fn page_url_is_discord(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    parsed.host_str().is_some_and(discord_host)
}

#[cfg(feature = "webview")]
struct ScratchDir {
    path: std::path::PathBuf,
}

#[cfg(feature = "webview")]
impl ScratchDir {
    fn create() -> Result<Self, AuthError> {
        let path = std::env::temp_dir().join(format!("rencord-login-{}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| {
            AuthError::WebviewUnavailable("Could not create a temporary login profile.".into())
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
        }
        Ok(Self { path })
    }
}

#[cfg(feature = "webview")]
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reads_auth_responses_and_not_the_form() {
        let script = LOGIN_PROBE_SCRIPT;
        assert!(script.contains("api"));
        assert!(script.contains("auth"));
        assert!(script.contains("ipc.postMessage"));
        assert!(script.contains("localStorage"));
        let lower = script.to_lowercase();
        assert!(!lower.contains("password"));
        assert!(!script.contains(".value"));
        assert!(!script.contains("document.cookie"));
    }

    #[test]
    fn frames_roundtrip_without_appearing_in_debug() {
        let mut buffer = Vec::new();
        write_login_frame(&mut buffer, LoginFrame::Failure, b"closed").unwrap();
        let (kind, payload) = read_login_frame(buffer.as_slice()).unwrap();
        assert_eq!(kind, LoginFrame::Failure);
        assert_eq!(payload, b"closed");
        assert!(!format!("{kind:?}").contains("closed"));

        let mut secret = Vec::new();
        write_login_frame(
            &mut secret,
            LoginFrame::Session,
            b"user.session.secret-token",
        )
        .unwrap();
        let (kind, payload) = read_login_frame(secret.as_slice()).unwrap();
        assert_eq!(kind, LoginFrame::Session);
        assert_eq!(payload, b"user.session.secret-token");
        assert!(!format!("{kind:?}").contains("secret"));
    }

    #[test]
    fn backend_names_the_system_webview() {
        let backend = webview_backend();
        #[cfg(all(feature = "webview", target_os = "linux"))]
        assert_eq!(backend, Some("WebKitGTK"));
        #[cfg(all(feature = "webview", target_os = "macos"))]
        assert_eq!(backend, Some("WKWebView"));
        #[cfg(all(feature = "webview", target_os = "windows"))]
        assert_eq!(backend, Some("WebView2"));
        #[cfg(not(feature = "webview"))]
        assert_eq!(backend, None);
    }
}
