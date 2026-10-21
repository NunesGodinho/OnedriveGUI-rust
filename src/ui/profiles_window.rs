//! The Profiles window: every profile's OneDrive client options, its selective-sync list and
//! account actions (`ProfileSettingsWindow` / `ProfileSettingsPage` in the Python app).
//!
//! Each profile is edited as a draft; Save writes the client config (and `sync_list`), Discard
//! restores the saved values. Closing with unsaved drafts asks first.

use std::collections::{BTreeSet, HashMap};

use egui::{Align, Layout, RichText, Ui, ViewportBuilder, ViewportCommand, ViewportId};

use crate::app::App;
use crate::onedrive_config::{self as odc, OdConfig};
use crate::profiles::{self, Profile};
use crate::ui::widgets::{AMBER, WARNING_TEXT, hint_label, icon, option_checkbox, option_number, option_text, pick_folder};
use crate::utils::expand_tilde;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Monitored,
    SyncOptions,
    Excluded,
    SelectiveSync,
    Webhooks,
    Logging,
}

const TABS: [(Tab, &str); 6] = [
    (Tab::Monitored, "Monitored Files"),
    (Tab::SyncOptions, "Sync Options"),
    (Tab::Excluded, "Excluded files"),
    (Tab::SelectiveSync, "Selective Sync"),
    (Tab::Webhooks, "Webhooks"),
    (Tab::Logging, "Logging"),
];

const UPLOAD_ONLY_NOTE: &str = "Perform a 'one-way' upload sync.\n\nNote: If a file or folder is present on OneDrive, that does not exist locally, \
     it will be removed.\nIf the data on OneDrive should be kept, please enable 'No remote delete' option.";
const PERMISSIONS_HINT: &str = "Important: Special permission bits (setuid, setgid, sticky bit) are not supported.\nValid permission values are from 000 to 777 only.";
const TENANT_HINT: &str = "Refer to the client documentation for single tenant application use.";
const SKIP_FILE_HINT: &str = "This option is used to 'skip' certain files and supports pattern matching.\n\
     Patterns are case insensitive. * and ? wildcards characters are supported.\nUse | to separate multiple patterns.";
const SKIP_DIR_HINT: &str = "This option is used to 'skip' certain directories and supports pattern matching.\n\
     Patterns are case insensitive. * and ? wildcards characters are supported.\nUse | to separate multiple patterns.\n\
     Important: Entries under Skip directories are relative to your Sync directory path.";
const SYNC_LIST_DOCS: &str = "https://github.com/abraunegg/onedrive/blob/master/docs/usage.md#performing-a-selective-synchronisation-via-sync_list-file";
const WEBHOOK_DOCS: &str = "https://github.com/abraunegg/onedrive/blob/master/docs/usage.md#use-webhook-to-subscribe-to-remote-updates-in-monitor-mode";
const BUSINESS_SHARED_DOCS: &str = "https://github.com/abraunegg/onedrive/blob/master/docs/business-shared-items.md";

/// One profile being edited.
struct Draft {
    profile: Profile,
    sync_list: String,
    sync_list_saved: String,
    new_skip_file: String,
    new_skip_dir: String,
    selected_skip_files: BTreeSet<String>,
    selected_skip_dirs: BTreeSet<String>,
}

impl Draft {
    fn new(profile: &Profile) -> Draft {
        let sync_list = profiles::read_sync_list(profile);
        Draft {
            profile: profile.clone(),
            sync_list_saved: sync_list.clone(),
            sync_list,
            new_skip_file: String::new(),
            new_skip_dir: String::new(),
            selected_skip_files: BTreeSet::new(),
            selected_skip_dirs: BTreeSet::new(),
        }
    }

    /// Only what this window edits counts; the account type and free space the client reports
    /// change underneath without being edits.
    fn is_unsaved(&self, saved: &Profile) -> bool {
        self.profile.onedrive != saved.onedrive || self.profile.auto_sync != saved.auto_sync || self.sync_list != self.sync_list_saved
    }
}

enum Confirm {
    Remove(String),
    Rename { old: String, new_name: String, error: Option<String> },
    CloseUnsaved(Vec<String>),
}

