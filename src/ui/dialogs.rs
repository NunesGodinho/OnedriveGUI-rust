//! Questions and messages over the main window, one at a time.

use std::time::Duration;

use egui::{Align, Layout, RichText};

use crate::app::{App, Dialog};
use crate::ui::widgets::{AMBER, WARNING_TEXT, icon};

/// Seconds before resync can be agreed to.
const RESYNC_COUNTDOWN: u64 = 5;

enum Answer {
    Yes,
    No,
}

/// Yes/No buttons, No first and focused (the safe default).
fn yes_no(ui: &mut egui::Ui, yes_label: &str, yes_enabled: bool) -> Option<Answer> {
    let mut answer = None;
    ui.add_space(8.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let no = ui.button("  No  ");
        if no.clicked() {
            answer = Some(Answer::No);
        }
        if ui.add_enabled(yes_enabled, egui::Button::new(yes_label)).clicked() {
            answer = Some(Answer::Yes);
        }
    });
    answer
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(dialog) = app.dialogs.front() else { return };
    let mut answer: Option<Answer> = None;
    let mut dismissed = false;

    // The main window is narrow: keep the dialog inside it.
    let width = (ctx.content_rect().width() - 64.0).clamp(200.0, 460.0);
    let modal = egui::Modal::new(egui::Id::new("dialog")).show(ctx, |ui| {
        ui.set_max_width(width);
        match dialog {
            Dialog::Quit => {
                ui.heading("Quit OnedriveLinux?");
                ui.add_space(6.0);
                ui.label("Would you like to stop all sync operations and quit OnedriveLinux?");
                answer = yes_no(ui, "  Yes  ", true);
            }
            Dialog::Resync { profile, opened } => {
                ui.horizontal(|ui| {
                    ui.add(icon("triangle-alert", 24.0, AMBER));
                    ui.heading(format!("Resync required for profile {profile}"));
                });
                ui.add_space(6.0);
                ui.label("An application configuration change has been detected where a resync is required.");
                ui.add_space(4.0);
                ui.label(
                    "The use of resync will remove your local 'onedrive' client state, thus no record will exist \
                     regarding your current 'sync status'.",
                );
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("This has the potential to overwrite local versions of files with potentially older versions downloaded from OneDrive which can lead to ");
                    ui.label(RichText::new("data loss").strong().color(WARNING_TEXT));
                    ui.label(".");
                });
                ui.add_space(4.0);
                ui.label("If in-doubt, backup your local data first before proceeding with resync.");
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Would you like to perform resync for profile {profile}?")).strong());
                let left = RESYNC_COUNTDOWN.saturating_sub(opened.elapsed().as_secs());
                let label = if left > 0 { format!("  Yes ({left})  ") } else { "  Yes  ".to_string() };
                answer = yes_no(ui, &label, left == 0);
                if left > 0 {
                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                }
            }
            Dialog::BigDelete { profile } => {
                ui.horizontal(|ui| {
                    ui.add(icon("triangle-alert", 24.0, AMBER));
                    ui.heading(format!("Big Delete detected for profile {profile}"));
                });
                ui.add_space(6.0);
                ui.label("An attempt to remove a large volume of data from OneDrive has been detected.");
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("This has the potential to delete a large volume of data from your OneDrive which can lead to ");
                    ui.label(RichText::new("data loss").strong().color(WARNING_TEXT));
                    ui.label(".");
                });
                ui.add_space(4.0);
                ui.label("If in-doubt, backup your OneDrive data first before proceeding with big delete.");
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Would you like to proceed with big delete for profile {profile}?")).strong());
                answer = yes_no(ui, "  Yes  ", true);
            }
            Dialog::Message { title, text, error } => {
                ui.horizontal(|ui| {
                    if *error {
                        ui.add(icon("triangle-alert", 24.0, AMBER));
                    }
                    ui.heading(title);
                });
                ui.add_space(6.0);
                ui.add(egui::Label::new(text).wrap());
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("  OK  ").clicked() {
                        dismissed = true;
                    }
                });
            }
        }
    });
    // Escape or a click outside counts as "No".
    if modal.should_close() && answer.is_none() {
        answer = Some(Answer::No);
        dismissed = true;
    }
    if answer.is_none() && !dismissed {
        return;
    }
    let Some(dialog) = app.dialogs.pop_front() else { return };
    match (dialog, answer) {
        (Dialog::Quit, Some(Answer::Yes)) => app.quit(ctx),
        (Dialog::Quit, _) => log::info!("[GUI] Keeping OnedriveLinux running."),
        (Dialog::Resync { profile, .. }, Some(Answer::Yes)) => {
            log::info!("Authorize sync: Yes");
            app.stop_sync(&profile);
            app.start_sync(&profile, &["--resync", "--resync-auth"]);
        }
        (Dialog::Resync { profile, .. }, _) => {
            log::info!("Authorize sync: No");
            app.stop_sync(&profile);
        }
        (Dialog::BigDelete { profile }, Some(Answer::Yes)) => {
            log::info!("Authorize big delete: Yes");
            app.stop_sync(&profile);
            app.start_sync(&profile, &["--force"]);
            if let Some(rt) = app.runtime.get_mut(&profile) {
                rt.status = "Big delete approved".into();
            }
        }
        (Dialog::BigDelete { profile }, _) => {
            log::info!("Authorize big delete: No");
            app.stop_sync(&profile);
            if let Some(rt) = app.runtime.get_mut(&profile) {
                rt.status = "Big delete denied".into();
            }
        }
        (Dialog::Message { .. }, _) => {}
    }
}
