use arboard::{Clipboard, ImageData};
use iced::Task;
use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::db::{ContentType, Db};
use crate::image_codec;

use super::{Jubako, Message, platform};

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
            save_text_if_new(db, &text);
            return;
        }
    }

    if let Ok(image) = clipboard.get_image() {
        let hash = image_codec::hash_rgba_sample(image.width, image.height, &image.bytes);
        if let Ok(mut st) = state.lock() {
            st.last_image_hash = hash;
        }
        save_image_if_new(db, &image, hash);
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
                platform::notify_clipboard_activity();
                changed = true;
                save_text_if_new(db, &text);
            }
        }
    }

    if !has_non_empty_text {
        if let Ok(image) = clipboard.get_image() {
            let hash = image_codec::hash_rgba_sample(image.width, image.height, &image.bytes);
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
                platform::notify_clipboard_activity();
                changed = true;
                save_image_if_new(db, &image, hash);
            }
        }
    }

    changed
}

fn save_text_if_new(db: &Db, text: &str) {
    if matches!(db.check_duplicate(ContentType::Text, text), Ok(Some(_))) {
        return;
    }
    if let Err(error) = db.insert_text_item(text) {
        eprintln!("Failed to save text item: {error}");
    }
}

fn save_image_if_new(db: &Db, image: &ImageData<'_>, hash: u64) {
    let description = image_codec::format_image_description(image.width, image.height, hash);
    if matches!(
        db.check_duplicate(ContentType::Image, &description),
        Ok(Some(_))
    ) {
        return;
    }

    let compressed = image_codec::compress_rgba(&image.bytes);
    let (tw, th, thumb_rgba) = image_codec::downscale_rgba(
        &image.bytes,
        image.width,
        image.height,
        image_codec::THUMBNAIL_MAX_WIDTH,
        image_codec::THUMBNAIL_MAX_HEIGHT,
    );
    let compressed_thumb = image_codec::compress_rgba(&thumb_rgba);

    if let Err(error) =
        db.insert_image_item(&description, &compressed, &compressed_thumb, tw, th)
    {
        eprintln!("Failed to save image item: {error}");
    }
}

pub(super) fn set_text_and_simulate_paste(app: &mut Jubako, content: String) -> Task<Message> {
    if let Ok(mut clipboard) = app.clipboard.lock() {
        let _ = clipboard.set_text(content.clone());
        if let Ok(mut st) = app.clipboard_state.lock() {
            st.last_text = content;
        }
    }

    let hide_task = app.hide_window();
    Task::batch(vec![hide_task, simulate_paste_task()])
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
        let hash = image_codec::hash_rgba_sample(image.width, image.height, &image.bytes);
        let _ = clipboard.set_image(image);
        if let Ok(mut st) = app.clipboard_state.lock() {
            st.last_image_hash = hash;
        }
    }

    let hide_task = app.hide_window();
    Task::batch(vec![hide_task, simulate_paste_task()])
}

fn simulate_paste_task() -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            platform::simulate_paste();
        },
        |_| Message::Noop,
    )
}