pub struct ProfilesWindow {
    selected: Option<String>,
    drafts: HashMap<String, Draft>,
    tab: Tab,
    confirm: Option<Confirm>,
    /// Last save problem, shown above the buttons.
    error: Option<String>,
}

impl ProfilesWindow {
    pub fn new(selected: Option<String>) -> ProfilesWindow {
        ProfilesWindow { selected, drafts: HashMap::new(), tab: Tab::Monitored, confirm: None, error: None }
    }

    pub fn select(&mut self, name: &str) {
        self.selected = Some(name.to_string());
    }

    fn unsaved(&self, app: &App) -> Vec<String> {
        app.profiles.iter().filter(|(n, p)| self.drafts.get(*n).is_some_and(|d| d.is_unsaved(p))).map(|(n, _)| n.clone()).collect()
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(mut window) = app.profiles_window.take() else { return };
    let mut open = true;
    let builder = ViewportBuilder::default().with_title("Profiles").with_app_id(crate::APP_ID).with_inner_size([1000.0, 700.0]).with_min_inner_size([820.0, 520.0]);
    ctx.show_viewport_immediate(ViewportId::from_hash_of("profiles-window"), builder, |ui, _| {
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            let unsaved = window.unsaved(app);
            if unsaved.is_empty() {
                log::debug!("[GUI] Closing Profiles window.");
                open = false;
            } else {
                ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
                window.confirm = Some(Confirm::CloseUnsaved(unsaved));
            }
        }
        contents(app, &mut window, ui, &mut open);
    });
    if open {
        app.profiles_window = Some(window);
    }
}

fn contents(app: &mut App, w: &mut ProfilesWindow, ui: &mut Ui, open: &mut bool) {
    // Keep the selection valid as profiles come and go.
    if w.selected.as_ref().is_none_or(|s| !app.profiles.contains_key(s)) {
        w.selected = app.profiles.keys().next().cloned();
    }
    w.drafts.retain(|name, _| app.profiles.contains_key(name));
    for (name, profile) in &app.profiles {
        w.drafts.entry(name.clone()).or_insert_with(|| Draft::new(profile));
    }

    confirmations(app, w, ui, open);

    egui::Panel::left("profile-list").resizable(false).exact_size(230.0).show(ui, |ui| {
        ui.add_space(6.0);
        ui.heading("Profiles");
        ui.separator();
        egui::Panel::bottom("profile-actions").frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 8))).show(ui, |ui| {
            if ui.add_sized([ui.available_width(), 26.0], egui::Button::new("Create / Import profile")).clicked() {
                app.open_wizard(false);
            }
            ui.horizontal(|ui| {
                let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                if ui.add_enabled(w.selected.is_some(), egui::Button::new("Rename").min_size([half, 24.0].into())).clicked()
                    && let Some(old) = w.selected.clone()
                {
                    w.confirm = Some(Confirm::Rename { new_name: old.clone(), old, error: None });
                }
                if ui.add_enabled(w.selected.is_some(), egui::Button::new("Remove").min_size([half, 24.0].into())).clicked()
                    && let Some(name) = w.selected.clone()
                {
                    w.confirm = Some(Confirm::Remove(name));
                }
            });
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (name, profile) in &app.profiles {
                let unsaved = w.drafts.get(name).is_some_and(|d| d.is_unsaved(profile));
                ui.horizontal(|ui| {
                    let selected = w.selected.as_deref() == Some(name.as_str());
                    if ui.selectable_label(selected, name).clicked() {
                        w.selected = Some(name.clone());
                    }
                    if unsaved {
                        ui.add(icon("triangle-alert", 16.0, AMBER)).on_hover_text("This profile has unsaved configuration changes.");
                    }
                });
            }
        });
    });

    egui::CentralPanel::default().show(ui, |ui| {
        let Some(name) = w.selected.clone() else {
            ui.centered_and_justified(|ui| ui.label("No profiles yet. Use \"Create / Import profile\" to add one."));
            return;
        };
        profile_page(app, w, ui, &name);
    });
}

