//! GPUI shell. `rencord --demo` uses local data. `--theme amoled|dark|light`
//! picks the starting theme. `--window 1440x900` and `--scale 1.5` size the
//! window for a layout check. `--bench` prints startup, RSS, and frame gaps,
//! then quits.

mod big_file_view;
mod settings_ui;

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use rencord_desktop::layout::{
    self, apply_drag, ColumnMetrics, DragContext, LayoutInput, LayoutStore, ReadingAnchor,
    ResolvedLayout, RightChoice, RightPanel, SavedColumns, SplitEdge,
};

use gpui_kit::assets::Assets;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rencord_desktop::{Palette, Shell, ThemeMode, MOTION_OVERLAY_MS};

mod golive;
mod hdr_ui;
mod stub;
use rencord_markdown::{parse, Block, Inline};
use rencord_model::{Message, Snowflake};

gpui_kit::actions!(
    rencord_ui,
    [
        QuickSwitcher,
        CloseLayer,
        NextChannel,
        PrevChannel,
        NextGuild,
        PrevGuild,
        OpenSettings,
        OpenInbox,
        ToggleSidebar,
        ToggleMembers,
        ToggleSplit,
        TogglePip,
        ShowHelp,
        SendMessage,
    ]
);

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

enum BigFileEvent {
    Progress(rencord_upload::ProgressSnap),
    Done(Result<rencord_upload::JobOutput, rencord_upload::UploadError>),
}

fn big_file_outcome(output: &rencord_upload::JobOutput) -> String {
    match output {
        rencord_upload::JobOutput::File { path, detail } => {
            format!("Compressed file is ready at {}. {detail}", path.display())
        }
        rencord_upload::JobOutput::Link {
            url,
            provider,
            expires,
            ..
        } => {
            let expires = expires.as_deref().unwrap_or("unspecified");
            format!("{url} ({provider}, {expires})")
        }
        rencord_upload::JobOutput::Split { note, .. } => note.clone(),
    }
}

mod login_view;
mod net;
mod voice_call;

use login_view::{LoginModel, LoginPreview};

fn main() {
    PROCESS_START.get_or_init(Instant::now);
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--discord-login-window") {
        std::process::exit(rencord_auth::discord_login_window_main());
    }
    if args.iter().any(|arg| arg == "--worker-check") {
        println!("WORKER_CHECK {}", env!("CARGO_PKG_VERSION"));
        println!("STREAM_WORKER {}", rencord_media::STREAM_WORKER_ARG);
        return;
    }
    if args
        .iter()
        .any(|arg| arg == rencord_media::STREAM_WORKER_ARG)
    {
        std::process::exit(rencord_media::worker_main());
    }
    let demo = args.iter().any(|arg| arg == "--demo");
    let stub_scene = args
        .windows(2)
        .find(|pair| pair[0] == "--stub")
        .and_then(|pair| match pair[1].as_str() {
            "guilds" => Some(stub::StubScene::Guilds),
            "scroll" => Some(stub::StubScene::Scroll),
            "retry" => Some(stub::StubScene::Retry),
            "layout" => Some(stub::StubScene::Layout),
            _ => None,
        });
    if let Some(scene) = stub_scene {
        let (api_base, gateway) = net::runtime().block_on(stub::serve(scene));
        net::set_endpoints(api_base, gateway);
    }
    if args.iter().any(|arg| arg == "--home") {
        stub::request_home();
    }
    let bench = args.iter().any(|arg| arg == "--bench");
    let theme = args
        .windows(2)
        .find(|pair| pair[0] == "--theme")
        .and_then(|pair| ThemeMode::parse(&pair[1]));
    let layout = args
        .windows(2)
        .find(|pair| pair[0] == "--layout")
        .and_then(|pair| rencord_desktop::LayoutKind::parse(&pair[1]));
    let density = args
        .windows(2)
        .find(|pair| pair[0] == "--density")
        .and_then(|pair| match pair[1].as_str() {
            "compact" => Some(rencord_desktop::Density::Compact),
            "cozy" => Some(rencord_desktop::Density::Cozy),
            "spacious" => Some(rencord_desktop::Density::Spacious),
            _ => None,
        });
    if args
        .windows(2)
        .any(|pair| pair[0] == "--bench" && pair[1] == "stream")
    {
        println!(
            "{}",
            rencord_media::format_bench(&rencord_media::bench_software(30))
        );
        println!(
            "{}",
            rencord_media::format_preset_matrix(&rencord_media::bench_preset_matrix())
        );
        for probe in rencord_media::probe_all() {
            println!(
                "ENCODER {:?} usable={} {}",
                probe.kind, probe.usable, probe.detail
            );
        }
        return;
    }
    let start_voice = args.iter().any(|arg| arg == "--voice");
    let golive = args
        .windows(2)
        .find(|pair| pair[0] == "--golive")
        .map(|pair| golive::GoLiveMode::parse(&pair[1]))
        .unwrap_or(golive::GoLiveMode::Off);
    let big_file = args
        .windows(2)
        .find(|pair| pair[0] == "--big-file")
        .map(|pair| match pair[1].as_str() {
            "progress" => rencord_upload::Flow::preview_progress(),
            "upload" => rencord_upload::Flow::preview_upload(),
            "error" => rencord_upload::Flow::preview_error(),
            _ => rencord_upload::Flow::preview_sheet(),
        });
    let reveal_hover = args.iter().any(|arg| arg == "--reveal-hover");
    let settings_view = args
        .windows(2)
        .find(|pair| pair[0] == "--settings")
        .map(|pair| pair[1].clone());
    let theme_file = args
        .windows(2)
        .find(|pair| pair[0] == "--theme-file")
        .map(|pair| std::path::PathBuf::from(&pair[1]));
    let preview = args
        .windows(2)
        .find(|pair| pair[0] == "--login-preview")
        .and_then(|pair| match pair[1].as_str() {
            "qr" => Some(LoginPreview::Qr),
            "confirm" => Some(LoginPreview::Confirm),
            "error" => Some(LoginPreview::Error),
            "captcha" => Some(LoginPreview::Captcha),
            _ => None,
        });
    let frame = args
        .windows(2)
        .find(|pair| pair[0] == "--window" || pair[0] == "--size")
        .and_then(|pair| parse_frame(&pair[1]));
    let scale = args
        .windows(2)
        .find(|pair| pair[0] == "--scale")
        .and_then(|pair| parse_scale(&pair[1]));
    let screenshot = args
        .windows(2)
        .find(|pair| pair[0] == "--screenshot")
        .map(|pair| pair[1].clone());
    let panel = args
        .windows(2)
        .find(|pair| pair[0] == "--panel")
        .and_then(|pair| parse_panel(&pair[1]));
    let open_drawer = args
        .windows(2)
        .any(|pair| pair[0] == "--open" && pair[1] == "drawer");
    let open_members = args
        .windows(2)
        .any(|pair| pair[0] == "--open" && pair[1] == "members");
    let open_settings = settings_view.is_some()
        || args.iter().any(|arg| arg == "--open-settings")
        || args
            .windows(2)
            .any(|pair| pair[0] == "--open" && pair[1] == "settings");
    if let Some(scale) = scale {
        // Read by GPUI before the X11 connection opens.
        std::env::set_var("GPUI_X11_SCALE_FACTOR", format!("{scale}"));
    }
    let app = gpui_kit::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        let _ = cx.text_system().add_fonts(vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-Regular.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-Medium.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-SemiBold.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Regular.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Medium.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf")),
        ]);
        cx.bind_keys([
            KeyBinding::new("ctrl-k", QuickSwitcher, None),
            KeyBinding::new("cmd-k", QuickSwitcher, None),
            KeyBinding::new("escape", CloseLayer, None),
            KeyBinding::new("alt-down", NextChannel, None),
            KeyBinding::new("alt-up", PrevChannel, None),
            KeyBinding::new("ctrl-alt-down", NextGuild, None),
            KeyBinding::new("ctrl-alt-up", PrevGuild, None),
            KeyBinding::new("ctrl-,", OpenSettings, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("ctrl-i", OpenInbox, None),
            KeyBinding::new("cmd-i", OpenInbox, None),
            KeyBinding::new("ctrl-b", ToggleSidebar, None),
            KeyBinding::new("cmd-b", ToggleSidebar, None),
            KeyBinding::new("ctrl-m", ToggleMembers, None),
            KeyBinding::new("cmd-m", ToggleMembers, None),
            KeyBinding::new("ctrl-\\", ToggleSplit, None),
            KeyBinding::new("ctrl-/", ShowHelp, None),
            KeyBinding::new("cmd-/", ShowHelp, None),
            KeyBinding::new("enter", SendMessage, None),
        ]);
        let persist_layout = frame.is_none() && screenshot.is_none() && !bench;
        let layout_path = rencord_platform::data_dir().join("layout.json");
        let layout_store = if persist_layout {
            LayoutStore::load(&layout_path)
        } else {
            LayoutStore::default()
        };
        let requested = frame.unwrap_or((1180., 760.));
        let window_bounds = if let Some((width, height)) = frame {
            if screenshot.is_some() {
                // Pin the origin so a grab of this rectangle does not include the desktop.
                Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(width), px(height)),
                )))
            } else {
                Some(WindowBounds::centered(size(px(width), px(height)), cx))
            }
        } else if let Some(saved) = layout_store.window {
            let scale = scale.unwrap_or(1.0);
            let clamped = layout::clamp_window(saved, &monitor_rects(cx), scale);
            Some(WindowBounds::Windowed(Bounds::new(
                point(px(clamped.x), px(clamped.y)),
                size(px(clamped.w), px(clamped.h)),
            )))
        } else {
            Some(WindowBounds::centered(
                size(px(requested.0), px(requested.1)),
                cx,
            ))
        };
        let options = WindowOptions {
            window_bounds,
            window_min_size: Some(size(px(480.), px(360.))),
            // Server decorations keep the client area opaque and edge to edge.
            // A client border would leave a transparent margin around the theme.
            // The Hexwave icon still rides on the window for X11, macOS, and Windows.
            window_background: WindowBackgroundAppearance::Opaque,
            window_decorations: Some(WindowDecorations::Server),
            titlebar: Some(TitlebarOptions {
                title: Some("Rencord".into()),
                ..Default::default()
            }),
            app_id: Some(rencord_platform::APP_ID.to_string()),
            icon: Some(window_icon()),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                RencordWindow::new(
                    demo && preview.is_none() && stub_scene.is_none(),
                    bench,
                    theme,
                    layout,
                    density,
                    start_voice,
                    reveal_hover,
                    golive,
                    screenshot.clone(),
                    panel,
                    open_drawer,
                    open_members,
                    open_settings,
                    persist_layout,
                    layout_store.clone(),
                    layout_path.clone(),
                    big_file,
                    settings_view,
                    theme_file,
                    preview,
                    window,
                    cx,
                )
            })
        })
        .expect("open Rencord window");
    });
}

fn parse_frame(raw: &str) -> Option<(f32, f32)> {
    let (width, height) = raw.split_once(['x', 'X'])?;
    let width = width.parse::<f32>().ok()?;
    let height = height.parse::<f32>().ok()?;
    if width >= 320.0 && height >= 240.0 && width.is_finite() && height.is_finite() {
        Some((width, height))
    } else {
        None
    }
}

fn parse_panel(raw: &str) -> Option<RightPanel> {
    match raw {
        "stream" => Some(RightPanel::Stream),
        "pinned" => Some(RightPanel::Pinned),
        "profile" => Some(RightPanel::Profile),
        "thread" => Some(RightPanel::Thread),
        _ => None,
    }
}

fn parse_scale(raw: &str) -> Option<f32> {
    let raw = raw.trim().trim_end_matches('%');
    let value = raw.parse::<f32>().ok()?;
    let scale = if value > 8.0 { value / 100.0 } else { value };
    if scale.is_finite() && scale > 0.0 {
        Some(scale)
    } else {
        None
    }
}

fn monitor_rects(cx: &App) -> Vec<layout::Rect> {
    cx.displays()
        .iter()
        .map(|display| {
            let bounds = display.visible_bounds();
            layout::Rect {
                x: bounds.origin.x.as_f32(),
                y: bounds.origin.y.as_f32(),
                w: bounds.size.width.as_f32(),
                h: bounds.size.height.as_f32(),
            }
        })
        .collect()
}

fn monitor_key(window: &Window, cx: &App) -> String {
    window
        .display(cx)
        .map(|display| {
            display
                .uuid()
                .map(|id| id.to_string())
                .unwrap_or_else(|_| u64::from(display.id()).to_string())
        })
        .unwrap_or_else(|| "primary".into())
}

struct RencordWindow {
    shell: Shell,
    token_input: Entity<InputState>,
    email_input: Entity<InputState>,
    password_input: Entity<InputState>,
    mfa_input: Entity<InputState>,
    login: LoginModel,
    vault: rencord_auth::AccountStore,
    session_machine: rencord_auth::SessionMachine,
    session_token: Option<rencord_model::Token>,
    /// REST client for the signed-in session; shared so rate limits are tracked.
    api: Option<Arc<rencord_api::DiscordApi>>,
    /// Dropping this shuts the live gateway down.
    gateway: Option<tokio::sync::mpsc::Sender<rencord_gateway::GatewayCommand>>,
    voice_join: Option<rencord_voice::VoiceJoin>,
    voice_cmd: Option<tokio::sync::mpsc::Sender<voice_call::VoiceCmd>>,
    voice_status: Option<std::sync::mpsc::Receiver<String>>,
    /// Guilds and channels whose channels or history were already requested.
    requested: HashSet<Snowflake>,
    /// Fetches that failed. They stay failed until Retry, so render does not try again.
    failed: HashMap<Snowflake, String>,
    loading_older: Option<Snowflake>,
    /// Channels whose history has reached the first message.
    history_start: HashSet<Snowflake>,
    login_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    remote_generation: u64,
    booted: bool,
    preview: Option<LoginPreview>,
    composer: Entity<InputState>,
    search_input: Entity<InputState>,
    palette_input: Entity<InputState>,
    audio_session: Option<rencord_voice::AudioSession>,
    focus: FocusHandle,
    timeline: ListState,
    bench: bool,
    attachment_path: Entity<InputState>,
    upload_cancel: Option<Arc<AtomicBool>>,
    bench_frames: u32,
    bench_last: Instant,
    frame_us: Vec<u128>,
    resize_us: Vec<u128>,
    collapsed: HashSet<&'static str>,
    pinned_top: bool,
    following_tail: bool,
    followed_channel: Option<Snowflake>,
    reveal_hover: bool,
    golive: golive::GoLiveMode,
    columns: SavedColumns,
    reading_anchor: ReadingAnchor,
    right_choice: RightChoice,
    channels_drawer: bool,
    members_overlay: bool,
    channels_as_drawer: bool,
    members_as_overlay: bool,
    right_overlay: bool,
    profile_key: String,
    layout_store: LayoutStore,
    layout_path: PathBuf,
    persist_layout: bool,
    screenshot: Option<String>,
    screenshot_frames: u32,
    screenshot_waits: u32,
    metrics: ColumnMetrics,
    frame: Option<ResolvedLayout>,
    suppress_split: Option<SplitEdge>,
    _bounds_observer: Subscription,
    _display_observer: Subscription,
    shown: rencord_desktop::Theme,
    fade_from: rencord_desktop::Theme,
    fade_to: rencord_desktop::Theme,
    fade_at: Option<Instant>,
    system_dark: Option<bool>,
    user_themes: Vec<rencord_desktop::Theme>,
    theme_stamp: u64,
    token_index: usize,
    token_filter: String,
    settings_page: settings_ui::SettingsPage,
    appearance_tab: settings_ui::AppearanceTab,
    picker: Option<settings_ui::PickerTarget>,
    preset_menu: Option<settings_ui::PresetMenu>,
    theme_code: Entity<InputState>,
    base_hex: Entity<InputState>,
    accent_hex: Entity<InputState>,
    token_query: Entity<InputState>,
}

struct SplitGhost;

impl Render for SplitGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(Clone)]
struct SplitDrag {
    edge: SplitEdge,
    start_x: Rc<Cell<f32>>,
    start_width: f32,
}

