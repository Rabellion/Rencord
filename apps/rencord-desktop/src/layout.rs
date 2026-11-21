//! Window breakpoints, column constraints, and placement.
//!
//! The desktop shell asks this module what to draw. Width, height, and aspect
//! ratio are the inputs. Column mins, the reading measure, and the splitter
//! limits live here so the view does not grow its own magic numbers.
//!
//! Logical pixels are the unit, the same ones GPUI reports after DPI scale.
//! A 1440×900 monitor at 200% is a 720×450 logical window.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{json, Value};

/// Width of the glyph "0" in Inter, as a fraction of the font size.
/// The reading measure is counted in these `ch` units.
pub const CH_EM: f32 = 0.62;

/// Target line length. Kept inside 80–100ch.
pub const READING_CH: f32 = 92.0;

/// Avatar, gap, and row padding that sit beside the message text.
pub const MESSAGE_GUTTER: f32 = 72.0;

/// Chat column must stay at least this wide before the member list drops.
pub const CHAT_MIN: f32 = 420.0;

/// Chat column must stay at least this wide before the channel list drops.
pub const CHAT_FLOOR: f32 = 280.0;

pub const CHANNELS_MIN: f32 = 168.0;
pub const CHANNELS_MAX: f32 = 360.0;
pub const MEMBERS_MIN: f32 = 156.0;
pub const MEMBERS_MAX: f32 = 360.0;
pub const RIGHT_MIN: f32 = 300.0;
pub const RIGHT_MAX: f32 = 4200.0;

/// Inner message cap starts once a flexible chat column is wider than this.
/// 1080p and 1440×900 stay full-bleed; 4K and ultrawide do not.
pub const INNER_CAP_CHAT: f32 = 1680.0;

/// Member list needs a column at least this tall.
pub const MIN_MEMBER_HEIGHT: f32 = 560.0;

/// Channel list needs a column at least this tall.
pub const MIN_CHANNEL_HEIGHT: f32 = 420.0;

pub const TOUCH_TARGET: f32 = 44.0;

/// Sizes the responsive pass has to keep honest.
pub const CHECK_SIZES: &[(f32, f32)] = &[
    (3840.0, 2160.0),
    (2560.0, 1440.0),
    (1920.0, 1080.0),
    (1440.0, 900.0),
    (1366.0, 768.0),
    (1280.0, 800.0),
    (1280.0, 720.0),
    (1000.0, 700.0),
    (800.0, 600.0),
    (2560.0, 1080.0),
    (3440.0, 1440.0),
    (5120.0, 1440.0),
    (1080.0, 1920.0),
    (900.0, 1440.0),
    (768.0, 1366.0),
    (800.0, 1280.0),
];

