use arboard::ImageData;
use enigo::{Direction, Key, Keyboard};
use iced::{window, Task};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use super::{Jubako, Message, ViewMode};

pub(super) fn capture_initial_clipboard(app: &mut Jubako) {
    if let Ok(mut clipboard) = app.clipboard.lock() {
        if let Ok(text) = clipboard.get_text() {
            if !text.is_empty() {
                app.last_clipboard_content = text.clone();
                match app.db.check_duplicate(&text) {
                    Ok(Some(_)) => {}
                    _ => {
                        let _ = app.db.insert_item(&text, "text");
                    }
                }
            }
        }

        if let Ok(image) = clipboard.get_image() {
            let hash = hash_image_data(&image);
            app.last_clipboard_image_hash = hash;
            let description = format!("{}x{}:{:016x}", image.width, image.height, hash);
            match app.db.check_image_duplicate(&description) {
                Ok(Some(_)) => {}
                _ => {
                    let _ = app.db.insert_image_item(&description, &image.bytes);
                }
            }
        }
    }
}

pub(super) fn poll_clipboard(app: &mut Jubako) -> bool {
    let mut should_refresh = false;

    if let Ok(mut clipboard) = app.clipboard.lock() {
        let mut text_changed = false;

        if let Ok(text) = clipboard.get_text() {
            if !text.is_empty() && text != app.last_clipboard_content {
                app.last_clipboard_content = text.clone();
                text_changed = true;

                match app.db.check_duplicate(&text) {
                    Ok(Some(_)) => {}
                    _ => {
                        if let Err(error) = app.db.insert_item(&text, "text") {
                            eprintln!("Failed to save item: {}", error);
                        }
                    }
                }

                if app.current_view == ViewMode::History && app.search_query.is_empty() {
                    should_refresh = true;
                }
            }
        }

        if !text_changed {
            if let Ok(image) = clipboard.get_image() {
                let hash = hash_image_data(&image);
                if hash != app.last_clipboard_image_hash {
                    app.last_clipboard_image_hash = hash;
                    let description = format!("{}x{}:{:016x}", image.width, image.height, hash);

                    match app.db.check_image_duplicate(&description) {
                        Ok(Some(_)) => {}
                        _ => {
                            if let Err(error) = app.db.insert_image_item(&description, &image.bytes)
                            {
                                eprintln!("Failed to save image item: {}", error);
                            }
                        }
                    }

                    if app.current_view == ViewMode::History && app.search_query.is_empty() {
                        should_refresh = true;
                    }
                }
            }
        }
    }

    should_refresh
}

pub(super) fn set_text_and_simulate_paste(app: &mut Jubako, content: String) -> Task<Message> {
    if let Ok(mut clipboard) = app.clipboard.lock() {
        let _ = clipboard.set_text(content.clone());
        app.last_clipboard_content = content;
    }

    app.is_visible = false;
    app.reset_transient_state();

    simulate_paste(app)
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
        app.last_clipboard_image_hash = hash;
    }

    app.is_visible = false;
    app.reset_transient_state();

    simulate_paste(app)
}

pub(super) fn parse_image_description(description: &str) -> Option<(usize, usize)> {
    let dimensions = description.split(':').next()?;
    let mut parts = dimensions.split('x');
    let width = parts.next()?.parse().ok()?;
    let height = parts.next()?.parse().ok()?;
    Some((width, height))
}

fn simulate_paste(app: &Jubako) -> Task<Message> {
    let enigo = app.enigo.clone();

    window::get_latest()
        .and_then(move |id| window::change_mode::<Message>(id, window::Mode::Hidden))
        .chain(Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(150)).await;
                if let Ok(mut enigo) = enigo.lock() {
                    let _ = enigo.key(Key::Control, Direction::Press);
                    let _ = enigo.key(Key::Unicode('v'), Direction::Click);
                    let _ = enigo.key(Key::Control, Direction::Release);
                }
            },
            |_| Message::Noop,
        ))
}

fn hash_image_data(image: &ImageData<'_>) -> u64 {
    let mut hasher = DefaultHasher::new();
    image.width.hash(&mut hasher);
    image.height.hash(&mut hasher);
    image.bytes.hash(&mut hasher);
    hasher.finish()
}