impl RencordWindow {
    #[allow(clippy::too_many_arguments)]
    fn new(
        demo: bool,
        bench: bool,
        theme: Option<ThemeMode>,
        layout: Option<rencord_desktop::LayoutKind>,
        density: Option<rencord_desktop::Density>,
        start_voice: bool,
        reveal_hover: bool,
        golive: golive::GoLiveMode,
        screenshot: Option<String>,
        panel: Option<RightPanel>,
        open_drawer: bool,
        open_members: bool,
        open_settings: bool,
        persist_layout: bool,
        layout_store: LayoutStore,
        layout_path: PathBuf,
        big_file: Option<rencord_upload::Flow>,
        settings_view: Option<String>,
        theme_file: Option<std::path::PathBuf>,
        preview: Option<LoginPreview>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut shell = if demo {
            if bench {
                Shell::demo_history(800)
            } else {
                Shell::demo()
            }
        } else {
            Shell::login()
        };
        let had_saved = shell.load_appearance();
        if !had_saved && rencord_platform::system_reduced_motion() == Some(true) {
            shell.prefs.reduced_motion = true;
        }
        if let Some(mode) = theme {
            shell.set_theme(mode);
        }
        if let Some(path) = theme_file {
            if let Some(loaded) = rencord_desktop::theme_file_from_path(&path) {
                shell.prefs.custom = Some(loaded);
            }
        }
        if open_settings {
            shell.settings_open = true;
        }
        let (settings_page, appearance_tab) = settings_view
            .as_deref()
            .map(settings_ui::parse_settings)
            .unwrap_or((
                settings_ui::SettingsPage::Appearance,
                settings_ui::AppearanceTab::Presets,
            ));
        let system_dark = rencord_platform::system_scheme().map(|scheme| scheme.prefers_dark());
        let initial = shell.prefs.resolved(system_dark);
        if let Some(layout) = layout {
            shell.prefs.layout = layout;
        }
        if let Some(density) = density {
            shell.prefs.density = density;
            if matches!(density, rencord_desktop::Density::Compact) {
                shell.prefs.font_px = 13.0;
                shell.prefs.icon_px = 40.0;
            }
        }
        if start_voice {
            shell.join_voice_demo();
        }
        if let Some(flow) = big_file {
            shell.flow = flow;
        }
        let token_input = cx.new(|cx| InputState::new(window, cx).placeholder("Token"));
        let email_input = cx.new(|cx| InputState::new(window, cx).placeholder("Email"));
        let password_input = cx.new(|cx| InputState::new(window, cx).placeholder("Password"));
        let mfa_input = cx.new(|cx| InputState::new(window, cx).placeholder("Authentication code"));
        let composer = cx.new(|cx| InputState::new(window, cx).placeholder("Message #general"));
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search this channel"));
        let palette_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Jump to a channel or command"));
        let attachment_path =
            cx.new(|cx| InputState::new(window, cx).placeholder("Path, or put/s3 endpoint"));
        let theme_code = cx.new(|cx| InputState::new(window, cx).placeholder("Paste a theme code"));
        let base_hex = cx.new(|cx| InputState::new(window, cx).placeholder("Base hex"));
        let accent_hex = cx.new(|cx| InputState::new(window, cx).placeholder("Accent hex"));
        let token_query = cx.new(|cx| InputState::new(window, cx).placeholder("Search tokens"));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let bounds_observer = cx.observe_window_bounds(window, |this, window, cx| {
            this.remember_bounds(window, cx);
        });
        let display_observer = cx.observe_window_display(window, |this, window, cx| {
            this.remember_bounds(window, cx);
        });
        let user_themes = rencord_theme::load_theme_dir(&rencord_platform::themes_dir());
        let theme_stamp = rencord_theme::dir_stamp(&rencord_platform::themes_dir());
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let scheme = rencord_platform::system_scheme().map(|item| item.prefers_dark());
            let stamp = rencord_theme::dir_stamp(&rencord_platform::themes_dir());
            let themes = rencord_theme::load_theme_dir(&rencord_platform::themes_dir());
            this.update(cx, |view, cx| {
                let scheme_changed = view.system_dark != scheme;
                let folder_changed = view.theme_stamp != stamp;
                if scheme_changed || folder_changed {
                    view.system_dark = scheme;
                    view.theme_stamp = stamp;
                    view.user_themes = themes;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        let login = match preview {
            Some(kind) => LoginModel::preview(kind),
            None => LoginModel::fresh(),
        };
        // Older builds saved the upload host's secret key in this file; remove it.
        if let Err(err) = rencord_upload::scrub_saved_secret(
            &rencord_platform::data_dir().join("upload-settings.json"),
        ) {
            tracing::warn!(
                "could not remove a saved upload secret: {}",
                rencord_auth::redact(&err.to_string())
            );
        }
        Self {
            shell,
            token_input,
            email_input,
            password_input,
            mfa_input,
            login,
            vault: Self::open_vault(),
            session_machine: rencord_auth::SessionMachine::new(),
            session_token: None,
            api: None,
            gateway: None,
            voice_join: None,
            voice_cmd: None,
            voice_status: None,
            requested: HashSet::new(),
            failed: HashMap::new(),
            loading_older: None,
            history_start: HashSet::new(),
            login_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            remote_generation: 0,
            booted: preview.is_some(),
            preview,
            composer,
            search_input,
            palette_input,
            audio_session: None,
            focus,
            timeline: ListState::new(0, ListAlignment::Bottom, px(160.)),
            bench,
            attachment_path,
            upload_cancel: None,
            bench_frames: 0,
            bench_last: Instant::now(),
            frame_us: Vec::new(),
            resize_us: Vec::new(),
            collapsed: HashSet::new(),
            pinned_top: false,
            following_tail: true,
            followed_channel: None,
            reveal_hover,
            golive,
            columns: SavedColumns::default(),
            reading_anchor: ReadingAnchor::Left,
            right_choice: panel.map(RightChoice::Open).unwrap_or(RightChoice::Auto),
            channels_drawer: open_drawer,
            members_overlay: open_members,
            channels_as_drawer: false,
            members_as_overlay: false,
            right_overlay: false,
            profile_key: String::new(),
            layout_store,
            layout_path,
            persist_layout,
            screenshot,
            screenshot_frames: 0,
            screenshot_waits: 0,
            metrics: ColumnMetrics::onyx(),
            frame: None,
            suppress_split: None,
            _bounds_observer: bounds_observer,
            _display_observer: display_observer,
            shown: initial.clone(),
            fade_from: initial.clone(),
            fade_to: initial,
            fade_at: None,
            system_dark,
            user_themes,
            theme_stamp,
            token_index: 0,
            token_filter: String::new(),
            settings_page,
            appearance_tab,
            picker: None,
            preset_menu: None,
            theme_code,
            base_hex,
            accent_hex,
            token_query,
        }
    }

    fn remember_bounds(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_profile(window, cx);
        if !self.persist_layout {
            return;
        }
        let bounds = window.bounds();
        let frame = layout::clamp_window(
            layout::Rect {
                x: bounds.origin.x.as_f32(),
                y: bounds.origin.y.as_f32(),
                w: bounds.size.width.as_f32(),
                h: bounds.size.height.as_f32(),
            },
            &monitor_rects(cx),
            window.scale_factor(),
        );
        self.layout_store.window = Some(frame);
        let _ = self.layout_store.save(&self.layout_path);
    }

    fn sync_profile(&mut self, window: &Window, cx: &mut Context<Self>) {
        let size = window.viewport_size();
        let height = size.height.as_f32().max(1.0);
        let aspect = layout::classify(size.width.as_f32() / height);
        let key = layout::profile_key(
            aspect,
            window.scale_factor(),
            Some(&monitor_key(window, cx)),
        );
        if key == self.profile_key {
            return;
        }
        if !self.profile_key.is_empty() {
            self.layout_store.profiles.insert(
                self.profile_key.clone(),
                layout::Profile {
                    columns: self.columns,
                    anchor: self.reading_anchor,
                },
            );
        }
        self.profile_key = key;
        if let Some(profile) = self.layout_store.profiles.get(&self.profile_key).cloned() {
            self.columns = profile.columns;
            self.reading_anchor = profile.anchor;
        }
        if self.persist_layout {
            let _ = self.layout_store.save(&self.layout_path);
        }
    }

    fn current_plan(&self, window: &Window) -> ResolvedLayout {
        let size = window.viewport_size();
        let palette = self.shell.prefs.palette_for(self.system_dark);
        let ink = palette.ink_chrome;
        let mut metrics = if ink {
            ColumnMetrics::onyx()
        } else {
            ColumnMetrics::crisp()
        };
        metrics.gap = palette.shell_gap;
        metrics.rail = palette.rail_w;
        metrics.channels = palette.side_w;
        metrics.members = palette.member_w;
        layout::resolve(&LayoutInput {
            width: size.width.as_f32(),
            height: size.height.as_f32(),
            scale: window.scale_factor(),
            message_px: if ink {
                palette.body_size
            } else {
                self.shell.prefs.font_px
            },
            show_members: self.shell.prefs.show_members,
            show_channels: self.shell.prefs.show_sidebar,
            reading_anchor: self.reading_anchor,
            right: self.right_choice,
            saved: self.columns,
            metrics,
            monitor: None,
        })
    }

    fn toggle_channels(&mut self) {
        if self.channels_as_drawer {
            self.channels_drawer = !self.channels_drawer;
        } else {
            self.shell.prefs.show_sidebar = !self.shell.prefs.show_sidebar;
            self.channels_drawer = false;
        }
    }

    fn toggle_members(&mut self) {
        if self.members_as_overlay {
            self.members_overlay = !self.members_overlay;
        } else {
            self.shell.prefs.show_members = !self.shell.prefs.show_members;
            self.members_overlay = false;
        }
    }

    fn toggle_pin_panel(&mut self) {
        self.right_choice = self.right_choice.toggle_pin();
    }

    fn drag_context(&self) -> DragContext {
        let plan = self.frame.clone();
        DragContext {
            viewport_width: plan.as_ref().map(|plan| plan.chat_width).unwrap_or(0.0),
            gap: self.metrics.gap,
            rail: plan
                .as_ref()
                .map(|plan| plan.rail)
                .unwrap_or(self.metrics.rail),
            channels: plan.as_ref().map(|plan| plan.channels).unwrap_or(0.0),
            members: plan.as_ref().map(|plan| plan.members).unwrap_or(0.0),
            right: plan.as_ref().map(|plan| plan.right).unwrap_or(0.0),
            chat_fixed: plan.and_then(|plan| {
                if plan.chat_grows {
                    None
                } else {
                    Some(plan.chat_width)
                }
            }),
        }
    }

    fn resize_column(&mut self, edge: SplitEdge, proposed: f32) {
        if self.suppress_split == Some(edge) {
            return;
        }
        let mut ctx = self.drag_context();
        // The drag budget is the whole window, not the chat column.
        if let Some(plan) = &self.frame {
            ctx.viewport_width = plan.chat_width
                + plan.rail
                + plan.channels
                + plan.members
                + plan.right
                + layout::shell_chrome(
                    1 + (plan.rail > 0.0) as u8
                        + (plan.channels > 0.0) as u8
                        + (plan.members > 0.0) as u8
                        + (plan.right > 0.0) as u8,
                    plan.gap,
                );
        }
        let width = apply_drag(edge, proposed, &ctx);
        match edge {
            SplitEdge::Channels => self.columns.channels = Some(width),
            SplitEdge::Members => self.columns.members = Some(width),
            SplitEdge::Right => self.columns.right = Some(width),
        }
        self.store_columns();
    }

    fn reset_column(&mut self, edge: SplitEdge) {
        self.columns = self.columns.reset(edge);
        self.store_columns();
    }

    fn grab_screenshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.stub_shot_ready() {
            self.screenshot_waits += 1;
            if self.screenshot_waits < 400 {
                cx.on_next_frame(window, |this, window, cx| {
                    this.grab_screenshot(window, cx);
                });
                return;
            }
        }
        self.screenshot_frames += 1;
        if self.screenshot_frames < 4 {
            cx.on_next_frame(window, |this, window, cx| {
                this.grab_screenshot(window, cx);
            });
            return;
        }
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        let bounds = window.bounds();
        let scale = window.scale_factor();
        let x = (bounds.origin.x.as_f32() * scale).round().max(0.0) as i32;
        let y = (bounds.origin.y.as_f32() * scale).round().max(0.0) as i32;
        let width = (bounds.size.width.as_f32() * scale).round().max(1.0) as i32;
        let height = (bounds.size.height.as_f32() * scale).round().max(1.0) as i32;
        // The frame that scheduled this callback is painted before the process sleeps.
        std::thread::sleep(Duration::from_millis(250));
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "x11grab",
                "-draw_mouse",
                "0",
                "-video_size",
                &format!("{width}x{height}"),
                "-i",
                &format!("{display}.0+{x},{y}"),
                "-frames:v",
                "1",
                &path,
            ])
            .status();
        match status {
            Ok(code) if code.success() => println!("SCREENSHOT {path}"),
            Ok(code) => eprintln!("screenshot ffmpeg status {code}"),
            Err(err) => eprintln!("screenshot ffmpeg failed: {err}"),
        }
        cx.quit();
    }

    fn stub_shot_ready(&self) -> bool {
        match stub::current() {
            None => true,
            Some(stub::StubScene::Guilds) => {
                self.shell
                    .session
                    .guilds
                    .iter()
                    .any(|guild| guild.name == "Analytical Engines")
                    && self
                        .shell
                        .session
                        .selected_channel
                        .is_some_and(|channel| !self.shell.session.messages_for(channel).is_empty())
            }
            Some(stub::StubScene::Retry) => !self.failed.is_empty(),
            Some(stub::StubScene::Scroll) => self.loading_older.is_some(),
            Some(stub::StubScene::Layout) => {
                self.shell
                    .session
                    .guilds
                    .iter()
                    .any(|guild| guild.name == "Analytical Engines")
                    && self
                        .shell
                        .session
                        .selected_channel
                        .is_some_and(|channel| !self.shell.session.messages_for(channel).is_empty())
            }
        }
    }

    fn store_columns(&mut self) {
        if self.profile_key.is_empty() {
            return;
        }
        self.layout_store.profiles.insert(
            self.profile_key.clone(),
            layout::Profile {
                columns: self.columns,
                anchor: self.reading_anchor,
            },
        );
        if self.persist_layout {
            let _ = self.layout_store.save(&self.layout_path);
        }
    }

