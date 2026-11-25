use iced::{Point, Size};

pub fn ensure_detached_background() {
    use std::ffi::OsStr;

    let should_stay_foreground = std::env::args_os()
        .any(|arg| arg == OsStr::new("--background") || arg == OsStr::new("--foreground"));

    if should_stay_foreground {
        return;
    }

    terminate_existing_resident_processes();

    if spawn_detached_background_process().is_ok() {
        std::process::exit(0);
    }
}

fn terminate_existing_resident_processes() {
    if let Err(error) = terminate_existing_resident_processes_inner() {
        eprintln!("Failed to terminate existing resident process: {error}");
    }
}

fn terminate_existing_resident_processes_inner() -> anyhow::Result<()> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
    };

    let current_pid = std::process::id();
    let current_exe_name_lower = std::env::current_exe()?
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .ok_or_else(|| anyhow::anyhow!("Failed to determine current executable file name"))?;

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let mut has_entry = Process32FirstW(snapshot, &mut entry).is_ok();
        while has_entry {
            if entry.th32ProcessID != current_pid {
                let exe_name_end = entry
                    .szExeFile
                    .iter()
                    .position(|ch| *ch == 0)
                    .unwrap_or(entry.szExeFile.len());
                let process_exe_name_lower =
                    String::from_utf16_lossy(&entry.szExeFile[..exe_name_end]).to_ascii_lowercase();

                if process_exe_name_lower == current_exe_name_lower {
                    if let Ok(process) = OpenProcess(PROCESS_TERMINATE, false, entry.th32ProcessID)
                    {
                        if TerminateProcess(process, 0).is_ok() {
                            let _ = WaitForSingleObject(process, 2_000);
                        }
                        let _ = CloseHandle(process);
                    }
                }
            }

            has_entry = Process32NextW(snapshot, &mut entry).is_ok();
        }

        let _ = CloseHandle(snapshot);
    }

    Ok(())
}

fn spawn_detached_background_process() -> std::io::Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut command = Command::new(std::env::current_exe()?);

    for arg in std::env::args_os().skip(1) {
        if arg != OsStr::new("--background") {
            command.arg(arg);
        }
    }

    command.arg("--background");
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    command.spawn()?;

    Ok(())
}

pub(in crate::app) fn ensure_startup_registration() {
    if cfg!(debug_assertions) || std::env::args().any(|arg| arg == "--no-autostart") {
        return;
    }

    if let Err(error) = ensure_startup_registration_inner() {
        eprintln!("Failed to configure startup registration: {error}");
    }
}

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

fn startup_command_string() -> anyhow::Result<String> {
    let exe = std::env::current_exe()?;
    let exe = exe
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Executable path contains non-UTF8 characters"))?;

    Ok(format!("\"{exe}\" --background"))
}

pub(in crate::app) fn wait_for_next_clipboard_update() -> bool {
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

pub(in crate::app) fn get_cursor_position() -> Option<Point> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut point = POINT { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut point)
            .ok()
            .map(|_| Point::new(point.x as f32, point.y as f32))
    }
}

pub(in crate::app) fn get_monitor_rect_at_cursor() -> Option<(Point, Size)> {
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

pub(in crate::app) fn get_monitor_scale_factor_at_cursor() -> Option<f32> {
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

pub(in crate::app) fn apply_tool_window_style() {}
pub(in crate::app) fn unhide_application() {}
pub(in crate::app) fn hide_application() {}
pub(in crate::app) fn simulate_paste() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        VIRTUAL_KEY, VK_CONTROL, VK_V,
    };

    fn key_input(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let inputs = [
        key_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key_input(VK_V, KEYBD_EVENT_FLAGS(0)),
        key_input(VK_V, KEYEVENTF_KEYUP),
        key_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];

    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}
