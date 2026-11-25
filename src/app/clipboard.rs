use arboard::{Clipboard, ImageData};
use iced::Task;
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::db::Db;

use super::{Jubako, Message};

const RLE_MAGIC: &[u8; 4] = b"JRLE";
pub(super) const THUMBNAIL_MAX_WIDTH: usize = 240;
pub(super) const THUMBNAIL_MAX_HEIGHT: usize = 160;

#[derive(Default)]
pub(super) struct ClipboardState {
    pub(super) last_text: String,
    pub(super) last_image_hash: u64,
}

pub(super) fn capture_initial_clipboard(
    db: &Db,
    clipboard: &Mutex<Clipboard>,
    state: &Mutex<ClipboardState>,
) {
    let Ok(mut clipboard) = clipboard.lock() else {
        return;
    };

    if let Ok(text) = clipboard.get_text() {
        if !text.is_empty() {
            if let Ok(mut st) = state.lock() {
                st.last_text = text.clone();
            }
            match db.check_duplicate(&text) {
                Ok(Some(_)) => {}
                _ => {
                    let _ = db.insert_item(&text, "text");
                }
            }
            return;
        }
    }

    if let Ok(image) = clipboard.get_image() {
        let hash = hash_image_data(&image);
        if let Ok(mut st) = state.lock() {
            st.last_image_hash = hash;
        }
        let description = format!("{}x{}:{:016x}", image.width, image.height, hash);
        match db.check_image_duplicate(&description) {
            Ok(Some(_)) => {}
            _ => {
                let compressed = compress_rgba(&image.bytes);
                let (tw, th, thumb_rgba) = downscale_rgba(
                    &image.bytes,
                    image.width,
                    image.height,
                    THUMBNAIL_MAX_WIDTH,
                    THUMBNAIL_MAX_HEIGHT,
                );
                let compressed_thumb = compress_rgba(&thumb_rgba);
                let _ = db.insert_image_item(&description, &compressed, &compressed_thumb, tw, th);
            }
        }
    }
}

