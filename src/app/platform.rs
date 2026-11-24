use iced::{Point, Size};
use std::sync::atomic::{AtomicBool, Ordering};

static CLIPBOARD_ACTIVITY: AtomicBool = AtomicBool::new(false);

pub(super) fn notify_clipboard_activity() {
    CLIPBOARD_ACTIVITY.store(true, Ordering::Relaxed);
}

#[cfg(target_os = "windows")]
pub(super) fn ensure_startup_registration() {
    if cfg!(debug_assertions) {
        return;
    }

    if std::env::args().any(|arg| arg == "--no-autostart") {
        return;
    }

    if let Err(error) = ensure_startup_registration_inner() {
        eprintln!("Failed to configure startup registration: {error}");
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn ensure_startup_registration() {}

#[cfg(target_os = "windows")]
fn ensure_startup_registration_inner() -> anyhow::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APP_NAME: &str = "Jubako";

    let command = startup_command_string()?;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu.create_subkey(RUN_KEY)?;
    let current: Option<String> = run_key.get_value(APP_NAME).ok();

    if current.as_deref() != Some(command.as_str()) {
        run_key.set_value(APP_NAME, &command)?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn startup_command_string() -> anyhow::Result<String> {
    let exe = std::env::current_exe()?;
    let exe = exe
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Executable path contains non-UTF8 characters"))?;

    Ok(format!("\"{exe}\" --background"))
}

#[cfg(target_os = "windows")]
pub(super) fn wait_for_next_clipboard_update() -> bool {
    use once_cell::sync::Lazy;
    use std::sync::{Mutex, mpsc};

    static EVENT_RX: Lazy<Option<Mutex<mpsc::Receiver<()>>>> = Lazy::new(|| {
        let (tx, rx) = mpsc::channel();

        match std::thread::Builder::new()
            .name("jubako-clipboard-listener".to_string())
            .spawn(move || clipboard_listener_thread(tx))
        {
            Ok(_) => Some(Mutex::new(rx)),
            Err(error) => {
                eprintln!("Failed to spawn clipboard listener thread: {error}");
                None
            }
        }
    });

    let Some(receiver) = EVENT_RX.as_ref() else {
        return false;
    };
    let Ok(receiver) = receiver.lock() else {
        return false;
    };

    receiver.recv().is_ok()
}

#[cfg(target_os = "windows")]
fn clipboard_listener_thread(sender: std::sync::mpsc::Sender<()>) {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::{
        AddClipboardFormatListener, RemoveClipboardFormatListener,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, HWND_MESSAGE, MSG, RegisterClassW,
        SetWindowLongPtrW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE,
        WM_NCCREATE, WM_NCDESTROY, WNDCLASSW,
    };
    use windows::core::PCWSTR;

    unsafe extern "system" fn clipboard_window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let create_struct = lparam.0 as *const CREATESTRUCTW;

                if !create_struct.is_null() {
                    let sender_ptr = unsafe { (*create_struct).lpCreateParams }
                        as *mut std::sync::mpsc::Sender<()>;
                    unsafe {
                        SetWindowLongPtrW(window, GWLP_USERDATA, sender_ptr as isize);
                    }
                }

                LRESULT(1)
            }
            WM_CLIPBOARDUPDATE => {
                let sender_ptr = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) }
                    as *mut std::sync::mpsc::Sender<()>;

                if !sender_ptr.is_null() {
                    let _ = unsafe { (*sender_ptr).send(()) };
                }

                LRESULT(0)
            }
            WM_NCDESTROY => {
                let sender_ptr = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) }
                    as *mut std::sync::mpsc::Sender<()>;

                if !sender_ptr.is_null() {
                    unsafe {
                        SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                        drop(Box::from_raw(sender_ptr));
                    }
                }

                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }

    let class_name: Vec<u16> = "JubakoClipboardListenerWindow\0".encode_utf16().collect();
    let class_name_ptr = PCWSTR(class_name.as_ptr());

    let class = WNDCLASSW {
        lpfnWndProc: Some(clipboard_window_proc),
        lpszClassName: class_name_ptr,
        ..Default::default()
    };

    unsafe {
        if RegisterClassW(&class) == 0 {
            eprintln!("Failed to register clipboard listener window class");
            return;
        }

        let sender_ptr = Box::into_raw(Box::new(sender));
        let window = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name_ptr,
            class_name_ptr,
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            None,
            Some(sender_ptr as *const c_void),
        ) {
            Ok(window) => window,
            Err(error) => {
                drop(Box::from_raw(sender_ptr));
                eprintln!("Failed to create clipboard listener window: {error}");
                return;
            }
        };

        if let Err(error) = AddClipboardFormatListener(window) {
            eprintln!("Failed to register clipboard format listener: {error}");
            let _ = DestroyWindow(window);
            return;
        }

        let mut message = MSG::default();
        loop {
            let status = GetMessageW(&mut message, HWND(std::ptr::null_mut()), 0, 0);

            if status.0 <= 0 {
                break;
            }

            let _ = TranslateMessage(&message);
            let _ = DispatchMessageW(&message);
        }

        let _ = RemoveClipboardFormatListener(window);
        let _ = DestroyWindow(window);
    }
}

