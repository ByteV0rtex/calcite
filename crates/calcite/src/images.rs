//! Remote image loading with a strict memory budget.
//!
//! Icons and gallery shots are downloaded on the backend runtime, decoded,
//! downscaled to twice their display size (sharp on Retina, no bigger), and
//! kept in an LRU capped by decoded bytes. A screen full of mod icons costs a
//! few megabytes instead of the hundreds a webview would spend.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{App, Global, ImageSource, RenderImage};
use image::{Frame, RgbaImage, imageops::FilterType};
use smallvec::smallvec;

use crate::backend::Backend;

/// Decoded-image budget. Roughly 300 icons at 128×128, or a few dozen
/// gallery images.
const BUDGET_BYTES: usize = 24 * 1024 * 1024;
/// Images asked for within this many lookups count as on screen.
const PROTECT_RECENT: u64 = 256;

#[derive(Clone)]
enum Slot {
    Loading,
    Ready(Arc<RenderImage>, usize),
    Failed,
}

pub struct Images {
    client: calcite_core::modrinth::Client,
    slots: HashMap<String, (Slot, u64)>,
    used_bytes: usize,
    tick: u64,
}

impl Global for Images {}

impl Images {
    pub fn init(client: calcite_core::modrinth::Client, cx: &mut App) {
        cx.set_global(Images {
            client,
            slots: HashMap::new(),
            used_bytes: 0,
            tick: 0,
        });
    }

    /// A local image file, decoded and shrunk like remote ones.
    pub fn get_file(path: &std::path::Path, max_px: u32, cx: &mut App) -> Option<ImageSource> {
        Self::get(&path.to_string_lossy(), max_px, cx)
    }

    /// Returns the image if it's ready; otherwise starts loading it and
    /// returns `None` so the caller can show a placeholder. `max_px` is the
    /// largest edge, in device pixels, the image will be drawn at.
    pub fn get(url: &str, max_px: u32, cx: &mut App) -> Option<ImageSource> {
        let key = format!("{url}@{max_px}");
        let images = cx.global_mut::<Images>();
        images.tick += 1;
        let tick = images.tick;
        if let Some((slot, last_used)) = images.slots.get_mut(&key) {
            *last_used = tick;
            return match slot {
                Slot::Ready(image, _) => Some(ImageSource::Render(image.clone())),
                Slot::Loading | Slot::Failed => None,
            };
        }
        images.slots.insert(key.clone(), (Slot::Loading, tick));

        let client = images.client.clone();
        let url = url.to_string();
        let backend = Backend::global(cx).clone();
        let fetch = backend.run(async move {
            // Local files (screenshots, world icons) are read from disk;
            // data URLs (server icons) carry the image inline.
            let bytes = if let Some(data) = url.strip_prefix("data:") {
                use base64::Engine;
                let encoded = data.split_once(',').map(|(_, b)| b).unwrap_or_default();
                base64::engine::general_purpose::STANDARD.decode(encoded.trim())?
            } else if url.starts_with('/') || url.contains(":\\") {
                tokio::fs::read(&url).await?
            } else {
                client.bytes(&url).await?
            };
            decode(&bytes, max_px)
        });
        cx.spawn(async move |cx| {
            let result = fetch.await.and_then(|r| r);
            cx.update(|cx| {
                let images = cx.global_mut::<Images>();
                let slot = match result {
                    Ok((image, size)) => {
                        images.used_bytes += size;
                        Slot::Ready(image, size)
                    }
                    Err(error) => {
                        tracing::debug!("image {key} failed: {error:#}");
                        Slot::Failed
                    }
                };
                if let Some(entry) = images.slots.get_mut(&key) {
                    entry.0 = slot;
                }
                images.evict();
                cx.refresh_windows();
            })
        })
        .detach();
        None
    }

    fn evict(&mut self) {
        while self.used_bytes > BUDGET_BYTES {
            // Never drop what was just asked for: that's what is on screen,
            // and dropping it would only load it again. Over budget is fine
            // until those scroll away.
            let recent = self.tick.saturating_sub(PROTECT_RECENT);
            let oldest = self
                .slots
                .iter()
                .filter(|(_, (slot, used))| matches!(slot, Slot::Ready(..)) && *used < recent)
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone());
            let Some(key) = oldest else { break };
            if let Some((Slot::Ready(_, size), _)) = self.slots.remove(&key) {
                self.used_bytes -= size;
            }
        }
    }
}

/// Turn an RGBA image into something GPUI can draw (it uploads BGRA).
pub fn render_image(mut rgba: RgbaImage) -> Arc<RenderImage> {
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(smallvec![Frame::new(rgba)]))
}

/// Decode, downscale to `max_px` on the longest edge, and convert to the
/// BGRA layout GPUI uploads.
fn decode(bytes: &[u8], max_px: u32) -> anyhow::Result<(Arc<RenderImage>, usize)> {
    let decoded = image::load_from_memory(bytes)?;
    let (w, h) = (decoded.width(), decoded.height());
    let scaled = if w.max(h) > max_px {
        decoded.resize(max_px, max_px, FilterType::Triangle)
    } else {
        decoded
    };
    let mut rgba: RgbaImage = scaled.into_rgba8();
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    let size = rgba.as_raw().len();
    let image = RenderImage::new(smallvec![Frame::new(rgba)]);
    Ok((Arc::new(image), size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn decode_downscales_and_swaps_to_bgra() {
        let (image, size) = decode(&png(512, 256), 128).unwrap();
        let dims = image.size(0);
        assert_eq!((dims.width.0, dims.height.0), (128, 64));
        assert_eq!(size, 128 * 64 * 4);
        let bytes = image.as_bytes(0).unwrap();
        // Red in RGBA becomes [0, 0, 255, 255] in BGRA.
        assert_eq!(&bytes[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn decode_keeps_small_images_as_is() {
        let (image, _) = decode(&png(64, 64), 128).unwrap();
        assert_eq!(image.size(0).width.0, 64);
    }
}
