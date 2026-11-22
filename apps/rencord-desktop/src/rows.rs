//! Message-row pieces: replies, attachments, link previews, system lines.
//! They read the message's structured fields; none of them parse text prefixes.

use super::*;
use rencord_model::{Attachment, Embed, Reply, User};

/// A downloaded picture and its size, so a frame can keep the right shape.
#[derive(Clone)]
pub(super) struct Picture {
    pub image: Arc<Image>,
    pub width: u32,
    pub height: u32,
}

/// What a row needs besides the message itself.
pub(super) struct RowEnv {
    /// Pictures already downloaded for this row, by address.
    pub pics: HashMap<String, Picture>,
    pub me: Option<Snowflake>,
    pub demo: bool,
    pub channels: Arc<HashMap<Snowflake, String>>,
    /// How each author in view is named and coloured in this server.
    pub authors: Arc<HashMap<Snowflake, rencord_desktop::AuthorStyle>>,
}

impl RowEnv {
    fn pic(&self, url: &str) -> Option<Picture> {
        self.pics.get(url).cloned()
    }
}

/// Every picture address a row may draw, so the window can fetch them.
pub(super) fn row_pictures(message: &Message) -> Vec<String> {
    let mut urls = Vec::new();
    for attachment in message.attachments.iter().filter(|a| a.is_image()) {
        urls.push(attachment_source(attachment));
    }
    for embed in &message.embeds {
        if let Some(image) = embed.image.as_ref().or(embed.thumbnail.as_ref()) {
            urls.push(image.best_url().to_owned());
        }
    }
    if let Some(author) = message.reply_to.as_ref().and_then(|r| r.author.as_ref()) {
        urls.push(user_picture_url(author));
    }
    urls.retain(|url| !url.is_empty() && !url.starts_with("demo://"));
    urls
}

pub(super) fn attachment_source(attachment: &Attachment) -> String {
    rencord_desktop::attachment_url(&attachment.url, attachment.proxy_url.as_deref())
}

pub(super) fn user_picture_url(user: &User) -> String {
    match user.avatar.as_deref() {
        Some(hash) => rencord_desktop::user_avatar_url(user.id.get(), hash),
        None => rencord_desktop::default_avatar_url(user.id.get(), user.discriminator.as_deref()),
    }
}

pub(super) fn presence_color(status: rencord_model::PresenceStatus, palette: Palette) -> u32 {
    use rencord_model::PresenceStatus;
    match status {
        PresenceStatus::Online => palette.status_online,
        PresenceStatus::Idle => palette.status_idle,
        PresenceStatus::Dnd => palette.status_dnd,
        PresenceStatus::Offline => palette.status_offline,
    }
}

fn first_letter(name: &str) -> String {
    name.chars()
        .next()
        .map(|letter| letter.to_uppercase().to_string())
        .unwrap_or_default()
}

pub(super) fn letter_circle(name: &str, size: f32, palette: Palette) -> AnyElement {
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(size / 2.))
        .bg(rgb(palette.elevated))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size * 0.42))
        .text_color(rgb(palette.muted))
        .child(first_letter(name))
        .into_any_element()
}

/// Opens a link from a message in the browser. Only web addresses are opened.
pub(super) fn open_link(url: &str, cx: &mut App) {
    if url.starts_with("https://") || url.starts_with("http://") {
        cx.open_url(url);
    }
}

/// A line such as "Ada joined the server." for a message with no text of its own.
pub(super) fn system_row(
    message: &Message,
    stamp: &rencord_desktop::Presented,
    palette: Palette,
) -> AnyElement {
    h_flex()
        .w_full()
        .px(px(16.))
        .py(px(6.))
        .gap(px(10.))
        .items_center()
        .child(
            div()
                .w(px(32.))
                .flex()
                .justify_end()
                .text_size(px(16.))
                .text_color(rgb(palette.status_online))
                .child("\u{2192}"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(14.))
                .text_color(rgb(palette.text_secondary))
                .child(rencord_desktop::system_text(message)),
        )
        .child(meta(stamp.short.clone(), palette.muted, 11., palette))
        .into_any_element()
}

pub(super) fn reply_header(reply: &Reply, env: &RowEnv, palette: Palette) -> AnyElement {
    let preview = rencord_desktop::reply_preview(reply);
    let mut line = h_flex()
        .gap(px(6.))
        .items_center()
        .child(icon("reply", 14., palette.muted));
    if let Some(author) = &reply.author {
        line = line.child(match (env.demo, env.pic(&user_picture_url(author))) {
            (false, Some(picture)) => img(picture.image)
                .size(px(16.))
                .flex_shrink_0()
                .rounded(px(8.))
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            _ => letter_circle(author.display_name(), 16., palette),
        });
    }
    if let Some(who) = preview.who {
        line = line.child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(palette.text_secondary))
                .child(format!("@{who}")),
        );
    }
    line.child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(12.))
            .text_color(rgb(palette.muted))
            .child(preview.text),
    )
    .into_any_element()
}

