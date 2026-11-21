//! Child webview that shows only the hCaptcha widget.
//!
//! Windows maps `https://discord.com/rencord-captcha.html` onto a local file so
//! the widget sees a discord.com document. The person solves it. If WebView2
//! is missing, or the host mapping fails, the caller opens Discord's page.

use raw_window_handle::HasWindowHandle;

use crate::captcha_page::WidgetEvent;
use crate::login_api::CaptchaChallenge;

#[cfg(all(windows, feature = "webview"))]
pub const CAPTCHA_DOCUMENT: &str = "https://discord.com/rencord-captcha.html";

pub fn embedded_captcha_available() -> bool {
    #[cfg(all(windows, feature = "webview"))]
    {
        wry::webview_version().is_ok()
    }
    #[cfg(not(all(windows, feature = "webview")))]
    {
        false
    }
}

pub fn present_embedded_captcha<W: HasWindowHandle>(
    parent: &W,
    origin_x: f32,
    origin_y: f32,
    width: f32,
    height: f32,
    challenge: &CaptchaChallenge,
    on_event: impl Fn(WidgetEvent) + Send + 'static,
) -> Result<(), String> {
    #[cfg(all(windows, feature = "webview"))]
    {
        present_windows(
            parent, origin_x, origin_y, width, height, challenge, on_event,
        )
    }
    #[cfg(not(all(windows, feature = "webview")))]
    {
        let _ = (
            parent, origin_x, origin_y, width, height, challenge, on_event,
        );
        Err("This build cannot embed hCaptcha in the window. Continue on Discord's page.".into())
    }
}

pub fn reload_embedded_captcha() {
    #[cfg(all(windows, feature = "webview"))]
    HOST.with(|slot| {
        if let Some(panel) = slot.borrow().as_ref() {
            let _ = panel.webview.load_url(CAPTCHA_DOCUMENT);
        }
    });
}

pub fn dismiss_embedded_captcha() {
    #[cfg(all(windows, feature = "webview"))]
    HOST.with(|slot| {
        if let Some(panel) = slot.borrow_mut().take() {
            let _ = std::fs::remove_dir_all(panel.dir);
        }
    });
}

#[cfg(all(windows, feature = "webview"))]
struct Panel {
    webview: wry::WebView,
    dir: std::path::PathBuf,
}

#[cfg(all(windows, feature = "webview"))]
thread_local! {
    static HOST: std::cell::RefCell<Option<Panel>> = const { std::cell::RefCell::new(None) };
}

#[cfg(all(windows, feature = "webview"))]
fn present_windows<W: HasWindowHandle>(
    parent: &W,
    origin_x: f32,
    origin_y: f32,
    width: f32,
    height: f32,
    challenge: &CaptchaChallenge,
    on_event: impl Fn(WidgetEvent) + Send + 'static,
) -> Result<(), String> {
    use crate::captcha_page::{parse_widget_message, widget_html};

    if wry::webview_version().is_err() {
        return Err("WebView2 is not installed. Continue on Discord's page.".into());
    }
    dismiss_embedded_captcha();
    let dir = std::env::temp_dir().join(format!("rencord-captcha-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    std::fs::write(dir.join("rencord-captcha.html"), widget_html(challenge))
        .map_err(|err| err.to_string())?;

    let bounds = wry::Rect {
        position: wry::dpi::LogicalPosition::new(origin_x as f64, origin_y as f64).into(),
        size: wry::dpi::LogicalSize::new(width as f64, height as f64).into(),
    };
    let webview = wry::WebViewBuilder::new()
        .with_bounds(bounds)
        .with_ipc_handler(move |request| {
            if let Some(event) = parse_widget_message(request.body()) {
                on_event(event);
            }
        })
        .build_as_child(parent)
        .map_err(|err| err.to_string())?;
    map_discord_host(&webview, &dir)?;
    webview
        .load_url(CAPTCHA_DOCUMENT)
        .map_err(|err| err.to_string())?;
    HOST.with(|slot| {
        *slot.borrow_mut() = Some(Panel { webview, dir });
    });
    Ok(())
}

#[cfg(all(windows, feature = "webview"))]
fn map_discord_host(webview: &wry::WebView, dir: &std::path::Path) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_3, COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY,
    };
    use windows::core::{Interface, HSTRING};
    use wry::WebViewExtWindows;

    let core = webview.webview();
    let host = core
        .cast::<ICoreWebView2_3>()
        .map_err(|err| format!("WebView2 cannot map a document host ({err})."))?;
    let folder = HSTRING::from(dir.to_string_lossy().as_ref());
    unsafe {
        host.SetVirtualHostNameToFolderMapping(
            &HSTRING::from("discord.com"),
            &folder,
            COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY,
        )
        .map_err(|err| format!("WebView2 rejected the discord.com document ({err})."))?;
    }
    Ok(())
}
