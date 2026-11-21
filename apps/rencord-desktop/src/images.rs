//! Pictures from Discord's CDN: avatars, server icons, attachments, link previews.
//!
//! Bytes come from `DiscordApi::fetch_image`, which only talks to Discord's image
//! hosts and caps the size. Everything here assumes the picture is hostile: it is
//! decoded under hard limits, shrunk to what the spot needs, and re-encoded
//! small before GPUI sees it.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use gpui_kit::{App, AsyncApp, Image, ImageFormat, WeakEntity};
use rencord_api::DiscordApi;

use super::RencordWindow;

/// Widest or tallest source image that is decoded at all.
const MAX_SOURCE_SIDE: u32 = 8192;
/// Most memory the decoder may allocate for one picture.
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;
/// Pictures kept. Past this the finished ones are dropped and fetched again if needed.
// shortcut: clear-all eviction, upgrade to LRU if scrolling a long channel refetches visibly.
const MAX_PICTURES: usize = 600;

/// A decoded picture and its size, ready to draw.
#[derive(Clone)]
pub struct Picture {
    pub image: Arc<Image>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone)]
enum Slot {
    Loading,
    Ready(Picture),
    // shortcut: a failed picture is not retried until the app restarts.
    Failed,
}

/// Pictures by URL. Cloning shares the same cache.
#[derive(Clone, Default)]
pub struct ImageCache {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
}

impl ImageCache {
    /// The picture, once it has been downloaded and decoded.
    pub fn get(&self, url: &str) -> Option<Picture> {
        match self.slots.lock().ok()?.get(url)? {
            Slot::Ready(picture) => Some(picture.clone()),
            _ => None,
        }
    }

    /// Puts a picture in directly. The demo uses this for its built-in images.
    pub fn seed(&self, url: &str, picture: Picture) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.insert(url.to_owned(), Slot::Ready(picture));
        }
    }

    /// True the first time a URL is asked for, so one download starts, not many.
    fn begin(&self, url: &str) -> bool {
        let Ok(mut slots) = self.slots.lock() else {
            return false;
        };
        if slots.contains_key(url) {
            return false;
        }
        if slots.len() >= MAX_PICTURES {
            slots.retain(|_, slot| matches!(slot, Slot::Loading));
        }
        slots.insert(url.to_owned(), Slot::Loading);
        true
    }

    fn finish(&self, url: &str, slot: Slot) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.insert(url.to_owned(), slot);
        }
    }

    /// Starts fetching `url` unless it is already known. The window repaints
    /// when the picture arrives. `max_side` is the longest edge worth keeping.
    pub fn request(
        &self,
        url: &str,
        max_side: u32,
        api: &Option<Arc<DiscordApi>>,
        cx: &mut App,
        owner: WeakEntity<RencordWindow>,
    ) {
        let Some(api) = api.clone() else {
            return;
        };
        if !self.begin(url) {
            return;
        }
        let cache = self.clone();
        let url = url.to_owned();
        cx.spawn(async move |cx: &mut AsyncApp| {
            let work = {
                let url = url.clone();
                super::net::runtime().spawn(async move {
                    let bytes = api.fetch_image(&url).await.map_err(|err| err.to_string())?;
                    tokio::task::spawn_blocking(move || prepare(&bytes, max_side))
                        .await
                        .map_err(|err| err.to_string())?
                })
            };
            let slot = match work.await {
                Ok(Ok(picture)) => Slot::Ready(picture),
                _ => Slot::Failed,
            };
            cache.finish(&url, slot);
            let _ = owner.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }
}

/// What a piece of UI needs to show pictures: the cache, the signed-in client
/// to download with, and the window to repaint when one arrives.
#[derive(Clone)]
pub struct Pictures {
    pub cache: ImageCache,
    pub api: Option<Arc<DiscordApi>>,
    pub owner: WeakEntity<RencordWindow>,
}

impl Pictures {
    /// The picture for `url` if it is ready. The first call starts the download.
    pub fn picture(&self, url: &str, max_side: u32, cx: &mut App) -> Option<Picture> {
        self.cache
            .request(url, max_side, &self.api, cx, self.owner.clone());
        self.cache.get(url)
    }
}

/// Decodes `bytes` under the limits above and returns a PNG no larger than
/// `max_side` on its longest edge. A GIF gives its first frame.
pub(crate) fn prepare(bytes: &[u8], max_side: u32) -> Result<Picture, String> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| err.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|err| err.to_string())?;
    let scaled = if decoded.width().max(decoded.height()) > max_side {
        decoded.thumbnail(max_side, max_side)
    } else {
        decoded
    };
    let (width, height) = (scaled.width(), scaled.height());
    let mut png = Vec::new();
    scaled
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    Ok(Picture {
        image: Arc::new(Image::from_bytes(ImageFormat::Png, png)),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn a_large_picture_is_shrunk_to_the_size_asked_for() {
        let picture = prepare(&png(2000, 1000), 512).unwrap();
        assert_eq!((picture.width, picture.height), (512, 256));
    }

    #[test]
    fn a_small_picture_is_left_alone() {
        let picture = prepare(&png(40, 30), 512).unwrap();
        assert_eq!((picture.width, picture.height), (40, 30));
    }

    #[test]
    fn a_jpeg_and_a_gif_decode_too() {
        let rgb = image::RgbImage::from_pixel(64, 64, image::Rgb([200, 10, 10]));
        let mut jpeg = Vec::new();
        rgb.write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(prepare(&jpeg, 128).unwrap().width, 64);
        let mut gif = Vec::new();
        image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 255, 0, 255]))
            .write_to(&mut Cursor::new(&mut gif), image::ImageFormat::Gif)
            .unwrap();
        assert_eq!(prepare(&gif, 128).unwrap().height, 16);
    }

    #[test]
    fn something_that_is_not_a_picture_is_refused() {
        assert!(prepare(b"<html>not an image</html>", 512).is_err());
        assert!(prepare(&[], 512).is_err());
    }

    #[test]
    fn a_picture_wider_than_the_limit_is_refused_not_decoded() {
        // Two rows, so the file is small, but 9000 wide is over the 8192 limit.
        let wide = png(MAX_SOURCE_SIDE + 808, 2);
        assert!(wide.len() < 200_000);
        assert!(prepare(&wide, 512).is_err());
    }

    #[test]
    fn a_url_is_only_started_once() {
        let cache = ImageCache::default();
        assert!(cache.begin("https://cdn.discordapp.com/a.png"));
        assert!(!cache.begin("https://cdn.discordapp.com/a.png"));
        assert!(cache.get("https://cdn.discordapp.com/a.png").is_none());
        cache.finish("https://cdn.discordapp.com/a.png", Slot::Failed);
        assert!(!cache.begin("https://cdn.discordapp.com/a.png"));
    }

    #[test]
    fn a_full_cache_drops_finished_pictures_but_not_downloads_in_flight() {
        let cache = ImageCache::default();
        assert!(cache.begin("pending"));
        for index in 0..MAX_PICTURES {
            cache.finish(&format!("done-{index}"), Slot::Failed);
        }
        assert!(cache.begin("one-more"));
        let slots = cache.slots.lock().unwrap();
        assert!(slots.len() < MAX_PICTURES);
        assert!(slots.contains_key("pending"));
        assert!(slots.contains_key("one-more"));
    }
}
