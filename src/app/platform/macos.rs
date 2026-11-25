use iced::{Point, Size};

pub fn ensure_detached_background() {}
pub(in crate::app) fn ensure_startup_registration() {}

mod timer {
    use std::ffi::{c_char, c_void};
    use std::sync::{Mutex, OnceLock};

    type DispatchObject = *mut c_void;
    type DispatchQueue = *mut c_void;
    type DispatchSource = *mut c_void;
    type DispatchSemaphore = *mut c_void;
    type DispatchTime = u64;

    const DISPATCH_TIME_NOW: DispatchTime = 0;
    const DISPATCH_TIME_FOREVER: DispatchTime = u64::MAX;
    const NSEC_PER_MSEC: u64 = 1_000_000;

    unsafe extern "C" {
        static _dispatch_source_type_timer: c_void;
        fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> DispatchQueue;
        fn dispatch_source_create(
            type_: *const c_void,
            handle: usize,
            mask: usize,
            queue: DispatchQueue,
        ) -> DispatchSource;
        fn dispatch_source_set_timer(
            source: DispatchSource,
            start: DispatchTime,
            interval: u64,
            leeway: u64,
        );
        fn dispatch_source_set_event_handler_f(
            source: DispatchSource,
            handler: extern "C" fn(*mut c_void),
        );
        fn dispatch_set_context(object: DispatchObject, context: *mut c_void);
        fn dispatch_resume(object: DispatchObject);
        fn dispatch_time(when: DispatchTime, delta: i64) -> DispatchTime;
        fn dispatch_semaphore_create(value: isize) -> DispatchSemaphore;
        fn dispatch_semaphore_wait(dsema: DispatchSemaphore, timeout: DispatchTime) -> isize;
        fn dispatch_semaphore_signal(dsema: DispatchSemaphore) -> isize;
    }

    extern "C" fn on_timer_fire(context: *mut c_void) {
        if !context.is_null() {
            unsafe {
                dispatch_semaphore_signal(context);
            }
        }
    }

    struct LeewayTimer {
        source: DispatchSource,
        semaphore: DispatchSemaphore,
    }

    unsafe impl Send for LeewayTimer {}

    impl LeewayTimer {
        fn new() -> Self {
            unsafe {
                let label = b"com.jubako.clipboard_timer\0";
                let queue =
                    dispatch_queue_create(label.as_ptr() as *const c_char, std::ptr::null());
                let semaphore = dispatch_semaphore_create(0);
                let source = dispatch_source_create(
                    &_dispatch_source_type_timer as *const _ as *const c_void,
                    0,
                    0,
                    queue,
                );
                dispatch_set_context(source, semaphore);
                dispatch_source_set_event_handler_f(source, on_timer_fire);
                dispatch_source_set_timer(source, DISPATCH_TIME_FOREVER, DISPATCH_TIME_FOREVER, 0);
                dispatch_resume(source);

                Self { source, semaphore }
            }
        }

        fn sleep(&self, delay_ms: u64, leeway_ms: u64) {
            unsafe {
                let start = dispatch_time(DISPATCH_TIME_NOW, (delay_ms * NSEC_PER_MSEC) as i64);
                dispatch_source_set_timer(
                    self.source,
                    start,
                    DISPATCH_TIME_FOREVER,
                    leeway_ms * NSEC_PER_MSEC,
                );
                dispatch_semaphore_wait(self.semaphore, DISPATCH_TIME_FOREVER);
            }
        }
    }

    pub(super) fn sleep_with_leeway(delay_ms: u64, leeway_ms: u64) {
        static TIMER: OnceLock<Mutex<LeewayTimer>> = OnceLock::new();
        let timer = TIMER.get_or_init(|| Mutex::new(LeewayTimer::new()));
        if let Ok(guard) = timer.lock() {
            guard.sleep(delay_ms, leeway_ms);
        } else {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
    }
}

mod activity {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state_id: u32, event_type: u32) -> f64;
        fn CGGetOnlineDisplayList(
            max_displays: u32,
            online_displays: *mut u32,
            display_count: *mut u32,
        ) -> i32;
        fn CGDisplayIsAsleep(display: u32) -> i32;
    }

    const CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: u32 = 1;
    const CG_ANY_INPUT_EVENT_TYPE: u32 = u32::MAX;

    pub(super) fn seconds_since_last_user_input() -> Option<f64> {
        let seconds = unsafe {
            CGEventSourceSecondsSinceLastEventType(
                CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE,
                CG_ANY_INPUT_EVENT_TYPE,
            )
        };

        (seconds.is_finite() && seconds >= 0.0).then_some(seconds)
    }

    pub(super) fn all_displays_are_asleep() -> bool {
        let mut display_count = 0u32;
        let first_result =
            unsafe { CGGetOnlineDisplayList(0, std::ptr::null_mut(), &mut display_count) };

        if first_result != 0 || display_count == 0 {
            return false;
        }

        const MAX_ONLINE_DISPLAYS: u32 = 32;
        if display_count > MAX_ONLINE_DISPLAYS {
            return false;
        }

        let capacity = display_count;
        let mut displays = [0u32; MAX_ONLINE_DISPLAYS as usize];
        let second_result =
            unsafe { CGGetOnlineDisplayList(capacity, displays.as_mut_ptr(), &mut display_count) };

        if second_result != 0 || display_count == 0 || display_count > capacity {
            return false;
        }

        displays[..display_count as usize]
            .iter()
            .all(|display| unsafe { CGDisplayIsAsleep(*display) != 0 })
    }
}