/// A picture in a rounded frame that opens `link` when clicked. While the picture
/// downloads, the frame keeps the size it will have and shows `label`.
#[allow(clippy::too_many_arguments)]
pub(super) fn picture_frame(
    key: SharedString,
    picture: Option<Picture>,
    known: (u32, u32),
    label: &str,
    link: &str,
    max: (f32, f32),
    palette: Palette,
) -> AnyElement {
    let (width, height) = match (&picture, known) {
        (Some(picture), _) => {
            rencord_desktop::fit_size(picture.width, picture.height, max.0, max.1)
        }
        (None, (width, height)) if width > 0 && height > 0 => {
            rencord_desktop::fit_size(width, height, max.0, max.1)
        }
        _ => (max.0.min(320.), max.1.min(180.)),
    };
    let link = link.to_owned();
    let frame = div()
        .id(key)
        .mt(px(4.))
        .w(px(width))
        .h(px(height))
        .flex_shrink_0()
        .rounded(px(8.))
        .overflow_hidden()
        .border_1()
        .border_color(rgb(palette.line))
        .bg(rgb(palette.elevated))
        .cursor_pointer()
        .on_click(move |_, _, cx| open_link(&link, cx));
    match picture {
        Some(picture) => frame
            .child(
                img(picture.image)
                    .size_full()
                    .rounded(px(8.))
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        None => frame
            .flex()
            .items_center()
            .justify_center()
            .child(paint(label.to_owned(), palette.muted, 12.))
            .into_any_element(),
    }
}

/// A video is not played here: its preview frame, if there is one, and a link
/// that opens it in the browser.
pub(super) fn video_card(
    key: SharedString,
    title: &str,
    url: &str,
    size: Option<u64>,
    preview: Option<Picture>,
    palette: Palette,
) -> AnyElement {
    let link = url.to_owned();
    let surface = match preview {
        Some(picture) => {
            let (width, height) =
                rencord_desktop::fit_size(picture.width, picture.height, 400., 300.);
            div()
                .w(px(width))
                .h(px(height))
                .rounded(px(8.))
                .overflow_hidden()
                .child(
                    img(picture.image)
                        .size_full()
                        .rounded(px(8.))
                        .object_fit(ObjectFit::Cover),
                )
                .into_any_element()
        }
        None => {
            let label = match size {
                Some(bytes) => {
                    format!("{title}  \u{00B7}  {}", rencord_upload::format_bytes(bytes))
                }
                None => title.to_owned(),
            };
            div()
                .w(px(400.))
                .h(px(225.))
                .rounded(px(8.))
                .bg(rgb(palette.elevated))
                .border_1()
                .border_color(rgb(palette.line))
                .flex()
                .items_center()
                .justify_center()
                .child(paint(label, palette.muted, 12.))
                .into_any_element()
        }
    };
    v_flex()
        .mt(px(4.))
        .gap(px(4.))
        .child(surface)
        .child(paint(
            "Video preview \u{00B7} playback opens in your browser",
            palette.muted,
            12.,
        ))
        .child(
            div()
                .id(key)
                .cursor_pointer()
                .text_size(px(13.))
                .text_color(rgb(palette.link))
                .child("Open video\u{2026}")
                .on_click(move |_, _, cx| open_link(&link, cx)),
        )
        .into_any_element()
}

pub(super) fn file_chip(
    key: SharedString,
    attachment: &Attachment,
    palette: Palette,
) -> AnyElement {
    let link = attachment.url.clone();
    h_flex()
        .id(key)
        .mt(px(4.))
        .gap(px(10.))
        .items_center()
        .px(px(12.))
        .py(px(8.))
        .max_w(px(400.))
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(palette.line))
        .bg(rgb(palette.elevated))
        .cursor_pointer()
        .child(icon("copy", 20., palette.muted))
        .child(
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(14.))
                        .text_color(rgb(palette.link))
                        .child(attachment.filename.clone()),
                )
                .child(meta(
                    rencord_upload::format_bytes(attachment.size),
                    palette.muted,
                    11.,
                    palette,
                )),
        )
        .on_click(move |_, _, cx| open_link(&link, cx))
        .into_any_element()
}

