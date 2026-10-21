//! The main window: the selected profile's status on the blue header, its recent transfers,
//! and buttons for the sync folder, the Profiles window and the GUI settings
//! (`ProfileStatusPage` in the Python app).

use std::time::SystemTime;

use egui::{Align, Color32, Frame, Layout, Margin, RichText, Sense, Ui, ViewportCommand};

use crate::app::App;
use crate::transfers::{Kind, TransferItem};
use crate::ui::widgets::{AMBER, ONEDRIVE_BLUE, RUNNING, STOPPED, icon, icon_button};
use crate::utils::{format_relative_time, open_path};
use crate::version::CLIENT_INSTALL_DOCS;

const ROW_HEIGHT: f32 = 62.0;
const WHITE: Color32 = Color32::WHITE;

pub fn show(app: &mut App, ui: &mut Ui) {
    egui::Panel::top("header").frame(Frame::NONE.fill(ONEDRIVE_BLUE).inner_margin(Margin::symmetric(10, 8))).show(ui, |ui| header(app, ui));
    egui::Panel::bottom("footer").frame(Frame::side_top_panel(ui.style()).inner_margin(Margin::symmetric(10, 6))).show(ui, |ui| footer(app, ui));
    egui::CentralPanel::default().show(ui, |ui| {
        if app.profiles.is_empty() {
            no_profiles(app, ui);
        } else {
            transfer_list(app, ui);
        }
    });
}

fn white(text: impl Into<String>) -> RichText {
    RichText::new(text).color(WHITE)
}

/// (status lines, tooltip) for a missing, unsupported or outdated OneDrive client.
fn version_warnings(app: &App) -> Option<(Vec<String>, String)> {
    let (line, tip) = app.client_version.as_ref()?.warning()?;
    Some((vec![line], tip))
}

fn header(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    // With a frameless window, the header moves the window. Added first, so the buttons on top
    // of it still take their clicks.
    if app.settings.frameless_window {
        let drag = ui.interact(ui.max_rect(), ui.id().with("window-drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
    }

    ui.horizontal(|ui| {
        if icon_button(ui, "power", 20.0, WHITE, true, "Quit").clicked() {
            app.request_quit(&ctx);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if app.settings.frameless_window && icon_button(ui, "x", 22.0, WHITE, true, "Close to tray/dock").clicked() {
                app.hide_window(&ctx);
            }
            if app.profiles.len() > 1 {
                let current = app.selected_name().unwrap_or_default();
                ui.add(icon("user-round", 20.0, WHITE));
                egui::ComboBox::from_id_salt("profile").selected_text(current).show_ui(ui, |ui| {
                    for (i, name) in app.profiles.keys().enumerate() {
                        ui.selectable_value(&mut app.selected, i, name);
                    }
                });
            } else if let Some(name) = app.selected_name() {
                ui.label(white(name).small());
            }
        });
    });

    let Some(name) = app.selected_name() else {
        ui.label(white("No OneDrive profile configured").size(15.0));
        return;
    };
    let can_sync = app.can_sync();
    let warnings = version_warnings(app);
    let Some(rt) = app.runtime.get(&name) else { return };
    let running = rt.is_running();

    ui.add_space(4.0);
    ui.add(egui::Label::new(white(&rt.status).size(15.0)).wrap());
    if !rt.account_type.is_empty() {
        ui.label(white(&rt.account_type).small());
    }
    if let Some((lines, _)) = &warnings {
        for line in lines {
            if line.starts_with("Unsupported") {
                ui.horizontal(|ui| {
                    ui.label(white(line.as_str()).color(Color32::from_rgb(255, 225, 120)));
                    ui.hyperlink_to(white("install docs"), CLIENT_INSTALL_DOCS);
                });
            } else {
                ui.label(white(line.as_str()).color(Color32::from_rgb(255, 225, 120)));
            }
        }
    }
    ui.add_space(4.0);

    let mut start = false;
    let mut stop = false;
    ui.horizontal(|ui| {
        if app.settings.combined_start_stop_button {
            let (name, tip) = if running { ("square", "Stop Sync") } else { ("play", "Start Sync") };
            if icon_button(ui, name, 24.0, WHITE, can_sync || running, tip).clicked() {
                if running {
                    stop = true;
                } else {
                    start = true;
                }
            }
        } else {
            start = icon_button(ui, "play", 22.0, WHITE, can_sync && !running, "Start Sync").clicked();
            stop = icon_button(ui, "square", 22.0, WHITE, running, "Stop Sync").clicked();
        }
        if let Some((_, tip)) = &warnings {
            ui.add(icon("triangle-alert", 20.0, AMBER)).on_hover_text(tip);
        }
        if let Some(error) = &rt.error {
            ui.add(icon("triangle-alert", 20.0, AMBER)).on_hover_text(error);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (name, tint, tip) = if running { ("circle-check", RUNNING, "Sync is running") } else { ("circle-x", STOPPED, "Sync is stopped") };
            ui.add(icon(name, 22.0, tint)).on_hover_text(tip);
            if !rt.free_space.is_empty() {
                ui.add(icon("hard-drive", 22.0, WHITE)).on_hover_text("Free Space");
                ui.label(white(&rt.free_space)).on_hover_text("Free Space");
            }
        });
    });
    if start {
        app.start_sync(&name, &[]);
    }
    if stop {
        app.stop_sync(&name);
    }
}

