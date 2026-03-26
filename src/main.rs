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
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_TERMINATE,
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
