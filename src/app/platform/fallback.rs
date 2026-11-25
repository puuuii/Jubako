use iced::{Point, Size};
use std::sync::atomic::{AtomicU64, Ordering};

pub fn ensure_detached_background() {}
pub(in crate::app) fn ensure_startup_registration() {}

pub(in crate::app) fn wait_for_next_clipboard_update() -> bool {
    const MIN_INTERVAL_MS: u64 = 1000;
    const MAX_INTERVAL_MS: u64 = 5000;

    static CURRENT_INTERVAL_MS: AtomicU64 = AtomicU64::new(MIN_INTERVAL_MS);

    let interval = if super::take_clipboard_activity() {
        MIN_INTERVAL_MS
    } else {
        let prev = CURRENT_INTERVAL_MS.load(Ordering::Relaxed);
        (prev * 2).min(MAX_INTERVAL_MS)
    };
    CURRENT_INTERVAL_MS.store(interval, Ordering::Relaxed);

    std::thread::sleep(std::time::Duration::from_millis(interval));
    true
}

pub(in crate::app) fn get_cursor_position() -> Option<Point> {
    None
}

pub(in crate::app) fn get_monitor_rect_at_cursor() -> Option<(Point, Size)> {
    None
}

pub(in crate::app) fn get_monitor_scale_factor_at_cursor() -> Option<f32> {
    None
}

pub(in crate::app) fn apply_tool_window_style() {}
pub(in crate::app) fn unhide_application() {}
pub(in crate::app) fn hide_application() {}
pub(in crate::app) fn simulate_paste() {}