fn footer(app: &mut App, ui: &mut Ui) {
    let sync_dir = app.selected_name().and_then(|n| app.profiles.get(&n)).map(|p| p.sync_dir());
    let tint = ui.visuals().text_color();
    ui.horizontal(|ui| {
        let width = 3.0 * 30.0 + 2.0 * 24.0;
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        ui.spacing_mut().item_spacing.x = 24.0;
        if icon_button(ui, "folder", 26.0, tint, sync_dir.is_some(), "Open Sync Directory").clicked()
            && let Some(dir) = &sync_dir
        {
            open_path(dir);
        }
        if icon_button(ui, "user-round", 26.0, tint, true, "Profiles").clicked() {
            app.open_profiles_window();
        }
        if icon_button(ui, "settings", 26.0, tint, true, "OnedriveLinux Settings").clicked() {
            app.open_gui_settings();
        }
    });
}

fn no_profiles(app: &mut App, ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.add(icon("cloud", 64.0, ONEDRIVE_BLUE));
        ui.add_space(8.0);
        ui.label("No OneDrive profile yet.");
        ui.add_space(8.0);
        if ui.button("Create or import a OneDrive profile").clicked() {
            app.open_wizard(true);
        }
    });
}

/// The Lucide icon for an entry: a bin for removals, else one by file type.
fn file_icon(item: &TransferItem) -> &'static str {
    if matches!(item.kind, Kind::Delete | Kind::Trash) {
        return "trash-2";
    }
    let ext = item.file_name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "bmp" | "svg" | "tif" | "tiff" => "image",
        "mp3" | "flac" | "ogg" | "wav" | "m4a" | "opus" => "file-music",
        "mp4" | "mkv" | "mov" | "avi" | "webm" => "film",
        "zip" | "gz" | "xz" | "7z" | "rar" | "tar" | "bz2" | "zst" | "iso" => "file-archive",
        "xls" | "xlsx" | "ods" | "csv" => "file-spreadsheet",
        "rs" | "py" | "js" | "ts" | "c" | "h" | "cpp" | "java" | "go" | "sh" | "html" | "css" | "json" | "xml" | "toml" | "yaml" | "yml" => "file-code",
        "pdf" | "doc" | "docx" | "odt" | "txt" | "md" | "rtf" | "ppt" | "pptx" | "odp" => "file-text",
        _ => "file",
    }
}

fn transfer_list(app: &mut App, ui: &mut Ui) {
    let Some(name) = app.selected_name() else { return };
    let Some(rt) = app.runtime.get(&name) else { return };
    let items = &rt.transfers.items;
    if items.is_empty() {
        ui.centered_and_justified(|ui| ui.weak("Recent file transfers will appear here."));
        return;
    }
    let now = SystemTime::now();
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, ROW_HEIGHT, items.len(), |ui, range| {
        for item in items.range(range) {
            transfer_row(ui, item, now);
        }
    });
}

/// One entry, exactly [`ROW_HEIGHT`] tall (as `show_rows` expects), with a divider below.
fn transfer_row(ui: &mut Ui, item: &TransferItem, now: SystemTime) {
    let muted = ui.visuals().weak_text_color();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, ui.visuals().widgets.noninteractive.bg_stroke);
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(2.0, 4.0))).layout(Layout::left_to_right(Align::Center)));
    row.horizontal_centered(|ui| {
        let tint = ui.visuals().text_color();
        ui.add_sized([34.0, 34.0], icon(file_icon(item), 24.0, tint));
        ui.vertical(|ui| {
            let time = item.completed_at.map(|t| format_relative_time(t, now)).unwrap_or_default();
            egui::Sides::new().shrink_left().truncate().show(
                ui,
                |ui| ui.label(RichText::new(&item.file_name).strong()).on_hover_text(&item.file_name),
                |ui| ui.label(RichText::new(time).small().color(muted)),
            );
            egui::Sides::new().shrink_left().truncate().show(
                ui,
                |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(&item.headline).small());
                    if let Some((label, folder)) = &item.folder_link
                        && ui.link(RichText::new(label).small()).on_hover_text(folder.display().to_string()).clicked()
                    {
                        open_path(folder);
                    }
                },
                |ui| ui.label(RichText::new(&item.detail).small().color(muted)),
            );
            if !item.complete {
                ui.add(egui::ProgressBar::new(f32::from(item.progress) / 100.0).desired_height(4.0));
            }
        });
    });
}