fn confirmations(app: &mut App, w: &mut ProfilesWindow, ui: &mut Ui, open: &mut bool) {
    let Some(confirm) = w.confirm.as_mut() else { return };
    let mut done = false;
    let ctx = ui.ctx().clone();
    let modal = egui::Modal::new(egui::Id::new("profiles-confirm")).show(&ctx, |ui| {
        ui.set_max_width(440.0);
        match confirm {
            Confirm::Remove(name) => {
                ui.heading("Confirm Profile Removal");
                ui.add_space(6.0);
                ui.label(format!("Are you sure you want to remove the profile {name}?"));
                ui.label(RichText::new("Its OneDrive client config and synced files are left on disk.").small().weak());
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("  No  ").clicked() {
                        log::info!("[GUI] Profile removal cancelled for: {name}");
                        done = true;
                    }
                    if ui.button("  Yes  ").clicked() {
                        if let Err(e) = app.remove_profile(name) {
                            w.error = Some(e);
                        }
                        w.drafts.remove(name.as_str());
                        done = true;
                    }
                });
            }
            Confirm::Rename { old, new_name, error } => {
                ui.heading("Rename Profile");
                ui.add_space(6.0);
                ui.label(format!("Enter new name for profile '{old}':"));
                let edit = ui.add(egui::TextEdit::singleline(new_name).desired_width(f32::INFINITY));
                edit.request_focus();
                if let Some(e) = error {
                    ui.colored_label(WARNING_TEXT, e.as_str());
                }
                ui.add_space(8.0);
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        log::info!("[GUI] Rename profile dialog cancelled.");
                        done = true;
                    }
                    if ui.button("  OK  ").clicked() || enter {
                        let new = new_name.trim().to_string();
                        if new.is_empty() {
                            *error = Some("Profile name cannot be empty.".into());
                        } else if new == *old {
                            done = true;
                        } else if app.profiles.contains_key(&new) {
                            *error = Some(format!("Profile '{new}' already exists."));
                        } else {
                            if let Err(e) = app.rename_profile(old, &new) {
                                w.error = Some(e);
                            }
                            if let Some(draft) = w.drafts.remove(old.as_str()) {
                                w.drafts.insert(new.clone(), draft);
                            }
                            w.selected = Some(new);
                            done = true;
                        }
                    }
                });
            }
            Confirm::CloseUnsaved(names) => {
                ui.heading("Discard changes?");
                ui.add_space(6.0);
                ui.label(format!("You have unsaved changes in profile(s) {}.", names.join(", ")));
                ui.label("Discard changes and close Profiles window?");
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("  No  ").clicked() {
                        log::debug!("[GUI] Keeping Profiles window open.");
                        done = true;
                    }
                    if ui.button("  Yes  ").clicked() {
                        log::debug!("[GUI] Discarding changes and closing Profiles window.");
                        *open = false;
                        done = true;
                    }
                });
            }
        }
    });
    if done || modal.should_close() {
        w.confirm = None;
    }
}

fn profile_page(app: &mut App, w: &mut ProfilesWindow, ui: &mut Ui, name: &str) {
    let Some(saved) = app.profiles.get(name).cloned() else { return };
    let mut save = false;
    let mut discard = false;
    let mut logout = false;

    ui.add_space(4.0);
    ui.heading(name);
    ui.horizontal(|ui| {
        for (tab, label) in TABS {
            ui.selectable_value(&mut w.tab, tab, label);
        }
    });
    ui.separator();

    let Some(draft) = w.drafts.get_mut(name) else { return };
    let unsaved = draft.is_unsaved(&saved);
    egui::Panel::bottom("profile-buttons").frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 8))).show(ui, |ui| {
        if let Some(e) = &w.error {
            ui.colored_label(WARNING_TEXT, e);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let save_text = if unsaved { RichText::new("Save").strong() } else { RichText::new("Save") };
            save = ui.add_enabled(unsaved, egui::Button::new(save_text).min_size([90.0, 26.0].into())).clicked();
            discard = ui.add_enabled(unsaved, egui::Button::new("Discard changes").min_size([120.0, 26.0].into())).clicked();
            if unsaved {
                ui.add(icon("triangle-alert", 18.0, AMBER));
                ui.label("Unsaved changes");
            }
        });
    });

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| match w.tab {
        Tab::Monitored => logout = monitored_tab(ui, draft, saved.is_business()),
        Tab::SyncOptions => sync_options_tab(ui, &mut draft.profile.onedrive),
        Tab::Excluded => excluded_tab(ui, draft),
        Tab::SelectiveSync => selective_sync_tab(ui, draft),
        Tab::Webhooks => webhooks_tab(ui, &mut draft.profile.onedrive),
        Tab::Logging => logging_tab(ui, &mut draft.profile.onedrive),
    });

    if discard {
        *draft = Draft::new(&saved);
        w.error = None;
    }
    if save {
        let (onedrive, auto_sync, sync_list) = (draft.profile.onedrive.clone(), draft.profile.auto_sync, draft.sync_list.clone());
        if let Some(p) = app.profiles.get_mut(name) {
            p.onedrive = onedrive;
            p.auto_sync = auto_sync;
        }
        let mut errors = Vec::new();
        if let Err(e) = profiles::save_all(&app.profiles) {
            errors.push(e);
        }
        match profiles::write_sync_list(&draft.profile, &sync_list) {
            Ok(()) => draft.sync_list_saved = sync_list,
            Err(e) => errors.push(e),
        }
        w.error = (!errors.is_empty()).then(|| errors.join("\n"));
    }
    if logout {
        app.logout(name);
    }
}