#[cfg(target_os = "macos")]
mod macos_timer {
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

    extern "C" {
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
                let queue = dispatch_queue_create(label.as_ptr() as *const c_char, std::ptr::null());
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

#[cfg(target_os = "macos")]
mod macos_activity {
    // CoreGraphics exposes both the system-wide input-idle timer and display
    // power state. Reading these values does not require installing a global
    // event tap.
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(
            state_id: u32,
            event_type: u32,
        ) -> f64;
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
        let first_result = unsafe {
            CGGetOnlineDisplayList(0, std::ptr::null_mut(), &mut display_count)
        };

        // Fail open: if CoreGraphics cannot report the display state, keep
        // clipboard monitoring enabled rather than silently missing copies.
        if first_result != 0 || display_count == 0 {
            return false;
        }

        // A fixed stack buffer avoids allocating a Vec on every monitor
        // cycle. More than 32 online displays is unusual; in that case fail
        // open and continue monitoring the clipboard.
        const MAX_ONLINE_DISPLAYS: u32 = 32;
        if display_count > MAX_ONLINE_DISPLAYS {
            return false;
        }

        let capacity = display_count;
        let mut displays = [0u32; MAX_ONLINE_DISPLAYS as usize];
        let second_result = unsafe {
            CGGetOnlineDisplayList(
                capacity,
                displays.as_mut_ptr(),
                &mut display_count,
            )
        };

        if second_result != 0 || display_count == 0 || display_count > capacity {
            return false;
        }

        displays[..display_count as usize]
            .iter()
            .all(|display| unsafe { CGDisplayIsAsleep(*display) != 0 })
    }
}

#[cfg(target_os = "macos")]
pub(super) fn wait_for_next_clipboard_update() -> bool {
    use objc2_app_kit::NSPasteboard;
    use std::sync::atomic::{AtomicIsize, AtomicU64};

    const MIN_INTERVAL_MS: u64 = 600;
    const MAX_INTERVAL_MS: u64 = 5000;
    const INPUT_IDLE_TIMEOUT_SECS: f64 = 60.0;
    // While paused, only check the inexpensive activity/display state. The
    // clipboard itself is not queried until monitoring resumes.
    const PAUSED_CHECK_INTERVAL_MS: u64 = 5_000;

    static LAST_CHANGE_COUNT: AtomicIsize = AtomicIsize::new(-1);
    static CURRENT_INTERVAL_MS: AtomicU64 = AtomicU64::new(MIN_INTERVAL_MS);

    let mut was_paused = false;

    loop {
        let input_idle_seconds = macos_activity::seconds_since_last_user_input();
        let input_idle = input_idle_seconds
            .map(|seconds| seconds >= INPUT_IDLE_TIMEOUT_SECS)
            .unwrap_or(false);
        let displays_asleep = macos_activity::all_displays_are_asleep();

        if input_idle || displays_asleep {
            was_paused = true;
            macos_timer::sleep_with_leeway(
                PAUSED_CHECK_INTERVAL_MS,
                PAUSED_CHECK_INTERVAL_MS / 2,
            );
            continue;
        }

        // Resume with a short first interval so clipboard changes made while
        // paused are picked up promptly.
        if was_paused {
            CURRENT_INTERVAL_MS.store(MIN_INTERVAL_MS, Ordering::Relaxed);
            was_paused = false;
        }

        let count = unsafe { NSPasteboard::generalPasteboard().changeCount() };
        let prev = LAST_CHANGE_COUNT.swap(count, Ordering::Relaxed);
        if prev != -1 && prev != count {
            return true;
        }

        let interval_ms = if CLIPBOARD_ACTIVITY.swap(false, Ordering::Relaxed) {
            MIN_INTERVAL_MS
        } else {
            let prev_interval = CURRENT_INTERVAL_MS.load(Ordering::Relaxed);
            (prev_interval * 2).min(MAX_INTERVAL_MS)
        };
        CURRENT_INTERVAL_MS.store(interval_ms, Ordering::Relaxed);

        // Do not let the adaptive clipboard timer overshoot the 60-second
        // inactivity boundary. Use zero leeway for this final partial wait.
        let until_idle_ms = input_idle_seconds.map(|seconds| {
            ((INPUT_IDLE_TIMEOUT_SECS - seconds).max(0.0) * 1000.0).ceil() as u64
        });
        let sleep_ms = until_idle_ms
            .filter(|remaining| *remaining > 0)
            .map(|remaining| interval_ms.min(remaining))
            .unwrap_or(interval_ms);
        let leeway_ms = if sleep_ms < interval_ms {
            0
        } else {
            (interval_ms / 2).max(200)
        };
        macos_timer::sleep_with_leeway(sleep_ms, leeway_ms);
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) fn wait_for_next_clipboard_update() -> bool {
    use std::sync::atomic::AtomicU64;

    const MIN_INTERVAL_MS: u64 = 1000;
    const MAX_INTERVAL_MS: u64 = 5000;

    static CURRENT_INTERVAL_MS: AtomicU64 = AtomicU64::new(MIN_INTERVAL_MS);

    let interval = if CLIPBOARD_ACTIVITY.swap(false, Ordering::Relaxed) {
        MIN_INTERVAL_MS
    } else {
        let prev = CURRENT_INTERVAL_MS.load(Ordering::Relaxed);
        (prev * 2).min(MAX_INTERVAL_MS)
    };
    CURRENT_INTERVAL_MS.store(interval, Ordering::Relaxed);

    std::thread::sleep(std::time::Duration::from_millis(interval));
    true
}

#[cfg(target_os = "windows")]
pub(super) fn get_cursor_position() -> Option<Point> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut point = POINT { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut point).is_ok() {
            Some(Point::new(point.x as f32, point.y as f32))
        } else {
            None
        }
    }
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn macos_cursor_logical() -> Option<core_graphics::geometry::CGPoint> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let event = CGEvent::new(source).ok()?;
    Some(event.location())
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
pub(super) fn get_cursor_position() -> Option<Point> {
    let pt = macos_cursor_logical()?;
    let scale = macos_display_at(pt).map(macos_display_scale).unwrap_or(1.0);
    Some(Point::new(pt.x as f32 * scale, pt.y as f32 * scale))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) fn get_cursor_position() -> Option<Point> {
    None
}

#[cfg(target_os = "windows")]
pub(super) fn get_monitor_rect_at_cursor() -> Option<(Point, Size)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    unsafe {
        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point).is_err() {
            return None;
        }

        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };

        if GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
            let rect = monitor_info.rcMonitor;
            Some((
                Point::new(rect.left as f32, rect.top as f32),
                Size::new(
                    (rect.right - rect.left) as f32,
                    (rect.bottom - rect.top) as f32,
                ),
            ))
        } else {
            None
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn get_monitor_rect_at_cursor() -> Option<(Point, Size)> {
    let pt = macos_cursor_logical()?;
    let display = macos_display_at(pt)?;
    let scale = macos_display_scale(display);
    let b = display.bounds();
    Some((
        Point::new(b.origin.x as f32 * scale, b.origin.y as f32 * scale),
        Size::new(b.size.width as f32 * scale, b.size.height as f32 * scale),
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) fn get_monitor_rect_at_cursor() -> Option<(Point, Size)> {
    None
}

#[cfg(target_os = "windows")]
pub(super) fn get_monitor_scale_factor_at_cursor() -> Option<f32> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut point = POINT { x: 0, y: 0 };

    unsafe {
        if GetCursorPos(&mut point).is_err() {
            return None;
        }

        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut dpi_x = 0u32;
        let mut dpi_y = 0u32;

        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() && dpi_x > 0
        {
            Some(dpi_x as f32 / 96.0)
        } else {
            None
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn get_monitor_scale_factor_at_cursor() -> Option<f32> {
    let pt = macos_cursor_logical()?;
    let display = macos_display_at(pt)?;
    Some(macos_display_scale(display))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) fn get_monitor_scale_factor_at_cursor() -> Option<f32> {
    None
}

#[cfg(target_os = "macos")]
pub(super) fn apply_tool_window_style() {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;

    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn apply_tool_window_style() {}

#[cfg(target_os = "macos")]
pub(super) fn hide_application() {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.hide(None);
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn hide_application() {}