pub(super) fn poll_clipboard_background(
    db: &Arc<Db>,
    clipboard: &Arc<Mutex<Clipboard>>,
    state: &Arc<Mutex<ClipboardState>>,
) -> bool {
    let Ok(mut clipboard) = clipboard.lock() else {
        return false;
    };

    let mut changed = false;
    let mut has_non_empty_text = false;

    if let Ok(text) = clipboard.get_text() {
        if !text.is_empty() {
            has_non_empty_text = true;
            let is_new = state
                .lock()
                .map(|mut st| {
                    if st.last_text != text {
                        st.last_text = text.clone();
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);

            if is_new {
                super::platform::notify_clipboard_activity();
                changed = true;

                match db.check_duplicate(&text) {
                    Ok(Some(_)) => {}
                    _ => {
                        if let Err(error) = db.insert_item(&text, "text") {
                            eprintln!("Failed to save item: {}", error);
                        }
                    }
                }
            }
        }
    }

    if !has_non_empty_text {
        if let Ok(image) = clipboard.get_image() {
            let hash = hash_image_data(&image);
            let is_new = state
                .lock()
                .map(|mut st| {
                    if st.last_image_hash != hash {
                        st.last_image_hash = hash;
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);

            if is_new {
                super::platform::notify_clipboard_activity();
                changed = true;
                let description = format!("{}x{}:{:016x}", image.width, image.height, hash);

                match db.check_image_duplicate(&description) {
                    Ok(Some(_)) => {}
                    _ => {
                        let compressed = compress_rgba(&image.bytes);
                        let (tw, th, thumb_rgba) = downscale_rgba(
                            &image.bytes,
                            image.width,
                            image.height,
                            THUMBNAIL_MAX_WIDTH,
                            THUMBNAIL_MAX_HEIGHT,
                        );
                        let compressed_thumb = compress_rgba(&thumb_rgba);
                        if let Err(error) = db.insert_image_item(
                            &description,
                            &compressed,
                            &compressed_thumb,
                            tw,
                            th,
                        ) {
                            eprintln!("Failed to save image item: {}", error);
                        }
                    }
                }
            }
        }
    }

    changed
}

pub(super) fn set_text_and_simulate_paste(app: &mut Jubako, content: String) -> Task<Message> {
    if let Ok(mut clipboard) = app.clipboard.lock() {
        let _ = clipboard.set_text(content.clone());
        if let Ok(mut st) = app.clipboard_state.lock() {
            st.last_text = content;
        }
    }

    let hide_task = app.hide_window();
    Task::batch(vec![hide_task, simulate_paste()])
}

pub(super) fn set_image_and_simulate_paste(
    app: &mut Jubako,
    width: usize,
    height: usize,
    rgba: Vec<u8>,
) -> Task<Message> {
    if let Ok(mut clipboard) = app.clipboard.lock() {
        let image = ImageData {
            width,
            height,
            bytes: Cow::Owned(rgba),
        };
        let hash = hash_image_data(&image);
        let _ = clipboard.set_image(image);
        if let Ok(mut st) = app.clipboard_state.lock() {
            st.last_image_hash = hash;
        }
    }

    let hide_task = app.hide_window();
    Task::batch(vec![hide_task, simulate_paste()])
}

pub(super) fn parse_image_description(description: &str) -> Option<(usize, usize)> {
    let dimensions = description.split(':').next()?;
    let mut parts = dimensions.split('x');
    let width = parts.next()?.parse().ok()?;
    let height = parts.next()?.parse().ok()?;
    Some((width, height))
}

pub(super) fn compress_rgba(rgba: &[u8]) -> Vec<u8> {
    if rgba.is_empty() || rgba.len() % 4 != 0 {
        return rgba.to_vec();
    }

    let mut out = Vec::with_capacity(rgba.len() / 2 + 4);
    out.extend_from_slice(RLE_MAGIC);

    let mut chunks = rgba.chunks_exact(4);
    let Some(first) = chunks.next() else {
        return rgba.to_vec();
    };
    let mut current: [u8; 4] = [first[0], first[1], first[2], first[3]];
    let mut count: u8 = 1;

    for chunk in chunks {
        let pixel = [chunk[0], chunk[1], chunk[2], chunk[3]];
        if pixel == current && count < u8::MAX {
            count += 1;
        } else {
            out.push(count);
            out.extend_from_slice(&current);
            current = pixel;
            count = 1;
        }
    }
    out.push(count);
    out.extend_from_slice(&current);

    if out.len() >= rgba.len() {
        rgba.to_vec()
    } else {
        out
    }
}

pub(super) fn decompress_rgba(data: &[u8], width: usize, height: usize) -> Vec<u8> {
    let expected_len = width.saturating_mul(height).saturating_mul(4);
    if data.len() != expected_len && data.starts_with(RLE_MAGIC) {
        let payload = &data[RLE_MAGIC.len()..];
        if payload.len() % 5 == 0 {
            let mut out = Vec::with_capacity(expected_len);
            for run in payload.chunks_exact(5) {
                let count = run[0] as usize;
                let pixel = &run[1..5];
                for _ in 0..count {
                    out.extend_from_slice(pixel);
                }
            }
            if out.len() == expected_len {
                return out;
            }
        }
    }

    data.to_vec()
}

pub(super) fn downscale_rgba(
    rgba: &[u8],
    src_w: usize,
    src_h: usize,
    max_w: usize,
    max_h: usize,
) -> (usize, usize, Vec<u8>) {
    if src_w == 0
        || src_h == 0
        || (src_w <= max_w && src_h <= max_h)
        || rgba.len() < src_w.saturating_mul(src_h).saturating_mul(4)
    {
        return (src_w, src_h, rgba.to_vec());
    }

    let scale = (max_w as f32 / src_w as f32).min(max_h as f32 / src_h as f32);
    let dst_w = ((src_w as f32 * scale).round() as usize).max(1);
    let dst_h = ((src_h as f32 * scale).round() as usize).max(1);

    let mut out = Vec::with_capacity(dst_w * dst_h * 4);
    for y in 0..dst_h {
        let sy = (y * src_h) / dst_h;
        for x in 0..dst_w {
            let sx = (x * src_w) / dst_w;
            let idx = (sy * src_w + sx) * 4;
            out.extend_from_slice(&rgba[idx..idx + 4]);
        }
    }

    (dst_w, dst_h, out)
}

fn simulate_paste() -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(Duration::from_millis(80)).await;

            #[cfg(target_os = "macos")]
            {
                use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
                use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

                if let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
                    const KEY_V: CGKeyCode = 9;
                    if let (Ok(down), Ok(up)) = (
                        CGEvent::new_keyboard_event(source.clone(), KEY_V, true),
                        CGEvent::new_keyboard_event(source, KEY_V, false),
                    ) {
                        down.set_flags(CGEventFlags::CGEventFlagCommand);
                        up.set_flags(CGEventFlags::CGEventFlagCommand);
                        down.post(CGEventTapLocation::HID);
                        up.post(CGEventTapLocation::HID);
                    }
                }
            }
        },
        |_| Message::Noop,
    )
}

fn hash_image_data(image: &ImageData<'_>) -> u64 {
    const SAMPLE_CHUNK: usize = 4096;

    let mut hasher = DefaultHasher::new();
    image.width.hash(&mut hasher);
    image.height.hash(&mut hasher);
    let bytes = &image.bytes;
    bytes.len().hash(&mut hasher);

    if bytes.len() <= SAMPLE_CHUNK * 3 {
        bytes.hash(&mut hasher);
    } else {
        bytes[..SAMPLE_CHUNK].hash(&mut hasher);
        let mid = bytes.len() / 2;
        bytes[mid - SAMPLE_CHUNK / 2..mid + SAMPLE_CHUNK / 2].hash(&mut hasher);
        bytes[bytes.len() - SAMPLE_CHUNK..].hash(&mut hasher);
    }

    hasher.finish()
}
