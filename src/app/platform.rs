use iced::{Point, Size};
use std::time::Duration;

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
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

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
pub(super) fn wait_for_clipboard_update(timeout: Duration) -> bool {
    use once_cell::sync::Lazy;
    use std::sync::{mpsc, Mutex};

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
        std::thread::sleep(timeout);
        return true;
    };
    let Ok(receiver) = receiver.lock() else {
        std::thread::sleep(timeout);
        return true;
    };

    match receiver.recv_timeout(timeout) {
        Ok(()) => true,
        Err(mpsc::RecvTimeoutError::Timeout) => false,
        Err(mpsc::RecvTimeoutError::Disconnected) => true,
    }
}

#[cfg(target_os = "windows")]
fn clipboard_listener_thread(sender: std::sync::mpsc::Sender<()>) {
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::{
        AddClipboardFormatListener, RemoveClipboardFormatListener,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW, TranslateMessage, CREATESTRUCTW,
        GWLP_USERDATA, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE,
        WM_NCCREATE, WM_NCDESTROY, WNDCLASSW,
    };

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

#[cfg(not(target_os = "windows"))]
pub(super) fn wait_for_clipboard_update(timeout: Duration) -> bool {
    std::thread::sleep(timeout);
    false
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

#[cfg(not(target_os = "windows"))]
pub(super) fn get_cursor_position() -> Option<Point> {
    None
}

#[cfg(target_os = "windows")]
pub(super) fn get_monitor_rect_at_cursor() -> Option<(Point, Size)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
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

#[cfg(not(target_os = "windows"))]
pub(super) fn get_monitor_rect_at_cursor() -> Option<(Point, Size)> {
    None
}

#[cfg(target_os = "windows")]
pub(super) fn get_monitor_scale_factor_at_cursor() -> Option<f32> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
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

#[cfg(not(target_os = "windows"))]
pub(super) fn get_monitor_scale_factor_at_cursor() -> Option<f32> {
    None
}

#[cfg(target_os = "windows")]
pub(super) fn apply_tool_window_style() {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };

    unsafe {
        let window = GetForegroundWindow();
        if !window.is_invalid() {
            let style = GetWindowLongW(window, GWL_EXSTYLE);
            SetWindowLongW(window, GWL_EXSTYLE, style | WS_EX_TOOLWINDOW.0 as i32);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn apply_tool_window_style() {}