    fn tick_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.shell.prefs.resolved(self.system_dark);
        if target.signature() != self.fade_to.signature() {
            self.fade_from = self.shown.clone();
            self.fade_to = target;
            self.fade_at = if self.shell.prefs.reduced_motion {
                None
            } else {
                Some(Instant::now())
            };
        }
        if self.shell.prefs.reduced_motion || self.fade_at.is_none() {
            self.shown = self.fade_to.clone();
            self.fade_at = None;
            return;
        }
        let started = self.fade_at.unwrap_or_else(Instant::now);
        let t = started.elapsed().as_secs_f32() / 0.18;
        if t >= 1.0 {
            self.shown = self.fade_to.clone();
            self.fade_at = None;
        } else {
            self.shown = self
                .fade_from
                .lerp(&self.fade_to, rencord_theme::ease_out_cubic(t));
            cx.on_next_frame(window, |this, window, cx| {
                this.tick_theme(window, cx);
                cx.notify();
            });
        }
    }

    fn sync_hex_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let theme = self.shell.prefs.resolved(self.system_dark);
        let base = theme.base.unwrap_or(theme.colors.bg_base).to_hex();
        let accent = theme.accent_seed.unwrap_or(theme.colors.accent).to_hex();
        for (field, hex) in [
            (self.base_hex.clone(), base),
            (self.accent_hex.clone(), accent),
        ] {
            let focused = field.read(cx).focus_handle(cx).is_focused(window);
            let current = field.read(cx).value().to_string();
            if !focused && current != hex {
                field.update(cx, |state, cx| state.set_value(hex, window, cx));
            }
        }
    }

    fn choose_preset(&mut self, mode: rencord_desktop::ThemeMode) {
        self.shell.set_theme(mode);
        self.shell.save_appearance();
    }

    fn recolor(&mut self, hex: u32, accent: bool) {
        let current = self.shell.prefs.base_theme();
        let color = rencord_theme::Color::rgb(hex);
        let base = if accent {
            current.base.unwrap_or(current.colors.bg_chat)
        } else {
            color
        };
        let accent_color = if accent {
            color
        } else {
            current.accent_seed.unwrap_or(current.colors.accent)
        };
        let mut derived = rencord_theme::derive_theme(
            "Custom",
            base,
            accent_color,
            Some(current.mode),
            current.chrome,
        );
        derived.background = current.background;
        derived.metrics.font_ui = current.metrics.font_ui;
        self.shell.prefs.custom = Some(derived);
        self.shell.save_appearance();
    }

    fn toggle_gradient(&mut self) {
        let mut theme = self.shell.prefs.ensure_custom().clone();
        theme.background.kind = match theme.background.kind {
            rencord_theme::BackgroundKind::Solid => rencord_theme::BackgroundKind::Gradient {
                angle: 160.0,
                stops: vec![
                    theme.colors.bg_base,
                    theme.colors.accent,
                    theme.colors.bg_sidebar,
                ],
            },
            rencord_theme::BackgroundKind::Gradient { .. } => rencord_theme::BackgroundKind::Solid,
        };
        theme.preset = None;
        self.shell.prefs.custom = Some(theme);
        self.shell.save_appearance();
    }

    fn save_theme_folder(&self) {
        let theme = self.shell.prefs.resolved(self.system_dark);
        let dir = rencord_platform::themes_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = theme.to_toml() {
            let _ = std::fs::write(dir.join(format!("{}.toml", theme.id)), text);
        }
    }

    fn import_theme_code(&mut self, text: &str) {
        if let Ok(theme) = rencord_theme::theme_from_code(text) {
            self.shell.prefs.custom = Some(theme);
            self.shell.save_appearance();
        } else if let Ok(theme) = rencord_theme::theme_from_toml(text) {
            self.shell.prefs.custom = Some(theme);
            self.shell.save_appearance();
        } else if let Ok(theme) = rencord_theme::theme_from_json(text) {
            self.shell.prefs.custom = Some(theme);
            self.shell.save_appearance();
        }
    }

    fn sync_queries(&mut self, cx: &mut Context<Self>) {
        let search = self.search_input.read(cx).value().to_string();
        if search != self.shell.search {
            self.shell.search = search;
        }
        let palette = self.palette_input.read(cx).value().to_string();
        if palette != self.shell.palette_query {
            self.shell.palette_query = palette;
        }
        let tokens = self.token_query.read(cx).value().to_string();
        if tokens != self.token_filter {
            self.token_filter = tokens;
        }
    }

    fn note_bench_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.bench {
            return;
        }
        if self.bench_frames == 0 {
            let interactive = PROCESS_START
                .get()
                .map(|start| start.elapsed().as_millis())
                .unwrap_or(0);
            println!("BENCH interactive_ms={interactive}");
            println!("BENCH rss_kib={}", rss_kib());
            println!("BENCH channel_switch_us={}", self.shell.last_switch_us);
            println!("BENCH scale={:.2}", window.scale_factor());
            let started = Instant::now();
            let passes = 200;
            for _ in 0..passes {
                for (width, height) in layout::CHECK_SIZES {
                    let _ = layout::resolve(&layout::LayoutInput::for_size(*width, *height));
                }
            }
            let each =
                started.elapsed().as_nanos() / (passes * layout::CHECK_SIZES.len() as u128).max(1);
            println!("BENCH layout_resolve_ns={each}");
        }
        let gap = self.bench_last.elapsed().as_micros();
        if self.bench_frames > 0 && self.bench_frames < 24 {
            self.frame_us.push(gap);
        } else if self.bench_frames >= 24 {
            self.resize_us.push(gap);
        }
        self.bench_last = Instant::now();
        self.bench_frames += 1;
        if self.bench_frames < 24 {
            cx.on_next_frame(window, |this, window, cx| {
                this.shell.step_channel(1);
                this.note_bench_frame(window, cx);
            });
            return;
        }
        let resize_at = (self.bench_frames - 24) as usize;
        if resize_at < layout::CHECK_SIZES.len() {
            let (width, height) = layout::CHECK_SIZES[resize_at];
            window.resize(size(px(width), px(height)));
            cx.on_next_frame(window, |this, window, cx| {
                this.note_bench_frame(window, cx);
            });
            return;
        }
        self.frame_us.sort_unstable();
        self.resize_us.sort_unstable();
        let p50 = self
            .frame_us
            .get(self.frame_us.len() / 2)
            .copied()
            .unwrap_or(0);
        let resize_p50 = self
            .resize_us
            .get(self.resize_us.len() / 2)
            .copied()
            .unwrap_or(0);
        let resize_p95 = self
            .resize_us
            .get(self.resize_us.len().saturating_mul(95) / 100)
            .copied()
            .unwrap_or(resize_p50);
        println!("BENCH frame_p50_us={p50}");
        println!("BENCH frames={}", self.frame_us.len());
        println!("BENCH resize_p50_us={resize_p50}");
        println!("BENCH resize_p95_us={resize_p95}");
        println!("BENCH resize_frames={}", self.resize_us.len());
        println!("BENCH channel_switch_us={}", self.shell.last_switch_us);
        match window.refresh_interval(cx) {
            Some(interval) if !interval.is_zero() => {
                let hz = 1.0 / interval.as_secs_f32();
                println!("BENCH refresh_hz={hz:.2}");
                println!("BENCH frame_budget_us={}", interval.as_micros());
                println!("BENCH display_is_120hz={}", hz >= 110.0);
            }
            _ => {
                println!("BENCH refresh_hz=unreported");
                println!("BENCH display_is_120hz=false");
            }
        }
        println!("BENCH inferred_120hz_budget_us=8333");
        println!("BENCH pacing=gpui follows the display refresh interval and this build does not cap frames at 60");
        cx.quit();
    }

    fn start_big_file(&mut self, cx: &mut Context<Self>) {
        let dir = rencord_platform::data_dir().join("uploads");
        let prepared = match self.shell.flow.prepare_job(&dir) {
            Ok(prepared) => prepared,
            Err(err) => {
                self.shell.flow.fail(err.to_string());
                return;
            }
        };
        let rencord_upload::Prepared::Job(job) = prepared else {
            if let rencord_upload::Prepared::Note(text) = prepared {
                if let Some(mutation) = self.shell.send_local(&text) {
                    self.dispatch_mutation(mutation, cx);
                }
                self.shell.flow.succeed(text);
            }
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.upload_cancel = Some(cancel.clone());
        let (tx, rx) = async_channel::unbounded();
        let progress = tx.clone();
        std::thread::spawn(move || {
            let result = rencord_upload::run_job(*job, &cancel, move |snap| {
                let _ = progress.send_blocking(BigFileEvent::Progress(snap));
            });
            let _ = tx.send_blocking(BigFileEvent::Done(result));
        });
        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                this.update(cx, |view, cx| {
                    match event {
                        BigFileEvent::Progress(snap) => view.shell.flow.apply_progress(snap),
                        BigFileEvent::Done(Ok(output)) => {
                            let text = big_file_outcome(&output);
                            if let Some(mutation) = view.shell.send_local(&text) {
                                view.dispatch_mutation(mutation, cx);
                            }
                            view.shell.flow.succeed(text);
                            view.upload_cancel = None;
                        }
                        BigFileEvent::Done(Err(err)) => {
                            view.shell.flow.fail(err.to_string());
                            view.upload_cancel = None;
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn join_parts(&mut self, first: &str) {
        let note = self
            .shell
            .flow
            .parsed_note()
            .or_else(|| rencord_upload::parse_note(&self.shell.flow.reassemble_note));
        let Some(note) = note else {
            self.shell.flow.error = Some("That note is not a rencord-split v1 message.".into());
            return;
        };
        let start = PathBuf::from(first);
        let Some(dir) = start.parent() else {
            self.shell.flow.error = Some("Pick the .001 file.".into());
            return;
        };
        let mut volumes = Vec::new();
        for index in 1..=note.parts {
            let name = format!("{}.{}.{:03}", note.name, note.format.label(), index);
            let path = if index == 1 && start.is_file() {
                start.clone()
            } else {
                dir.join(&name)
            };
            match std::fs::read(&path) {
                Ok(bytes) => volumes.push(bytes),
                Err(err) => {
                    self.shell.flow.error = Some(format!("missing {name}: {err}"));
                    return;
                }
            }
        }
        match rencord_upload::reassemble(&volumes) {
            Ok(bytes) => {
                let output = dir.join(&note.name);
                if std::fs::write(&output, &bytes).is_ok() {
                    self.shell.flow.succeed(format!(
                        "Reassembled {} ({} bytes) at {}",
                        note.name,
                        bytes.len(),
                        output.display()
                    ));
                    self.shell.note(format!("Reassembled {}", note.name));
                } else {
                    self.shell.flow.error = Some("could not write the joined file".into());
                }
            }
            Err(err) => self.shell.flow.error = Some(err.to_string()),
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().to_string();
        if let Some(mutation) = self.shell.send_local(&text) {
            self.dispatch_mutation(mutation, cx);
        }
        cx.notify();
    }

    fn dispatch_mutation(&mut self, mutation: rencord_desktop::Mutation, cx: &mut Context<Self>) {
        if self.shell.demo {
            self.shell
                .note("Demo keeps the change locally. A signed-in session would call Discord.");
            return;
        }
        let Some(api) = self.api.clone() else {
            self.shell.rollback(&mutation);
            self.shell
                .note("No session stored. The change was rolled back.");
            return;
        };
        let outbound = mutation.clone();
        let task = net::runtime().spawn(async move { net::push_mutation(&api, &outbound).await });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = task.await else {
                return;
            };
            this.update(cx, |view, cx| {
                match outcome {
                    Err(net::BootError::Unauthorized) => {
                        view.shell.rollback(&mutation);
                        view.note_unauthorized(cx);
                    }
                    Err(net::BootError::Other(err)) => {
                        view.shell.rollback(&mutation);
                        view.shell
                            .note(format!("Discord rejected the change: {err}"));
                    }
                    Ok(sent) => {
                        if let Some(sent) = sent {
                            view.shell.confirm_create(&mutation, sent);
                        }
                        view.shell.note("Discord accepted the change.");
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Ends the live side of a session: the gateway stops when its sender drops.
    fn drop_live_session(&mut self) {
        self.session_token = None;
        self.api = None;
        self.gateway = None;
        self.requested.clear();
        self.failed.clear();
        self.loading_older = None;
        self.history_start.clear();
    }

    /// Loads what the selection needs the first time it is shown: a guild's
    /// channels and a channel's recent history. Runs from render, so every way
    /// of changing the selection (click, keys, switcher, search) is covered.
    fn load_selection(&mut self, cx: &mut Context<Self>) {
        let Some(api) = self.api.clone() else {
            return;
        };
        if let Some(guild) = self.shell.session.selected_guild {
            if self.shell.session.guild_channels(guild).is_empty()
                && !self.failed.contains_key(&guild)
                && self.requested.insert(guild)
            {
                let api = api.clone();
                let task = net::runtime().spawn(async move { api.guild_channels(guild).await });
                cx.spawn(async move |this, cx| {
                    let Ok(result) = task.await else {
                        return;
                    };
                    this.update(cx, |view, cx| {
                        match result {
                            Ok(channels) => {
                                for channel in channels {
                                    view.shell.session.upsert_channel(channel);
                                }
                                let session = &mut view.shell.session;
                                if session.selected_guild == Some(guild)
                                    && session.selected_channel.is_none()
                                {
                                    session.select_guild(guild);
                                }
                            }
                            Err(err) => {
                                record_fetch_failure(
                                    &mut view.requested,
                                    &mut view.failed,
                                    guild,
                                    fetch_failure_text("channels", &err),
                                    true,
                                );
                            }
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
        }
        if let Some(channel) = self.shell.session.selected_channel {
            if !self.failed.contains_key(&channel) && self.requested.insert(channel) {
                let task = net::runtime().spawn(async move { api.messages(channel, 50).await });
                cx.spawn(async move |this, cx| {
                    let Ok(result) = task.await else {
                        return;
                    };
                    this.update(cx, |view, cx| {
                        match result {
                            Ok(history) => view.shell.session.merge_history(channel, history),
                            Err(err) => {
                                record_fetch_failure(
                                    &mut view.requested,
                                    &mut view.failed,
                                    channel,
                                    fetch_failure_text("messages", &err),
                                    true,
                                );
                            }
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// One older page when the timeline is at the top. Not called again until
    /// this page returns, and not at all once a short page marks the start.
    fn scroll_back(&mut self, cx: &mut Context<Self>) {
        let Some(api) = self.api.clone() else {
            return;
        };
        let Some(channel) = self.shell.session.selected_channel else {
            return;
        };
        if self.loading_older.is_some()
            || self.history_start.contains(&channel)
            || self.failed.contains_key(&channel)
        {
            return;
        }
        if !self.requested.contains(&channel) {
            return;
        }
        let Some(oldest) = self
            .shell
            .session
            .messages_for(channel)
            .first()
            .map(|message| message.id)
        else {
            return;
        };
        self.loading_older = Some(channel);
        cx.notify();
        let task =
            net::runtime().spawn(async move { api.messages_before(channel, oldest, 50).await });
        cx.spawn(async move |this, cx| {
            let Ok(result) = task.await else {
                return;
            };
            this.update(cx, |view, cx| {
                if view.loading_older == Some(channel) {
                    view.loading_older = None;
                }
                match result {
                    Ok(history) => {
                        if history.len() < 50 {
                            view.history_start.insert(channel);
                        }
                        let top = view.timeline.logical_scroll_top();
                        let anchor = top.item_ix.saturating_sub(1);
                        let prepended = view.shell.session.merge_older(channel, history);
                        let count = view.shell.session.messages_for(channel).len();
                        view.timeline.reset(count);
                        view.timeline.scroll_to(ListOffset {
                            item_ix: anchor + prepended,
                            offset_in_item: top.offset_in_item,
                        });
                    }
                    Err(err) => {
                        record_fetch_failure(
                            &mut view.requested,
                            &mut view.failed,
                            channel,
                            fetch_failure_text("messages", &err),
                            false,
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn reopen_audio(&mut self) {
        if self.audio_session.is_none() {
            return;
        }
        let inputs = rencord_voice::list_input_devices();
        let outputs = rencord_voice::list_output_devices();
        let mut input = self.shell.prefs.input_device.clone();
        let mut output = self.shell.prefs.output_device.clone();
        if let Some(name) = &input {
            if !inputs.iter().any(|item| item == name) {
                input = None;
            }
        }
        if let Some(name) = &output {
            if !outputs.iter().any(|item| item == name) {
                output = None;
            }
        }
        match rencord_voice::open_call_audio(input.as_deref(), output.as_deref()) {
            Ok(session) => {
                if session.input_fallback || session.output_fallback {
                    self.shell.note(format!(
                        "A chosen device was missing. Using {} and {}.",
                        session.input_name, session.output_name
                    ));
                } else {
                    self.shell.note(format!(
                        "Audio devices: {} / {}",
                        session.input_name, session.output_name
                    ));
                }
                self.shell.prefs.input_device = Some(session.input_name.clone());
                self.shell.prefs.output_device = Some(session.output_name.clone());
                self.audio_session = Some(session);
            }
            Err(err) => self.shell.note(format!("Could not switch devices: {err}")),
        }
        self.push_voice_cmd(voice_call::VoiceCmd::Devices {
            input: self.shell.prefs.input_device.clone(),
            output: self.shell.prefs.output_device.clone(),
        });
    }

    fn join_voice(&mut self, cx: &mut Context<Self>) {
        if self.shell.demo || self.gateway.is_none() {
            self.shell.join_voice_demo();
            match rencord_voice::open_call_audio(
                self.shell.prefs.input_device.as_deref(),
                self.shell.prefs.output_device.as_deref(),
            ) {
                Ok(session) => {
                    self.audio_session = Some(session);
                    self.shell
                        .note("cpal devices are open. Demo voice stays on the loopback.");
                }
                Err(err) => self.shell.note(format!(
                    "cpal devices were not opened ({err}). Loopback stats still ran."
                )),
            }
            return;
        }
        let Some(user) = self.shell.session.user.as_ref().map(|user| user.id) else {
            self.shell.session.status_line = "Voice error (voice): not signed in".into();
            return;
        };
        let channel = self
            .shell
            .session
            .channels
            .values()
            .find(|channel| channel.kind.is_voice())
            .map(|channel| channel.id)
            .or(self.shell.session.selected_channel);
        let Some(channel) = channel else {
            self.shell.session.status_line =
                "Voice error (voice): no voice channel is selected".into();
            return;
        };
        let guild = self.shell.session.selected_guild;
        let join = self
            .voice_join
            .get_or_insert_with(|| rencord_voice::VoiceJoin::new(user));
        let update = join.join(guild, channel);
        self.shell.session.status_line = join.status.text();
        self.send_voice_state(update);
        cx.notify();
    }

    fn send_voice_state(&self, update: rencord_voice::VoiceGatewayUpdate) {
        let Some(gateway) = &self.gateway else {
            return;
        };
        let _ = gateway.try_send(rencord_gateway::GatewayCommand::UpdateVoiceState {
            guild_id: update.guild_id,
            channel_id: update.channel_id,
            self_mute: update.self_mute,
            self_deaf: update.self_deaf,
        });
    }

    fn on_gateway_event(&mut self, event: rencord_gateway::GatewayEvent, cx: &mut Context<Self>) {
        let voice_state = match &event {
            rencord_gateway::GatewayEvent::VoiceState(state) => {
                Some((state.user_id, state.channel_id, state.session_id.clone()))
            }
            _ => None,
        };
        let voice_server = match &event {
            rencord_gateway::GatewayEvent::VoiceServer(update) => {
                Some((update.token.clone(), update.endpoint.clone()))
            }
            _ => None,
        };
        self.shell.session.apply_gateway(event);
        if self.shell.demo {
            return;
        }
        let Some(user) = self.shell.session.user.as_ref().map(|user| user.id) else {
            return;
        };
        let join = self
            .voice_join
            .get_or_insert_with(|| rencord_voice::VoiceJoin::new(user));
        let connect = if let Some((user_id, channel_id, session_id)) = voice_state {
            join.on_voice_state(user_id, channel_id, &session_id)
        } else if let Some((token, endpoint)) = voice_server {
            join.on_voice_server(&token, endpoint.as_deref())
        } else {
            None
        };
        self.shell.session.status_line = join.status.text();
        if let Some(connect) = connect {
            self.spawn_call(connect, cx);
        }
    }

    fn spawn_call(&mut self, connect: rencord_voice::VoiceConnect, cx: &mut Context<Self>) {
        if let Some(cmd) = self.voice_cmd.take() {
            let _ = cmd.try_send(voice_call::VoiceCmd::Leave);
        }
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(8);
        let (status_tx, status_rx) = std::sync::mpsc::channel();
        self.voice_cmd = Some(cmd_tx);
        self.voice_status = Some(status_rx);
        let user_id = connect.user_id.parse().unwrap_or(0);
        let channel_id = self
            .voice_join
            .as_ref()
            .and_then(|join| join.channel_id())
            .unwrap_or(0);
        let target = rencord_voice::VoiceTarget {
            ws_url: connect.ws_url,
            server_id: connect.server_id,
            user_id: connect.user_id,
            session_id: connect.session_id,
            token: connect.token,
        };
        let server = connect.server_name;
        let input = self.shell.prefs.input_device.clone();
        let output = self.shell.prefs.output_device.clone();
        net::runtime().spawn(voice_call::run_call(voice_call::CallRequest {
            target,
            user_id,
            channel_id,
            commands: cmd_rx,
            status: status_tx,
            server,
            input,
            output,
        }));
        cx.notify();
    }

    fn drain_voice_status(&mut self) {
        let Some(rx) = &self.voice_status else {
            return;
        };
        while let Ok(line) = rx.try_recv() {
            self.shell.session.status_line = line;
        }
    }

    fn push_voice_cmd(&self, cmd: voice_call::VoiceCmd) {
        if let Some(tx) = &self.voice_cmd {
            let _ = tx.try_send(cmd);
        }
    }

    fn sync_voice_flags(&mut self) {
        if let Some(join) = self.voice_join.as_mut() {
            let update = join.set_self_state(self.shell.muted, self.shell.deafened);
            self.send_voice_state(update);
        }
        self.push_voice_cmd(voice_call::VoiceCmd::Mute(self.shell.muted));
        self.push_voice_cmd(voice_call::VoiceCmd::Deaf(self.shell.deafened));
    }

    fn leave_voice_call(&mut self) {
        if let Some(join) = self.voice_join.as_mut() {
            let update = join.leave();
            let line = join.status.text();
            self.send_voice_state(update);
            self.shell.session.status_line = line;
        } else {
            self.shell.leave_voice();
        }
        self.push_voice_cmd(voice_call::VoiceCmd::Leave);
        self.voice_cmd = None;
        self.shell.session.voice_channel = None;
    }
}

impl Render for RencordWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain_voice_status();
        self.sync_queries(cx);
        self.load_selection(cx);
        if stub::current() == Some(stub::StubScene::Scroll) && self.loading_older.is_none() {
            self.scroll_back(cx);
        }
        self.tick_theme(window, cx);
        let palette = rencord_desktop::Palette::from_theme(
            &self.shown,
            self.shell.prefs.font_px,
            self.shell.prefs.density,
        );
        apply_kit_theme(&palette, cx);
        if self.bench && self.bench_frames == 0 {
            cx.on_next_frame(window, |this, window, cx| {
                this.note_bench_frame(window, cx);
            });
        }
        if self.screenshot.is_some() && self.screenshot_frames == 0 && !self.bench {
            cx.on_next_frame(window, |this, window, cx| {
                this.grab_screenshot(window, cx);
            });
        }
        let signed_in = self.shell.session.user.is_some() && !self.login.adding;
        let root = div()
            .id("rencord-root")
            .role(Role::Document)
            .aria_label("Rencord")
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(palette.app_bg))
            .font_family("Segoe UI Variable, Segoe UI, Inter")
            .text_color(rgb(palette.text))
            .text_size(px(self.shell.prefs.font_px))
            .on_action(cx.listener(|this, _: &QuickSwitcher, _, cx| {
                this.shell.toggle_palette();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CloseLayer, _, cx| {
                this.shell.close_layers();
                this.channels_drawer = false;
                this.members_overlay = false;
                if this.right_overlay {
                    this.right_choice = RightChoice::Auto;
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextChannel, _, cx| {
                this.shell.step_channel(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PrevChannel, _, cx| {
                this.shell.step_channel(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextGuild, _, cx| {
                this.shell.step_guild(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PrevGuild, _, cx| {
                this.shell.step_guild(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.shell.toggle_settings();
                this.reopen_audio();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenInbox, _, cx| {
                this.shell.inbox_open = !this.shell.inbox_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.toggle_channels();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleMembers, _, cx| {
                this.toggle_members();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSplit, _, cx| {
                this.shell.prefs.split_pane = !this.shell.prefs.split_pane;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TogglePip, _, cx| {
                this.shell.prefs.pip = !this.shell.prefs.pip;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SendMessage, _, cx| {
                this.send(cx);
            }))
            .on_action(cx.listener(|this, _: &ShowHelp, _, cx| {
                this.shell.help_open = !this.shell.help_open;
                cx.notify();
            }));
        if self.golive == golive::GoLiveMode::Picker {
            return root.child(golive::picker(palette)).into_any_element();
        }
        if self.golive == golive::GoLiveMode::Viewer {
            return root.child(golive::viewer(palette)).into_any_element();
        }
        if self.golive == golive::GoLiveMode::Pip {
            return root.child(golive::pip(palette)).into_any_element();
        }
        if !signed_in {
            return root.child(self.login_screen(window, cx)).into_any_element();
        }
        root.child(self.shell_view(window, cx)).into_any_element()
    }
}

impl RencordWindow {
    #[allow(clippy::too_many_arguments)]
    fn guild_channel_row(
        &self,
        view: Entity<Self>,
        channel: &rencord_model::Channel,
        selected: Option<Snowflake>,
        palette: Palette,
        favorites_open: bool,
        favorite_ids: &[Snowflake],
        row_px: f32,
        in_voice: bool,
    ) -> gpui::AnyElement {
        let badge = self.shell.session.unread.contains(&channel.id).then_some(1);
        let shown_as_favorite = favorites_open && favorite_ids.contains(&channel.id);
        let prefix = if channel.kind.is_voice() { ")) " } else { "# " };
        channel_row(
            view,
            channel.id,
            format!("{prefix}{}", channel.name),
            selected == Some(channel.id) && !shown_as_favorite,
            palette,
            badge,
            channel.kind.is_voice(),
            row_px,
            in_voice,
        )
        .into_any_element()
    }

    fn shell_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if stub::take_home() {
            self.shell.session.open_home();
        }
        let palette = rencord_desktop::Palette::from_theme(
            &self.shown,
            self.shell.prefs.font_px,
            self.shell.prefs.density,
        );
        let density = self.shell.prefs.density;
        let this = cx.entity();
        let guilds = self.shell.session.guilds.clone();
        let selected_guild = self.shell.session.selected_guild;
        let channels = selected_guild
            .map(|id| {
                self.shell
                    .session
                    .guild_channels(id)
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let dms = self
            .shell
            .session
            .dm_channels()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let selected = self.shell.session.selected_channel;
        let messages = self.shell.visible_messages();
        let status = self.shell.session.status_line.clone();
        let members = self.shell.members.clone();
        let reactions = self.shell.reactions.clone();
        let font_px = self.shell.prefs.font_px;
        let theme_label = self.shell.prefs.theme.label();
        let voice = self.shell.voice_report.clone();
        let muted = self.shell.muted;
        let deafened = self.shell.deafened;
        let reply = self.shell.reply_to;
        let editing = self.shell.editing;
        let switch_us = self.shell.last_switch_us;
        let settings = self.shell.settings_open;
        let palette_open = self.shell.palette_open;
        let commands = self.shell.palette_commands();
        let inputs = rencord_voice::list_input_devices();
        let outputs = rencord_voice::list_output_devices();
        let chosen_in = self.shell.prefs.input_device.clone();
        let chosen_out = self.shell.prefs.output_device.clone();

        let user_name = self
            .shell
            .session
            .user
            .as_ref()
            .map(|user| user.display_name().to_string())
            .unwrap_or_else(|| "You".into());
        let guild_name = guilds
            .iter()
            .find(|guild| Some(guild.id) == selected_guild)
            .map(|guild| guild.name.clone())
            .unwrap_or_else(|| "Direct Messages".into());
        let home_active = selected.is_some_and(|id| dms.iter().any(|channel| channel.id == id));
        let ink = palette.ink_chrome;
        let theme = self.shown.clone();
        self.sync_profile(window, cx);
        let plan = self.current_plan(window);
        self.channels_as_drawer = plan.channels_as_drawer;
        self.members_as_overlay = plan.members_as_overlay;
        self.right_overlay = plan.right_as_overlay;
        self.metrics = if ink {
            ColumnMetrics::onyx()
        } else {
            ColumnMetrics::crisp()
        };
        self.frame = Some(plan.clone());
        let rail_w = if plan.rail > 0.0 {
            plan.rail
        } else {
            palette.rail_w
        };
        let tile = if plan.touch {
            layout::TOUCH_TARGET.min(palette.tile.max(40.))
        } else {
            palette.tile
        };
        let tile_radius = palette.tile_radius;
        let hit = if plan.touch {
            layout::TOUCH_TARGET
        } else {
            32.
        };
        let header_h = if plan.touch {
            layout::TOUCH_TARGET
        } else {
            palette.header_h
        };
        let pill_ink = palette.pill;
        let home_fill = if home_active {
            palette.home_bg_active
        } else {
            palette.home_bg
        };
        let home_border = if home_active {
            palette.home_border_active
        } else {
            palette.home_border
        };
        let mut rail = v_flex()
            .w(px(rail_w))
            .h_full()
            .bg(rgb(palette.rail))
            .items_center()
            .gap(px(8.))
            .py(px(palette.rail_pad));
        {
            let view = this.clone();
            rail = rail.child(rail_slot(
                "rail-home".into(),
                home_active,
                if home_active {
                    Pill::Selected
                } else {
                    Pill::None
                },
                home_fill,
                home_fill,
                pill_ink,
                home_border,
                rail_w,
                tile,
                tile_radius,
                hexwave_mark(
                    rencord_desktop::rail_home_glyph_px(tile),
                    palette.text,
                    window.scale_factor(),
                ),
                move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.shell.session.open_home();
                        cx.notify();
                    });
                },
            ));
        }
        if palette.show_top_rule {
            rail = rail.child(rail_rule(rail_w, palette.line));
        }
        for guild in guilds {
            let id = guild.id;
            let mark = guild_mark(&guild.name);
            let active = selected_guild == Some(id);
            let view = this.clone();
            let tile_colors = rencord_desktop::guild_tile(&guild.name, active, &theme);
            let fill = tile_colors.fill;
            let border = fill;
            let letter = tile_colors.letter;
            let guild_unread = self.shell.session.unread.iter().any(|channel_id| {
                self.shell
                    .session
                    .channels
                    .get(channel_id)
                    .is_some_and(|channel| channel.guild_id == Some(id))
            });
            rail = rail.child(rail_slot(
                format!("guild-{id}").into(),
                active,
                if active {
                    Pill::Selected
                } else if guild_unread {
                    Pill::Unread
                } else {
                    Pill::None
                },
                fill,
                fill,
                pill_ink,
                border,
                rail_w,
                tile.max(48.),
                if active { 16. } else { 24. },
                tile_initial(&mark, letter),
                move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.shell.session.select_guild(id);
                        cx.notify();
                    });
                },
            ));
        }
        rail = rail.child(rail_rule(rail_w, palette.border_strong));
        rail = rail.child(rail_action(
            this.clone(),
            "rail-add",
            "plus",
            true,
            palette,
            rail_w,
            tile,
            tile_radius,
            |view, cx| {
                view.shell.note("Creating a server is not in this build.");
                cx.notify();
            },
        ));
        rail = rail.child(rail_action(
            this.clone(),
            "rail-explore",
            "compass",
            false,
            palette,
            rail_w,
            tile,
            tile_radius,
            |view, cx| {
                view.shell.note("Server discovery is not in this build.");
                cx.notify();
            },
        ));

        let mut sidebar = v_flex()
            .w_full()
            .h_full()
            .bg(rgb(palette.sidebar))
            .child(
                div()
                    .h(px(palette.header_h))
                    .px(px(palette.header_px))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(palette.line))
                    .text_size(px(palette.title_px))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(palette.text))
                    .child(guild_name.clone()),
            )
            .child(if ink {
                h_flex()
                    .mx(px(10.))
                    .mt(px(8.))
                    .mb(px(4.))
                    .h(px(30.))
                    .px(px(8.))
                    .gap(px(6.))
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(palette.line))
                    .bg(rgb(palette.input))
                    .child(icon("search", 13., palette.faint))
                    .child(
                        div().flex_1().overflow_hidden().child(
                            Input::new(&self.search_input)
                                .bg(rgb(palette.input))
                                .text_color(rgb(palette.faint))
                                .border_color(rgb(palette.input)),
                        ),
                    )
                    .into_any_element()
            } else {
                div()
                    .px(px(8.))
                    .py(px(8.))
                    .child(
                        Input::new(&self.search_input)
                            .bg(rgb(palette.input))
                            .text_color(rgb(palette.text))
                            .border_color(rgb(palette.input)),
                    )
                    .into_any_element()
            });
        let row_px = density.channel_row_px();
        let favorites_open = !self.collapsed.contains("favorites");
        let text_open = !self.collapsed.contains("text");
        let voice_open = !self.collapsed.contains("voice");
        let dm_open = !self.collapsed.contains("dms");
        let favorite_ids = self.shell.favorites.clone();
        let in_voice = voice.is_some();
        let mut nav = v_flex()
            .id("channel-nav")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll();
        nav = nav.child(category_header(
            this.clone(),
            "favorites",
            "FAVORITES",
            favorites_open,
            palette,
        ));
        if favorites_open {
            for id in &favorite_ids {
                if let Some(channel) = self.shell.session.channels.get(id) {
                    nav = nav.child(channel_row(
                        this.clone(),
                        channel.id,
                        format!("* {}", channel.name),
                        selected == Some(channel.id),
                        palette,
                        self.shell.session.unread.contains(&channel.id).then_some(1),
                        false,
                        row_px,
                        false,
                    ));
                }
            }
        }
        if selected_guild.is_some() {
            if let Some(guild) = selected_guild {
                if let Some(message) = self.failed.get(&guild).cloned() {
                    nav = nav.child(fetch_error(
                        this.clone(),
                        "retry-channels",
                        "retry-channels-button",
                        message,
                        palette,
                        move |view, cx| {
                            view.failed.remove(&guild);
                            cx.notify();
                        },
                    ));
                }
            }
            let categories: Vec<_> = channels
                .iter()
                .filter(|channel| channel.kind == rencord_model::ChannelKind::GuildCategory)
                .cloned()
                .collect();
            if categories.is_empty() {
                nav = nav.child(category_header(
                    this.clone(),
                    "text",
                    "TEXT CHANNELS",
                    text_open,
                    palette,
                ));
                if text_open {
                    for channel in channels.iter().filter(|channel| channel.kind.is_text()) {
                        nav = nav.child(self.guild_channel_row(
                            this.clone(),
                            channel,
                            selected,
                            palette,
                            favorites_open,
                            &favorite_ids,
                            row_px,
                            false,
                        ));
                    }
                }
                nav = nav.child(category_header(
                    this.clone(),
                    "voice",
                    "VOICE CHANNELS",
                    voice_open,
                    palette,
                ));
                if voice_open {
                    for channel in channels.iter().filter(|channel| channel.kind.is_voice()) {
                        nav = nav.child(self.guild_channel_row(
                            this.clone(),
                            channel,
                            selected,
                            palette,
                            favorites_open,
                            &favorite_ids,
                            row_px,
                            in_voice,
                        ));
                    }
                }
            } else if text_open {
                for category in categories {
                    nav = nav.child(div().px(px(8.)).pt(px(16.)).pb(px(4.)).child(paint(
                        category.name.to_uppercase(),
                        palette.muted,
                        12.,
                    )));
                    for channel in channels
                        .iter()
                        .filter(|channel| channel.parent_id == Some(category.id))
                    {
                        nav = nav.child(self.guild_channel_row(
                            this.clone(),
                            channel,
                            selected,
                            palette,
                            favorites_open,
                            &favorite_ids,
                            row_px,
                            in_voice && channel.kind.is_voice(),
                        ));
                    }
                }
            }
        } else {
            nav = nav.child(category_header(
                this.clone(),
                "dms",
                "DIRECT MESSAGES",
                dm_open,
                palette,
            ));
            if dm_open {
                for channel in &dms {
                    nav = nav.child(channel_row(
                        this.clone(),
                        channel.id,
                        format!("dm {}", channel.name),
                        selected == Some(channel.id),
                        palette,
                        self.shell.session.unread.contains(&channel.id).then_some(1),
                        false,
                        row_px,
                        false,
                    ));
                }
            }
        }
        sidebar = sidebar.child(nav);

        if self.followed_channel != selected {
            self.followed_channel = selected;
            self.following_tail = true;
        }
        let count =
            messages.len() + usize::from(self.loading_older == self.shell.session.selected_channel);
        if self.timeline.item_count() != count {
            self.timeline.reset(count);
        }
        if stub::current() == Some(stub::StubScene::Scroll) && self.loading_older.is_some() {
            self.timeline.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
        }
        if self.shell.demo && !self.bench && !self.pinned_top {
            self.timeline.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
            self.pinned_top = true;
        }
        if let Some(index) = self.shell.take_jump() {
            self.timeline.scroll_to_reveal_item(index);
        }
        let timeline_state = self.timeline.clone();
        let list_messages = messages.clone();
        let list_palette = palette;
        let list_theme = theme.clone();
        let list_entity = this.clone();
        let list_reactions = reactions.clone();
        let reveal_hover = self.reveal_hover;
        let loading_older = self.loading_older == self.shell.session.selected_channel;
        let _density = density;
        {
            let view = cx.entity();
            self.timeline.set_scroll_handler(move |event, _window, cx| {
                view.update(cx, |this, cx| {
                    if this.following_tail != event.is_following_tail {
                        this.following_tail = event.is_following_tail;
                        cx.notify();
                    }
                    if event.visible_range.start == 0 && event.is_scrolled {
                        this.scroll_back(cx);
                    }
                });
            });
        }
        let list_now = rencord_desktop::now_local();
        let timeline = list(timeline_state, move |index, _window, _cx| {
            let message_index = if loading_older {
                if index == 0 {
                    return div()
                        .h(px(28.))
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .text_color(rgb(list_palette.muted))
                        .child("Loading earlier messages")
                        .into_any_element();
                }
                index - 1
            } else {
                index
            };
            let Some(message) = list_messages.get(message_index).cloned() else {
                return div().into_any_element();
            };
            let stamp = rencord_desktop::present_time(&message.timestamp, list_now);
            let previous = message_index
                .checked_sub(1)
                .and_then(|index| list_messages.get(index));
            let show_day = rencord_desktop::day_changed(
                previous.map(|item| item.timestamp.as_str()),
                &message.timestamp,
                list_now,
            );
            let grouped = !show_day
                && previous.is_some_and(|item| {
                    rencord_desktop::groups_with(
                        item.author.id.get(),
                        &item.timestamp,
                        item.kind,
                        message.author.id.get(),
                        &message.timestamp,
                        message.kind,
                    )
                });
            if let Some(line) =
                rencord_model::system_line(message.kind, message.author.display_name())
            {
                return h_flex()
                    .px(px(16.))
                    .h(px(32.))
                    .items_center()
                    .gap(px(8.))
                    .child(paint("→", list_palette.muted, 14.))
                    .child(paint(line, list_palette.muted, 14.))
                    .into_any_element();
            }
            let row = message_row(
                list_entity.clone(),
                &message,
                grouped,
                list_reactions
                    .get(&message.id.get())
                    .cloned()
                    .unwrap_or_default(),
                list_palette,
                &list_theme,
                font_px,
                reveal_hover && message.content.contains("Mention"),
                &stamp,
                reveal_hover,
            );
            if show_day {
                v_flex()
                    .child(date_pill(&stamp.day, list_palette))
                    .child(row)
                    .into_any_element()
            } else {
                row.into_any_element()
            }
        })
        .size_full();

        let channel_name = channel_title(selected, &channels, &dms);
        let composer_hint = rencord_desktop::composer_placeholder(&channel_name);
        self.composer.update(cx, |input, cx| {
            input.set_placeholder(SharedString::from(composer_hint.clone()), window, cx);
        });
        let mut chat = v_flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(rgb(palette.chat))
            .child(
                h_flex()
                    .h(px(header_h))
                    .px(px(12.))
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(palette.line))
                    .child(
                        h_flex()
                            .id("channel-title")
                            .flex_1()
                            .min_w(px(0.))
                            .h_full()
                            .gap(px(8.))
                            .items_center()
                            .overflow_hidden()
                            .when(plan.channels_as_drawer, |title| {
                                let view = this.clone();
                                title.child(drawer_button(
                                    view,
                                    palette,
                                    hit.max(layout::TOUCH_TARGET),
                                ))
                            })
                            .child(icon("hash", palette.hash_icon, palette.muted))
                            .child(
                                div()
                                    .text_size(px(palette.title_px))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(palette.text))
                                    .child(channel_name.clone()),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(2.))
                            .flex_shrink_0()
                            .items_center()
                            .when(!ink, |tools| {
                                tools.child(header_icon(
                                    this.clone(),
                                    "hdr-threads",
                                    "threads",
                                    hit,
                                    palette,
                                    |view, cx| {
                                        view.right_choice = match view.right_choice {
                                            RightChoice::Open(RightPanel::Thread) => {
                                                RightChoice::Auto
                                            }
                                            _ => RightChoice::Open(RightPanel::Thread),
                                        };
                                        cx.notify();
                                    },
                                ))
                            })
                            .child(header_icon(
                                this.clone(),
                                "hdr-pin",
                                "pin",
                                hit,
                                palette,
                                |view, cx| {
                                    view.toggle_pin_panel();
                                    cx.notify();
                                },
                            ))
                            .child(header_icon(
                                this.clone(),
                                "hdr-members",
                                "members",
                                hit,
                                palette,
                                |view, cx| {
                                    view.toggle_members();
                                    cx.notify();
                                },
                            ))
                            .child(header_icon(
                                this.clone(),
                                "hdr-search",
                                "search",
                                hit,
                                palette,
                                |view, cx| {
                                    view.shell.toggle_palette();
                                    cx.notify();
                                },
                            ))
                            .child(header_icon(
                                this.clone(),
                                "hdr-inbox",
                                "inbox",
                                hit,
                                palette,
                                |view, cx| {
                                    view.shell.inbox_open = !view.shell.inbox_open;
                                    cx.notify();
                                },
                            )),
                    ),
            );
        if !self.following_tail {
            chat = chat.child(jump_to_present(this.clone(), palette));
        }
        chat = chat.child(message_column(
            &plan,
            v_flex()
                .size_full()
                .children(self.shell.session.selected_channel.and_then(|channel| {
                    self.failed.get(&channel).cloned().map(|message| {
                        fetch_error(
                            this.clone(),
                            "retry-history",
                            "retry-history-button",
                            message,
                            palette,
                            move |view, cx| {
                                view.failed.remove(&channel);
                                if view.requested.contains(&channel) {
                                    view.scroll_back(cx);
                                }
                                cx.notify();
                            },
                        )
                    })
                }))
                .child(timeline),
        ));
        let hits = self.shell.search_hits();
        if !self.shell.search.trim().is_empty() {
            chat = chat.child(search_hits_bar(this.clone(), &hits, palette));
        }
        if let Some(id) = reply {
            chat = chat.child(hint_bar(palette, format!("Replying to {id}")));
        }
        if let Some(id) = editing {
            chat = chat.child(hint_bar(palette, format!("Editing {id}")));
        }
        chat = chat.child(
            h_flex()
                .mx(px(12.))
                .mb(px(12.))
                .px(px(4.))
                .h(px(if plan.touch {
                    layout::TOUCH_TARGET
                } else {
                    palette.composer_h
                }))
                .items_center()
                .gap(px(2.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(palette.line))
                .bg(rgb(palette.input))
                .child(header_icon(
                    this.clone(),
                    "composer-plus",
                    "plus",
                    32.,
                    palette,
                    |view, cx| {
                        let typed = view.composer.read(cx).value().to_string();
                        let typed = typed.trim();
                        if !typed.is_empty() && std::path::Path::new(typed).is_file() {
                            let _ = view.shell.flow.stage_path(std::path::Path::new(typed));
                        } else if !view.shell.flow.open {
                            view.shell.settings_open = false;
                            view.shell.flow.open_picker();
                        } else {
                            view.shell.flow.close();
                        }
                        cx.notify();
                    },
                ))
                .child(
                    div().flex_1().child(
                        Input::new(&self.composer)
                            .bg(rgb(palette.input))
                            .text_color(rgb(palette.text))
                            .border_color(rgb(palette.input)),
                    ),
                )
                .child(header_icon(
                    this.clone(),
                    "composer-gif",
                    "gif",
                    32.,
                    palette,
                    |view, cx| {
                        view.shell.note("GIF search is not in this build.");
                        cx.notify();
                    },
                ))
                .child(header_icon(
                    this.clone(),
                    "composer-emoji",
                    "emoji",
                    32.,
                    palette,
                    |view, cx| {
                        view.shell.note("The emoji picker is not in this build.");
                        cx.notify();
                    },
                ))
                .on_drop({
                    let view = this.clone();
                    move |paths: &ExternalPaths, _, cx| {
                        let Some(path) = paths.paths().first().cloned() else {
                            return;
                        };
                        view.update(cx, |window, cx| {
                            window.shell.settings_open = false;
                            let _ = window.shell.flow.stage_path(&path);
                            cx.notify();
                        });
                    }
                }),
        );
        if self.shell.prefs.stats_hud {
            chat = chat.child(
                div()
                    .px(px(12.))
                    .pb(px(8.))
                    .child(paint(
                        format!(
                            "{status}  ·  {channel_name}  ·  {theme_label}  ·  {}  ·  switch {switch_us} µs",
                            self.shell.prefs.layout.label()
                        ),
                        palette.muted,
                        12.,
                    )),
            );
        }

        let mut people = v_flex()
            .w_full()
            .h_full()
            .bg(rgb(palette.members))
            .child(section_label(
                if members.is_empty() {
                    "MEMBERS"
                } else {
                    "ONLINE"
                },
                palette.muted,
            ));
        if members.is_empty() {
            people = people.child(div().px(px(16.)).py(px(8.)).child(paint(
                "No members loaded yet.",
                palette.muted,
                13.,
            )));
        }
        for member in members {
            let (name, activity) = member
                .split_once(" · ")
                .map(|(name, activity)| (name.to_owned(), activity.to_owned()))
                .unwrap_or_else(|| (member.clone(), "Online".into()));
            let role = role_color(&name, &theme);
            let dot = match activity.as_str() {
                "Idle" => palette.status_idle,
                "Offline" => palette.status_offline,
                "In voice" => palette.status_online,
                _ => palette.status_online,
            };
            let speaking = activity == "In voice" && palette.ink_chrome;
            people = people.child(
                h_flex()
                    .mx(px(8.))
                    .px(px(6.))
                    .h(px(palette.member_row))
                    .gap(px(8.))
                    .items_center()
                    .rounded(px(6.))
                    .hover(|style| style.bg(rgb(palette.hover)))
                    .child(if speaking {
                        div()
                            .p(px(2.))
                            .rounded(px(20.))
                            .border_2()
                            .border_color(rgb(palette.accent))
                            .child(portrait_status(&name, 32., dot, palette.members))
                            .into_any_element()
                    } else {
                        portrait_status(&name, 32., dot, palette.members).into_any_element()
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .gap(px(0.))
                            .child(paint(name, role, 14.))
                            .child(meta(activity, palette.muted, 11., palette)),
                    ),
            );
        }

        let layout_kind = self.shell.prefs.layout;
        let dock_hit = if plan.touch { 40. } else { 32. };
        let body = match layout_kind {
            rencord_desktop::LayoutKind::Discord => {
                let voice_panel = self.voice_card(
                    cx,
                    palette,
                    &user_name,
                    voice.as_ref(),
                    muted,
                    deafened,
                    dock_hit,
                );
                let (chat, sidebar) = if plan.voice_in_chat {
                    (chat.child(voice_panel), sidebar)
                } else {
                    (chat, sidebar.child(voice_panel))
                };
                discord_columns(
                    cx,
                    &plan,
                    palette,
                    rail,
                    sidebar,
                    chat,
                    people,
                    voice.is_some(),
                    self.channels_drawer,
                    self.members_overlay,
                )
            }
            rencord_desktop::LayoutKind::Keyboard => v_flex()
                .size_full()
                .child(
                    h_flex()
                        .h(px(40.))
                        .px(px(12.))
                        .items_center()
                        .bg(rgb(palette.rail))
                        .child(Label::new("Quick switcher is the home. Ctrl+K. Two panes.")),
                )
                .child(
                    h_flex().flex_1().child(chat).child(
                        v_flex()
                            .w(px(280.))
                            .h_full()
                            .bg(rgb(palette.sidebar))
                            .border_l_1()
                            .border_color(rgb(palette.line))
                            .child(section_label("Split pane", palette.faint))
                            .child(Label::new(
                                channels
                                    .iter()
                                    .find(|channel| {
                                        Some(channel.id) != selected && channel.kind.is_text()
                                    })
                                    .map(|channel| format!("#{}", channel.name))
                                    .unwrap_or_else(|| "No second channel".into()),
                            ))
                            .child(self.voice_card(
                                cx,
                                palette,
                                &user_name,
                                voice.as_ref(),
                                muted,
                                deafened,
                                dock_hit,
                            )),
                    ),
                )
                .into_any_element(),
            rencord_desktop::LayoutKind::Sidebar => div()
                .relative()
                .size_full()
                .child(
                    h_flex()
                        .size_full()
                        .child(
                            v_flex()
                                .w(px(300.))
                                .h_full()
                                .bg(rgb(palette.sidebar))
                                .child(section_label("Favorites", palette.faint))
                                .child(Label::new("# general"))
                                .child(section_label("Inbox", palette.faint))
                                .child(Label::new("Mentions stay in this list"))
                                .child(h_flex().flex_1().child(rail).child(sidebar)),
                        )
                        .child(chat)
                        .child(people),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(16.))
                        .right(px(16.))
                        .w(px(280.))
                        .child(self.voice_card(
                            cx,
                            palette,
                            &user_name,
                            voice.as_ref(),
                            muted,
                            deafened,
                            dock_hit,
                        )),
                )
                .into_any_element(),
        };

        let mut layer = div().size_full().relative().child(body);
        if palette_open {
            layer = layer.child(self.palette_layer(cx, palette, &commands));
        }
        if self.shell.inbox_open {
            layer = layer.child(self.inbox_layer(cx, palette));
        }
        if self.shell.help_open {
            layer = layer.child(hint_bar(
                palette,
                "Ctrl+K palette, Ctrl+B channels, Ctrl+M members, Ctrl+, settings. Double-click a column edge to reset it.".into(),
            ));
        }
        if settings {
            self.sync_hex_fields(window, cx);
            let jitter = voice
                .as_ref()
                .map(|report| {
                    format!(
                        "Loopback played {} of {}, concealed {}, hard gaps {}, target {} ms.",
                        report.played,
                        report.sent,
                        report.concealed,
                        report.hard_gaps,
                        report.target_ms
                    )
                })
                .unwrap_or_else(|| "Join voice to see the loopback.".into());
            return settings_ui::page(
                self,
                this,
                palette,
                &user_name,
                &inputs,
                &outputs,
                chosen_in.as_deref(),
                chosen_out.as_deref(),
                &jitter,
            )
            .into_any_element();
        }
        if self.shell.flow.open {
            let flow = self.shell.flow.clone();
            layer = layer.child(big_file_view::sheet(
                this,
                &self.attachment_path,
                &flow,
                palette,
            ));
        }
        layer.into_any_element()
    }

    fn inbox_layer(&mut self, cx: &mut Context<Self>, palette: Palette) -> impl IntoElement {
        let this = cx.entity();
        let hits = self.shell.mentions();
        let mut list = v_flex().gap_1();
        for hit in hits.iter().take(8) {
            let channel_id = hit.channel_id;
            let message_id = hit.message.id;
            let label = format!(
                "#{} {}",
                hit.channel_name,
                hit.message.content.replace('\n', " ")
            );
            let view = this.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("inbox-{message_id}")))
                    .cursor_pointer()
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .bg(rgb(if self.shell.prefs.soft_ping {
                        palette.ping
                    } else {
                        palette.elevated
                    }))
                    .child(Label::new(label))
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.shell.jump_to(channel_id, message_id);
                            this.shell.inbox_open = false;
                            cx.notify();
                        });
                    }),
            );
        }
        overlay(
            palette,
            v_flex()
                .w(px(480.))
                .p(px(12.))
                .gap_2()
                .rounded(px(12.))
                .bg(rgb(palette.elevated))
                .border_1()
                .border_color(rgb(palette.line))
                .child(Label::new("Mentions"))
                .child(list)
                .child(small_button(
                    this,
                    "mark-read",
                    "Mark all read",
                    |view, cx| {
                        view.shell.mark_mentions_read();
                        cx.notify();
                    },
                )),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn voice_card(
        &mut self,
        cx: &mut Context<Self>,
        palette: Palette,
        user_name: &str,
        voice: Option<&rencord_voice::LoopbackReport>,
        muted: bool,
        deafened: bool,
        dock_hit: f32,
    ) -> impl IntoElement {
        let this = cx.entity();
        let in_call = voice.is_some();
        let status = if muted {
            "Muted"
        } else if deafened {
            "Deafened"
        } else if in_call {
            "In voice"
        } else {
            "Online"
        };
        let onyx = palette.ink_chrome;
        let dot = if muted || deafened {
            palette.status_dnd
        } else {
            palette.status_online
        };
        let mut column = v_flex().w_full().flex_shrink_0().bg(rgb(palette.sidebar));
        if let Some(report) = voice {
            let view = this.clone();
            column = column.child(
                v_flex()
                    .mx(px(palette.voice_mx))
                    .mt(px(palette.voice_mt))
                    .px(px(palette.voice_px))
                    .py(px(8.))
                    .gap(px(2.))
                    .rounded(px(palette.voice_radius))
                    .border_1()
                    .border_color(rgb(palette.voice_border))
                    .bg(rgb(palette.voice_bg))
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(palette.voice_label))
                                    .font_family(palette.font_meta)
                                    .child("Voice Connected"),
                            )
                            .child(hang_up_button(view, palette)),
                    )
                    .child(paint("voice", palette.voice_meta, 12.))
                    .child(waveform(if onyx { palette.accent } else { palette.link }))
                    .child(meta(
                        format!("{} ms  ·  0.2% plc", report.target_ms),
                        palette.muted,
                        11.,
                        palette,
                    )),
            );
        }
        column.child(
            h_flex()
                .h(px(52.))
                .pl(px(palette.dock_pl))
                .pr(px(palette.dock_pr))
                .gap(px(6.))
                .items_center()
                .border_t_1()
                .border_color(rgb(if onyx { palette.line } else { palette.sidebar }))
                .bg(rgb(palette.sidebar))
                .child(portrait_status(user_name, 32., dot, palette.sidebar))
                .child(
                    v_flex()
                        .flex_1()
                        .overflow_hidden()
                        .child(
                            div()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .text_size(px(palette.dock_name))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(palette.text))
                                .whitespace_nowrap()
                                .child(truncate_end(user_name, 14)),
                        )
                        .child(meta(status, palette.muted, 11., palette)),
                )
                .child(
                    h_flex()
                        .gap(px(1.))
                        .flex_shrink_0()
                        .child(dock_circle(
                            this.clone(),
                            "mic-btn",
                            "Mic",
                            dock_hit,
                            palette,
                            muted,
                            |view, cx| {
                                view.shell.toggle_mute();
                                view.sync_voice_flags();
                                cx.notify();
                            },
                        ))
                        .child(dock_circle(
                            this.clone(),
                            "deaf-btn",
                            "Deaf",
                            dock_hit,
                            palette,
                            deafened,
                            |view, cx| {
                                view.shell.toggle_deaf();
                                view.sync_voice_flags();
                                cx.notify();
                            },
                        ))
                        .child(dock_circle(
                            this,
                            "gear-btn",
                            "Set",
                            dock_hit,
                            palette,
                            false,
                            |view, cx| {
                                view.shell.toggle_settings();
                                view.reopen_audio();
                                cx.notify();
                            },
                        )),
                ),
        )
    }

    fn palette_layer(
        &mut self,
        cx: &mut Context<Self>,
        palette: Palette,
        commands: &[String],
    ) -> impl IntoElement {
        let this = cx.entity();
        let mut list = v_flex().gap_1().max_h(px(280.)).overflow_hidden();
        for command in commands.iter().take(12) {
            let label = command.clone();
            let view = this.clone();
            let hover = palette.hover;
            list = list.child(
                div()
                    .id(SharedString::from(format!("cmd-{label}")))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(hover)))
                    .child(Label::new(label.clone()))
                    .on_click(move |_, _, cx| {
                        let label = label.clone();
                        view.update(cx, |this, cx| {
                            this.shell.run_palette_command(&label);
                            cx.notify();
                        });
                    }),
            );
        }
        overlay(
            palette,
            v_flex()
                .w(px(460.))
                .p(px(12.))
                .gap_2()
                .rounded(px(12.))
                .bg(rgb(palette.elevated))
                .border_1()
                .border_color(rgb(palette.line))
                .child(Label::new("Quick switcher"))
                .child(Input::new(&self.palette_input))
                .child(list)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(palette.faint))
                        .child(Label::new(
                            "Enter a row. Esc closes. Alt+arrows move without this panel.",
                        )),
                )
                .with_animation(
                    "palette-in",
                    Animation::new(Duration::from_millis(MOTION_OVERLAY_MS)),
                    |card, delta| card.opacity(delta),
                ),
        )
    }
}

fn overlay(palette: Palette, child: impl IntoElement) -> impl IntoElement {
    div()
        .absolute()
        .size_full()
        .bg(rgb(palette.app_bg))
        .occlude()
        .child(
            div()
                .size_full()
                .items_center()
                .justify_center()
                .bg(rgba(palette.overlay))
                .child(child),
        )
}

fn category_header(
    view: Entity<RencordWindow>,
    key: &'static str,
    title: &'static str,
    open: bool,
    palette: Palette,
) -> impl IntoElement {
    let mark = if open { "⌄" } else { "›" };
    let onyx = palette.ink_chrome;
    div()
        .id(SharedString::from(format!("cat-{key}")))
        .px(px(8.))
        .pt(px(if onyx { 8. } else { 14. }))
        .pb(px(2.))
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                if !this.collapsed.remove(key) {
                    this.collapsed.insert(key);
                }
                cx.notify();
            });
        })
        .child(
            h_flex()
                .gap(px(4.))
                .items_center()
                .child(paint(mark, palette.muted, if onyx { 11. } else { 12. }))
                .child(paint(title, palette.muted, if onyx { 11. } else { 12. })),
        )
}

fn section_label(text: &str, color: u32) -> impl IntoElement {
    div()
        .px(px(12.))
        .pt(px(10.))
        .text_size(px(11.))
        .text_color(rgb(color))
        .child(paint(text.to_uppercase(), color, 12.))
}

#[allow(clippy::too_many_arguments)]
fn channel_row(
    view: Entity<RencordWindow>,
    id: Snowflake,
    label: String,
    active: bool,
    palette: Palette,
    badge: Option<u32>,
    joins_voice: bool,
    row_px: f32,
    live_dot: bool,
) -> impl IntoElement {
    let hover = palette.hover;
    let bg = if active {
        palette.selected
    } else {
        palette.sidebar
    };
    let (kind, title) = if let Some(name) = label.strip_prefix("# ") {
        ("hash", name.to_owned())
    } else if let Some(name) = label.strip_prefix(")) ") {
        ("speaker", name.to_owned())
    } else if let Some(name) = label.strip_prefix("* ") {
        ("star", name.to_owned())
    } else if let Some(name) = label.strip_prefix("dm ") {
        ("dm", name.to_owned())
    } else {
        ("hash", label.clone())
    };
    let onyx = palette.ink_chrome;
    let color = if active {
        palette.text
    } else if onyx {
        palette.text_secondary
    } else {
        palette.muted
    };
    let icon_color = if active && onyx && kind != "speaker" && kind != "dm" {
        palette.accent
    } else {
        color
    };
    let label_px = palette.channel_label;
    let glyph_px = palette.channel_icon;
    let height = if kind == "dm" { 42. } else { row_px.max(32.) };
    let (badge_bg, badge_fg) = (palette.badge_bg, palette.badge_fg);
    div()
        .id(SharedString::from(format!("channel-{id}")))
        .flex_shrink_0()
        .mx(px(8.))
        .px(px(8.))
        .h(px(height))
        .rounded(px(4.))
        .overflow_hidden()
        .bg(rgb(bg))
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(if active { bg } else { hover })))
        .child(
            h_flex()
                .size_full()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(if kind == "dm" {
                            portrait_status(
                                &title,
                                palette.dm_avatar,
                                palette.status_online,
                                palette.sidebar,
                            )
                            .into_any_element()
                        } else {
                            icon(
                                if kind == "speaker" { "speaker" } else { "hash" },
                                glyph_px,
                                icon_color,
                            )
                            .into_any_element()
                        })
                        .child(paint(title.clone(), color, label_px)),
                )
                .children(if kind == "star" {
                    Some(
                        icon(
                            "star",
                            12.,
                            if onyx { palette.accent } else { palette.muted },
                        )
                        .into_any_element(),
                    )
                } else if live_dot {
                    Some(
                        div()
                            .size(px(6.))
                            .rounded(px(3.))
                            .bg(rgb(palette.status_online))
                            .into_any_element(),
                    )
                } else {
                    badge.map(|count| {
                        h_flex()
                            .min_w(px(16.))
                            .h(px(16.))
                            .px(px(4.))
                            .rounded(px(8.))
                            .items_center()
                            .justify_center()
                            .bg(rgb(badge_bg))
                            .child(paint(count.to_string(), badge_fg, 10.))
                            .into_any_element()
                    })
                }),
        )
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                if joins_voice {
                    this.join_voice(cx);
                } else {
                    this.shell.select_channel_timed(id);
                }
                cx.notify();
            });
        })
}

fn hint_bar(palette: Palette, text: String) -> impl IntoElement {
    div()
        .px(px(12.))
        .py(px(4.))
        .text_size(px(12.))
        .text_color(rgb(palette.muted))
        .bg(rgb(palette.elevated))
        .child(Label::new(text))
}

fn hang_up_button(view: Entity<RencordWindow>, palette: Palette) -> impl IntoElement {
    h_flex()
        .id("voice-leave")
        .size(px(28.))
        .flex_shrink_0()
        .rounded(px(14.))
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(rgba(palette.hangup_bg))
        .child(icon("phone", 14., palette.hangup_fg))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                this.leave_voice_call();
                cx.notify();
            });
        })
}

fn dock_circle(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    hit: f32,
    palette: Palette,
    active: bool,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    let glyph = match (label, active) {
        ("Mic", true) => "mic-off",
        ("Mic", false) => "mic",
        ("Deaf", true) => "deafen-off",
        ("Deaf", false) => "deafen",
        ("Set", _) => "settings",
        _ => "plus",
    };
    let ink = if active && label != "Set" {
        palette.danger
    } else {
        palette.text
    };
    h_flex()
        .id(id)
        .size(px(hit))
        .flex_shrink_0()
        .rounded(px(6.))
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(palette.hover)))
        .child(icon(glyph, 16., ink))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| action(this, cx));
        })
}

fn fetch_error(
    view: Entity<RencordWindow>,
    row_id: &'static str,
    button_id: &'static str,
    message: String,
    palette: Palette,
    retry: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    h_flex()
        .id(row_id)
        .px(px(12.))
        .py(px(6.))
        .gap(px(8.))
        .items_center()
        .bg(rgb(palette.elevated))
        .text_size(px(12.))
        .text_color(rgb(palette.danger))
        .child(div().flex_1().child(message))
        .child(small_button(view, button_id, "Retry", retry))
}

fn small_button(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    Button::new(id).label(label).on_click(move |_, _, cx| {
        view.update(cx, |this, cx| action(this, cx));
    })
}

fn channel_title(
    selected: Option<Snowflake>,
    channels: &[rencord_model::Channel],
    dms: &[rencord_model::Channel],
) -> String {
    channels
        .iter()
        .chain(dms.iter())
        .find(|channel| Some(channel.id) == selected)
        .map(|channel| channel.name.clone())
        .unwrap_or_else(|| "Select a channel".into())
}

fn rail_rule(width: f32, color: u32) -> impl IntoElement {
    h_flex()
        .w(px(width))
        .h(px(8.))
        .items_center()
        .justify_center()
        .child(div().w(px(28.)).h(px(1.)).bg(rgb(color)))
}

#[derive(Clone, Copy)]
enum Pill {
    None,
    Unread,
    Selected,
}

fn tile_initial(letter: &str, color: u32) -> impl IntoElement {
    div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(16.))
        .line_height(px(16.))
        .text_color(rgb(color))
        .child(letter.to_owned())
}

#[allow(clippy::too_many_arguments)]
fn rail_slot(
    id: SharedString,
    _active: bool,
    pill: Pill,
    fill: u32,
    hover: u32,
    pill_ink: u32,
    border: u32,
    rail_w: f32,
    tile: f32,
    radius: f32,
    mark: impl IntoElement,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (pill_w, pill_h) = match pill {
        Pill::Selected => (4., 22.),
        Pill::Unread => (8., 8.),
        Pill::None => (0., 0.),
    };
    let mut slot = h_flex()
        .relative()
        .w(px(rail_w))
        .h(px(tile))
        .flex_shrink_0()
        .items_center()
        .justify_center();
    if pill_h > 0. {
        slot = slot.child(
            div()
                .absolute()
                .left(px(2.))
                .top(px((tile - pill_h) / 2.))
                .w(px(pill_w))
                .h(px(pill_h))
                .rounded(px(pill_h / 2.))
                .bg(rgb(pill_ink)),
        );
    }
    slot.child(
        h_flex()
            .id(id)
            .size(px(tile))
            .flex_shrink_0()
            .rounded(px(radius))
            .items_center()
            .justify_center()
            .bg(rgb(fill))
            .border_1()
            .border_color(rgb(border))
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(hover)))
            .child(mark)
            .on_click(on_click),
    )
}

