//! The hCaptcha document served at `https://discord.com/rencord-captcha`.
//!
//! The page is only the widget. The person solves it. Nothing here requests a
//! token from a solving service or changes the challenge.

use serde_json::{json, Value};

use crate::login_api::CaptchaChallenge;

#[cfg_attr(not(test), allow(dead_code))]
pub const CAPTCHA_PAGE: &str = "https://discord.com/rencord-captcha";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WidgetEvent {
    Solved(String),
    Expired,
    Error,
    Closed,
}

/// True when the webview asked for the local widget document.
#[cfg_attr(not(test), allow(dead_code))]
pub fn is_captcha_page(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.host_str() == Some("discord.com")
        && (parsed.path() == "/rencord-captcha" || parsed.path() == "/rencord-captcha.html")
}

/// Dark hCaptcha widget. `sitekey` and `rqdata` are passed through as JSON strings.
#[cfg_attr(not(any(test, all(windows, feature = "webview"))), allow(dead_code))]
pub fn widget_html(challenge: &CaptchaChallenge) -> String {
    let sitekey = serde_json::to_string(&challenge.sitekey).unwrap_or_else(|_| "\"\"".into());
    let rqdata = challenge
        .rqdata
        .as_deref()
        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into()))
        .unwrap_or_else(|| "null".into());
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Confirm you're human</title>
<script src="https://js.hcaptcha.com/1/api.js" async defer></script>
<style>
  html, body {{ margin: 0; height: 100%; background: #0A0A14; color: #EDEEF7;
    font-family: Inter, "Segoe UI", sans-serif; }}
  .wrap {{ min-height: 100%; display: flex; align-items: center; justify-content: center; }}
</style>
</head>
<body>
<div class="wrap"><div id="slot"></div></div>
<script>
  const sitekey = {sitekey};
  const rqdata = {rqdata};
  function post(payload) {{
    try {{ window.ipc.postMessage(JSON.stringify(payload)); }} catch (err) {{}}
  }}
  function mount() {{
    if (!window.hcaptcha) {{ setTimeout(mount, 40); return; }}
    const options = {{
      sitekey: sitekey,
      theme: "dark",
      callback: function (token) {{ post({{ op: "solved", token: token }}); }},
      "expired-callback": function () {{ post({{ op: "expired" }}); }},
      "error-callback": function () {{ post({{ op: "error" }}); }}
    }};
    if (rqdata) options.rqdata = rqdata;
    window.hcaptcha.render("slot", options);
  }}
  mount();
</script>
</body>
</html>
"#
    )
}

#[cfg_attr(not(any(test, all(windows, feature = "webview"))), allow(dead_code))]
pub fn parse_widget_message(message: &str) -> Option<WidgetEvent> {
    let value: Value = serde_json::from_str(message).ok()?;
    match value.get("op").and_then(|item| item.as_str())? {
        "solved" => {
            let token = value.get("token").and_then(|item| item.as_str())?;
            if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_whitespace) {
                return None;
            }
            Some(WidgetEvent::Solved(token.to_owned()))
        }
        "expired" => Some(WidgetEvent::Expired),
        "error" => Some(WidgetEvent::Error),
        "closed" => Some(WidgetEvent::Closed),
        _ => None,
    }
}

/// JSON the widget script would post. Tests use this shape.
#[cfg_attr(not(test), allow(dead_code))]
pub fn widget_message(op: &str, token: Option<&str>) -> String {
    match token {
        Some(token) => json!({ "op": op, "token": token }).to_string(),
        None => json!({ "op": op }).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn challenge() -> CaptchaChallenge {
        CaptchaChallenge {
            sitekey: "site-key".into(),
            service: "hcaptcha".into(),
            session_id: Some("sess-1".into()),
            rqdata: Some("rq-data".into()),
            rqtoken: Some("rq-token".into()),
        }
    }

    #[test]
    fn the_page_is_only_the_dark_widget() {
        let html = widget_html(&challenge());
        assert!(html.contains("https://js.hcaptcha.com/1/api.js"));
        assert!(html.contains("\"site-key\""));
        assert!(html.contains("\"rq-data\""));
        assert!(html.contains("theme: \"dark\""));
        assert!(html.contains("rqdata"));
        assert!(!html.to_lowercase().contains("anticaptcha"));
        assert!(!html.to_lowercase().contains("solver"));
        assert!(is_captcha_page(&format!("{CAPTCHA_PAGE}?x=1")));
        assert!(is_captcha_page("https://discord.com/rencord-captcha.html"));
        assert!(!is_captcha_page("https://discord.com/login"));
        assert!(!is_captcha_page("http://discord.com/rencord-captcha"));
        assert!(!is_captcha_page("https://evil.example/rencord-captcha"));
    }

    #[test]
    fn widget_messages_parse_without_keeping_a_blank_token() {
        let solved = parse_widget_message(&widget_message("solved", Some("P1_token"))).unwrap();
        assert_eq!(solved, WidgetEvent::Solved("P1_token".into()));
        assert_eq!(
            parse_widget_message(&widget_message("expired", None)),
            Some(WidgetEvent::Expired)
        );
        assert_eq!(
            parse_widget_message(&widget_message("error", None)),
            Some(WidgetEvent::Error)
        );
        assert!(parse_widget_message(&widget_message("solved", Some(""))).is_none());
        assert!(parse_widget_message("not json").is_none());
    }
}