/// Captures written to `docs/design/onyx/responsive/`.
pub const SCREENSHOT_SIZES: &[(&str, f32, f32)] = &[
    ("1920x1080", 1920.0, 1080.0),
    ("1080x1920", 1080.0, 1920.0),
    ("3440x1440", 3440.0, 1440.0),
    ("5120x1440", 5120.0, 1440.0),
    ("1000x700", 1000.0, 700.0),
    ("1440x900", 1440.0, 900.0),
    ("900x1440", 900.0, 1440.0),
    ("1366x768", 1366.0, 768.0),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AspectClass {
    /// Taller than it is wide (9:16, 900×1440, and nearby).
    Portrait,
    /// 4:3, 16:10, and 16:9.
    Landscape,
    /// About 21:9.
    Wide,
    /// About 32:9.
    Ultrawide,
}

impl AspectClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
            Self::Wide => "wide",
            Self::Ultrawide => "ultrawide",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnMode {
    /// Rail, channels, chat, and members.
    Classic,
    /// Rail, channels, and chat. Members are an overlay.
    NoMembers,
    /// Rail and chat. Channels are a drawer.
    ChannelsDrawer,
    /// Portrait: narrow rail, chat, drawers for the lists.
    Portrait,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingAnchor {
    Left,
    Center,
}

impl ReadingAnchor {
    pub fn toggle(self) -> Self {
        match self {
            Self::Left => Self::Center,
            Self::Center => Self::Left,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "center" => Self::Center,
            _ => Self::Left,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightPanel {
    Thread,
    Pinned,
    Profile,
    Stream,
}

impl RightPanel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Thread => "thread",
            Self::Pinned => "pinned",
            Self::Profile => "profile",
            Self::Stream => "stream",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Thread => "Thread",
            Self::Pinned => "Pinned",
            Self::Profile => "Profile",
            Self::Stream => "Stream",
        }
    }
}

/// What the extra right-hand region is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightChoice {
    /// No extra panel. A stream opens only when one is actually on screen.
    Auto,
    Open(RightPanel),
    Closed,
}

impl RightChoice {
    pub fn toggle_pin(self) -> Self {
        match self {
            Self::Open(RightPanel::Pinned) => Self::Auto,
            _ => Self::Open(RightPanel::Pinned),
        }
    }

    pub fn displayed(self, _aspect: AspectClass) -> Option<RightPanel> {
        match self {
            Self::Closed | Self::Auto => None,
            Self::Open(panel) => Some(panel),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitEdge {
    Channels,
    Members,
    Right,
}

/// Theme-facing column defaults. The rail is 72 px. A gap above zero is the floating-panel option.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnMetrics {
    pub rail: f32,
    pub channels: f32,
    pub members: f32,
    pub gap: f32,
}

impl ColumnMetrics {
    pub fn onyx() -> Self {
        Self {
            rail: 72.0,
            channels: 240.0,
            members: 240.0,
            gap: 0.0,
        }
    }

    pub fn crisp() -> Self {
        Self {
            rail: 72.0,
            channels: 240.0,
            members: 240.0,
            gap: 0.0,
        }
    }
}

/// Widths the user has dragged. `None` means the metric default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SavedColumns {
    pub channels: Option<f32>,
    pub members: Option<f32>,
    pub right: Option<f32>,
}

impl SavedColumns {
    pub fn reset(self, edge: SplitEdge) -> Self {
        let mut next = self;
        match edge {
            SplitEdge::Channels => next.channels = None,
            SplitEdge::Members => next.members = None,
            SplitEdge::Right => next.right = None,
        }
        next
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutInput {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub message_px: f32,
    pub show_members: bool,
    pub show_channels: bool,
    pub reading_anchor: ReadingAnchor,
    pub right: RightChoice,
    pub saved: SavedColumns,
    pub metrics: ColumnMetrics,
    pub monitor: Option<String>,
}

impl LayoutInput {
    pub fn for_size(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            scale: 1.0,
            message_px: 14.0,
            show_members: true,
            show_channels: true,
            reading_anchor: ReadingAnchor::Left,
            right: RightChoice::Auto,
            saved: SavedColumns::default(),
            metrics: ColumnMetrics::onyx(),
            monitor: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedLayout {
    pub aspect: AspectClass,
    pub mode: ColumnMode,
    pub scale_bucket: u16,
    pub profile_key: String,
    pub rail: f32,
    pub channels: f32,
    pub members: f32,
    pub right: f32,
    pub chat_width: f32,
    pub chat_grows: bool,
    pub reading_width: Option<f32>,
    pub reading_anchor: ReadingAnchor,
    pub channels_as_drawer: bool,
    pub members_as_overlay: bool,
    pub right_as_overlay: bool,
    pub voice_in_chat: bool,
    pub touch: bool,
    pub gap: f32,
    pub hide_topic: bool,
    pub drawer: f32,
    pub right_kind: Option<RightPanel>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragContext {
    pub viewport_width: f32,
    pub gap: f32,
    pub rail: f32,
    pub channels: f32,
    pub members: f32,
    pub right: f32,
    pub chat_fixed: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(self) -> f32 {
        self.y + self.h
    }
}

/// Classify `width / height`.
pub fn classify(ratio: f32) -> AspectClass {
    if !ratio.is_finite() || ratio < 0.9 {
        AspectClass::Portrait
    } else if ratio < 1.95 {
        AspectClass::Landscape
    } else if ratio < 2.7 {
        AspectClass::Wide
    } else {
        AspectClass::Ultrawide
    }
}

/// 100, 125, 150, 200, or the nearest 25% step.
pub fn scale_bucket(scale: f32) -> u16 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let percent = scale * 100.0;
    for (target, slack) in [(100.0, 8.0), (125.0, 8.0), (150.0, 8.0), (200.0, 12.0)] {
        if (percent - target).abs() < slack {
            return target as u16;
        }
    }
    ((percent / 25.0).round() * 25.0).clamp(50.0, 400.0) as u16
}

pub fn profile_key(aspect: AspectClass, scale: f32, monitor: Option<&str>) -> String {
    format!(
        "{}:{}:{}",
        aspect.as_str(),
        scale_bucket(scale),
        monitor.unwrap_or("primary")
    )
}

/// Message text width in logical pixels, at [`READING_CH`].
pub fn reading_px(message_px: f32) -> f32 {
    let px = if message_px.is_finite() && message_px > 0.0 {
        message_px
    } else {
        14.0
    };
    px * CH_EM * READING_CH
}

/// Chat column that holds a 92ch line plus the avatar gutter.
pub fn reading_column_px(message_px: f32) -> f32 {
    reading_px(message_px) + MESSAGE_GUTTER
}

/// Outer padding plus the gap between each pair of columns.
pub fn shell_chrome(columns: u8, gap: f32) -> f32 {
    gap * (columns as f32 + 1.0)
}

pub fn resolve(input: &LayoutInput) -> ResolvedLayout {
    let width = if input.width.is_finite() {
        input.width.max(1.0)
    } else {
        1.0
    };
    let height = if input.height.is_finite() {
        input.height.max(1.0)
    } else {
        1.0
    };
    let gap = input.metrics.gap.max(0.0);
    let rail_w = input.metrics.rail.max(0.0);
    let aspect = classify(width / height);
    let scale_bucket = scale_bucket(input.scale);
    let profile_key = profile_key(aspect, input.scale, input.monitor.as_deref());
    let kind = input.right.displayed(aspect);
    let touch = aspect == AspectClass::Portrait;

    if aspect == AspectClass::Portrait {
        return portrait_layout(input, width, gap, rail_w, aspect, scale_bucket, profile_key);
    }

    let tall_enough_members = height >= MIN_MEMBER_HEIGHT;
    let tall_enough_channels = height >= MIN_CHANNEL_HEIGHT;
    let classic_need = rail_w + CHANNELS_MIN + MEMBERS_MIN + CHAT_MIN + shell_chrome(4, gap);
    let three_need = rail_w + CHANNELS_MIN + CHAT_FLOOR + shell_chrome(3, gap);
    let members_can = tall_enough_members && width + 0.5 >= classic_need;
    let channels_can = tall_enough_channels && width + 0.5 >= three_need;
    let members_fit = input.show_members && members_can;
    let channels_fit = input.show_channels && channels_can;

    let mut channels = if channels_fit {
        input
            .saved
            .channels
            .unwrap_or(input.metrics.channels)
            .clamp(CHANNELS_MIN, CHANNELS_MAX)
    } else {
        0.0
    };
    let mut members = if members_fit {
        input
            .saved
            .members
            .unwrap_or(input.metrics.members)
            .clamp(MEMBERS_MIN, MEMBERS_MAX)
    } else {
        0.0
    };
    let mut rail = rail_w;
    if width + 0.5 < rail_w + CHAT_FLOOR + shell_chrome(2, gap) {
        rail = 0.0;
        channels = 0.0;
    }

    let wide = matches!(aspect, AspectClass::Wide | AspectClass::Ultrawide);
    let chat_fixed_target = reading_column_px(input.message_px).clamp(760.0, 980.0);
    let mut right = 0.0;
    let mut chat_grows = true;
    let chat_width;
    let mut right_overlay = false;

    if wide && channels > 0.0 && members > 0.0 {
        let members_used = members.clamp(MEMBERS_MIN, MEMBERS_MAX);
        if kind.is_some() {
            let chrome = shell_chrome(5, gap);
            let leftover = width - chrome - rail - channels - chat_fixed_target - members_used;
            if leftover >= RIGHT_MIN {
                members = members_used;
                right = leftover.min(RIGHT_MAX);
                chat_width = chat_fixed_target;
                chat_grows = false;
            } else {
                chat_width = flex_chat(width, gap, rail, &mut channels, &mut members, &mut right);
                if right == 0.0 {
                    right_overlay = true;
                }
            }
        } else {
            // Four columns at a reading width. The spare canvas is a gutter,
            // not a stream panel.
            let chrome = shell_chrome(4, gap);
            let room = width - chrome - rail - channels - members_used;
            if room >= CHAT_MIN {
                members = members_used;
                chat_width = chat_fixed_target.min(room);
                chat_grows = false;
            } else {
                chat_width = flex_chat(width, gap, rail, &mut channels, &mut members, &mut right);
            }
        }
    } else {
        if kind.is_some() && channels > 0.0 {
            let trial_right = input
                .saved
                .right
                .unwrap_or(RIGHT_MIN)
                .clamp(RIGHT_MIN, 480.0);
            let cols = column_count(rail, channels, members, trial_right);
            let room = width - shell_chrome(cols, gap) - rail - channels - members - trial_right;
            if room >= 640.0 {
                right = trial_right;
            } else {
                right_overlay = true;
            }
        }
        chat_width = flex_chat(width, gap, rail, &mut channels, &mut members, &mut right);
        if right == 0.0 && kind.is_some() && !wide {
            right_overlay = true;
        }
    }

    let reading_width = if !chat_grows || chat_width > INNER_CAP_CHAT {
        Some(reading_px(input.message_px))
    } else {
        None
    };

    let mode = if channels == 0.0 {
        ColumnMode::ChannelsDrawer
    } else if members == 0.0 {
        ColumnMode::NoMembers
    } else {
        ColumnMode::Classic
    };

    ResolvedLayout {
        aspect,
        mode,
        scale_bucket,
        profile_key,
        rail,
        channels,
        members,
        right,
        chat_width: chat_width.max(0.0),
        chat_grows,
        reading_width,
        reading_anchor: input.reading_anchor,
        channels_as_drawer: !channels_can,
        members_as_overlay: !members_can,
        right_as_overlay: right_overlay && right == 0.0,
        voice_in_chat: channels == 0.0,
        touch,
        gap,
        hide_topic: chat_width < 760.0 || channels == 0.0,
        drawer: drawer_width(width, rail, gap),
        right_kind: if right > 0.0 || right_overlay {
            kind
        } else {
            None
        },
    }
}

fn portrait_layout(
    input: &LayoutInput,
    width: f32,
    gap: f32,
    rail_w: f32,
    aspect: AspectClass,
    scale_bucket: u16,
    profile_key: String,
) -> ResolvedLayout {
    let kind = input.right.displayed(aspect);
    let show_rail = width + 0.5 >= rail_w + CHAT_FLOOR + shell_chrome(2, gap);
    let rail = if show_rail { rail_w } else { 0.0 };
    let channel_room = width - shell_chrome(3, gap) - rail - CHAT_FLOOR;
    let channels = if channel_room + 0.5 >= CHANNELS_MIN {
        channel_room.min(240.0)
    } else {
        0.0
    };
    let cols = 1 + (rail > 0.0) as u8 + (channels > 0.0) as u8;
    let chat = (width - shell_chrome(cols, gap) - rail - channels).max(0.0);
    ResolvedLayout {
        aspect,
        mode: ColumnMode::Portrait,
        scale_bucket,
        profile_key,
        rail,
        channels,
        members: 0.0,
        right: 0.0,
        chat_width: chat,
        chat_grows: true,
        reading_width: if chat > INNER_CAP_CHAT {
            Some(reading_px(input.message_px))
        } else {
            None
        },
        reading_anchor: input.reading_anchor,
        channels_as_drawer: channels == 0.0,
        members_as_overlay: true,
        right_as_overlay: kind.is_some(),
        voice_in_chat: true,
        touch: true,
        gap,
        hide_topic: true,
        drawer: drawer_width(width, rail, gap),
        right_kind: kind,
    }
}

fn flex_chat(
    width: f32,
    gap: f32,
    rail: f32,
    channels: &mut f32,
    members: &mut f32,
    right: &mut f32,
) -> f32 {
    let mut chat = raw_chat(width, gap, rail, *channels, *members, *right);
    let floor = if *members > 0.0 { CHAT_MIN } else { CHAT_FLOOR };
    if chat < floor && *right > 0.0 {
        let need = floor - chat;
        let shrink = need.min((*right - RIGHT_MIN).max(0.0));
        *right -= shrink;
        chat += shrink;
        if *right < RIGHT_MIN - 0.5 {
            chat += *right;
            *right = 0.0;
        }
    }
    if chat < floor && *members > 0.0 {
        let need = floor - chat;
        let shrink = need.min((*members - MEMBERS_MIN).max(0.0));
        *members -= shrink;
        chat += shrink;
    }
    if chat < CHAT_FLOOR && *channels > 0.0 {
        let need = CHAT_FLOOR - chat;
        let shrink = need.min((*channels - CHANNELS_MIN).max(0.0));
        *channels -= shrink;
        chat += shrink;
    }
    chat
}

fn raw_chat(width: f32, gap: f32, rail: f32, channels: f32, members: f32, right: f32) -> f32 {
    let cols = column_count(rail, channels, members, right);
    width - shell_chrome(cols, gap) - rail - channels - members - right
}

fn column_count(rail: f32, channels: f32, members: f32, right: f32) -> u8 {
    1 + shown(rail) + shown(channels) + shown(members) + shown(right)
}

fn shown(value: f32) -> u8 {
    if value > 0.5 {
        1
    } else {
        0
    }
}

pub fn drawer_width(viewport: f32, rail: f32, gap: f32) -> f32 {
    let room = (viewport - rail - gap * 3.0).max(160.0);
    room.clamp(200.0, 320.0).min(room)
}

/// Clamp a dragged column so the flexible region keeps its floor.
///
/// When the chat column is flexible, the drag gives way to it. On a wide
/// frame the chat column is fixed and the right-hand panel is the flexible
/// region, so a drag gives way to that panel instead.
pub fn apply_drag(edge: SplitEdge, proposed: f32, ctx: &DragContext) -> f32 {
    let (min, max) = match edge {
        SplitEdge::Channels => (CHANNELS_MIN, CHANNELS_MAX),
        SplitEdge::Members => (MEMBERS_MIN, MEMBERS_MAX),
        SplitEdge::Right => (RIGHT_MIN, RIGHT_MAX),
    };
    let value = if proposed.is_finite() {
        proposed.clamp(min, max)
    } else {
        min
    };
    let chat_is_flex = ctx.chat_fixed.is_none();
    let reserved_chat = ctx.chat_fixed.unwrap_or(0.0);
    let peers = match edge {
        SplitEdge::Channels => {
            ctx.rail
                + ctx.members
                + if chat_is_flex {
                    ctx.right
                } else {
                    reserved_chat
                }
        }
        SplitEdge::Members => {
            ctx.rail
                + ctx.channels
                + if chat_is_flex {
                    ctx.right
                } else {
                    reserved_chat
                }
        }
        SplitEdge::Right => ctx.rail + ctx.channels + ctx.members + reserved_chat,
    };
    let columns = match edge {
        SplitEdge::Channels => column_count(
            ctx.rail,
            1.0,
            ctx.members,
            if chat_is_flex { ctx.right } else { 1.0 },
        ),
        SplitEdge::Members => column_count(
            ctx.rail,
            ctx.channels,
            1.0,
            if chat_is_flex { ctx.right } else { 1.0 },
        ),
        SplitEdge::Right => column_count(ctx.rail, ctx.channels, ctx.members, 1.0),
    };
    let flex_floor = if chat_is_flex {
        match edge {
            SplitEdge::Channels if ctx.members <= 0.0 => CHAT_FLOOR,
            _ => CHAT_MIN,
        }
    } else {
        RIGHT_MIN
    };
    let flex = ctx.viewport_width - shell_chrome(columns, ctx.gap) - peers - value;
    if flex < flex_floor {
        return (ctx.viewport_width - shell_chrome(columns, ctx.gap) - peers - flex_floor)
            .clamp(min, max);
    }
    value
}

pub fn reset_width(edge: SplitEdge, metrics: ColumnMetrics) -> f32 {
    match edge {
        SplitEdge::Channels => metrics.channels,
        SplitEdge::Members => metrics.members,
        SplitEdge::Right => RIGHT_MIN,
    }
}

/// Round a logical pixel so it lands on a device pixel.
pub fn snap_device_px(value: f32, scale: f32) -> f32 {
    if !scale.is_finite() || scale <= 0.0 || !value.is_finite() {
        return value;
    }
    (value * scale).round() / scale
}

fn snap_floor(value: f32, scale: f32) -> f32 {
    if !scale.is_finite() || scale <= 0.0 || !value.is_finite() {
        return value;
    }
    (value * scale).floor().max(1.0) / scale
}

/// Move `window` fully inside the monitor it overlaps most.
///
/// An empty monitor list keeps a positive size and refuses a negative origin.
/// `scale` snaps the result onto device pixels (1, 1.25, 1.5, 2, …).
pub fn clamp_window(window: Rect, monitors: &[Rect], scale: f32) -> Rect {
    if monitors.is_empty() {
        return Rect {
            x: snap_device_px(window.x.max(0.0), scale),
            y: snap_device_px(window.y.max(0.0), scale),
            w: snap_device_px(window.w.max(480.0), scale),
            h: snap_device_px(window.h.max(360.0), scale),
        };
    }
    let monitor = best_monitor(window, monitors);
    let w = snap_floor(window.w.min(monitor.w).max(320.0).min(monitor.w), scale);
    let h = snap_floor(window.h.min(monitor.h).max(240.0).min(monitor.h), scale);
    let max_x = monitor.x + monitor.w - w;
    let max_y = monitor.y + monitor.h - h;
    let x = snap_device_px(window.x.clamp(monitor.x, max_x.max(monitor.x)), scale);
    let y = snap_device_px(window.y.clamp(monitor.y, max_y.max(monitor.y)), scale);
    let x = x.clamp(monitor.x, max_x.max(monitor.x));
    let y = y.clamp(monitor.y, max_y.max(monitor.y));
    Rect { x, y, w, h }
}

fn best_monitor(window: Rect, monitors: &[Rect]) -> Rect {
    let mut best = monitors[0];
    let mut best_area = -1.0;
    for monitor in monitors {
        let area = overlap(window, *monitor);
        if area > best_area {
            best_area = area;
            best = *monitor;
        }
    }
    if best_area > 0.0 {
        return best;
    }
    let center_x = window.x + window.w * 0.5;
    let center_y = window.y + window.h * 0.5;
    let mut nearest = monitors[0];
    let mut nearest_d = f32::MAX;
    for monitor in monitors {
        let dx = (monitor.x + monitor.w * 0.5) - center_x;
        let dy = (monitor.y + monitor.h * 0.5) - center_y;
        let dist = dx * dx + dy * dy;
        if dist < nearest_d {
            nearest_d = dist;
            nearest = *monitor;
        }
    }
    nearest
}

fn overlap(a: Rect, b: Rect) -> f32 {
    let w = (a.right().min(b.right()) - a.x.max(b.x)).max(0.0);
    let h = (a.bottom().min(b.bottom()) - a.y.max(b.y)).max(0.0);
    w * h
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub columns: SavedColumns,
    pub anchor: ReadingAnchor,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutStore {
    pub window: Option<Rect>,
    pub profiles: BTreeMap<String, Profile>,
}

impl LayoutStore {
    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        Self::from_json(&text)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_json())
    }

    pub fn from_json(text: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return Self::default();
        };
        let mut store = Self::default();
        if let Some(window) = value.get("window") {
            store.window = rect_from_json(window);
        }
        if let Some(profiles) = value.get("profiles").and_then(Value::as_object) {
            for (key, profile) in profiles {
                store
                    .profiles
                    .insert(key.clone(), profile_from_json(profile));
            }
        }
        store
    }

    pub fn to_json(&self) -> String {
        let mut profiles = serde_json::Map::new();
        for (key, profile) in &self.profiles {
            profiles.insert(
                key.clone(),
                json!({
                    "channels": profile.columns.channels,
                    "members": profile.columns.members,
                    "right": profile.columns.right,
                    "anchor": profile.anchor.as_str(),
                }),
            );
        }
        let window = self.window.map(|rect| {
            json!({
                "x": rect.x,
                "y": rect.y,
                "w": rect.w,
                "h": rect.h,
            })
        });
        json!({
            "window": window,
            "profiles": profiles,
        })
        .to_string()
    }
}

fn rect_from_json(value: &Value) -> Option<Rect> {
    Some(Rect {
        x: value.get("x")?.as_f64()? as f32,
        y: value.get("y")?.as_f64()? as f32,
        w: value.get("w")?.as_f64()? as f32,
        h: value.get("h")?.as_f64()? as f32,
    })
}

fn profile_from_json(value: &Value) -> Profile {
    let column = |name: &str| value.get(name).and_then(Value::as_f64).map(|n| n as f32);
    Profile {
        columns: SavedColumns {
            channels: column("channels"),
            members: column("members"),
            right: column("right"),
        },
        anchor: ReadingAnchor::parse(
            value
                .get("anchor")
                .and_then(Value::as_str)
                .unwrap_or("left"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(width: f32, height: f32) -> ResolvedLayout {
        resolve(&LayoutInput::for_size(width, height))
    }

    fn near(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.6, "{actual} != {expected}");
    }

    #[test]
    fn reading_measure_stays_between_80_and_100_ch() {
        for px in [13.0, 14.0, 15.0, 17.0] {
            let ch = reading_px(px) / (px * CH_EM);
            assert!((80.0..=100.0).contains(&ch), "{px}px is {ch}ch");
        }
    }

    #[test]
    fn scale_buckets_cover_the_desktop_steps() {
        assert_eq!(scale_bucket(1.0), 100);
        assert_eq!(scale_bucket(1.25), 125);
        assert_eq!(scale_bucket(1.5), 150);
        assert_eq!(scale_bucket(2.0), 200);
        assert_ne!(
            profile_key(AspectClass::Landscape, 1.0, Some("a")),
            profile_key(AspectClass::Landscape, 2.0, Some("a"))
        );
        assert_ne!(
            profile_key(AspectClass::Landscape, 1.25, Some("a")),
            profile_key(AspectClass::Portrait, 1.25, Some("a"))
        );
    }

    #[test]
    fn published_sizes_pick_the_expected_mode() {
        let classic = [
            (3840.0, 2160.0),
            (1920.0, 1080.0),
            (1440.0, 900.0),
            (1366.0, 768.0),
            (1280.0, 800.0),
            (1280.0, 720.0),
            (1000.0, 700.0),
        ];
        for (width, height) in classic {
            let layout = plan(width, height);
            assert_eq!(layout.mode, ColumnMode::Classic, "{width}x{height}");
            assert!(layout.rail > 0.0);
            assert!(layout.channels >= CHANNELS_MIN);
            assert!(layout.members >= MEMBERS_MIN);
            assert!(
                layout.chat_width + 0.5 >= CHAT_MIN,
                "{width} chat {}",
                layout.chat_width
            );
            assert!(!layout.channels_as_drawer);
            assert!(!layout.members_as_overlay);
            assert!(!layout.voice_in_chat);
            assert_eq!(layout.aspect, AspectClass::Landscape);
        }

        let wide = [(2560.0, 1080.0), (3440.0, 1440.0)];
        for (width, height) in wide {
            let layout = plan(width, height);
            assert_eq!(layout.aspect, AspectClass::Wide, "{width}x{height}");
            assert_eq!(layout.mode, ColumnMode::Classic);
            assert!(!layout.chat_grows);
            assert_eq!(layout.right, 0.0, "{width}x{height} stays four columns");
            assert!(layout.right_kind.is_none());
            assert!(layout.chat_width < width * 0.5);
            let used = layout.rail + layout.channels + layout.chat_width + layout.members;
            assert!(width - used > 400.0, "gutter on {width}");
            let ch = layout.reading_width.unwrap() / (14.0 * CH_EM);
            assert!((80.0..=100.0).contains(&ch));
        }

        let ultra = plan(5120.0, 1440.0);
        assert_eq!(ultra.aspect, AspectClass::Ultrawide);
        assert_eq!(ultra.mode, ColumnMode::Classic);
        assert!(
            ultra.chat_width < 1100.0,
            "messages span {}",
            ultra.chat_width
        );
        assert_eq!(ultra.right, 0.0);
        assert!(ultra.right_kind.is_none());
        let used = ultra.rail + ultra.channels + ultra.chat_width + ultra.members;
        assert!(5120.0 - used > 1000.0, "ultrawide gutter");

        for (width, height) in [
            (1080.0, 1920.0),
            (900.0, 1440.0),
            (768.0, 1366.0),
            (800.0, 1280.0),
        ] {
            let layout = plan(width, height);
            assert_eq!(layout.mode, ColumnMode::Portrait, "{width}x{height}");
            assert_eq!(layout.aspect, AspectClass::Portrait);
            assert!(layout.rail > 0.0, "narrow rail");
            assert!(layout.channels >= CHANNELS_MIN, "{width}x{height}");
            assert_eq!(layout.members, 0.0);
            assert!(!layout.channels_as_drawer, "{width}x{height}");
            assert!(layout.members_as_overlay);
            assert!(layout.voice_in_chat);
            assert!(layout.touch);
            assert!(layout.chat_width > 400.0);
            assert!(layout.drawer >= 200.0 && layout.drawer <= 320.0);
        }

        let small = plan(800.0, 600.0);
        assert_eq!(small.mode, ColumnMode::NoMembers);
        assert!(small.channels >= CHANNELS_MIN);
        assert_eq!(small.members, 0.0);
        assert!(small.members_as_overlay);
        assert!(!small.voice_in_chat);
        assert!(small.chat_width + 0.5 >= CHAT_FLOOR);
    }

    #[test]
    fn stream_panel_opens_only_while_watching() {
        let idle = plan(3440.0, 1440.0);
        assert_eq!(idle.right, 0.0);
        assert!(idle.right_kind.is_none());
        let mut watching = LayoutInput::for_size(5120.0, 1440.0);
        watching.right = RightChoice::Open(RightPanel::Stream);
        let layout = resolve(&watching);
        assert_eq!(layout.right_kind, Some(RightPanel::Stream));
        assert!(layout.right > 2000.0, "watching uses {}", layout.right);
        assert!(layout.chat_width < 1100.0);
    }

    #[test]
    fn screenshot_sizes_are_part_of_the_matrix() {
        for (name, width, height) in SCREENSHOT_SIZES {
            let layout = plan(*width, *height);
            match *name {
                "1920x1080" | "1440x900" | "1366x768" | "1000x700" => {
                    assert_eq!(layout.mode, ColumnMode::Classic, "{name}");
                }
                "1080x1920" | "900x1440" => {
                    assert_eq!(layout.mode, ColumnMode::Portrait, "{name}");
                }
                "3440x1440" => {
                    assert_eq!(layout.aspect, AspectClass::Wide, "{name}");
                    assert_eq!(layout.right, 0.0);
                    assert!(layout.chat_width < 1100.0);
                }
                "5120x1440" => {
                    assert_eq!(layout.aspect, AspectClass::Ultrawide, "{name}");
                    assert_eq!(layout.right, 0.0);
                    assert!(layout.chat_width < 1100.0);
                }
                other => panic!("unexpected screenshot {other}"),
            }
        }
    }

    #[test]
    fn sixteen_by_ten_and_low_res_keep_four_columns() {
        for (width, height) in [(1440.0, 900.0), (1280.0, 800.0), (1366.0, 768.0)] {
            let layout = plan(width, height);
            assert_eq!(layout.mode, ColumnMode::Classic);
            assert!(
                layout.chat_grows,
                "{width}x{height} should keep a flexible chat"
            );
            assert!(layout.reading_width.is_none(), "no inner cap at {width}");
        }
    }

    #[test]
    fn four_k_caps_the_line_without_dropping_a_column() {
        let layout = plan(3840.0, 2160.0);
        assert_eq!(layout.mode, ColumnMode::Classic);
        assert!(layout.chat_grows);
        let reading = layout.reading_width.expect("4k reading cap");
        let ch = reading / (14.0 * CH_EM);
        assert!((80.0..=100.0).contains(&ch));
        assert!(layout.chat_width > reading);
    }

    #[test]
    fn logical_pixels_not_physical_ones() {
        let mut input = LayoutInput::for_size(1440.0, 900.0);
        input.scale = 2.0;
        let hidpi = resolve(&input);
        assert_eq!(hidpi.mode, ColumnMode::Classic);
        assert_eq!(hidpi.scale_bucket, 200);
        // The same glass at 200% is 720×450 logical pixels.
        let logical = plan(720.0, 450.0);
        assert_ne!(logical.mode, ColumnMode::Classic);
    }

    #[test]
    fn height_can_drop_columns_the_width_would_keep() {
        let short = plan(1920.0, 400.0);
        assert_eq!(short.aspect, AspectClass::Ultrawide);
        assert_eq!(short.channels, 0.0);
        assert_eq!(short.members, 0.0);
        assert!(short.channels_as_drawer);
    }

    #[test]
    fn hidden_lists_stay_hidden_until_asked() {
        let mut input = LayoutInput::for_size(1440.0, 900.0);
        input.show_members = false;
        let layout = resolve(&input);
        assert_eq!(layout.members, 0.0);
        assert!(!layout.members_as_overlay);
        input.show_channels = false;
        input.show_members = true;
        let layout = resolve(&input);
        assert_eq!(layout.channels, 0.0);
        assert!(layout.voice_in_chat);
    }

    #[test]
    fn splitter_drag_respects_min_max_and_chat_floor() {
        let metrics = ColumnMetrics::onyx();
        let ctx = DragContext {
            viewport_width: 1440.0,
            gap: metrics.gap,
            rail: metrics.rail,
            channels: metrics.channels,
            members: metrics.members,
            right: 0.0,
            chat_fixed: None,
        };
        near(apply_drag(SplitEdge::Channels, 10.0, &ctx), CHANNELS_MIN);
        near(apply_drag(SplitEdge::Channels, 5000.0, &ctx), CHANNELS_MAX);
        near(apply_drag(SplitEdge::Members, 1.0, &ctx), MEMBERS_MIN);
        near(apply_drag(SplitEdge::Members, 900.0, &ctx), MEMBERS_MAX);

        let tight = DragContext {
            viewport_width: 1000.0,
            ..ctx
        };
        let dragged = apply_drag(SplitEdge::Channels, CHANNELS_MAX, &tight);
        assert!(dragged + 0.5 < CHANNELS_MAX);
        assert!(dragged + 0.5 >= CHANNELS_MIN);
        let chat = 1000.0 - shell_chrome(4, metrics.gap) - metrics.rail - dragged - metrics.members;
        assert!(chat + 0.5 >= CHAT_MIN, "chat {chat}");

        assert!((reset_width(SplitEdge::Channels, metrics) - metrics.channels).abs() < 0.1);
        let saved = SavedColumns {
            channels: Some(360.0),
            members: Some(300.0),
            right: Some(400.0),
        };
        assert!(saved.reset(SplitEdge::Channels).channels.is_none());
        assert!(saved.reset(SplitEdge::Members).members.is_none());
        assert_eq!(saved.reset(SplitEdge::Channels).members, Some(300.0));
    }

    #[test]
    fn double_click_reset_restores_the_sixteen_by_ten_columns() {
        let mut input = LayoutInput::for_size(1440.0, 900.0);
        input.saved.channels = Some(CHANNELS_MAX);
        input.saved.members = Some(MEMBERS_MAX);
        let wide = resolve(&input);
        assert!(wide.channels > input.metrics.channels + 10.0);
        input.saved = input
            .saved
            .reset(SplitEdge::Channels)
            .reset(SplitEdge::Members);
        let restored = resolve(&input);
        near(restored.channels, input.metrics.channels);
        near(restored.members, input.metrics.members);
        assert_eq!(restored.mode, ColumnMode::Classic);
    }

    #[test]
    fn window_is_clamped_onto_the_visible_monitor() {
        let monitors = [
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
            },
            Rect {
                x: 1920.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
            },
        ];
        let off = clamp_window(
            Rect {
                x: -200.0,
                y: -80.0,
                w: 1280.0,
                h: 800.0,
            },
            &monitors,
            1.0,
        );
        assert!(off.x >= 0.0 && off.y >= 0.0);
        assert!(off.right() <= 1920.0 + 0.1);
        assert!(off.bottom() <= 1080.0 + 0.1);

        let huge = clamp_window(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 4000.0,
                h: 2000.0,
            },
            &monitors,
            1.0,
        );
        assert!(huge.w <= 1920.0);
        assert!(huge.h <= 1080.0);
        assert!(huge.x >= 0.0 && huge.right() <= 1920.0 + 0.1);

        let second = clamp_window(
            Rect {
                x: 2000.0,
                y: 40.0,
                w: 1200.0,
                h: 800.0,
            },
            &monitors,
            1.0,
        );
        assert!(second.x >= 1920.0);
        assert!(second.right() <= 3840.0 + 0.1);

        let gone = clamp_window(
            Rect {
                x: -4000.0,
                y: 100.0,
                w: 800.0,
                h: 600.0,
            },
            &monitors,
            1.0,
        );
        assert!(gone.x >= monitors[0].x);
        assert!(gone.right() <= monitors[0].right() + 0.1);
    }

    #[test]
    fn clamp_snaps_to_device_pixels_at_each_scale() {
        let monitor = [Rect {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        }];
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let placed = clamp_window(
                Rect {
                    x: 10.4,
                    y: 12.2,
                    w: 1000.4,
                    h: 700.3,
                },
                &monitor,
                scale,
            );
            for value in [placed.x, placed.y, placed.w, placed.h] {
                let device = value * scale;
                assert!(
                    (device - device.round()).abs() < 1e-3,
                    "{value} at {scale} -> {device}"
                );
            }
            assert!(placed.right() <= 1920.0 + 0.1);
            assert!(placed.bottom() <= 1080.0 + 0.1);
        }
    }

    #[test]
    fn profile_roundtrip_and_empty_file() {
        let mut store = LayoutStore {
            window: Some(Rect {
                x: 12.0,
                y: 8.0,
                w: 1440.0,
                h: 900.0,
            }),
            ..LayoutStore::default()
        };
        store.profiles.insert(
            profile_key(AspectClass::Landscape, 1.25, Some("mon")),
            Profile {
                columns: SavedColumns {
                    channels: Some(240.0),
                    members: Some(180.0),
                    right: None,
                },
                anchor: ReadingAnchor::Center,
            },
        );
        let parsed = LayoutStore::from_json(&store.to_json());
        assert_eq!(parsed, store);
        assert_eq!(LayoutStore::from_json("not json").window, None);

        let dir = std::env::temp_dir().join(format!(
            "rencord-layout-{}-{}",
            std::process::id(),
            scale_bucket(1.5)
        ));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("layout.json");
        store.save(&path).unwrap();
        assert_eq!(LayoutStore::load(&path), store);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_is_cheap() {
        let started = std::time::Instant::now();
        for _ in 0..200 {
            for (width, height) in CHECK_SIZES {
                let _ = plan(*width, *height);
            }
        }
        let per = started.elapsed().as_nanos() as f64 / (200.0 * CHECK_SIZES.len() as f64);
        assert!(per < 50_000.0, "{per} ns");
    }
}