#[allow(clippy::too_many_arguments)]
fn rail_action(
    view: Entity<RencordWindow>,
    id: &'static str,
    glyph: &'static str,
    dashed: bool,
    palette: Palette,
    rail_w: f32,
    tile: f32,
    radius: f32,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    let tile_el = h_flex()
        .size(px(tile))
        .flex_shrink_0()
        .rounded(px(radius))
        .items_center()
        .justify_center()
        .bg(rgb(palette.add_bg))
        .border_1()
        .border_color(rgb(palette.add_border));
    let tile_el = if dashed {
        tile_el.border_dashed()
    } else {
        tile_el
    };
    let tile_el = tile_el
        .id(id)
        .cursor_pointer()
        .hover(|style| style.border_color(rgb(palette.text)))
        .child(icon(glyph, 18., palette.add_glyph))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| action(this, cx));
        });
    h_flex()
        .w(px(rail_w))
        .h(px(tile))
        .items_center()
        .justify_center()
        .child(tile_el)
}

fn drawer_button(view: Entity<RencordWindow>, palette: Palette, hit: f32) -> impl IntoElement {
    let bar = div()
        .w(px(16.))
        .h(px(2.))
        .rounded(px(1.))
        .bg(rgb(palette.text));
    v_flex()
        .id("open-channels")
        .size(px(hit))
        .flex_shrink_0()
        .rounded(px(6.))
        .items_center()
        .justify_center()
        .gap(px(3.))
        .cursor_pointer()
        .border_1()
        .border_color(rgb(palette.line))
        .hover(move |style| style.bg(rgb(palette.hover)))
        .child(bar)
        .child(
            div()
                .w(px(16.))
                .h(px(2.))
                .rounded(px(1.))
                .bg(rgb(palette.text)),
        )
        .child(
            div()
                .w(px(16.))
                .h(px(2.))
                .rounded(px(1.))
                .bg(rgb(palette.text)),
        )
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                this.toggle_channels();
                cx.notify();
            });
        })
}

