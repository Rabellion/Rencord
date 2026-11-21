//! Welcome screen. The QR code is the primary sign-in. Discord's own page is
//! beside it. A pasted token stays behind Other options.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;
use rencord_auth::{
    complete_remote_login, dismiss_embedded_captcha, embedded_captcha_available, fetch_avatar,
    present_embedded_captcha, redact, reload_embedded_captcha, render_qr_png, AccountRecord,
    AccountStore, AuthClient, AuthError, CaptchaChallenge, CaptchaSolution, LoginOutcome,
    MfaChallenge, MfaMethod, RemoteProgress, WidgetEvent, AUTH_API_ROOT, REMOTE_AUTH_GATEWAY,
};
use rencord_desktop::{stacks_login, Palette};
use rencord_model::Token;
use rencord_platform::{FileSecretStore, SecretStore};

use super::net::{self, BootError, Bootstrap, GatewayLink};
use super::RencordWindow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginPhase {
    Qr,
    Confirm,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginPreview {
    Qr,
    Confirm,
    Error,
    Captcha,
}

pub struct LoginModel {
    pub phase: LoginPhase,
    pub status: String,
    pub error: Option<String>,
    pub qr_png: Vec<u8>,
    pub qr_image: Option<Arc<Image>>,
    pub preview_name: String,
    pub preview_handle: String,
    pub avatar_bytes: Option<Vec<u8>>,
    pub avatar_image: Option<Arc<Image>>,
    pub other_open: bool,
    pub token_open: bool,
    pub password_open: bool,
    pub mfa: Option<MfaChallenge>,
    pub accounts: Vec<AccountRecord>,
    pub busy: bool,
    pub adding: bool,
    pub captcha: Option<CaptchaChallenge>,
    pub captcha_mock: bool,
    pub captcha_note: Option<String>,
    captcha_wait: Option<CaptchaWait>,
    captcha_tx: Option<tokio::sync::mpsc::UnboundedSender<Option<CaptchaSolution>>>,
    captcha_attached: bool,
}

#[derive(Clone)]
enum CaptchaWait {
    Remote,
    Password {
        email: String,
        password: String,
    },
    Mfa {
        method: MfaMethod,
        ticket: String,
        instance: Option<String>,
        code: String,
    },
}

impl LoginModel {
    pub fn fresh() -> Self {
        Self {
            phase: LoginPhase::Qr,
            status: "Waiting for Discord…".into(),
            error: None,
            qr_png: Vec::new(),
            qr_image: None,
            preview_name: String::new(),
            preview_handle: String::new(),
            avatar_bytes: None,
            avatar_image: None,
            other_open: false,
            token_open: false,
            password_open: false,
            mfa: None,
            accounts: Vec::new(),
            busy: false,
            adding: false,
            captcha: None,
            captcha_mock: false,
            captcha_note: None,
            captcha_wait: None,
            captcha_tx: None,
            captcha_attached: false,
        }
    }

    pub fn preview(kind: LoginPreview) -> Self {
        let mut model = Self::fresh();
        match kind {
            LoginPreview::Qr => {
                let url = "https://discord.com/ra/UZ0-kOVzXDZTFVV5_QlpURSO2BQHrtkKWHNpIGoDI0k";
                model.qr_png = render_qr_png(url).unwrap_or_default();
                model.status = "Scan with the Discord app".into();
            }
            LoginPreview::Confirm => {
                model.phase = LoginPhase::Confirm;
                model.preview_name = "Ada Lovelace".into();
                model.preview_handle = "ada".into();
                model.avatar_bytes = Some(include_bytes!("../assets/avatars/ada.png").to_vec());
                model.status = "Waiting for approval…".into();
            }
            LoginPreview::Error => {
                model.phase = LoginPhase::Error;
                model.error = Some(
                    "Discord returned 400 (50035): The remote authentication ticket was not accepted. This message is long enough to wrap inside the card instead of running off both edges.".into(),
                );
                model.status = "Discord could not finish signing in".into();
            }
            LoginPreview::Captcha => {
                model.captcha = Some(CaptchaChallenge {
                    sitekey: "preview-sitekey".into(),
                    service: "hcaptcha".into(),
                    session_id: None,
                    rqdata: None,
                    rqtoken: None,
                });
                model.captcha_mock = true;
                model.captcha_attached = true;
                model.status = "Discord asked for a captcha.".into();
            }
        }
        model
    }
}

enum QrMsg {
    Progress(RemoteProgress),
    Done(Result<Token, AuthError>),
}

impl RencordWindow {
    pub(super) fn open_vault() -> AccountStore {
        AccountStore::keyring()
    }

    pub(super) fn login_screen(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if !self.booted {
            self.booted = true;
            if self.preview.is_none() && !self.shell.demo {
                self.boot_login(cx);
            }
        }
        let palette = self.shell.prefs.palette();
        let size = window.viewport_size();
        let stack = stacks_login(size.width.as_f32(), size.height.as_f32());
        let card = self.login_card(cx, palette);
        let copy = self.login_copy(cx, palette, window.scale_factor());
        let row = if stack {
            v_flex()
                .w_full()
                .max_w(px(420.))
                .items_center()
                .gap(px(28.))
                .child(copy)
                .child(card)
                .into_any_element()
        } else {
            h_flex()
                .items_center()
                .justify_center()
                .gap(px(72.))
                .child(card)
                .child(copy)
                .into_any_element()
        };
        if self.login.captcha.is_some() {
            self.attach_captcha_host(window, cx);
        }
        let screen = v_flex()
            .id("login-screen")
            .size_full()
            .overflow_y_scroll()
            .items_center()
            .justify_center()
            .gap(px(28.))
            .px(px(28.))
            .py(px(32.))
            .bg(rgb(palette.app_bg))
            .child(row)
            .child(self.account_row(cx, palette));
        let mut root = div().relative().size_full().child(screen);
        if self.login.captcha.is_some() {
            root = root.child(self.captcha_overlay(cx, palette));
        }
        root
    }

    fn login_card(&mut self, cx: &mut Context<Self>, palette: Palette) -> impl IntoElement {
        let face = match self.login.phase {
            LoginPhase::Confirm => self.confirm_face(palette).into_any_element(),
            LoginPhase::Error => self.error_face(cx, palette).into_any_element(),
            LoginPhase::Qr => self.qr_face(palette).into_any_element(),
        };
        let this = cx.entity();
        let link = if self.login.phase == LoginPhase::Confirm {
            text_button(
                this,
                "login-not-you",
                "Not you? Use a new code",
                palette,
                |view, _window, cx| view.start_qr(cx),
            )
            .into_any_element()
        } else {
            div().h(px(16.)).into_any_element()
        };
        v_flex()
            .w(px(320.))
            .items_center()
            .gap(px(10.))
            .child(stage(face, self.login.phase, palette))
            .child(self.caption(palette))
            .child(link)
    }

    fn caption(&self, palette: Palette) -> impl IntoElement {
        let mut row = h_flex()
            .h(px(18.))
            .items_center()
            .justify_center()
            .gap(px(6.));
        if self.login.phase == LoginPhase::Confirm {
            row = row.child(wait_spinner(palette));
        }
        row.child(mono(self.login.status.clone(), palette, 12.))
    }

    fn qr_face(&mut self, palette: Palette) -> impl IntoElement {
        if self.login.qr_image.is_none() && !self.login.qr_png.is_empty() {
            self.login.qr_image = image_from_bytes(&self.login.qr_png);
        }
        if let Some(image) = self.login.qr_image.clone() {
            img(image)
                .size(px(278.))
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        } else {
            div()
                .size(px(248.))
                .items_center()
                .justify_center()
                .child(mono("Preparing the code…", palette, 12.))
                .into_any_element()
        }
    }

    fn confirm_face(&mut self, palette: Palette) -> impl IntoElement {
        if self.login.avatar_image.is_none() {
            if let Some(bytes) = &self.login.avatar_bytes {
                self.login.avatar_image = image_from_bytes(bytes);
            }
        }
        let portrait = if let Some(image) = self.login.avatar_image.clone() {
            img(image)
                .size(px(88.))
                .rounded(px(44.))
                .object_fit(ObjectFit::Cover)
                .into_any_element()
        } else {
            initial_bubble(&bubble_initial(&self.login.preview_name), palette, 88.)
                .into_any_element()
        };
        v_flex()
            .items_center()
            .gap(px(8.))
            .child(portrait)
            .child(ink("Confirm on your phone", palette.text, 18., true))
            .child(ink(
                self.login.preview_name.clone(),
                palette.text,
                16.,
                true,
            ))
            .child(mono(
                format!("@{}", self.login.preview_handle),
                palette,
                12.,
            ))
            .into_any_element()
    }

    fn error_face(&mut self, cx: &mut Context<Self>, palette: Palette) -> impl IntoElement {
        let message = self
            .login
            .error
            .clone()
            .unwrap_or_else(|| "Discord could not finish signing in.".into());
        let this = cx.entity();
        v_flex()
            .size_full()
            .px(px(16.))
            .py(px(12.))
            .items_center()
            .justify_center()
            .gap(px(12.))
            .overflow_hidden()
            .child(wrapped_ink(message, palette.danger, 14.))
            .child(accent_button(
                this,
                "login-refresh",
                "New code",
                palette,
                |view, _window, cx| {
                    view.start_qr(cx);
                },
            ))
            .into_any_element()
    }

    fn login_copy(
        &mut self,
        cx: &mut Context<Self>,
        palette: Palette,
        scale: f32,
    ) -> impl IntoElement {
        let this = cx.entity();
        let mut column = v_flex()
            .w(px(360.))
            .gap(px(14.))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(10.))
                    .child(hexwave_mark(palette.text, 36., scale))
                    .child(ink("Rencord", palette.text, 28., true)),
            )
            .child(ink("Sign in", palette.text, 16., true))
            .child(ink(
                "Scan the code with the Discord mobile app. It will ask you to confirm this login.",
                palette.text,
                14.,
                false,
            ));
        if let Some(error) = &self.login.error {
            if self.login.phase != LoginPhase::Error {
                column = column.child(ink(error.clone(), palette.danger, 13., false));
            }
        }
        column = column.child(accent_button(
            this.clone(),
            "login-discord",
            "Log in with Discord",
            palette,
            |view, _window, cx| view.open_webview(cx),
        ));
        column = column.child(text_button(
            this.clone(),
            "login-other",
            if self.login.other_open {
                "Hide other options"
            } else {
                "Other options"
            },
            palette,
            |view, _window, cx| {
                view.login.other_open = !view.login.other_open;
                cx.notify();
            },
        ));
        if self.login.other_open {
            column = column.child(self.other_options(cx, palette));
        }
        if self.login.adding {
            column = column.child(text_button(
                this,
                "login-back",
                "Back to the signed-in account",
                palette,
                |view, _window, cx| {
                    view.login.adding = false;
                    cx.notify();
                },
            ));
        }
        column = column.child(mono(
            "Unofficial client. Discord's terms forbid this, and the account can be terminated.",
            palette,
            12.,
        ));
        column
    }

    fn other_options(&mut self, cx: &mut Context<Self>, palette: Palette) -> impl IntoElement {
        let this = cx.entity();
        let mut column = v_flex()
            .gap(px(10.))
            .child(text_button(
                this.clone(),
                "login-email-toggle",
                "Email and password",
                palette,
                |view, _window, cx| {
                    view.login.password_open = !view.login.password_open;
                    view.login.token_open = false;
                    cx.notify();
                },
            ))
            .child(text_button(
                this.clone(),
                "login-token-toggle",
                "Use a token",
                palette,
                |view, _window, cx| {
                    view.login.token_open = !view.login.token_open;
                    view.login.password_open = false;
                    cx.notify();
                },
            ));
        if self.login.password_open {
            column = column.child(self.password_form(cx, palette));
        }
        if self.login.token_open {
            column = column.child(self.token_form(cx, palette));
        }
        column
    }

    fn password_form(&mut self, cx: &mut Context<Self>, palette: Palette) -> impl IntoElement {
        let this = cx.entity();
        let mut column = v_flex()
            .gap(px(8.))
            .child(Input::new(&self.email_input))
            .child(Input::new(&self.password_input));
        if let Some(mfa) = self.login.mfa.clone() {
            column = column
                .child(ink(
                    "Enter the code from your authenticator, a text, or a backup code.",
                    palette.text,
                    13.,
                    false,
                ))
                .child(Input::new(&self.mfa_input))
                .child(accent_button(
                    this.clone(),
                    "login-mfa-submit",
                    "Submit code",
                    palette,
                    |view, window, cx| view.submit_mfa(window, cx),
                ));
            if mfa.sms {
                column = column.child(text_button(
                    this.clone(),
                    "login-sms",
                    "Text me a code",
                    palette,
                    |view, _window, cx| view.send_login_sms(cx),
                ));
            }
            if mfa.webauthn.is_some() {
                column = column.child(text_button(
                    this.clone(),
                    "login-passkey",
                    "Use a passkey in Discord's page",
                    palette,
                    |view, _window, cx| view.open_webview(cx),
                ));
            }
        } else {
            column = column.child(accent_button(
                this,
                "login-password-submit",
                "Continue",
                palette,
                |view, window, cx| view.submit_password(window, cx),
            ));
        }
        column
    }

    fn token_form(&mut self, cx: &mut Context<Self>, palette: Palette) -> impl IntoElement {
        let this = cx.entity();
        v_flex()
            .gap(px(8.))
            .child(mono(
                "Advanced. The token is stored in the OS keychain, not in a file.",
                palette,
                12.,
            ))
            .child(Input::new(&self.token_input))
            .child(accent_button(
                this,
                "login-token-submit",
                "Use token",
                palette,
                |view, _window, cx| {
                    view.submit_token(cx);
                },
            ))
    }

    fn account_row(&mut self, cx: &mut Context<Self>, palette: Palette) -> impl IntoElement {
        if self.login.accounts.is_empty() {
            return div().into_any_element();
        }
        let mut row = h_flex().gap(px(12.)).items_center().justify_center();
        for account in self.login.accounts.clone() {
            let this = cx.entity();
            let id = account.user_id.clone();
            let switch_id = id.clone();
            let remove_id = id.clone();
            let name = account.display_name().to_owned();
            let initial = account.initial();
            let remove_view = this.clone();
            row = row.child(
                v_flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .id(SharedString::from(format!("acct-{id}")))
                            .cursor_pointer()
                            .child(initial_bubble(&initial, palette, 40.))
                            .on_click(move |_, _, cx| {
                                let switch_id = switch_id.clone();
                                this.update(cx, |view, cx| view.switch_account(&switch_id, cx));
                            }),
                    )
                    .child(mono(name, palette, 11.))
                    .child(text_button(
                        remove_view,
                        SharedString::from(format!("acct-remove-{id}")),
                        "Remove",
                        palette,
                        move |view, _window, cx| view.remove_account(&remove_id, cx),
                    )),
            );
        }
        row.into_any_element()
    }

    pub(super) fn boot_login(&mut self, cx: &mut Context<Self>) {
        self.reload_accounts();
        if let Some(id) = self.vault.active_user().ok().flatten() {
            if let Ok(Some(token)) = self.vault.load_token(&id) {
                self.login.status = "Checking the saved session…".into();
                self.begin_session(token, false, cx);
                return;
            }
        }
        if let Some(token) = FileSecretStore::in_data_dir().load_token().ok().flatten() {
            self.login.status = "Moving the saved session into the keychain…".into();
            self.begin_session(Token::new(token), true, cx);
            return;
        }
        self.session_machine.first_run();
        self.start_qr(cx);
    }

    fn reload_accounts(&mut self) {
        self.login.accounts = self.vault.list_accounts().unwrap_or_default();
    }

    pub(super) fn start_qr(&mut self, cx: &mut Context<Self>) {
        self.login_cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        self.login_cancel = Arc::clone(&cancel);
        self.remote_generation = self.remote_generation.wrapping_add(1);
        let generation = self.remote_generation;
        self.login.phase = LoginPhase::Qr;
        self.login.error = None;
        self.login.busy = false;
        self.login.status = "Waiting for Discord…".into();
        self.login.qr_png.clear();
        self.login.qr_image = None;
        self.clear_captcha_panel();
        let (tx, rx) = async_channel::bounded(16);
        let (solution_tx, mut solution_rx) = tokio::sync::mpsc::unbounded_channel();
        self.login.captcha_tx = Some(solution_tx);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio");
            let result = runtime.block_on(complete_remote_login(
                REMOTE_AUTH_GATEWAY,
                AUTH_API_ROOT,
                &cancel,
                &mut solution_rx,
                |progress| {
                    let _ = tx.send_blocking(QrMsg::Progress(progress));
                },
            ));
            let _ = tx.send_blocking(QrMsg::Done(result));
        });
        cx.spawn(async move |this, cx| {
            while let Ok(message) = rx.recv().await {
                let stop = this
                    .update(cx, |view, cx| {
                        if view.remote_generation != generation {
                            return true;
                        }
                        view.apply_qr(message, cx);
                        false
                    })
                    .unwrap_or(true);
                if stop {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn apply_qr(&mut self, message: QrMsg, cx: &mut Context<Self>) {
        match message {
            QrMsg::Progress(RemoteProgress::Qr { png, .. }) => {
                self.login.phase = LoginPhase::Qr;
                self.login.qr_png = png;
                self.login.qr_image = None;
                self.login.status = "Scan with the Discord app".into();
            }
            QrMsg::Progress(RemoteProgress::Confirm(user)) => {
                self.login.phase = LoginPhase::Confirm;
                self.login.preview_name = user.display_name().to_owned();
                self.login.preview_handle = user.username.clone();
                self.login.avatar_bytes = None;
                self.login.avatar_image = None;
                self.login.status = "Waiting for approval…".into();
                let url = user.avatar_url();
                self.load_avatar(url, cx);
            }
            QrMsg::Progress(RemoteProgress::Refreshing) => {
                self.login.status = "Refreshing the code…".into();
                self.login.qr_png.clear();
                self.login.qr_image = None;
            }
            QrMsg::Progress(RemoteProgress::Captcha(challenge)) => {
                self.login.captcha_wait = Some(CaptchaWait::Remote);
                self.show_captcha(challenge);
            }
            QrMsg::Done(Ok(token)) => {
                self.clear_captcha_panel();
                self.begin_session(token, false, cx);
            }
            QrMsg::Done(Err(AuthError::Cancelled)) => {}
            QrMsg::Done(Err(AuthError::Captcha(challenge))) => {
                self.login.captcha_wait = Some(CaptchaWait::Remote);
                self.show_captcha(challenge);
            }
            QrMsg::Done(Err(err)) => {
                self.login.phase = LoginPhase::Error;
                self.login.status = err.caption().into();
                self.login.error = Some(err.public_message());
            }
        }
        cx.notify();
    }

    fn load_avatar(&mut self, url: String, cx: &mut Context<Self>) {
        let generation = self.remote_generation;
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio");
            let bytes = runtime.block_on(fetch_avatar(&url)).ok();
            let _ = tx.send_blocking(bytes);
        });
        cx.spawn(async move |this, cx| {
            let Ok(bytes) = rx.recv().await else {
                return;
            };
            this.update(cx, |view, cx| {
                if view.remote_generation != generation {
                    return;
                }
                if let Some(bytes) = bytes {
                    view.login.avatar_bytes = Some(bytes);
                    view.login.avatar_image = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn begin_session(
        &mut self,
        token: Token,
        migrate_file: bool,
        cx: &mut Context<Self>,
    ) {
        self.login_cancel.store(true, Ordering::Relaxed);
        self.remote_generation = self.remote_generation.wrapping_add(1);
        self.login.busy = true;
        self.login.status = "Signing in…".into();
        let wire = token.expose().to_owned();
        // The shared runtime keeps the gateway started here alive after bootstrap returns.
        let task = net::runtime().spawn(net::fetch_bootstrap(wire));
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = task.await else {
                return;
            };
            this.update(cx, |view, cx| {
                match outcome {
                    Ok(snapshot) => view.finish_sign_in(token, snapshot, migrate_file, cx),
                    Err(BootError::Unauthorized) => {
                        view.session_machine.restart();
                        view.session_machine.unauthorized();
                        view.drop_live_session();
                        view.login.busy = false;
                        if migrate_file {
                            let _ = FileSecretStore::in_data_dir().clear_token();
                        }
                        view.start_qr(cx);
                        view.login.error = Some("This session is no longer valid.".into());
                        view.login.status = "Sign in again".into();
                    }
                    Err(BootError::Other(message)) => {
                        view.login.busy = false;
                        view.login.phase = LoginPhase::Error;
                        view.login.error = Some(message);
                        view.login.status = "Could not reach Discord".into();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn finish_sign_in(
        &mut self,
        token: Token,
        snapshot: Bootstrap,
        migrate_file: bool,
        cx: &mut Context<Self>,
    ) {
        let record = AccountRecord::from_user(&snapshot.user);
        match self.vault.save_account(&record, &token) {
            Ok(()) => {
                if migrate_file {
                    let _ = FileSecretStore::in_data_dir().clear_token();
                }
            }
            Err(err) => {
                self.shell.note(err.public_message());
                self.session_machine.storage_unavailable();
            }
        }
        self.session_machine.restart();
        let _ = self.session_machine.established(record.user_id.clone());
        self.session_token = Some(token);
        self.requested.clear();
        self.loading_older = None;
        self.history_start.clear();
        let api = snapshot.api.clone();
        self.api = Some(api.clone());
        let GatewayLink {
            mut events,
            commands,
            ended,
        } = snapshot.gateway;
        // Replacing the sender stops the previous account's gateway.
        self.gateway = Some(commands);
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.recv().await {
                let applied = this.update(cx, |view, cx| {
                    // Events still queued from a session that was replaced are dropped.
                    let current = view
                        .api
                        .as_ref()
                        .is_some_and(|live| Arc::ptr_eq(live, &api));
                    if current {
                        view.on_gateway_event(event, cx);
                        cx.notify();
                    }
                    current
                });
                if !matches!(applied, Ok(true)) {
                    break;
                }
            }
            // The gateway stopped; a bad token means the session is over.
            if ended
                .await
                .is_ok_and(|outcome| net::session_invalid(&outcome))
            {
                let _ = this.update(cx, |view, cx| {
                    if view
                        .api
                        .as_ref()
                        .is_some_and(|live| Arc::ptr_eq(live, &api))
                    {
                        view.note_unauthorized(cx);
                    }
                });
            }
        })
        .detach();
        self.shell.set_user(snapshot.user);
        self.shell.load_appearance();
        self.shell.consider_discord_appearance(
            snapshot.discord_theme.as_deref(),
            snapshot.discord_client_theme.as_deref(),
        );
        for guild in snapshot.guilds {
            self.shell.session.upsert_guild(guild);
        }
        for channel in snapshot.channels {
            self.shell.session.upsert_channel(channel);
        }
        if let Some(guild) = self.shell.session.guilds.first().cloned() {
            self.shell.session.select_guild(guild.id);
        }
        self.shell
            .note(format!("Signed in. {}", redact(&snapshot.gateway_note)));
        self.login.busy = false;
        self.login.adding = false;
        self.login.mfa = None;
        self.reload_accounts();
    }

    fn submit_token(&mut self, cx: &mut Context<Self>) {
        let token = self.token_input.read(cx).value().trim().to_owned();
        if token.is_empty() {
            self.login.error = Some("The token is empty.".into());
            cx.notify();
            return;
        }
        self.begin_session(Token::new(token), false, cx);
    }

    fn submit_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let email = self.email_input.read(cx).value().trim().to_owned();
        let password = self.password_input.read(cx).value().to_string();
        self.clear_secret_input(&self.password_input, window, cx);
        if email.is_empty() || password.is_empty() {
            self.login.error = Some("Email and password are both required.".into());
            cx.notify();
            return;
        }
        self.login.captcha_wait = Some(CaptchaWait::Password {
            email: email.clone(),
            password: password.clone(),
        });
        self.login.busy = true;
        self.login.error = None;
        self.login.status = "Asking Discord…".into();
        cx.notify();
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio");
            let outcome =
                runtime.block_on(AuthClient::new().login_with_password(&email, &password));
            let _ = tx.send_blocking(outcome);
        });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.recv().await else {
                return;
            };
            this.update(cx, |view, cx| {
                view.login.busy = false;
                view.apply_login_outcome(outcome, cx);
            })
            .ok();
        })
        .detach();
    }

    fn submit_mfa(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(challenge) = self.login.mfa.clone() else {
            return;
        };
        let code = self.mfa_input.read(cx).value().trim().to_owned();
        self.clear_secret_input(&self.mfa_input, window, cx);
        if code.is_empty() {
            self.login.error = Some("Enter the code first.".into());
            cx.notify();
            return;
        }
        let method = if challenge.sms && !challenge.totp {
            MfaMethod::Sms
        } else if code.len() > 8 {
            MfaMethod::Backup
        } else {
            MfaMethod::Totp
        };
        self.login.captcha_wait = Some(CaptchaWait::Mfa {
            method,
            ticket: challenge.ticket.clone(),
            instance: challenge.login_instance_id.clone(),
            code: code.clone(),
        });
        self.login.busy = true;
        cx.notify();
        let ticket = challenge.ticket.clone();
        let instance = challenge.login_instance_id.clone();
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio");
            let outcome = runtime.block_on(AuthClient::new().submit_mfa(
                method,
                &ticket,
                instance.as_deref(),
                &code,
            ));
            let _ = tx.send_blocking(outcome);
        });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.recv().await else {
                return;
            };
            this.update(cx, |view, cx| {
                view.login.busy = false;
                view.apply_login_outcome(outcome, cx);
            })
            .ok();
        })
        .detach();
    }

    fn send_login_sms(&mut self, cx: &mut Context<Self>) {
        let Some(challenge) = self.login.mfa.clone() else {
            return;
        };
        let ticket = challenge.ticket.clone();
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio");
            let outcome = runtime.block_on(AuthClient::new().send_sms(&ticket));
            let _ = tx.send_blocking(outcome);
        });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.recv().await else {
                return;
            };
            this.update(cx, |view, cx| {
                view.login.status = match outcome {
                    Ok(phone) => format!("Discord sent a code to {phone}."),
                    Err(err) => err.public_message(),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn apply_login_outcome(
        &mut self,
        outcome: Result<LoginOutcome, AuthError>,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            Ok(LoginOutcome::Session { token, .. }) => {
                self.login.mfa = None;
                self.clear_captcha_panel();
                self.begin_session(token, false, cx);
            }
            Ok(LoginOutcome::Mfa(challenge)) => {
                self.login.captcha_wait = None;
                self.login.mfa = Some(challenge);
                self.login.password_open = true;
                self.login.other_open = true;
                self.login.status = "Discord asked for a second step.".into();
                cx.notify();
            }
            Ok(LoginOutcome::Captcha(challenge)) => {
                self.show_captcha(challenge);
                cx.notify();
            }
            Err(err) => {
                self.login.captcha_wait = None;
                self.login.error = Some(err.public_message());
                self.login.status = "Discord did not accept that".into();
                if matches!(err, AuthError::WebviewUnavailable(_)) {
                    self.login.password_open = true;
                    self.login.other_open = true;
                }
                cx.notify();
            }
        }
    }

    pub(super) fn open_webview(&mut self, cx: &mut Context<Self>) {
        if self.login.busy {
            return;
        }
        self.login.busy = true;
        self.login.status = "Opening Discord's login page…".into();
        cx.notify();
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let outcome = rencord_auth::open_discord_login_window();
            let _ = tx.send_blocking(outcome);
        });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.recv().await else {
                return;
            };
            this.update(cx, |view, cx| {
                view.login.busy = false;
                match outcome {
                    Ok(token) => view.begin_session(token, false, cx),
                    Err(err) => {
                        view.login.error = Some(err.public_message());
                        view.login.status = "The login window did not finish".into();
                        if matches!(err, AuthError::WebviewUnavailable(_)) {
                            view.login.other_open = true;
                            view.login.password_open = true;
                        }
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn show_captcha(&mut self, challenge: CaptchaChallenge) {
        dismiss_embedded_captcha();
        self.login.captcha = Some(challenge);
        self.login.captcha_mock = false;
        self.login.captcha_attached = false;
        self.login.captcha_note = None;
        self.login.status = "Discord asked for a captcha.".into();
        self.login.error = None;
    }

    fn clear_captcha_panel(&mut self) {
        dismiss_embedded_captcha();
        self.login.captcha = None;
        self.login.captcha_mock = false;
        self.login.captcha_note = None;
        self.login.captcha_wait = None;
        self.login.captcha_attached = false;
        if let Some(tx) = self.login.captcha_tx.take() {
            let _ = tx.send(None);
        }
    }

    fn attach_captcha_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.login.captcha_mock || self.login.captcha_attached {
            return;
        }
        let Some(challenge) = self.login.captcha.clone() else {
            return;
        };
        self.login.captcha_attached = true;
        if !embedded_captcha_available() {
            self.login.captcha_note = Some(
                "This build cannot embed hCaptcha in the window. Continue on Discord's page."
                    .into(),
            );
            return;
        }
        let size = window.viewport_size();
        let panel_w = 440.0_f32;
        let panel_h = 520.0_f32;
        let x = (size.width.as_f32() - panel_w) / 2.0 + 28.0;
        let y = (size.height.as_f32() - panel_h) / 2.0 + 96.0;
        let (tx, rx) = async_channel::bounded(4);
        let opened = present_embedded_captcha(
            window,
            x,
            y,
            panel_w - 56.0,
            320.0,
            &challenge,
            move |event| {
                let _ = tx.try_send(event);
            },
        );
        if let Err(message) = opened {
            self.login.captcha_note = Some(message);
            return;
        }
        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                this.update(cx, |view, cx| view.on_widget_event(event, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn on_widget_event(&mut self, event: WidgetEvent, cx: &mut Context<Self>) {
        match event {
            WidgetEvent::Solved(token) => self.retry_with_solution(token, cx),
            WidgetEvent::Expired => {
                self.login.captcha_note = Some("The check expired. Try it again.".into());
                cx.notify();
            }
            WidgetEvent::Error => {
                self.login.captcha_note =
                    Some("hCaptcha did not accept this window. Continue on Discord's page.".into());
                cx.notify();
            }
            WidgetEvent::Closed => {
                self.finish_captcha_without_token();
                cx.notify();
            }
        }
    }

    fn retry_with_solution(&mut self, token: String, cx: &mut Context<Self>) {
        let Some(challenge) = self.login.captcha.clone() else {
            return;
        };
        let solution = CaptchaSolution {
            key: token,
            rqtoken: challenge.rqtoken.clone(),
            session_id: challenge.session_id.clone(),
        };
        dismiss_embedded_captcha();
        self.login.captcha = None;
        self.login.captcha_attached = false;
        self.login.captcha_note = None;
        self.login.status = "Sending the captcha to Discord…".into();
        match self.login.captcha_wait.clone() {
            Some(CaptchaWait::Remote) => {
                if let Some(tx) = &self.login.captcha_tx {
                    let _ = tx.send(Some(solution));
                }
            }
            Some(CaptchaWait::Password { email, password }) => {
                self.login.busy = true;
                let (tx, rx) = async_channel::bounded(1);
                std::thread::spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("tokio");
                    let outcome = runtime.block_on(AuthClient::new().login_with_password_solved(
                        &email,
                        &password,
                        Some(&solution),
                    ));
                    let _ = tx.send_blocking(outcome);
                });
                cx.spawn(async move |this, cx| {
                    let Ok(outcome) = rx.recv().await else {
                        return;
                    };
                    this.update(cx, |view, cx| {
                        view.login.busy = false;
                        view.apply_login_outcome(outcome, cx);
                    })
                    .ok();
                })
                .detach();
            }
            Some(CaptchaWait::Mfa {
                method,
                ticket,
                instance,
                code,
            }) => {
                self.login.busy = true;
                let (tx, rx) = async_channel::bounded(1);
                std::thread::spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("tokio");
                    let outcome = runtime.block_on(AuthClient::new().submit_mfa_solved(
                        method,
                        &ticket,
                        instance.as_deref(),
                        &code,
                        Some(&solution),
                    ));
                    let _ = tx.send_blocking(outcome);
                });
                cx.spawn(async move |this, cx| {
                    let Ok(outcome) = rx.recv().await else {
                        return;
                    };
                    this.update(cx, |view, cx| {
                        view.login.busy = false;
                        view.apply_login_outcome(outcome, cx);
                    })
                    .ok();
                })
                .detach();
            }
            None => {
                self.login.captcha_note =
                    Some("The captcha was solved, but the login to retry is gone.".into());
                self.login.captcha = Some(challenge);
            }
        }
        cx.notify();
    }

    fn finish_captcha_without_token(&mut self) {
        dismiss_embedded_captcha();
        if matches!(self.login.captcha_wait, Some(CaptchaWait::Remote)) {
            if let Some(tx) = &self.login.captcha_tx {
                let _ = tx.send(None);
            }
        }
        self.login.captcha = None;
        self.login.captcha_mock = false;
        self.login.captcha_attached = false;
        self.login.captcha_wait = None;
        self.login.captcha_note = None;
        self.login.status = "The captcha was closed.".into();
    }

    fn captcha_overlay(&mut self, cx: &mut Context<Self>, palette: Palette) -> impl IntoElement {
        let this = cx.entity();
        let note = self.login.captcha_note.clone();
        let mock = self.login.captcha_mock;
        let mut card = v_flex()
            .w(px(440.))
            .p(px(24.))
            .gap(px(14.))
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(palette.line))
            .bg(rgb(palette.elevated))
            .child(ink("Confirm you're human", palette.text, 20., true))
            .child(ink(
                "Discord asked for a check. Rencord does not solve it.",
                palette.muted,
                14.,
                false,
            ));
        if let Some(note) = note {
            card = card.child(wrapped_ink(note, palette.danger, 14.));
        }
        if mock {
            card = card.child(mock_hcaptcha(palette));
        } else {
            card = card.h(px(520.)).child(div().w_full().h(px(320.)));
        }
        card = card.child(
            h_flex()
                .gap(px(12.))
                .child(accent_button(
                    this.clone(),
                    "captcha-retry",
                    "Try again",
                    palette,
                    |view, _window, cx| {
                        view.login.captcha_note = None;
                        reload_embedded_captcha();
                        cx.notify();
                    },
                ))
                .child(text_button(
                    this.clone(),
                    "captcha-discord-page",
                    "Use Discord's page",
                    palette,
                    |view, _window, cx| {
                        view.finish_captcha_without_token();
                        view.open_webview(cx);
                    },
                ))
                .child(text_button(
                    this,
                    "captcha-close",
                    "Close",
                    palette,
                    |view, _window, cx| {
                        view.finish_captcha_without_token();
                        cx.notify();
                    },
                )),
        );
        div()
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(palette.overlay))
            .child(card)
    }

    fn switch_account(&mut self, user_id: &str, cx: &mut Context<Self>) {
        match self.vault.load_token(user_id) {
            Ok(Some(token)) => {
                let _ = self.vault.set_active_user(Some(user_id));
                self.begin_session(token, false, cx);
            }
            _ => {
                self.login.error = Some("That account has no saved session.".into());
                cx.notify();
            }
        }
    }

    pub(super) fn remove_account(&mut self, user_id: &str, cx: &mut Context<Self>) {
        if self
            .session_token
            .as_ref()
            .and_then(|_| self.active_user_id())
            .as_deref()
            == Some(user_id)
        {
            self.logout(cx);
            return;
        }
        if let Err(err) = self.vault.remove_account(user_id) {
            self.shell.note(err.public_message());
        }
        self.reload_accounts();
        cx.notify();
    }

    fn active_user_id(&self) -> Option<String> {
        self.shell
            .session
            .user
            .as_ref()
            .map(|user| user.id.to_string())
    }

    pub(super) fn logout(&mut self, cx: &mut Context<Self>) {
        let token = self.session_token.clone();
        let user_id = self.active_user_id();
        self.login.adding = false;
        self.drop_live_session();
        self.shell.session.user = None;
        self.session_machine.logged_out();
        if let Some(id) = &user_id {
            let _ = self.vault.remove_account(id);
        }
        self.reload_accounts();
        self.shell.note("Signed out.");
        if let Some(token) = token {
            net::runtime().spawn(async move {
                let _ = AuthClient::new().logout(&token).await;
            });
        }
        self.start_qr(cx);
    }

    pub(super) fn add_account(&mut self, cx: &mut Context<Self>) {
        self.login.adding = true;
        self.login.busy = false;
        self.start_qr(cx);
    }

    pub(super) fn note_unauthorized(&mut self, cx: &mut Context<Self>) {
        self.session_machine.restart();
        self.session_machine.unauthorized();
        if let Some(id) = self.active_user_id() {
            let _ = self.vault.remove_account(&id);
        }
        self.drop_live_session();
        self.shell.session.user = None;
        self.reload_accounts();
        self.shell.note("The session expired. Sign in again.");
        self.start_qr(cx);
        self.login.error = Some("This session is no longer valid. Sign in again.".into());
        self.login.status = "Signed out".into();
    }

    fn clear_secret_input(
        &self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        input.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
    }
}

fn accent_button(
    view: Entity<RencordWindow>,
    id: &'static str,
    label: &'static str,
    palette: Palette,
    action: impl Fn(&mut RencordWindow, &mut Window, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(40.))
        .px(px(16.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(rgb(palette.accent))
        .child(ink(label, palette.on_accent, 14., true))
        .on_click(move |_, window, cx| {
            view.update(cx, |this, cx| action(this, window, cx));
        })
}

fn text_button(
    view: Entity<RencordWindow>,
    id: impl Into<SharedString>,
    label: &'static str,
    palette: Palette,
    action: impl Fn(&mut RencordWindow, &mut Window, &mut Context<RencordWindow>) + 'static,
) -> impl IntoElement {
    div()
        .id(id.into())
        .cursor_pointer()
        .child(ink(label, palette.link, 13., false))
        .on_click(move |_, window, cx| {
            view.update(cx, |this, cx| action(this, window, cx));
        })
}

fn initial_bubble(initial: &str, palette: Palette, size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .rounded(px(size / 2.))
        .border_1()
        .border_color(rgb(palette.line))
        .bg(rgb(palette.app_bg))
        .flex()
        .items_center()
        .justify_center()
        .child(ink(initial, palette.text, size * 0.38, true))
}

fn mock_hcaptcha(palette: Palette) -> impl IntoElement {
    h_flex()
        .w(px(304.))
        .h(px(76.))
        .px(px(14.))
        .items_center()
        .gap(px(12.))
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(palette.line))
        .bg(rgb(palette.app_bg))
        .child(
            div()
                .size(px(28.))
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(palette.muted)),
        )
        .child(ink("I am human", palette.text, 16., false))
        .child(
            div()
                .flex_1()
                .flex()
                .justify_end()
                .child(ink("hCaptcha", palette.muted, 12., true)),
        )
}

fn wrapped_ink(text: impl AsRef<str>, color: u32, size: f32) -> impl IntoElement {
    div()
        .w_full()
        .min_w(px(0.))
        .whitespace_normal()
        .text_center()
        .text_size(px(size))
        .text_color(rgb(color))
        .line_height(px(size + 6.))
        .child(text.as_ref().to_owned())
}

fn ink(text: impl AsRef<str>, color: u32, size: f32, strong: bool) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .font_weight(if strong {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .line_height(px(size + 6.))
        .child(text.as_ref().to_owned())
}

fn mono(text: impl AsRef<str>, palette: Palette, size: f32) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(rgb(palette.muted))
        .font_family(palette.font_mono)
        .line_height(px(size + 6.))
        .child(text.as_ref().to_owned())
}

/// The left card is the same square in every sign-in state.
fn stage(face: impl IntoElement, phase: LoginPhase, palette: Palette) -> impl IntoElement {
    let paper = phase == LoginPhase::Qr;
    div()
        .w(px(280.))
        .h(px(280.))
        .flex_shrink_0()
        .rounded(px(12.))
        .border_1()
        .border_color(rgb(palette.line))
        .bg(rgb(if paper { palette.qr_bg } else { palette.app_bg }))
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .child(face)
}

fn hexwave_mark(color: u32, size: f32, scale: f32) -> impl IntoElement {
    let png = rencord_desktop::hexwave_png(color, size, scale);
    let image = Image::from_bytes(ImageFormat::Png, png);
    img(std::sync::Arc::new(image))
        .size(px(size))
        .object_fit(ObjectFit::Contain)
        .flex_shrink_0()
}

fn wait_spinner(palette: Palette) -> impl IntoElement {
    let mut dots = div().relative().size(px(16.)).flex_shrink_0();
    for index in 0..8 {
        let angle = index as f32 / 8.0 * std::f32::consts::TAU;
        let x = 7.0 + angle.cos() * 5.5;
        let y = 7.0 + angle.sin() * 5.5;
        dots = dots.child(
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .size(px(2.5))
                .rounded(px(2.))
                .bg(rgb(palette.muted))
                .with_animation(
                    SharedString::from(format!("login-wait-{index}")),
                    Animation::new(std::time::Duration::from_millis(960)).repeat(),
                    move |dot, delta| {
                        let phase = (delta + index as f32 / 8.0).fract();
                        let opacity = if phase < 0.18 { 1.0 } else { 0.28 };
                        dot.opacity(opacity)
                    },
                ),
        );
    }
    dots
}

fn bubble_initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|ch| ch.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
}

fn image_from_bytes(bytes: &[u8]) -> Option<Arc<Image>> {
    let format = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        ImageFormat::Png
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        ImageFormat::Jpeg
    } else if bytes.starts_with(b"GIF") {
        ImageFormat::Gif
    } else {
        return None;
    };
    Some(Arc::new(Image::from_bytes(format, bytes.to_vec())))
}
