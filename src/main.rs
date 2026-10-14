#![windows_subsystem = "windows"]
#![allow(dead_code)]

mod app;
mod art;
mod geometry;
mod gfx;
mod i18n;
mod icon;
mod input;
mod media;
mod mpv;
mod player;
mod playlist;
mod settings;
mod shell;
mod speed;
mod spring;
mod tokens;
mod update;
mod ui;
mod volume;
mod win;

/// A crash leaves a readable note next to the settings instead of vanishing silently.
fn install_panic_log() {
    std::panic::set_hook(Box::new(|info| {
        let text = format!("{info}\n{}", std::backtrace::Backtrace::force_capture());
        let _ = std::fs::write(settings::data_dir().join("crash.log"), &text);
        win::message_box(&format!("Reyer stopped unexpectedly.\n\n{info}\n\nDetails: %APPDATA%/Reyer/crash.log"), "Reyer");
    }));
}

fn main() {
    install_panic_log();
    win::set_app_id();
    let code = shell::run(app::Cli::parse());
    std::process::exit(code);
}