fn header_icon(
    view: Entity<RencordWindow>,
    id: &'static str,
    glyph: &'static str,
    hit: f32,
    palette: Palette,
    action: impl Fn(&mut RencordWindow, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    let hover = palette.hover;
    div()
        .id(id)
        .size(px(hit))
        .rounded(px(4.))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon(glyph, 20., palette.muted))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| action(this, cx));
        })
}

fn guild_mark(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .take(1)
        .collect::<String>()
        .to_uppercase()
}

struct TimeTip {
    text: String,
    bg: u32,
    fg: u32,
    line: u32,
}

impl Render for TimeTip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(rgb(self.bg))
            .border_1()
            .border_color(rgb(self.line))
            .text_size(px(12.))
            .text_color(rgb(self.fg))
            .child(self.text.clone())
    }
}

fn timestamp_label(
    message_id: u64,
    stamp: &rencord_desktop::Presented,
    reveal: bool,
    palette: Palette,
) -> impl IntoElement {
    let full = stamp.full.clone();
    let bg = palette.elevated;
    let fg = palette.text;
    let line = palette.line;
    let mut label = h_flex()
        .id(SharedString::from(format!("stamp-{message_id}")))
        .items_center()
        .gap(px(6.))
        .font_family(palette.font_ui)
        .text_size(px(11.))
        .text_color(rgb(palette.muted))
        .child(stamp.short.clone());
    if full != stamp.short {
        let hover = full.clone();
        label = label.tooltip(move |_window, cx| {
            cx.new(|_| TimeTip {
                text: hover.clone(),
                bg,
                fg,
                line,
            })
            .into()
        });
    }
    if reveal && full != stamp.short {
        label = label.child(
            div()
                .ml(px(8.))
                .px(px(8.))
                .py(px(2.))
                .rounded(px(6.))
                .bg(rgb(bg))
                .border_1()
                .border_color(rgb(line))
                .text_size(px(12.))
                .text_color(rgb(fg))
                .child(full),
        );
    }
    label
}

