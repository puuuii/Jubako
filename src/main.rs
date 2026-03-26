#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod db;

#[cfg(target_os = "windows")]
fn run_detached_if_needed() {
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

#[cfg(target_os = "windows")]
fn terminate_existing_resident_processes() {
    if let Err(error) = terminate_existing_resident_processes_inner() {
        eprintln!("Failed to terminate existing resident process: {error}");
    }
}

#[cfg(target_os = "windows")]
fn terminate_existing_resident_processes_inner() -> anyhow::Result<()> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    let current_pid = std::process::id();
    let current_exe = std::env::current_exe()?;
    let current_exe = std::fs::canonicalize(&current_exe).unwrap_or(current_exe);
    let current_exe_lower = current_exe.to_string_lossy().to_ascii_lowercase();

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let mut has_entry = Process32FirstW(snapshot, &mut entry).is_ok();
        while has_entry {
            if entry.th32ProcessID != current_pid {
                if let Ok(process) = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
                    false,
                    entry.th32ProcessID,
                ) {
                    let mut path_buffer = vec![0u16; 32768];
                    let mut path_len = path_buffer.len() as u32;

                    if QueryFullProcessImageNameW(
                        process,
                        PROCESS_NAME_FORMAT(0),
                        PWSTR(path_buffer.as_mut_ptr()),
                        &mut path_len,
                    )
                    .is_ok()
                    {
                        path_buffer.truncate(path_len as usize);
                        let process_exe = PathBuf::from(OsString::from_wide(&path_buffer));
                        let process_exe =
                            std::fs::canonicalize(&process_exe).unwrap_or(process_exe);
                        let process_exe_lower = process_exe.to_string_lossy().to_ascii_lowercase();

                        if process_exe_lower == current_exe_lower {
                            if TerminateProcess(process, 0).is_ok() {
                                let _ = WaitForSingleObject(process, 2_000);
                            }
                        }
                    }

                    let _ = CloseHandle(process);
                }
            }

            has_entry = Process32NextW(snapshot, &mut entry).is_ok();
        }

        let _ = CloseHandle(snapshot);
    }

    Ok(())
}

#[cfg(target_os = "windows")]
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

fn main() -> iced::Result {
    #[cfg(target_os = "windows")]
    run_detached_if_needed();

    app::run()
}
