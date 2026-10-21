//! OnedriveLinux: a GUI for the OneDrive Client for Linux (abraunegg/onedrive) with multi-account
//! support. A Rust/egui port of the Python OneDriveGUI; it shares its settings and profiles in
//! `~/.config/onedrive-gui`, so either can be used.
//!
//! Usage: `onedrivelinux [--version] [--help]`

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod app;
mod assets;
mod autostart;
mod events;
mod gui_settings;
mod ini;
mod logger;
mod maintenance;
mod onedrive_config;
mod parser;
mod process;
mod profiles;
mod transfers;
mod tray;
mod ui;
mod utils;
mod version;
mod worker;

use std::sync::Arc;

use eframe::egui;

/// The `.desktop` file name, so Wayland compositors and docks match the window to its icon.
pub const APP_ID: &str = "OnedriveLinux";

const HELP: &str = "\
OnedriveLinux - a GUI for the OneDrive Client for Linux

Usage: onedrivelinux [OPTIONS]

Options:
  --version   Print the version and exit
  -h, --help  Print this help and exit

Environment:
  RUST_LOG=<level>         Log level (debug, info, warning, error); overrides the GUI setting
  ONEDRIVELINUX_WAYLAND=1    Use native Wayland even with a tray icon (the window then
                           minimizes instead of hiding in the tray)

Settings and profiles: ~/.config/onedrive-gui (shared with the Python OneDriveGUI)";

/// Whether to run through XWayland: winit can't hide a window on Wayland, which hiding in the
/// tray needs. Only when there is a tray and an X server to use.
fn prefer_x11(has_tray: bool) -> bool {
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    has_tray && set("WAYLAND_DISPLAY") && set("DISPLAY") && std::env::var("ONEDRIVELINUX_WAYLAND").map_or(true, |v| v != "1")
}

fn main() -> eframe::Result {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("onedrivelinux {}", version::GUI_VERSION);
                return Ok(());
            }
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            other => eprintln!("onedrivelinux: ignoring unknown argument {other:?} (see --help)"),
        }
    }

    let settings = gui_settings::GuiSettings::load();
    logger::install(&settings);
    std::panic::set_hook(Box::new(|info| {
        let trace = std::backtrace::Backtrace::capture();
        log::error!("internal error: {info}\n{trace}");
    }));
    log::info!("Starting OnedriveLinux v{}", version::GUI_VERSION);
    log::info!("Onedrive client location: '{}'", settings.client_bin());

    let profiles = profiles::load();
    if !profiles.is_empty()
        && let Err(e) = profiles::save_all(&profiles)
    {
        log::error!("[GUI] {e}");
    }

    let (events, rx) = events::channel_pair();
    let tray = tray::spawn(events.clone());
    // Start in the tray (or minimized) when asked to; never when there is no profile yet, so the
    // setup wizard can be seen.
    let start_hidden = settings.start_minimized && !profiles.is_empty();
    let hidden_in_tray = start_hidden && tray.is_some();
    match (start_hidden, tray.is_some()) {
        (true, true) => log::info!("[GUI] Starting OnedriveLinux minimized to system tray"),
        (true, false) => log::info!("[GUI] Starting OnedriveLinux minimized to taskbar/dock"),
        _ => log::info!("[GUI] Starting OnedriveLinux maximized"),
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("OnedriveLinux v{}", version::GUI_VERSION))
        .with_app_id(APP_ID)
        .with_inner_size([440.0, 720.0])
        .with_min_inner_size([360.0, 420.0])
        .with_decorations(!settings.frameless_window)
        .with_visible(!hidden_in_tray);
    if let Some(icon) = assets::icon_data(assets::APP_ICON) {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    if prefer_x11(tray.is_some()) {
        log::info!("[GUI] Wayland session with a tray: using XWayland so the window can hide in the tray (ONEDRIVELINUX_WAYLAND=1 to opt out)");
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            builder.with_x11();
        }));
    }

    eframe::run_native(
        "OnedriveLinux",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, settings, profiles, events, rx, tray, start_hidden)))),
    )
}