pub(in crate::app) fn wait_for_next_clipboard_update() -> bool {
    use objc2_app_kit::NSPasteboard;
    use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};

    const MIN_INTERVAL_MS: u64 = 600;
    const MAX_INTERVAL_MS: u64 = 5000;
    const INPUT_IDLE_TIMEOUT_SECS: f64 = 60.0;
    const PAUSED_CHECK_INTERVAL_MS: u64 = 5_000;

    static LAST_CHANGE_COUNT: AtomicIsize = AtomicIsize::new(-1);
    static CURRENT_INTERVAL_MS: AtomicU64 = AtomicU64::new(MIN_INTERVAL_MS);

    let mut was_paused = false;

    loop {
        let input_idle_seconds = activity::seconds_since_last_user_input();
        let input_idle = input_idle_seconds
            .map(|seconds| seconds >= INPUT_IDLE_TIMEOUT_SECS)
            .unwrap_or(false);
        let displays_asleep = activity::all_displays_are_asleep();

        if input_idle || displays_asleep {
            was_paused = true;
            timer::sleep_with_leeway(PAUSED_CHECK_INTERVAL_MS, PAUSED_CHECK_INTERVAL_MS / 2);
            continue;
        }

        if was_paused {
            CURRENT_INTERVAL_MS.store(MIN_INTERVAL_MS, Ordering::Relaxed);
            was_paused = false;
        }

        let count = unsafe { NSPasteboard::generalPasteboard().changeCount() };
        let prev = LAST_CHANGE_COUNT.swap(count, Ordering::Relaxed);
        if prev != -1 && prev != count {
            return true;
        }

        let interval_ms = if super::take_clipboard_activity() {
            MIN_INTERVAL_MS
        } else {
            let prev_interval = CURRENT_INTERVAL_MS.load(Ordering::Relaxed);
            (prev_interval * 2).min(MAX_INTERVAL_MS)
        };
        CURRENT_INTERVAL_MS.store(interval_ms, Ordering::Relaxed);

        let until_idle_ms = input_idle_seconds
            .map(|seconds| ((INPUT_IDLE_TIMEOUT_SECS - seconds).max(0.0) * 1000.0).ceil() as u64);
        let sleep_ms = until_idle_ms
            .filter(|remaining| *remaining > 0)
            .map(|remaining| interval_ms.min(remaining))
            .unwrap_or(interval_ms);
        let leeway_ms = if sleep_ms < interval_ms {
            0
        } else {
            (interval_ms / 2).max(200)
        };
        timer::sleep_with_leeway(sleep_ms, leeway_ms);
    }
}

fn macos_display_scale(display: core_graphics::display::CGDisplay) -> f32 {
    let bounds = display.bounds();
    if bounds.size.width > 0.0 {
        if let Some(mode) = display.display_mode() {
            let scale = mode.pixel_width() as f32 / bounds.size.width as f32;
            if scale > 0.0 {
                return scale;
            }
        }
    }
    1.0
}

fn macos_cursor_logical() -> Option<core_graphics::geometry::CGPoint> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let event = CGEvent::new(source).ok()?;
    Some(event.location())
}

fn macos_display_at(
    point: core_graphics::geometry::CGPoint,
) -> Option<core_graphics::display::CGDisplay> {
    use core_graphics::display::CGDisplay;

    let ids = CGDisplay::active_displays().ok()?;
    for id in ids {
        let display = CGDisplay::new(id);
        let b = display.bounds();
        if point.x >= b.origin.x
            && point.x < b.origin.x + b.size.width
            && point.y >= b.origin.y
            && point.y < b.origin.y + b.size.height
        {
            return Some(display);
        }
    }
    Some(CGDisplay::main())
}

pub(in crate::app) fn get_cursor_position() -> Option<Point> {
    let pt = macos_cursor_logical()?;
    let scale = macos_display_at(pt).map(macos_display_scale).unwrap_or(1.0);
    Some(Point::new(pt.x as f32 * scale, pt.y as f32 * scale))
}

pub(in crate::app) fn get_monitor_rect_at_cursor() -> Option<(Point, Size)> {
    let pt = macos_cursor_logical()?;
    let display = macos_display_at(pt)?;
    let scale = macos_display_scale(display);
    let b = display.bounds();
    Some((
        Point::new(b.origin.x as f32 * scale, b.origin.y as f32 * scale),
        Size::new(b.size.width as f32 * scale, b.size.height as f32 * scale),
    ))
}

pub(in crate::app) fn get_monitor_scale_factor_at_cursor() -> Option<f32> {
    let pt = macos_cursor_logical()?;
    let display = macos_display_at(pt)?;
    Some(macos_display_scale(display))
}

pub(in crate::app) fn apply_tool_window_style() {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;

    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

pub(in crate::app) fn unhide_application() {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        unsafe {
            app.unhide(None);
            app.activate();
        }
    }
}

pub(in crate::app) fn hide_application() {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.hide(None);
    }
}

pub(in crate::app) fn simulate_paste() {
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
