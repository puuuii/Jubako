#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod db;
mod image_codec;

fn main() -> iced::Result {
    app::platform::ensure_detached_background();
    app::run()
}
