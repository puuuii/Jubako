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

    if spawn_detached_background_process().is_ok() {
        std::process::exit(0);
    }
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