fn group(ui: &mut Ui, title: &str, hint: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    let heading = ui.label(RichText::new(title).strong());
    if !hint.is_empty() {
        heading.on_hover_text(hint);
    }
    egui::Frame::group(ui.style()).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

/// A text option with a folder picker next to it.
fn folder_option(ui: &mut Ui, config: &mut OdConfig, key: &str, label: &str, hint: &str, enabled: bool) {
    ui.horizontal(|ui| {
        ui.add_enabled_ui(enabled, |ui| hint_label(ui, label, hint));
        let browse = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let browse = ui.add_enabled(enabled, egui::Button::new("Browse")).clicked();
            option_text(ui, config, key, "", enabled);
            browse
        });
        if browse.inner {
            let start = expand_tilde(odc::text(config, key));
            if let Some(dir) = pick_folder(Some(&start)) {
                log::info!("{}", dir.display());
                config.insert(key.to_string(), dir.display().to_string());
            }
        }
    });
}

/// Returns whether Logout was clicked.
fn monitored_tab(ui: &mut Ui, draft: &mut Draft, business: bool) -> bool {
    let mut logout = false;
    let config = &mut draft.profile.onedrive;
    group(ui, "Monitored directory", "", |ui| {
        folder_option(ui, config, "sync_dir", "Sync Directory:", "Specify local sync directory location for this profile.", true);
        let enabled = business || odc::is_true(config, "sync_business_shared_items");
        ui.horizontal(|ui| {
            option_checkbox(ui, config, "sync_business_shared_items", "Sync business shared items", "Only available for Business accounts.", enabled);
            ui.hyperlink_to("(docs)", BUSINESS_SHARED_DOCS);
        });
    });
    group(ui, "Recycle Bin", "", |ui| {
        option_checkbox(
            ui,
            config,
            "use_recycle_bin",
            "Use Recycle Bin",
            "By default, OneDrive client will process online deletions and directly delete the corresponding file or folder from your \
             configured 'sync_dir'.\nEnable this option to move deleted files to your Linux user default 'Recycle Bin', so that you can \
             manually delete the files at your own discretion.",
            true,
        );
        let enabled = odc::is_true(config, "use_recycle_bin");
        folder_option(
            ui,
            config,
            "recycle_bin_path",
            "Recycle Bin Path",
            "Specify an explicit 'Recycle Bin' directory or keep empty to use the system default recycle bin path.",
            enabled,
        );
    });
    group(ui, "Account actions", "", |ui| {
        ui.checkbox(&mut draft.profile.auto_sync, "Auto-sync on GUI startup").on_hover_text("Automatically start sync for this profile when OnedriveLinux starts.");
        option_checkbox(
            ui,
            &mut draft.profile.onedrive,
            "display_manager_integration",
            "Display manager integration",
            "Controls whether the client integrates the configured 'sync_dir' with the desktop's file manager (e.g. Nautilus for GNOME, \
             Dolphin for KDE), adding it as a \"special place\" in the sidebar and setting a custom OneDrive folder icon where supported.",
            true,
        );
        ui.add_space(4.0);
        logout = ui.button("Logout").on_hover_text("Logout user from this profile and force login during next sync attempt.").clicked();
    });
    logout
}