pub(super) fn attachment_view(
    attachment: &Attachment,
    message_id: Snowflake,
    index: usize,
    env: &RowEnv,
    palette: Palette,
) -> AnyElement {
    let key = SharedString::from(format!("att-{message_id}-{index}"));
    if attachment.url.starts_with("demo://") {
        return div()
            .id(key)
            .mt(px(4.))
            .w(px(320.))
            .h(px(76.))
            .rounded(px(8.))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(palette.line))
            .child(bench_photo())
            .into_any_element();
    }
    let source = attachment
        .proxy_url
        .clone()
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| attachment.url.clone());
    if attachment.is_image() {
        let picture = env.pic(&source);
        return picture_frame(
            key,
            picture,
            (
                attachment.width.unwrap_or(0),
                attachment.height.unwrap_or(0),
            ),
            &attachment.filename,
            &attachment.url,
            (400., 300.),
            palette,
        );
    }
    if attachment.is_video() {
        return video_card(
            key,
            &attachment.filename,
            &attachment.url,
            Some(attachment.size),
            None,
            palette,
        );
    }
    file_chip(key, attachment, palette)
}

/// A link preview. Returns `None` for an embed with nothing to show.
pub(super) fn embed_view(
    embed: &Embed,
    message_id: Snowflake,
    index: usize,
    env: &RowEnv,
    palette: Palette,
) -> Option<AnyElement> {
    let image = rencord_desktop::embed_picture(embed);
    if !rencord_desktop::embed_has_body(embed) {
        return None;
    }
    let key = SharedString::from(format!("embed-{message_id}-{index}"));
    let link = embed.url.clone();
    let picture = image.and_then(|image| env.pic(image.best_url()));
    let video = embed.video.is_some() || matches!(embed.kind.as_deref(), Some("video" | "gifv"));
    let has_text = embed.title.is_some() || embed.description.is_some();
    // A direct link to a picture is shown as the picture alone.
    if !has_text && !video {
        let image = image?;
        return Some(picture_frame(
            key,
            picture,
            (image.width.unwrap_or(0), image.height.unwrap_or(0)),
            "image",
            link.as_deref().unwrap_or(&image.url),
            (400., 300.),
            palette,
        ));
    }
    let bar = embed
        .color
        .map_or(palette.accent, |color| color & 0x00FF_FFFF);
    let mut card = v_flex().gap(px(4.));
    if let Some(name) = embed
        .provider
        .as_ref()
        .and_then(|provider| provider.name.as_deref())
        .filter(|name| !name.is_empty())
    {
        card = card.child(paint(name.to_owned(), palette.muted, 11.));
    }
    if let Some(title) = embed.title.as_deref().filter(|title| !title.is_empty()) {
        let title = div()
            .id(key.clone())
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(palette.link))
            .child(title.to_owned());
        card = card.child(match link.clone() {
            Some(url) => title
                .cursor_pointer()
                .on_click(move |_, _, cx| open_link(&url, cx))
                .into_any_element(),
            None => title.into_any_element(),
        });
    }
    if let Some(text) = embed.description.as_deref().filter(|text| !text.is_empty()) {
        card = card.child(
            div()
                .text_size(px(13.))
                .line_height(px(18.))
                .line_clamp(4)
                .text_color(rgb(palette.text_secondary))
                .child(text.to_owned()),
        );
    }
    if let Some(picture) = picture {
        let (width, height) = rencord_desktop::fit_size(picture.width, picture.height, 360., 240.);
        card = card.child(
            div()
                .mt(px(4.))
                .w(px(width))
                .h(px(height))
                .rounded(px(6.))
                .overflow_hidden()
                .child(
                    img(picture.image)
                        .size_full()
                        .rounded(px(6.))
                        .object_fit(ObjectFit::Cover),
                ),
        );
    }
    if video {
        card = card.child(paint(
            "Video preview \u{00B7} playback opens in your browser",
            palette.muted,
            12.,
        ));
        if let Some(url) = link {
            card = card.child(
                div()
                    .id(SharedString::from(format!(
                        "embed-open-{message_id}-{index}"
                    )))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(rgb(palette.link))
                    .child("Open video\u{2026}")
                    .on_click(move |_, _, cx| open_link(&url, cx)),
            );
        }
    }
    Some(
        div()
            .mt(px(4.))
            .max_w(px(432.))
            .rounded(px(6.))
            .bg(rgb(palette.elevated))
            .border_1()
            .border_color(rgb(palette.line))
            .child(
                div()
                    .border_l_4()
                    .border_color(rgb(bar))
                    .px(px(14.))
                    .py(px(10.))
                    .child(card),
            )
            .into_any_element(),
    )
}