#[allow(clippy::too_many_arguments)]
fn message_row(
    view: Entity<RencordWindow>,
    message: &Message,
    grouped: bool,
    reactions: Vec<rencord_desktop::Reaction>,
    palette: Palette,
    theme: &rencord_desktop::Theme,
    font_px: f32,
    pin_actions: bool,
    stamp: &rencord_desktop::Presented,
    reveal_time: bool,
) -> impl IntoElement {
    let id = message.id;
    let onyx = palette.ink_chrome;
    let name = message.author.display_name().to_string();
    let role = role_color(&name, theme);
    let body_source = message.content.clone();
    let (reply_note, body_source) = if let Some(rest) = body_source.strip_prefix("replying to ") {
        let (target, text) = rest.split_once(':').unwrap_or(("a message", rest));
        let who = match target.trim() {
            "1000" | "1" => "Ada",
            "2" => "Charles",
            "3" => "Lovelace",
            _ => "a message",
        };
        (Some(format!("Replying to {who}")), text.trim().to_owned())
    } else {
        (None, body_source)
    };
    let body_source = body_source
        .lines()
        .filter(|line| !line.contains("attachment:"))
        .collect::<Vec<_>>()
        .join("\n");
    let group = SharedString::from(format!("row-{id}"));
    let hover = palette.hover;
    let mut body = v_flex().flex_1().min_w(px(0.)).gap_1();
    if !grouped {
        body = body.child(
            h_flex()
                .gap_2()
                .items_baseline()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(role))
                        .child(name.clone()),
                )
                .child(timestamp_label(
                    message.id.get(),
                    stamp,
                    reveal_time,
                    palette,
                )),
        );
    }
    if let Some(note) = reply_note {
        body = body.child(
            h_flex()
                .gap(px(6.))
                .items_center()
                .child(icon("reply", 14., palette.muted))
                .child(paint(note, palette.muted, 12.)),
        );
    }
    body = body.child(message_blocks(&body_source, id, palette, font_px));
    if !reactions.is_empty() {
        let mut pills = h_flex().gap_1().mt(px(2.));
        for reaction in &reactions {
            pills = pills.child(
                div()
                    .px(px(6.))
                    .h(px(24.))
                    .rounded(px(8.))
                    .items_center()
                    .bg(rgb(if reaction.mine {
                        palette.ping
                    } else {
                        palette.elevated
                    }))
                    .border_1()
                    .border_color(rgb(if reaction.mine {
                        palette.accent
                    } else {
                        palette.line
                    }))
                    .text_size(px(13.))
                    .text_color(rgb(palette.text))
                    .child(format!(
                        "{} {}",
                        if reaction.emoji == "wave" {
                            "👋"
                        } else {
                            reaction.emoji.as_str()
                        },
                        reaction.count
                    )),
            );
        }
        body = body.child(pills);
    }
    if message.content.contains("](") {
        let embed = v_flex()
            .gap(px(2.))
            .child(paint("example.com", palette.muted, 11.))
            .child(paint("Jitter notes", palette.link, 14.))
            .child(paint(
                "How the adaptive buffer stays under the idle budget.",
                if onyx {
                    palette.text_secondary
                } else {
                    palette.text
                },
                13.,
            ));
        body = body.child(if onyx {
            div()
                .mt(px(7.))
                .max_w(px(380.))
                .rounded(px(6.))
                .bg(rgb(palette.chat))
                .border_1()
                .border_color(rgb(palette.line))
                .child(
                    div()
                        .border_l_3()
                        .border_color(rgb(palette.accent))
                        .px(px(14.))
                        .py(px(8.))
                        .child(embed),
                )
                .into_any_element()
        } else {
            v_flex()
                .mt(px(4.))
                .max_w(px(432.))
                .px(px(16.))
                .py(px(8.))
                .rounded(px(12.))
                .bg(rgb(palette.elevated))
                .border_l_4()
                .border_color(rgb(palette.link))
                .child(embed)
                .into_any_element()
        });
    }
    if message.content.contains("rencord-split v1") {
        body = body.child(big_file_view::reassemble_chip(
            view.clone(),
            message.content.clone(),
            palette,
        ));
    }
    if message.content.contains("attachment:bench.png") {
        body = body.child(
            v_flex()
                .mt(px(7.))
                .gap_1()
                .child(
                    div()
                        .w(px(320.))
                        .h(px(76.))
                        .rounded(px(if onyx { 6. } else { 12. }))
                        .overflow_hidden()
                        .border_1()
                        .border_color(rgb(palette.line))
                        .child(bench_photo()),
                )
                .child(if onyx {
                    meta("voice-bench.jpg", palette.muted, 11., palette).into_any_element()
                } else {
                    paint("voice-bench.jpg", palette.muted, 12.).into_any_element()
                }),
        );
    }
    let avatar = if onyx { 32. } else { 40. };
    h_flex()
        .id(group.clone())
        .group(group.clone())
        .relative()
        .w_full()
        .px(px(16.))
        .py(px(if grouped {
            2.
        } else if onyx {
            4.
        } else {
            8.
        }))
        .border_l_3()
        .border_color(rgb(if message.content.contains("Mention") {
            palette.accent
        } else {
            palette.chat
        }))
        .bg(rgb(if message.content.contains("Mention") {
            palette.ping
        } else {
            palette.chat
        }))
        .gap(px(if onyx { 10. } else { 16. }))
        .items_start()
        .hover(move |style| style.bg(rgb(hover)))
        .child(if grouped {
            div().w(px(avatar)).h(px(8.)).into_any_element()
        } else {
            portrait(&name, avatar).into_any_element()
        })
        .child(body)
        .child(
            h_flex()
                .absolute()
                .top(px(-12.))
                .right(px(12.))
                .gap_1()
                .px(px(4.))
                .h(px(32.))
                .items_center()
                .rounded(px(8.))
                .bg(rgb(palette.elevated))
                .border_1()
                .border_color(rgb(palette.hover_border))
                .opacity(if pin_actions { 1. } else { 0. })
                .group_hover(group, |style| style.opacity(1.))
                .child(react_action(view.clone(), id, palette))
                .child(icon_action(
                    view.clone(),
                    format!("reply-{id}"),
                    "Reply",
                    id,
                    true,
                    palette,
                ))
                .child(link_action(view.clone(), id, palette))
                .child(div().w(px(1.)).h(px(16.)).bg(rgb(palette.hover_divider)))
                .child(delete_action(view, id, palette)),
        )
}

