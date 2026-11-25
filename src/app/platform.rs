use std::sync::atomic::{AtomicBool, Ordering};

static CLIPBOARD_ACTIVITY: AtomicBool = AtomicBool::new(false);

pub(super) fn notify_clipboard_activity() {
    CLIPBOARD_ACTIVITY.store(true, Ordering::Relaxed);
}

#[cfg(not(target_os = "windows"))]
pub(super) fn take_clipboard_activity() -> bool {
    CLIPBOARD_ACTIVITY.swap(false, Ordering::Relaxed)
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::ensure_detached_background;
#[cfg(target_os = "windows")]
pub(super) use windows::{
    apply_tool_window_style, ensure_startup_registration, get_cursor_position,
    get_monitor_rect_at_cursor, get_monitor_scale_factor_at_cursor, hide_application,
    simulate_paste, unhide_application, wait_for_next_clipboard_update,
};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::ensure_detached_background;
#[cfg(target_os = "macos")]
pub(super) use macos::{
    apply_tool_window_style, ensure_startup_registration, get_cursor_position,
    get_monitor_rect_at_cursor, get_monitor_scale_factor_at_cursor, hide_application,
    simulate_paste, unhide_application, wait_for_next_clipboard_update,
};

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod fallback;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub use fallback::ensure_detached_background;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) use fallback::{
    apply_tool_window_style, ensure_startup_registration, get_cursor_position,
    get_monitor_rect_at_cursor, get_monitor_scale_factor_at_cursor, hide_application,
    simulate_paste, unhide_application, wait_for_next_clipboard_update,
};