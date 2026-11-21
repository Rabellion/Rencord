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
            let Panel {
                webview,
                context,
                dir,
                profile,
            } = panel;
            // Close the browser before deleting the folders it was using.
            drop(webview);
            drop(context);
            let _ = std::fs::remove_dir_all(dir);
            remove_when_released(profile);
        }
    });
}

/// The browser keeps its profile files open for a moment after the panel closes,
/// so retry for a few seconds off the UI thread.
// shortcut: best effort. A crash or a very slow exit leaves the folder in the
// temp directory, which is the OS's to clean; upgrade if that ever adds up.
#[cfg(all(windows, feature = "webview"))]
fn remove_when_released(dir: std::path::PathBuf) {
    std::thread::spawn(move || {
        for _ in 0..20 {
            if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

#[cfg(all(windows, feature = "webview"))]
struct Panel {
    webview: wry::WebView,
    // Owns the WebView2 profile folder choice for as long as the panel lives.
    context: wry::WebContext,
    /// The widget document served under the discord.com mapping.
    dir: std::path::PathBuf,
    /// WebView2's profile. Kept apart from `dir` so the page cannot reach it.
    profile: std::path::PathBuf,
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
    if wry::webview_version().is_err() {
        return Err("WebView2 is not installed. Continue on Discord's page.".into());
    }
    dismiss_embedded_captcha();
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("rencord-captcha-{pid}"));
    // Without this WebView2 puts its profile next to Rencord.exe, which fails with
    // "Access is denied" when that folder is not writable (an all-users install).
    let profile = std::env::temp_dir().join(format!("rencord-captcha-profile-{pid}"));
    let built = build_panel(
        parent, origin_x, origin_y, width, height, challenge, &dir, &profile, on_event,
    );
    match built {
        Ok(panel) => {
            HOST.with(|slot| *slot.borrow_mut() = Some(panel));
            Ok(())
        }
        Err(err) => {
            let _ = std::fs::remove_dir_all(&dir);
            remove_when_released(profile);
            Err(err)
        }
    }
}

#[cfg(all(windows, feature = "webview"))]
#[allow(clippy::too_many_arguments)]
fn build_panel<W: HasWindowHandle>(
    parent: &W,
    origin_x: f32,
    origin_y: f32,
    width: f32,
    height: f32,
    challenge: &CaptchaChallenge,
    dir: &std::path::Path,
    profile: &std::path::Path,
    on_event: impl Fn(WidgetEvent) + Send + 'static,
) -> Result<Panel, String> {
    use crate::captcha_page::{parse_widget_message, widget_html};

    std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    std::fs::create_dir_all(profile).map_err(|err| err.to_string())?;
    std::fs::write(dir.join("rencord-captcha.html"), widget_html(challenge))
        .map_err(|err| err.to_string())?;

    let bounds = wry::Rect {
        position: wry::dpi::LogicalPosition::new(origin_x as f64, origin_y as f64).into(),
        size: wry::dpi::LogicalSize::new(width as f64, height as f64).into(),
    };
    let mut context = wry::WebContext::new(Some(profile.to_path_buf()));
    let webview = wry::WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(bounds)
        .with_ipc_handler(move |request| {
            if let Some(event) = parse_widget_message(request.body()) {
                on_event(event);
            }
        })
        .build_as_child(parent)
        .map_err(|err| err.to_string())?;
    map_discord_host(&webview, dir)?;
    webview
        .load_url(CAPTCHA_DOCUMENT)
        .map_err(|err| err.to_string())?;
    Ok(Panel {
        webview,
        context,
        dir: dir.to_path_buf(),
        profile: profile.to_path_buf(),
    })
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
