use iced::{Point, Size};

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
