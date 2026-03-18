use iced::{Point, Size};

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
                Size::new((rect.right - rect.left) as f32, (rect.bottom - rect.top) as f32),
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
