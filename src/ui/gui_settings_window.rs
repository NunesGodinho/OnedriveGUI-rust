//! OnedriveLinux's own settings (`GuiSettingsWindow` in the Python app). Edits apply on Save,
//! which also creates or removes the autostart entry.

use egui::{Align, Layout, RichText, Ui, ViewportBuilder, ViewportId};

use crate::app::App;
use crate::gui_settings::{DEBUG_LEVELS, GuiSettings};
use crate::ui::widgets::{OK_TEXT, WARNING_TEXT, pick_file, pick_folder};
use crate::utils::expand_tilde;

pub struct GuiSettingsWindow {
    draft: GuiSettings,
    /// Result of the last save: (message, is error).
    status: Option<(String, bool)>,
}

impl GuiSettingsWindow {
    pub fn new(settings: &GuiSettings) -> GuiSettingsWindow {
        GuiSettingsWindow { draft: settings.clone(), status: None }
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(mut window) = app.gui_settings_window.take() else { return };
    let mut open = true;
    let builder = ViewportBuilder::default().with_title("OnedriveLinux Settings").with_app_id(crate::APP_ID).with_inner_size([520.0, 600.0]).with_min_inner_size([440.0, 480.0]);
    ctx.show_viewport_immediate(ViewportId::from_hash_of("gui-settings-window"), builder, |ui, _| {
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            open = false;
        }
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| contents(app, &mut window, ui));
        });
    });
    if open {
        app.gui_settings_window = Some(window);
    }
}

fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong());
    egui::Frame::group(ui.style()).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

fn contents(app: &mut App, w: &mut GuiSettingsWindow, ui: &mut Ui) {
    let d = &mut w.draft;
    group(ui, "OnedriveLinux behaviour", |ui| {
        ui.checkbox(&mut d.start_minimized, "Start OnedriveLinux minimized");
        ui.checkbox(&mut d.autostart_enabled, "Start OnedriveLinux automatically on login");
    });
    group(ui, "OneDrive client options", |ui| {
        ui.label("Path to OneDrive client binary");
        ui.horizontal(|ui| {
            let browse = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let clicked = ui.button("Browse").clicked();
                ui.add(egui::TextEdit::singleline(&mut d.client_bin_path).hint_text("onedrive").desired_width(f32::INFINITY));
                clicked
            });
            if browse.inner
                && let Some(file) = pick_file(Some(std::path::Path::new("/usr/bin")))
            {
                d.client_bin_path = file.display().to_string();
            }
        });
    });
    group(ui, "Appearance", |ui| {
        ui.checkbox(&mut d.frameless_window, "Frameless window").on_hover_text("Hide the window's title bar; drag the blue header to move it.");
        ui.checkbox(&mut d.combined_start_stop_button, "Combined Start/Stop button");
    });
    group(ui, "Logging", |ui| {
        ui.label(RichText::new("Logging changes take effect after a restart.").small().weak());
        egui::Grid::new("gui-logging").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("Debug level:");
            egui::ComboBox::from_id_salt("debug-level").selected_text(d.debug_level.clone()).show_ui(ui, |ui| {
                for level in DEBUG_LEVELS {
                    ui.selectable_value(&mut d.debug_level, level.to_string(), level);
                }
            });
            ui.end_row();
            ui.label("Log file:");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut d.log_file).desired_width(260.0));
                if ui.button("Browse").clicked() {
                    let current = expand_tilde(&d.log_file);
                    if let Some(dir) = pick_folder(current.parent()) {
                        let name = current.file_name().map_or_else(|| "onedrive-gui.log".into(), |n| n.to_string_lossy().into_owned());
                        d.log_file = dir.join(name).display().to_string();
                    }
                }
            });
            ui.end_row();
            ui.label("Log rotation (hours):");
            ui.add(egui::DragValue::new(&mut d.log_rotation_interval).range(1..=8760));
            ui.end_row();
            ui.label("Log backup count:");
            ui.add(egui::DragValue::new(&mut d.log_backup_count).range(0..=100));
            ui.end_row();
        });
        ui.checkbox(&mut d.show_debug, "Show debug logs in console (stdout)");
        ui.checkbox(&mut d.save_debug, "Save debug logs to file");
    });

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if let Some((text, error)) = &w.status {
            ui.colored_label(if *error { WARNING_TEXT } else { OK_TEXT }, text);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let changed = w.draft != app.settings;
            if ui.add_enabled(changed, egui::Button::new("Save").min_size([90.0, 26.0].into())).clicked() {
                let frameless = w.draft.frameless_window;
                if frameless != app.settings.frameless_window {
                    ui.ctx().send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Decorations(!frameless));
                }
                w.status = Some(save(app, &w.draft));
            }
        });
    });
}

/// Save the settings, apply what can change live, and sync the autostart entry.
fn save(app: &mut App, draft: &GuiSettings) -> (String, bool) {
    let client_changed = draft.client_bin() != app.settings.client_bin();
    app.settings = draft.clone();
    if let Err(e) = app.settings.save() {
        log::error!("[GUI][SETTINGS] {e}");
        return (e, true);
    }
    if client_changed {
        crate::version::check(app.events.clone(), app.settings.client_bin());
    }
    match crate::autostart::sync(app.settings.autostart_enabled) {
        Ok(()) => ("Settings saved.".into(), false),
        Err(e) => {
            log::error!("[GUI][SETTINGS] {e}");
            (format!("Settings saved, but the autostart entry could not be updated: {e}"), true)
        }
    }
}