/// Clear options that the change of `key` made incompatible (`validate_checkbox_input`).
fn enforce_rules(config: &mut OdConfig, key: &str) {
    let on = odc::is_true(config, key);
    match (key, on) {
        ("download_only", true) => odc::set_bool(config, "upload_only", false),
        ("download_only", false) => odc::set_bool(config, "cleanup_local_files", false),
        ("upload_only", true) => odc::set_bool(config, "download_only", false),
        ("upload_only", false) => odc::set_bool(config, "no_remote_delete", false),
        ("cleanup_local_files", true) => odc::set_bool(config, "download_only", true),
        ("local_first", false) => odc::set_bool(config, "mirror_local_state", false),
        _ => {}
    }
}

fn sync_options_tab(ui: &mut Ui, config: &mut OdConfig) {
    group(ui, "Account Bandwidth Limit", "To minimise the Internet bandwidth for upload and download operations, you can configure the bandwidth limit option.", |ui| {
        let mut rate = odc::int(config, "rate_limit").clamp(131_072, 125_000_000);
        ui.horizontal(|ui| {
            ui.label("Rate Limit [B/s]");
            let a = ui.add(egui::DragValue::new(&mut rate).range(131_072..=125_000_000).speed(12_500.0));
            let b = ui.add(egui::Slider::new(&mut rate, 131_072..=125_000_000).show_value(false));
            if a.changed() || b.changed() {
                config.insert("rate_limit".into(), rate.to_string());
            }
            ui.label(format!("{:.2} Mbit/s", rate as f64 * 8.0 / 1_000_000.0));
        });
    });

    let download_only = odc::is_true(config, "download_only");
    let upload_only = odc::is_true(config, "upload_only");
    let cleanup = odc::is_true(config, "cleanup_local_files");
    let local_first = odc::is_true(config, "local_first");

    group(ui, "Advanced Properties", "", |ui| {
        egui::Grid::new("advanced-properties").num_columns(2).spacing([24.0, 8.0]).striped(true).show(ui, |ui| {
            let numbers: [(&str, &str, &str, std::ops::RangeInclusive<i64>); 11] = [
                (
                    "monitor_interval",
                    "Monitor interval",
                    "The wait time 'between' syncs when running in monitor mode, in seconds. When this interval expires, the client checks \
                     OneDrive for changes online, performs data integrity checks and scans the local Sync Directory for new content.\n\
                     Default: 300 seconds; 600 runs the sync process every 10 minutes.",
                    0..=1_000_000,
                ),
                (
                    "monitor_fullscan_frequency",
                    "Monitor full-scan frequency",
                    "The number of 'monitor_interval' iterations between full scans of your data (integrity and consistency). With the default \
                     of 12 and a monitor interval of 300 seconds, a full scan runs every hour.",
                    0..=1_000_000,
                ),
                ("classify_as_big_delete", "Classify as big delete", "Number of children in a path that is locally removed which will be classified as a 'big data delete'", 0..=1_000_000),
                (
                    "operation_timeout",
                    "Operation timeout",
                    "The maximum amount of time (seconds) a file operation is allowed to take. This includes DNS resolution, connecting, data transfer, etc.",
                    0..=1_000_000,
                ),
                ("connect_timeout", "Connect timeout", "Seconds allowed for connecting to the OneDrive service.", 0..=1_000_000),
                ("data_timeout", "Data timeout", "Seconds without data received before a transfer is considered stalled.", 0..=1_000_000),
                ("ip_protocol_version", "IP Protocol", "0: IPv4 and IPv6 (default)\n1: IPv4 only\n2: IPv6 only", 0..=2),
                ("inotify_delay", "Inotify delay", "Seconds to delay processing of local change events (with 'Delay inotify processing').", 0..=1_000_000),
                ("threads", "Threads", "Number of worker threads the client uses for transfers.", 1..=16),
                ("sync_file_permissions", "Sync file permissions", PERMISSIONS_HINT, 0..=777),
                ("sync_dir_permissions", "Sync dir permissions", PERMISSIONS_HINT, 0..=777),
            ];
            for (key, label, hint, range) in numbers {
                hint_label(ui, label, hint);
                option_number(ui, config, key, range, true);
                ui.end_row();
            }

            let authoritative_enabled = download_only && cleanup;
            hint_label(
                ui,
                "Monitor authoritative sync",
                "Controls the authoritative cleanup behaviour in monitor mode when using Download only and Cleanup local files.\n\
                 'monitor_and_signal' (default) performs authoritative cleanup on every scheduled and signal-triggered sync.\n\
                 'monitor_interval' performs it only on scheduled monitor interval syncs.\n\
                 'monitor_fullscan_frequency' performs it only when the full-scan frequency cadence is reached.",
            );
            let mut current = odc::text(config, "monitor_authoritative_sync").to_string();
            ui.add_enabled_ui(authoritative_enabled, |ui| {
                egui::ComboBox::from_id_salt("monitor_authoritative_sync").selected_text(current.clone()).show_ui(ui, |ui| {
                    for option in ["monitor_and_signal", "monitor_interval", "monitor_fullscan_frequency"] {
                        ui.selectable_value(&mut current, option.to_string(), option);
                    }
                });
            });
            if current != odc::text(config, "monitor_authoritative_sync") {
                config.insert("monitor_authoritative_sync".into(), current);
            }
            ui.end_row();

            for (key, label, hint) in [
                ("user_agent", "User-agent", "Specify a User Agent string to the http client"),
                ("azure_ad_endpoint", "Azure AD endpoint", TENANT_HINT),
                ("azure_tenant_id", "Azure tenant ID", TENANT_HINT),
                ("drive_id", "SharePoint Library Drive ID", "The drive of a SharePoint library this profile syncs."),
            ] {
                hint_label(ui, label, hint);
                option_text(ui, config, key, "", true);
                ui.end_row();
            }
        });
    });

    group(ui, "Advanced options", "", |ui| {
        let options: [(&str, &str, &str, bool); 20] = [
            ("force_http_11", "Force HTTP 1.1", "Forces the use of HTTP/1.1 instead of the default HTTP/2.", true),
            (
                "download_only",
                "Download only",
                "Replicate the OneDrive online state locally, by only downloading changes from OneDrive. Do not upload local changes to OneDrive.",
                !upload_only,
            ),
            ("upload_only", "Upload only", UPLOAD_ONLY_NOTE, !download_only),
            ("cleanup_local_files", "Cleanup local files", "Cleanup local files and folders if they are removed online. This option can only be used with Download only mode.", download_only),
            ("no_remote_delete", "No remote delete", "Do not delete local file 'deletes' from OneDrive when using Upload only.", upload_only),
            ("remove_source_files", "Remove source files", "Remove source file after successful transfer to OneDrive when using Upload only.", true),
            ("local_first", "Local first", "Synchronize from the local directory source first, before downloading changes from OneDrive.", true),
            (
                "mirror_local_state",
                "Mirror local state",
                "When used with Local first, the local filesystem is treated as the authoritative source of truth and Microsoft OneDrive is \
                 reconciled to match the local state.\nFiles or directories that exist online but not locally will be removed from Microsoft OneDrive.\n\
                 Enabling this may permanently remove remote-only files - validate the intended actions with a dry run first.",
                local_first,
            ),
            ("dry_run", "Dry run", "Perform a trial sync with no changes made.", true),
            ("resync", "Full re-sync", "Forget the last saved state, perform a full sync.", true),
            ("bypass_data_preservation", "Bypass data preservation", "", true),
            ("check_nomount", "Check for .nomount", "Check for the presence of .nosync in the syncdir root. If found, do not perform sync.", true),
            ("disable_upload_validation", "Disable upload validation", "Disable upload validation when uploading to OneDrive.", true),
            (
                "disable_download_validation",
                "Disable download validation",
                "Disable the integrity validation of files downloaded from Microsoft OneDrive. Sometimes, particularly with SharePoint, the file \
                 size reported by the OneDrive API differs from the bytes received for the same file.",
                true,
            ),
            ("disable_upload_hash_streaming", "Disable upload hash streaming", "Always generate upload hashes from the local file instead of while streaming the upload.", true),
            ("display_running_config", "Display running config", "", true),
            ("delay_inotify_processing", "Delay inotify processing", "", true),
            ("force_session_upload", "Force Session Upload", "", true),
            (
                "disable_websocket_support",
                "Disable WebSocket support",
                "Disables the built-in WebSocket support (RFC6455) used for near real-time notifications of online changes.",
                true,
            ),
            (
                "disable_version_check",
                "Disable version check",
                "Whether the client checks the GitHub API for the current application version and grace period of running older application versions.",
                true,
            ),
        ];
        egui::Grid::new("advanced-options").num_columns(2).spacing([32.0, 6.0]).show(ui, |ui| {
            for (i, (key, label, hint, enabled)) in options.iter().enumerate() {
                if option_checkbox(ui, config, key, label, hint, *enabled) {
                    enforce_rules(config, key);
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    });
}

/// A `|`-separated pattern list with add and remove (Skip files / Skip directories).
fn pattern_list(ui: &mut Ui, id: &str, config: &mut OdConfig, key: &str, new_entry: &mut String, selected: &mut BTreeSet<String>) {
    let mut items = odc::list(config, key);
    egui::Frame::NONE.stroke(ui.visuals().widgets.noninteractive.bg_stroke).inner_margin(4).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt(id).max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
            if items.is_empty() {
                ui.weak("(none)");
            }
            for item in &items {
                let is_selected = selected.contains(item);
                if ui.selectable_label(is_selected, item).clicked() {
                    if is_selected {
                        selected.remove(item);
                    } else {
                        selected.insert(item.clone());
                    }
                }
            }
        });
    });
    let mut changed = false;
    ui.horizontal(|ui| {
        let edit = ui.add(egui::TextEdit::singleline(new_entry).hint_text("Pattern").desired_width(200.0));
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button("Add").clicked() || enter {
            let entry = new_entry.trim().to_string();
            if entry.is_empty() {
                log::info!("Ignoring empty value.");
            } else if items.contains(&entry) {
                log::info!("Item already in exemption list.");
            } else {
                items.push(entry);
                changed = true;
            }
            new_entry.clear();
        }
        if ui.add_enabled(!selected.is_empty(), egui::Button::new("Remove")).clicked() {
            items.retain(|i| !selected.contains(i));
            selected.clear();
            changed = true;
        }
    });
    if changed {
        odc::set_list(config, key, &items);
    }
}