fn react_action(view: Entity<RencordWindow>, id: Snowflake, palette: Palette) -> impl IntoElement {
    let hover = palette.hover;
    div()
        .id(SharedString::from(format!("react-bar-{id}")))
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon("emoji", 16., palette.muted))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                let mutation = this.shell.toggle_reaction(id.get(), "wave");
                this.dispatch_mutation(mutation, cx);
                cx.notify();
            });
        })
}

fn link_action(view: Entity<RencordWindow>, id: Snowflake, palette: Palette) -> impl IntoElement {
    let hover = palette.hover;
    div()
        .id(SharedString::from(format!("link-{id}")))
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon("link", 16., palette.muted))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                this.shell.note(format!("Message {id} link stays local."));
                cx.notify();
            });
        })
}

fn icon_action(
    view: Entity<RencordWindow>,
    element_id: String,
    label: &'static str,
    id: Snowflake,
    reply: bool,
    palette: Palette,
) -> impl IntoElement {
    let glyph = if reply { "reply" } else { "pencil" };
    let hover = palette.hover;
    div()
        .id(SharedString::from(element_id))
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .aria_label(label)
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon(glyph, 15., palette.muted))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                if reply {
                    this.shell.begin_reply(id);
                } else {
                    this.shell.begin_edit(id);
                }
                cx.notify();
            });
        })
}

fn role_color(name: &str, theme: &rencord_desktop::Theme) -> u32 {
    rencord_desktop::role_color(name, theme).to_u32()
}

fn delete_action(view: Entity<RencordWindow>, id: Snowflake, palette: Palette) -> impl IntoElement {
    let hover = palette.hover;
    div()
        .id(SharedString::from(format!("delete-{id}")))
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .aria_label("Delete")
        .hover(move |style| style.bg(rgb(hover)))
        .child(icon("trash", 15., palette.text_danger))
        .on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                if let Some(message) = this.shell.delete_local(id) {
                    this.dispatch_mutation(rencord_desktop::Mutation::Delete { message }, cx);
                }
                cx.notify();
            });
        })
}

fn message_blocks(
    content: &str,
    id: Snowflake,
    palette: Palette,
    font_px: f32,
) -> impl IntoElement {
    let blocks = parse(content);
    let mut column = v_flex().gap_1();
    if blocks.is_empty() {
        return column.child(markdown_line(content, palette, font_px));
    }
    for (index, block) in blocks.into_iter().enumerate() {
        match block {
            Block::Code { text, language } => {
                column = column.child(code_block(id, index, text, language, palette, font_px));
            }
            Block::Paragraph { inlines }
            | Block::Heading { inlines, .. }
            | Block::Quote { inlines } => {
                column = column.child(rich_inlines(&inlines, palette, font_px));
            }
        }
    }
    column
}

fn code_block(
    id: Snowflake,
    index: usize,
    text: String,
    language: Option<String>,
    palette: Palette,
    font_px: f32,
) -> impl IntoElement {
    let copy_text = text.clone();
    let copy_group = SharedString::from(format!("code-copy-{id}-{index}"));
    let copy_hover = copy_group.clone();
    let onyx = palette.ink_chrome;
    let mut source: Vec<&str> = text.split('\n').collect();
    if source.last().is_some_and(|line| line.is_empty()) {
        source.pop();
    }
    let mut lines = v_flex().w_full().overflow_hidden();
    for (line_index, line) in source.iter().enumerate() {
        lines = lines.child(
            div()
                .id(SharedString::from(format!(
                    "code-{id}-{index}-{line_index}"
                )))
                .whitespace_nowrap()
                .font_family("JetBrains Mono")
                .text_size(px(if onyx {
                    12.5
                } else {
                    (font_px - 1.0).max(12.0)
                }))
                .line_height(px(if onyx { 16. } else { 22. }))
                .h(px(if onyx { 16. } else { 22. }))
                .overflow_hidden()
                .child(highlight_code_line(line, palette)),
        );
    }
    v_flex()
        .group(copy_group)
        .w_full()
        .bg(rgb(palette.code_bg))
        .rounded(px(if onyx { 6. } else { 12. }))
        .border_1()
        .border_color(rgb(palette.line))
        .overflow_hidden()
        .when(!onyx, |block| block.p(px(16.)).gap_1())
        .child(
            h_flex()
                .h(px(if onyx { 22. } else { 24. }))
                .px(px(if onyx { 10. } else { 0. }))
                .justify_between()
                .items_center()
                .when(onyx, |bar| bar.border_b_1().border_color(rgb(palette.line)))
                .child(meta(
                    language.unwrap_or_else(|| "text".into()),
                    palette.muted,
                    11.,
                    palette,
                ))
                .child(
                    h_flex()
                        .id(SharedString::from(format!("copy-{id}-{index}")))
                        .size(px(28.))
                        .rounded(px(6.))
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .opacity(0.)
                        .group_hover(copy_hover, |style| style.opacity(1.))
                        .hover(|style| style.bg(rgb(palette.hover)))
                        .child(icon("copy", 16., palette.muted))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                        }),
                ),
        )
        .child(
            div()
                .px(px(if onyx { 12. } else { 0. }))
                .py(px(if onyx { 7. } else { 0. }))
                .child(lines),
        )
}

fn highlight_code_line(line: &str, palette: Palette) -> StyledText {
    const KEYWORDS: &[&str] = &[
        "fn", "let", "mut", "pub", "struct", "impl", "return", "if", "else", "for", "while", "use",
        "mod", "match", "self",
    ];
    let mut text = String::new();
    let mut runs = Vec::new();
    let mut rest = line;
    if rest.is_empty() {
        rest = " ";
    }
    while !rest.is_empty() {
        let (token, next) = split_token(rest);
        let keyword = KEYWORDS.contains(&token);
        let numeric = !token.is_empty() && token.chars().all(|ch| ch.is_ascii_digit());
        text.push_str(token);
        let mut font = font(palette.font_mono);
        if keyword {
            font.weight = FontWeight::BOLD;
        }
        let color = if keyword {
            palette.syntax_keyword
        } else if numeric {
            palette.syntax_number
        } else {
            palette.text
        };
        runs.push(TextRun {
            len: token.len(),
            font,
            color: rgb(color).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
        rest = next;
    }
    StyledText::new(text).with_runs(runs)
}

fn split_token(input: &str) -> (&str, &str) {
    let mut end = 0;
    for (index, ch) in input.char_indices() {
        if index == 0 {
            end = ch.len_utf8();
            if ch.is_ascii_alphanumeric() || ch == '_' {
                continue;
            }
            break;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    input.split_at(end.max(1).min(input.len()))
}

fn search_hits_bar(
    view: Entity<RencordWindow>,
    hits: &[rencord_desktop::SearchHit],
    palette: Palette,
) -> impl IntoElement {
    let mut column = v_flex()
        .px(px(12.))
        .py(px(6.))
        .gap_1()
        .bg(rgb(palette.elevated))
        .child(Label::new(format!(
            "{} local hits. Filters: from: in: has: before: after:",
            hits.len()
        )));
    for hit in hits.iter().take(6) {
        let channel_id = hit.channel_id;
        let message_id = hit.message.id;
        let label = format!(
            "#{}  {}",
            hit.channel_name,
            hit.message.content.replace('\n', " ")
        );
        let row = view.clone();
        column = column.child(
            div()
                .id(SharedString::from(format!("hit-{message_id}")))
                .cursor_pointer()
                .px(px(6.))
                .py(px(2.))
                .rounded(px(6.))
                .hover(|style| style.bg(rgb(palette.hover)))
                .child(Label::new(label))
                .on_click(move |_, _, cx| {
                    row.update(cx, |this, cx| {
                        this.shell.jump_to(channel_id, message_id);
                        this.shell.search.clear();
                        cx.notify();
                    });
                }),
        );
    }
    column
}

fn markdown_line(content: &str, palette: Palette, font_px: f32) -> impl IntoElement {
    let blocks = parse(content);
    let mut text = String::new();
    let mut runs = Vec::new();
    {
        let base = font(".SystemUIFont");
        let mut push = |chunk: &str, color: u32, weight: FontWeight| {
            if chunk.is_empty() {
                return;
            }
            text.push_str(chunk);
            let mut font = base.clone();
            font.weight = weight;
            runs.push(TextRun {
                len: chunk.len(),
                font,
                color: rgb(color).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        };
        if blocks.is_empty() {
            push(content, palette.text, FontWeight::NORMAL);
        }
        for block in &blocks {
            match block {
                Block::Paragraph { inlines }
                | Block::Heading { inlines, .. }
                | Block::Quote { inlines } => {
                    flatten_inlines(inlines, palette, &mut push);
                }
                Block::Code { text: body, .. } => {
                    push(body, palette.link, FontWeight::NORMAL);
                }
            }
            push(" ", palette.text, FontWeight::NORMAL);
        }
    }
    if text.is_empty() {
        text.push(' ');
        let mut font = font(".SystemUIFont");
        font.weight = FontWeight::NORMAL;
        runs.push(TextRun {
            len: 1,
            font,
            color: rgb(palette.text).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    div()
        .text_size(px(font_px))
        .overflow_hidden()
        .child(StyledText::new(text).with_runs(runs))
}

fn flatten_inlines(
    inlines: &[Inline],
    palette: Palette,
    push: &mut impl FnMut(&str, u32, FontWeight),
) {
    for inline in inlines {
        match inline {
            Inline::Text(value) => push(value, palette.text, FontWeight::NORMAL),
            Inline::Bold(inner) => {
                let mut flat = String::new();
                collect_plain(inner, &mut flat);
                push(&flat, palette.text, FontWeight::BOLD);
            }
            Inline::Italic(inner) => {
                let mut flat = String::new();
                collect_plain(inner, &mut flat);
                push(&flat, palette.muted, FontWeight::NORMAL);
            }
            Inline::Code(value) => push(value, palette.link, FontWeight::NORMAL),
            Inline::Link { text, .. } => push(text, palette.link, FontWeight::NORMAL),
            Inline::Mention { id } => push(&format!("@{id}"), palette.accent, FontWeight::BOLD),
            Inline::Spoiler(inner) => {
                let mut flat = String::new();
                collect_plain(inner, &mut flat);
                push(&flat, palette.faint, FontWeight::NORMAL);
            }
        }
    }
}

fn collect_plain(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(value) | Inline::Code(value) => out.push_str(value),
            Inline::Bold(inner) | Inline::Italic(inner) | Inline::Spoiler(inner) => {
                collect_plain(inner, out)
            }
            Inline::Link { text, .. } => out.push_str(text),
            Inline::Mention { id } => {
                out.push('@');
                out.push_str(id);
            }
        }
    }
}

fn apply_kit_theme(palette: &Palette, cx: &mut App) {
    let mode = if palette.light { "light" } else { "dark" };
    let hex = |color: u32| format!("#{color:06X}");
    let value = serde_json::json!({
        "name": "Rencord",
        "mode": mode,
            "font.family": palette.font_ui,
        "font.size": 15,
        "colors": {
            "background": hex(palette.chat),
            "foreground": hex(palette.text),
            "muted.foreground": hex(palette.muted),
            "muted.background": hex(palette.sidebar),
            "border": hex(palette.line),
            "input": hex(palette.input),
            "primary": hex(palette.accent),
            "primary.foreground": hex(palette.accent_text),
            "accent.background": hex(palette.hover),
            "accent.foreground": hex(palette.text),
            "button.background": hex(palette.elevated),
            "button.foreground": hex(palette.text),
        }
    });
    if let Ok(config) = serde_json::from_value::<gpui_kit::component::theme::ThemeConfig>(value) {
        gpui_kit::component::theme::Theme::global_mut(cx).apply_config(&std::rc::Rc::new(config));
    }
}

fn window_icon() -> std::sync::Arc<image::RgbaImage> {
    // Larger than 64 px is stored as an empty `_NET_WM_ICON` by this GPUI build.
    let bytes = include_bytes!("../../../assets/brand/linux/hexwave-64.png");
    let icon = image::load_from_memory(bytes)
        .expect("Hexwave window icon")
        .into_rgba8();
    std::sync::Arc::new(icon)
}

/// Smooth Hexwave from `hexwave-mark.svg`. Two SVG layers share the viewBox:
/// the knockout body in `ink`, and the third bar in `accent`. GPUI anti-aliases
/// each layer.
fn rail_hexwave(ink: u32, accent: u32) -> impl IntoElement {
    let _ = accent;
    hexwave_mark(32., ink, 1.)
}

fn hexwave_mark(size: f32, ink: u32, scale: f32) -> impl IntoElement {
    let png = rencord_desktop::hexwave_png(ink, size, scale);
    let image = Image::from_bytes(ImageFormat::Png, png);
    img(std::sync::Arc::new(image))
        .size(px(size))
        .flex_shrink_0()
}

fn icon(name: &str, size: f32, color: u32) -> Svg {
    let raw = match name {
        "hash" => include_str!("../../../docs/design/icons/hash.svg"),
        "speaker" => include_str!("../../../docs/design/icons/speaker.svg"),
        "mic" => include_str!("../../../docs/design/icons/mic.svg"),
        "deafen" => include_str!("../../../docs/design/icons/deafen.svg"),
        "settings" => include_str!("../../../docs/design/icons/settings.svg"),
        "plus" => include_str!("../../../docs/design/icons/plus.svg"),
        "emoji" => include_str!("../../../docs/design/icons/emoji.svg"),
        "gif" => include_str!("../../../docs/design/icons/gif.svg"),
        "pin" => include_str!("../../../docs/design/icons/pin.svg"),
        "inbox" => include_str!("../../../docs/design/icons/inbox.svg"),
        "search" => include_str!("../../../docs/design/icons/search.svg"),
        "reply" => include_str!("../../../docs/design/icons/reply.svg"),
        "pencil" => include_str!("../../../docs/design/icons/pencil.svg"),
        "trash" => include_str!("../../../docs/design/icons/trash.svg"),
        "threads" => include_str!("../../../docs/design/icons/threads.svg"),
        "members" => include_str!("../../../docs/design/icons/members.svg"),
        "home" => include_str!("../../../docs/design/icons/home.svg"),
        "compass" => include_str!("../../../docs/design/icons/compass.svg"),
        "phone" => include_str!("../../../docs/design/icons/phone.svg"),
        "send" => include_str!("../../../docs/design/icons/send.svg"),
        "copy" => include_str!("../../../docs/design/icons/copy.svg"),
        "link" => include_str!("../../../docs/design/icons/link.svg"),
        "compress" => include_str!("../../../docs/design/icons/compress.svg"),
        "split" => include_str!("../../../docs/design/icons/split.svg"),
        "star" => include_str!("../../../docs/design/icons/star.svg"),
        "mic-off" => include_str!("../../../docs/design/icons/mic-off.svg"),
        "deafen-off" => include_str!("../../../docs/design/icons/deafen-off.svg"),
        _ => include_str!("../../../docs/design/icons/hash.svg"),
    };
    let colored = raw.replace("currentColor", &format!("#{color:06X}"));
    svg()
        .data(colored.as_bytes())
        .size(px(size))
        .text_color(rgb(color))
}

fn portrait(name: &str, size: f32) -> impl IntoElement {
    let bytes: &'static [u8] = match name {
        "Charles" => include_bytes!("../assets/avatars/charles.png"),
        "Lovelace" => include_bytes!("../assets/avatars/lovelace.png"),
        _ => include_bytes!("../assets/avatars/ada.png"),
    };
    let image = Image::from_bytes(ImageFormat::Png, bytes.to_vec());
    // The image paints its own corners. A parent radius does not clip it.
    img(std::sync::Arc::new(image))
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(size / 2.))
        .object_fit(ObjectFit::Cover)
}

fn portrait_status(name: &str, size: f32, dot: u32, ring: u32) -> impl IntoElement {
    div()
        .relative()
        .size(px(size))
        .flex_shrink_0()
        .child(portrait(name, size))
        .child(
            div()
                .absolute()
                .bottom(px(-1.))
                .right(px(-1.))
                .size(px(10.))
                .rounded(px(5.))
                .bg(rgb(dot))
                .border_2()
                .border_color(rgb(ring)),
        )
}

fn message_column(plan: &ResolvedLayout, timeline: impl IntoElement) -> AnyElement {
    if let Some(reading) = plan.reading_width {
        let text = div()
            .h_full()
            .w_full()
            .max_w(px(reading + layout::MESSAGE_GUTTER))
            .overflow_hidden()
            .child(timeline);
        if plan.reading_anchor == ReadingAnchor::Center {
            return h_flex()
                .flex_1()
                .w_full()
                .min_h(px(0.))
                .justify_center()
                .child(text)
                .into_any_element();
        }
        return div()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .overflow_hidden()
            .child(text)
            .into_any_element();
    }
    div()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .overflow_hidden()
        .child(timeline)
        .into_any_element()
}

fn column_gap(gap: f32) -> gpui_kit::Div {
    div().w(px(gap)).h_full().flex_shrink_0()
}

fn column_splitter(
    id: &'static str,
    edge: SplitEdge,
    start_width: f32,
    gap: f32,
    palette: Palette,
    view: Entity<RencordWindow>,
) -> impl IntoElement {
    let drag = SplitDrag {
        edge,
        start_x: Rc::new(Cell::new(f32::NAN)),
        start_width,
    };
    let hover = palette.line;
    div()
        .id(id)
        .w(px(gap))
        .h_full()
        .flex_shrink_0()
        .cursor_col_resize()
        .hover(move |style| style.bg(rgb(hover)))
        .on_mouse_down(MouseButton::Left, {
            let view = view.clone();
            move |event: &MouseDownEvent, _, cx| {
                view.update(cx, |this, cx| {
                    if event.click_count >= 2 {
                        this.reset_column(edge);
                        this.suppress_split = Some(edge);
                    } else {
                        this.suppress_split = None;
                    }
                    cx.notify();
                });
            }
        })
        .on_drag(drag, |drag, point, _window, cx| {
            drag.start_x.set(point.x.as_f32());
            cx.new(|_| SplitGhost)
        })
        .on_drag_move({
            let view = view.clone();
            move |event: &DragMoveEvent<SplitDrag>, _window, cx| {
                let drag = event.drag(cx).clone();
                let start = drag.start_x.get();
                if !start.is_finite() {
                    return;
                }
                let delta = event.event.position.x.as_f32() - start;
                let proposed = match drag.edge {
                    SplitEdge::Right => drag.start_width - delta,
                    _ => drag.start_width + delta,
                };
                view.update(cx, |this, cx| {
                    this.resize_column(drag.edge, proposed);
                    cx.notify();
                });
            }
        })
}

fn right_region(kind: RightPanel, palette: Palette) -> impl IntoElement {
    let mut column = v_flex().size_full().bg(rgb(palette.members));
    column = column.child(section_label(kind.title(), palette.muted));
    match kind {
        RightPanel::Stream => {
            column.child(div().flex_1().min_h(px(0.)).child(golive::viewer(palette)))
        }
        RightPanel::Pinned => column.child(div().px(px(12.)).child(paint(
            "No pinned messages.",
            palette.muted,
            13.,
        ))),
        RightPanel::Thread => column.child(div().px(px(12.)).child(paint(
            "Threads are not in this build.",
            palette.muted,
            13.,
        ))),
        RightPanel::Profile => column.child(div().px(px(12.)).child(paint(
            "Member profile.",
            palette.muted,
            13.,
        ))),
    }
}

#[allow(clippy::too_many_arguments)]
fn discord_columns(
    cx: &mut Context<RencordWindow>,
    plan: &ResolvedLayout,
    palette: Palette,
    rail: impl IntoElement,
    sidebar: impl IntoElement,
    chat: impl IntoElement,
    people: impl IntoElement,
    in_call: bool,
    channels_open: bool,
    members_open: bool,
) -> AnyElement {
    let view = cx.entity();
    let gap = plan.gap;
    let mut row = h_flex().size_full();
    if !plan.chat_grows && plan.right <= 0.5 {
        row = row.justify_center();
    }
    let mut drawer: Option<AnyElement> = None;

    if plan.rail > 0.0 && plan.channels > 0.0 {
        row = row
            .child(
                hairline(panel(palette, palette.rail), palette, Hairline::Right)
                    .w(px(plan.rail))
                    .flex_shrink_0()
                    .child(rail),
            )
            .child(column_gap(gap))
            .child(
                hairline(panel(palette, palette.sidebar), palette, Hairline::Right)
                    .w(px(plan.channels))
                    .flex_shrink_0()
                    .child(sidebar),
            )
            .child(column_splitter(
                "split-channels",
                SplitEdge::Channels,
                plan.channels,
                gap,
                palette,
                view.clone(),
            ));
    } else if plan.rail > 0.0 {
        row = row
            .child(
                hairline(panel(palette, palette.rail), palette, Hairline::Right)
                    .w(px(plan.rail))
                    .flex_shrink_0()
                    .child(rail),
            )
            .child(column_gap(gap));
        if channels_open {
            drawer = Some(sidebar.into_any_element());
        }
    } else if plan.channels > 0.0 {
        row = row
            .child(
                hairline(panel(palette, palette.sidebar), palette, Hairline::Right)
                    .w(px(plan.channels))
                    .flex_shrink_0()
                    .child(sidebar),
            )
            .child(column_splitter(
                "split-channels",
                SplitEdge::Channels,
                plan.channels,
                gap,
                palette,
                view.clone(),
            ));
    } else if channels_open {
        drawer = Some(
            h_flex()
                .size_full()
                .child(div().w(px(68.)).h_full().flex_shrink_0().child(rail))
                .child(div().flex_1().h_full().min_w(px(0.)).child(sidebar))
                .into_any_element(),
        );
    }

    let chat_panel = panel(palette, palette.chat).min_w(px(0.));
    let chat_panel = if plan.chat_grows {
        chat_panel.flex_1()
    } else {
        chat_panel.w(px(plan.chat_width)).flex_shrink_0()
    };
    row = row.child(chat_panel.child(chat));

    let mut members_layer: Option<AnyElement> = None;
    if plan.members > 0.0 {
        row = row
            .child(column_splitter(
                "split-members",
                SplitEdge::Members,
                plan.members,
                gap,
                palette,
                view.clone(),
            ))
            .child(
                hairline(panel(palette, palette.members), palette, Hairline::Left)
                    .w(px(plan.members))
                    .flex_shrink_0()
                    .child(people),
            );
    } else if members_open && plan.members_as_overlay {
        members_layer = Some(people.into_any_element());
    }

    if let Some(kind) = plan.right_kind.filter(|_| plan.right > 0.0) {
        let before_right = if plan.chat_grows {
            column_splitter(
                "split-right",
                SplitEdge::Right,
                plan.right,
                gap,
                palette,
                view.clone(),
            )
            .into_any_element()
        } else {
            column_gap(gap).into_any_element()
        };
        let right_panel = if plan.chat_grows {
            panel(palette, palette.members)
                .w(px(plan.right))
                .flex_shrink_0()
        } else {
            panel(palette, palette.members).flex_1().min_w(px(0.))
        };
        row = row
            .child(before_right)
            .child(right_panel.child(right_region(kind, palette)));
    }

    let reserve = if plan.voice_in_chat {
        if in_call {
            176.0
        } else {
            60.0
        }
    } else {
        0.0
    };
    let show_right_overlay = plan.right_as_overlay && plan.right_kind.is_some();
    let mut shell = div()
        .size_full()
        .relative()
        .bg(rgb(palette.app_bg))
        .p(px(gap))
        .child(row);

    if drawer.is_some() || members_layer.is_some() || show_right_overlay {
        let scrim = view.clone();
        shell = shell.child(
            div()
                .id("layout-scrim")
                .absolute()
                .top(px(0.))
                .left(px(0.))
                .right(px(0.))
                .bottom(px(reserve))
                .occlude()
                .on_click(move |_, _, cx| {
                    scrim.update(cx, |this, cx| {
                        this.channels_drawer = false;
                        this.members_overlay = false;
                        if this.right_overlay {
                            this.right_choice = RightChoice::Auto;
                        }
                        cx.notify();
                    });
                }),
        );
    }
    if let Some(drawer) = drawer {
        shell = shell.child(
            div()
                .absolute()
                .top(px(gap))
                .left(px(gap
                    + plan.rail
                    + if plan.rail > 0.0 { gap } else { 0.0 }))
                .bottom(px(gap + reserve))
                .w(px(plan.drawer))
                .occlude()
                .child(panel(palette, palette.sidebar).size_full().child(drawer)),
        );
    }
    if let Some(people) = members_layer {
        let close = view.clone();
        shell = shell.child(
            div()
                .absolute()
                .inset(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    v_flex()
                        .id("members-modal")
                        .w(px(360.))
                        .max_h(px(560.))
                        .rounded(px(8.))
                        .bg(rgb(palette.elevated))
                        .border_1()
                        .border_color(rgb(palette.line))
                        .occlude()
                        .child(
                            h_flex()
                                .px(px(16.))
                                .pt(px(16.))
                                .justify_between()
                                .items_start()
                                .child(
                                    v_flex()
                                        .child(
                                            div()
                                                .text_size(px(16.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(palette.text))
                                                .child("Members"),
                                        )
                                        .child(paint(
                                            "Everyone with access to this conversation.",
                                            palette.muted,
                                            13.,
                                        )),
                                )
                                .child(
                                    div()
                                        .id("members-close")
                                        .cursor_pointer()
                                        .child(paint("×", palette.text, 18.))
                                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                            close.update(cx, |this, cx| {
                                                this.members_overlay = false;
                                                cx.notify();
                                            });
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .id("members-scroll")
                                .flex_1()
                                .overflow_y_scroll()
                                .child(people),
                        ),
                ),
        );
    }
    if show_right_overlay {
        if let Some(kind) = plan.right_kind {
            shell = shell.child(
                div()
                    .absolute()
                    .top(px(gap))
                    .right(px(gap))
                    .bottom(px(gap + reserve))
                    .w(px(360.))
                    .occlude()
                    .child(
                        panel(palette, palette.members)
                            .size_full()
                            .child(right_region(kind, palette)),
                    ),
            );
        }
    }
    shell.into_any_element()
}

enum Hairline {
    Right,
    Left,
}

fn hairline(panel: gpui_kit::Div, palette: Palette, side: Hairline) -> gpui_kit::Div {
    if palette.shell_gap > 0.5 || palette.radius_panel > 0.5 {
        return panel;
    }
    match side {
        Hairline::Right => panel.border_r_1().border_color(rgb(palette.line)),
        Hairline::Left => panel.border_l_1().border_color(rgb(palette.line)),
    }
}

fn panel(palette: Palette, fill: u32) -> gpui_kit::Div {
    let floating =
        palette.shell_gap > 0.5 || palette.radius_panel > 0.5 || palette.shadow_blur > 0.5;
    let mut panel = div().h_full().overflow_hidden().bg(rgb(fill));
    if floating {
        panel = panel
            .rounded(px(palette.radius_panel))
            .border_1()
            .border_color(rgb(palette.line));
    }
    if palette.shadow_blur > 0.5 {
        panel.shadow(vec![BoxShadow::new(
            px(0.),
            px(palette.shadow_y),
            rgba(palette.shadow).into(),
        )
        .blur_radius(px(palette.shadow_blur))
        .spread_radius(px(palette.shadow_spread))])
    } else {
        panel
    }
}

fn jump_to_present(view: Entity<RencordWindow>, palette: Palette) -> impl IntoElement {
    h_flex()
        .id("jump-to-present")
        .mx(px(12.))
        .mt(px(6.))
        .h(px(28.))
        .px(px(12.))
        .rounded(px(6.))
        .items_center()
        .justify_between()
        .bg(rgb(palette.accent))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            view.update(cx, |this, cx| {
                this.timeline.scroll_to_end();
                this.following_tail = true;
                cx.notify();
            });
        })
        .child(banner_ink("Jump to present", palette.accent_text, 13.))
}

fn banner_ink(text: &str, color: u32, size: f32) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .font_weight(FontWeight::SEMIBOLD)
        .line_height(px(size + 2.))
        .child(text.to_owned())
}

fn date_pill(label: &str, palette: Palette) -> impl IntoElement {
    h_flex()
        .items_center()
        .px(px(16.))
        .py(px(8.))
        .gap(px(8.))
        .child(div().flex_1().h(px(1.)).bg(rgb(palette.line)))
        .child(paint(label, palette.muted, 12.))
        .child(div().flex_1().h(px(1.)).bg(rgb(palette.line)))
}

fn meta(text: impl Into<String>, color: u32, size: f32, palette: Palette) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .line_height(px(size + 3.))
        .font_family(palette.font_meta)
        .child(text.into())
}

fn waveform(color: u32) -> impl IntoElement {
    let percents = [
        18, 28, 46, 70, 88, 64, 90, 72, 40, 22, 36, 68, 92, 76, 50, 28, 20, 44, 80, 96, 70, 42, 24,
        38, 62, 84, 58, 30, 18, 26, 48, 66, 40, 22,
    ];
    let mut row = h_flex().h(px(26.)).items_center().gap(px(2.));
    for (index, percent) in percents.iter().enumerate() {
        row = row.child(
            div()
                .id(SharedString::from(format!("wave-{index}")))
                .w(px(3.))
                .h(px(((*percent as f32) * 0.26).max(2.)))
                .rounded(px(2.))
                .bg(rgb(color)),
        );
    }
    row
}

fn truncate_end(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        text.to_owned()
    } else {
        let keep: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{keep}…")
    }
}

fn paint(text: impl Into<String>, color: u32, size: f32) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .line_height(px(size + 4.))
        .font_weight(FontWeight::NORMAL)
        .child(text.into())
}

fn mention_label(id: &str) -> String {
    match id {
        "1" => "@Ada".into(),
        "2" => "@Charles".into(),
        "3" => "@Lovelace".into(),
        other => format!("@{other}"),
    }
}

fn rich_inlines(inlines: &[Inline], palette: Palette, font_px: f32) -> impl IntoElement {
    let mut text = String::new();
    let mut runs = Vec::new();
    walk_inlines(
        inlines,
        palette,
        FontWeight::NORMAL,
        &mut |chunk, color, weight, background| {
            if chunk.is_empty() {
                return;
            }
            text.push_str(chunk);
            let mut face = font(palette.font_ui);
            face.weight = weight;
            runs.push(TextRun {
                len: chunk.len(),
                font: face,
                color: rgb(color).into(),
                background_color: background.map(|color| rgb(color).into()),
                underline: None,
                strikethrough: None,
            });
        },
    );
    if text.is_empty() {
        text.push(' ');
        let mut face = font(palette.font_ui);
        face.weight = FontWeight::NORMAL;
        runs.push(TextRun {
            len: 1,
            font: face,
            color: rgb(palette.text).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    let onyx = palette.ink_chrome;
    div()
        .w_full()
        .min_w(px(0.))
        .whitespace_normal()
        .text_size(px(if onyx { 14. } else { font_px }))
        .line_height(px(if onyx { 20. } else { font_px + 4. }))
        .child(StyledText::new(text).with_runs(runs))
}

fn walk_inlines(
    inlines: &[Inline],
    palette: Palette,
    weight: FontWeight,
    push: &mut impl FnMut(&str, u32, FontWeight, Option<u32>),
) {
    for inline in inlines {
        match inline {
            Inline::Text(value) => push(value, palette.text, weight, None),
            Inline::Bold(inner) => walk_inlines(inner, palette, FontWeight::SEMIBOLD, push),
            Inline::Italic(inner) | Inline::Spoiler(inner) => {
                walk_inlines(inner, palette, weight, push)
            }
            Inline::Code(value) => push(value, palette.link, weight, Some(palette.elevated)),
            Inline::Link { text, .. } => push(text, palette.link, weight, None),
            Inline::Mention { id } => {
                let label = mention_label(id);
                let (foreground, wash) = (palette.mention_fg, palette.mention_bg);
                push(&label, foreground, FontWeight::SEMIBOLD, Some(wash));
            }
        }
    }
}

fn bench_photo() -> impl IntoElement {
    let image = Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../../docs/design/fixtures/bench.png").to_vec(),
    );
    img(std::sync::Arc::new(image)).size_full()
}

fn fetch_failure_text(kind: &str, err: &rencord_api::ApiError) -> String {
    tracing::debug!(
        "{kind} fetch failed: {}",
        rencord_auth::redact(&err.to_string())
    );
    match err {
        rencord_api::ApiError::Status { status, body } => {
            rencord_auth::redact(&rencord_api::explain_http_failure(kind, *status, body))
        }
        other => format!(
            "Couldn't load {kind}. {}",
            rencord_auth::redact(&other.to_string())
        ),
    }
}

fn record_fetch_failure(
    requested: &mut HashSet<Snowflake>,
    failed: &mut HashMap<Snowflake, String>,
    id: Snowflake,
    message: String,
    drop_request: bool,
) {
    if drop_request {
        requested.remove(&id);
    }
    failed.insert(id, message);
}

/// True while a fetch is in flight or waiting for an explicit retry.
#[cfg(test)]
fn fetch_is_blocked(
    requested: &HashSet<Snowflake>,
    failed: &HashMap<Snowflake, String>,
    id: Snowflake,
) -> bool {
    failed.contains_key(&id) || requested.contains(&id)
}

fn rss_kib() -> u64 {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return 0;
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kb| kb.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod fetch_failure {
    use super::{fetch_is_blocked, record_fetch_failure};
    use rencord_model::Snowflake;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn a_failed_fetch_is_not_retried_until_retry_is_pressed() {
        let id = Snowflake::new(8);
        let mut requested = HashSet::from([id]);
        let mut failed = HashMap::new();
        record_fetch_failure(
            &mut requested,
            &mut failed,
            id,
            "discord returned 500".into(),
            true,
        );
        assert!(!requested.contains(&id));
        assert!(fetch_is_blocked(&requested, &failed, id));
        failed.remove(&id);
        assert!(!fetch_is_blocked(&requested, &failed, id));
    }
}