fn excluded_tab(ui: &mut Ui, draft: &mut Draft) {
    let config = &mut draft.profile.onedrive;
    group(ui, "Skip files", SKIP_FILE_HINT, |ui| {
        pattern_list(ui, "skip_file", config, "skip_file", &mut draft.new_skip_file, &mut draft.selected_skip_files);
    });
    group(ui, "Skip directories", SKIP_DIR_HINT, |ui| {
        pattern_list(ui, "skip_dir", config, "skip_dir", &mut draft.new_skip_dir, &mut draft.selected_skip_dirs);
        option_checkbox(ui, config, "skip_dir_strict_match", "Strict match", "Match skip_dir entries against the full path only.", true);
    });
    group(ui, "Others", "", |ui| {
        option_checkbox(ui, config, "skip_symlinks", "Skip symlinks", "Skip syncing of symlinks.", true);
        option_checkbox(ui, config, "check_nosync", "Check for .nosync", "Check for the presence of .nosync in each directory. If found, skip directory from sync.", true);
        option_checkbox(ui, config, "skip_dotfiles", "Skip dotfiles (hidden files)", "Skip dot files and folders from syncing", true);
    });
}

fn selective_sync_tab(ui: &mut Ui, draft: &mut Draft) {
    group(ui, "Sync List", "", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("Selective sync allows you to sync only specific files and directories. See the");
            ui.hyperlink_to("official documentation", SYNC_LIST_DOCS);
            ui.label("for more details.");
        });
        ui.label(RichText::new(format!("Saved to {}", draft.profile.sync_list_path().display())).small().weak());
        option_checkbox(ui, &mut draft.profile.onedrive, "sync_root_files", "Sync root files", "Sync all files in Sync Directory root when using Sync List.", true);
        ui.add_space(4.0);
        ui.add(egui::TextEdit::multiline(&mut draft.sync_list).code_editor().desired_rows(18).desired_width(f32::INFINITY).hint_text("/Documents\n!/Documents/Temp\nPictures/Camera"));
    });
}

fn webhooks_tab(ui: &mut Ui, config: &mut OdConfig) {
    group(ui, "Webhook settings", "", |ui| {
        ui.horizontal(|ui| {
            option_checkbox(ui, config, "webhook_enabled", "Enable webhook", "Subscribe to remote updates in monitor mode.", true);
            ui.hyperlink_to("(webhook documentation)", WEBHOOK_DOCS);
        });
        let enabled = odc::is_true(config, "webhook_enabled");
        egui::Grid::new("webhooks").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
            for (key, label) in [("webhook_public_url", "Public URL"), ("webhook_listening_host", "Listening host")] {
                ui.add_enabled_ui(enabled, |ui| ui.label(label));
                option_text(ui, config, key, "", enabled);
                ui.end_row();
            }
            for (key, label, range) in [
                ("webhook_listening_port", "Listening port", 1..=65_535),
                ("webhook_expiration_interval", "Expiration interval", 0..=1_000_000),
                ("webhook_renewal_interval", "Renewal interval", 0..=1_000_000),
            ] {
                ui.add_enabled_ui(enabled, |ui| ui.label(label));
                option_number(ui, config, key, range, enabled);
                ui.end_row();
            }
        });
    });
}

fn logging_tab(ui: &mut Ui, config: &mut OdConfig) {
    group(ui, "Logging", "", |ui| {
        option_checkbox(ui, config, "enable_logging", "Enable Logging", "Enable OneDrive client activity logging to a separate log file.", true);
        let enabled = odc::is_true(config, "enable_logging");
        folder_option(ui, config, "log_dir", "Log location:", "Directory where logging output is saved to, needs to end with a slash.", enabled);
        option_checkbox(ui, config, "debug_https", "Debug HTTPS", "Debug OneDrive HTTPS communication.", true);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(enabled, |ui| hint_label(ui, "Monitor log frequency", "Frequency of logging in monitor mode."));
            option_number(ui, config, "monitor_log_frequency", 0..=1_000_000, enabled);
        });
    });
    group(ui, "Notifications", "", |ui| {
        option_checkbox(ui, config, "disable_notifications", "Disable notifications", "Do not use desktop notifications in monitor mode.", true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incompatible_options_are_cleared() {
        let mut c = odc::with_defaults(OdConfig::new());
        odc::set_bool(&mut c, "upload_only", true);
        odc::set_bool(&mut c, "no_remote_delete", true);
        odc::set_bool(&mut c, "upload_only", false);
        enforce_rules(&mut c, "upload_only");
        assert!(!odc::is_true(&c, "no_remote_delete"));

        odc::set_bool(&mut c, "download_only", true);
        enforce_rules(&mut c, "download_only");
        odc::set_bool(&mut c, "cleanup_local_files", true);
        enforce_rules(&mut c, "cleanup_local_files");
        assert!(odc::is_true(&c, "download_only"));
        odc::set_bool(&mut c, "download_only", false);
        enforce_rules(&mut c, "download_only");
        assert!(!odc::is_true(&c, "cleanup_local_files"));

        odc::set_bool(&mut c, "local_first", true);
        odc::set_bool(&mut c, "mirror_local_state", true);
        odc::set_bool(&mut c, "local_first", false);
        enforce_rules(&mut c, "local_first");
        assert!(!odc::is_true(&c, "mirror_local_state"));
    }

    #[test]
    fn reported_values_are_not_edits() {
        let saved = profiles::new_profile("p", "~/P", None);
        let draft = Draft::new(&saved);
        let mut reported = saved.clone();
        reported.account_type = "Business".into();
        reported.free_space = "1.0TiB".into();
        assert!(!draft.is_unsaved(&reported));
        let mut edited = Draft::new(&saved);
        edited.profile.auto_sync = true;
        assert!(edited.is_unsaved(&saved));
    }
}
