//! The app: profiles, their sync workers and the windows. Background threads report through
//! [`AppEvent`]s, handled in [`eframe::App::logic`] so they're processed even while the main
//! window is hidden in the tray.

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::ViewportCommand;
use indexmap::IndexMap;

use crate::events::{AppEvent, EventSender, TrayAction, WorkerMsg};
use crate::gui_settings::GuiSettings;
use crate::maintenance;
use crate::parser::ParserEvent;
use crate::profiles::{self, Profile, Profiles};
use crate::transfers::TransferList;
use crate::tray::{TrayHandle, TrayState};
use crate::ui::gui_settings_window::GuiSettingsWindow;
use crate::ui::login_window::LoginWindow;
use crate::ui::profiles_window::ProfilesWindow;
use crate::ui::wizard::Wizard;
use crate::version::{self, ClientVersion};
use crate::worker::{SyncWorker, WorkerConfig};

/// Status messages that read as a sync in progress (for the tray icon).
const SYNCING_HINTS: [&str; 4] = ["sync in progress", "processing", "starting", "initializing"];
/// Errors that don't turn the tray icon red.
const IGNORABLE_ERRORS: [&str; 2] = ["inotify_add_watch failed", "The local file system returned an error with the following message"];

/// What the status page shows for one profile.
pub struct ProfileRuntime {
    pub worker: Option<SyncWorker>,
    pub status: String,
    /// Full error text behind the warning icon.
    pub error: Option<String>,
    pub account_type: String,
    pub free_space: String,
    pub transfers: TransferList,
}

impl ProfileRuntime {
    fn new(profile: &Profile) -> ProfileRuntime {
        ProfileRuntime {
            worker: None,
            status: "OneDrive sync is not running".into(),
            error: None,
            account_type: profile.account_type.clone(),
            free_space: if profile.free_space == "0" { String::new() } else { profile.free_space.clone() },
            transfers: TransferList::default(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.worker.is_some()
    }
}

/// Modal questions and messages, shown one at a time over the main window.
pub enum Dialog {
    Quit,
    /// Resync needs the user's consent; `Yes` is enabled after a five-second countdown.
    Resync { profile: String, opened: Instant },
    BigDelete { profile: String },
    Message { title: String, text: String, error: bool },
}

pub struct App {
    pub settings: GuiSettings,
    pub profiles: Profiles,
    pub runtime: IndexMap<String, ProfileRuntime>,
    /// The profile shown in the main window.
    pub selected: usize,
    pub events: EventSender,
    rx: Receiver<AppEvent>,
    tray: Option<TrayHandle>,
    pub client_version: Option<ClientVersion>,
    next_worker_id: u64,
    pub dialogs: VecDeque<Dialog>,
    pub profiles_window: Option<ProfilesWindow>,
    pub gui_settings_window: Option<GuiSettingsWindow>,
    pub wizard: Option<Wizard>,
    pub logins: IndexMap<String, LoginWindow>,
    /// Main window shown (not hidden in the tray).
    pub visible: bool,
    /// Set once the user confirmed quitting: the next close request is let through.
    allow_close: bool,
    auto_sync_done: bool,
    /// Results of the SharePoint lookups, for the wizard.
    pub sharepoint_sites: Option<Vec<String>>,
    pub sharepoint_libraries: Option<IndexMap<String, String>>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        settings: GuiSettings,
        profiles: Profiles,
        events: EventSender,
        rx: Receiver<AppEvent>,
        tray: Option<TrayHandle>,
        start_hidden: bool,
    ) -> App {
        events.attach(&cc.egui_ctx);
        crate::ui::fonts::install(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        // Without a tray, "hidden" means minimized to the taskbar.
        if start_hidden && tray.is_none() {
            cc.egui_ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        let visible = !(start_hidden && tray.is_some());
        let runtime = profiles.iter().map(|(n, p)| (n.clone(), ProfileRuntime::new(p))).collect();
        version::check(events.clone(), settings.client_bin());
        if let Err(e) = crate::autostart::sync(settings.autostart_enabled) {
            log::error!("[GUI][SETTINGS] {e}");
        }
        let wizard = profiles.is_empty().then(|| Wizard::new(1));
        let mut app = App {
            settings,
            profiles,
            runtime,
            selected: 0,
            events,
            rx,
            tray,
            client_version: None,
            next_worker_id: 1,
            dialogs: VecDeque::new(),
            profiles_window: None,
            gui_settings_window: None,
            wizard,
            logins: IndexMap::new(),
            visible,
            allow_close: false,
            auto_sync_done: false,
            sharepoint_sites: None,
            sharepoint_libraries: None,
        };
        app.update_tray();
        app
    }

    pub fn client(&self) -> String {
        self.settings.client_bin()
    }

    /// Sync can start unless the installed client is known to be missing or too old.
    pub fn can_sync(&self) -> bool {
        self.client_version.as_ref().is_none_or(ClientVersion::meets_minimum)
    }

    pub fn selected_name(&self) -> Option<String> {
        self.profiles.get_index(self.selected).map(|(n, _)| n.clone())
    }

    // ---- sync workers -------------------------------------------------------------------

    pub fn start_sync(&mut self, profile: &str, extra_args: &[&str]) {
        let Some(p) = self.profiles.get(profile) else { return };
        let config = WorkerConfig {
            profile: profile.to_string(),
            client: self.client(),
            config_dir: p.config_dir(),
            sync_dir: p.sync_dir(),
            extra_args: extra_args.iter().map(|s| s.to_string()).collect(),
        };
        let id = self.next_worker_id;
        let Some(rt) = self.runtime.get_mut(profile) else { return };
        rt.error = None;
        if rt.worker.is_some() {
            log::warn!("Worker for profile {profile} is already running. Please stop it first.");
            return;
        }
        self.next_worker_id += 1;
        rt.worker = Some(SyncWorker::start(id, config, self.events.clone()));
    }

    pub fn stop_sync(&mut self, profile: &str) {
        let Some(rt) = self.runtime.get_mut(profile) else { return };
        match rt.worker.take() {
            Some(worker) => {
                worker.stop();
                rt.status = "OneDrive sync has been stopped".into();
                log::info!("OneDrive sync for profile {profile} has been stopped.");
            }
            None => log::info!("OneDrive for profile {profile} is not running."),
        }
    }

    fn stop_all(&mut self) {
        let names: Vec<String> = self.runtime.keys().cloned().collect();
        for name in names {
            self.stop_sync(&name);
        }
    }

    /// Stop the profile's sync, then log it out.
    pub fn logout(&mut self, profile: &str) {
        self.stop_sync(profile);
        if let Some(p) = self.profiles.get(profile) {
            maintenance::logout(self.events.clone(), self.client(), profile.to_string(), p.config_dir());
        }
    }

    // ---- profiles -----------------------------------------------------------------------

    /// Add a profile made by the wizard, save everything and show it.
    pub fn add_profile(&mut self, name: String, profile: Profile) -> Result<(), String> {
        log::info!("[MAIN_WINDOW] Adding profile {name}");
        self.runtime.insert(name.clone(), ProfileRuntime::new(&profile));
        self.profiles.insert(name.clone(), profile);
        self.selected = self.profiles.get_index_of(&name).unwrap_or(0);
        if let Some(w) = self.profiles_window.as_mut() {
            w.select(&name);
        }
        profiles::save_all(&self.profiles)
    }

    pub fn remove_profile(&mut self, name: &str) -> Result<(), String> {
        log::info!("[GUI] Removing profile: {name}");
        self.stop_sync(name);
        self.profiles.shift_remove(name);
        self.runtime.shift_remove(name);
        self.logins.shift_remove(name);
        self.selected = self.selected.min(self.profiles.len().saturating_sub(1));
        profiles::save_profiles_file(&self.profiles)
    }

    pub fn rename_profile(&mut self, old: &str, new: &str) -> Result<(), String> {
        log::info!("[GUI] Renaming profile '{old}' to '{new}'.");
        fn rename_key<V>(map: &mut IndexMap<String, V>, old: &str, new: &str) {
            if let Some((index, _, value)) = map.shift_remove_full(old) {
                let (last, _) = map.insert_full(new.to_string(), value);
                map.move_index(last, index.min(map.len().saturating_sub(1)));
            }
        }
        rename_key(&mut self.profiles, old, new);
        rename_key(&mut self.runtime, old, new);
        rename_key(&mut self.logins, old, new);
        profiles::save_all(&self.profiles)
    }

    // ---- windows ------------------------------------------------------------------------

    pub fn show_window(&mut self, ctx: &egui::Context) {
        self.visible = true;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    /// Hide to the tray when there is one, else minimize to the taskbar.
    pub fn hide_window(&mut self, ctx: &egui::Context) {
        if self.tray.is_some() {
            log::debug!("[GUI] Minimizing main window to tray");
            self.visible = false;
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        } else {
            log::debug!("[GUI] Minimizing main window to taskbar/dock");
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
    }

    pub fn open_profiles_window(&mut self) {
        if self.profiles_window.is_none() {
            self.profiles_window = Some(ProfilesWindow::new(self.selected_name()));
        }
    }

    pub fn open_gui_settings(&mut self) {
        if self.gui_settings_window.is_none() {
            self.gui_settings_window = Some(GuiSettingsWindow::new(&self.settings));
        }
    }

    /// The wizard, starting at its first page or at the create/import choice.
    pub fn open_wizard(&mut self, from_start: bool) {
        if self.wizard.is_none() {
            self.wizard = Some(Wizard::new(if from_start { 1 } else { 3 }));
        }
    }

    pub fn open_login(&mut self, profile: &str) {
        if let Some(p) = self.profiles.get(profile)
            && !self.logins.contains_key(profile)
        {
            log::info!("[GUI] Opening external login window");
            self.logins.insert(profile.to_string(), LoginWindow::new(p));
        }
    }

    /// Ask before quitting (shows the window so the question can be seen).
    pub fn request_quit(&mut self, ctx: &egui::Context) {
        if !self.dialogs.iter().any(|d| matches!(d, Dialog::Quit)) {
            self.dialogs.push_back(Dialog::Quit);
        }
        self.show_window(ctx);
    }

    pub fn quit(&mut self, ctx: &egui::Context) {
        log::info!("Quitting OnedriveLinux");
        self.stop_all();
        if let Some(tray) = self.tray.take() {
            tray.shutdown();
        }
        self.allow_close = true;
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    pub fn message(&mut self, title: &str, text: String, error: bool) {
        self.dialogs.push_back(Dialog::Message { title: title.into(), text, error });
    }

    /// The main window was asked to close: hide it instead, unless quitting.
    pub fn on_close_requested(&mut self, ctx: &egui::Context) {
        if !self.allow_close {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_window(ctx);
        }
    }

    // ---- events -------------------------------------------------------------------------

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                AppEvent::Worker { id, msg } => self.on_worker(ctx, id, msg),
                AppEvent::Tray(action) => self.on_tray(ctx, action),
                AppEvent::Login { profile, result } => self.on_login(&profile, result),
                AppEvent::Logout { profile, result } => {
                    if let Some(rt) = self.runtime.get_mut(&profile) {
                        rt.status = match result {
                            Ok(()) => "You have been logged out".into(),
                            Err(e) => format!("Logout failed: {e}"),
                        };
                    }
                }
                AppEvent::SharePointSites(sites) => self.sharepoint_sites = Some(sites),
                AppEvent::SharePointLibraries(libs) => self.sharepoint_libraries = Some(libs),
                AppEvent::ClientVersion(v) => {
                    self.client_version = Some(v);
                    self.auto_sync();
                }
            }
        }
    }

    /// Start the profiles marked "Auto-sync on GUI startup", once the client is known to work.
    fn auto_sync(&mut self) {
        if self.auto_sync_done {
            return;
        }
        self.auto_sync_done = true;
        let can_sync = self.can_sync();
        let names: Vec<String> = self.profiles.iter().filter(|(_, p)| p.auto_sync).map(|(n, _)| n.clone()).collect();
        for name in names {
            log::info!("[{name}] Auto-sync enabled for profile; compatible client found: {can_sync}");
            if can_sync {
                self.start_sync(&name, &[]);
            }
        }
    }

    fn on_worker(&mut self, ctx: &egui::Context, id: u64, msg: WorkerMsg) {
        let Some(name) = self.runtime.iter().find(|(_, rt)| rt.worker.as_ref().is_some_and(|w| w.id == id)).map(|(n, _)| n.clone()) else {
            return; // a stopped worker's last words
        };
        let Some(rt) = self.runtime.get_mut(&name) else { return };
        match msg {
            WorkerMsg::Transfer(item) => rt.transfers.apply(item),
            WorkerMsg::Exited(how) => {
                rt.worker = None;
                if rt.status == "OneDrive sync is starting..." || how.starts_with("Could not start") {
                    rt.status = how;
                }
                log::info!("[{name}] Removing worker");
            }
            WorkerMsg::Parsed(event) => match event {
                ParserEvent::Status(s) => {
                    rt.status = s.status_message;
                    rt.error = s.error_message;
                    if !s.free_space.is_empty() {
                        rt.free_space = s.free_space.clone();
                    }
                    if !s.account_type.is_empty() {
                        rt.account_type = s.account_type.clone();
                    }
                    // Remember what the client reported, so it shows after a restart.
                    if let Some(p) = self.profiles.get_mut(&name) {
                        let free_changed = !s.free_space.is_empty() && p.free_space != s.free_space;
                        let type_changed = !s.account_type.is_empty() && p.account_type != s.account_type;
                        if free_changed {
                            p.free_space = s.free_space;
                        }
                        if type_changed {
                            p.account_type = s.account_type;
                            log::info!("[{name}] Saved account type: {}", p.account_type);
                        }
                        if (type_changed || free_changed)
                            && let Err(e) = profiles::save_profiles_file(&self.profiles)
                        {
                            log::error!("[GUI] {e}");
                        }
                    }
                }
                ParserEvent::ClearWarning => rt.error = None,
                ParserEvent::LoginRequired { .. } => self.open_login(&name),
                ParserEvent::BrowserLogin => crate::utils::notify(
                    "OnedriveLinux - Login required",
                    &format!("Please complete the OneDrive login for '{name}' in the web browser window that just opened."),
                ),
                ParserEvent::ResyncRequired => {
                    if !self.dialogs.iter().any(|d| matches!(d, Dialog::Resync { profile, .. } if *profile == name)) {
                        self.dialogs.push_back(Dialog::Resync { profile: name, opened: Instant::now() });
                        self.show_window(ctx);
                    }
                }
                ParserEvent::BigDeleteRequired => {
                    if !self.dialogs.iter().any(|d| matches!(d, Dialog::BigDelete { profile } if *profile == name)) {
                        self.dialogs.push_back(Dialog::BigDelete { profile: name });
                        self.show_window(ctx);
                    }
                }
                ParserEvent::Transfer(_) => {} // converted to a list entry by the worker
            },
        }
    }

    fn on_login(&mut self, profile: &str, result: Result<(), String>) {
        let rt = self.runtime.get_mut(profile);
        match result {
            Ok(()) => {
                if let Some(rt) = rt {
                    rt.status = "Login successful. Please, start sync manually.".into();
                }
                self.logins.shift_remove(profile);
                self.message("Login successful", "Login successful!  Please start sync manually.".into(), false);
            }
            Err(reason) => {
                if let Some(rt) = rt {
                    rt.status = "Login failed.".into();
                }
                if let Some(w) = self.logins.get_mut(profile) {
                    w.busy = false;
                }
                self.message("Login failed", format!("Login failed!  Please verify your response URL and try again.\n\n{reason}"), true);
            }
        }
    }

    fn on_tray(&mut self, ctx: &egui::Context, action: TrayAction) {
        match action {
            TrayAction::Activate => {
                let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
                if !self.visible {
                    self.show_window(ctx);
                } else if focused {
                    self.hide_window(ctx);
                } else {
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
            }
            TrayAction::ShowHide => {
                if self.visible {
                    self.hide_window(ctx);
                } else {
                    self.show_window(ctx);
                }
            }
            TrayAction::Settings => {
                // Child windows only run while the main window does.
                self.show_window(ctx);
                self.open_profiles_window();
            }
            TrayAction::Quit => self.request_quit(ctx),
        }
    }

    // ---- tray ---------------------------------------------------------------------------

    fn is_ignorable(text: &str) -> bool {
        let lower = text.to_lowercase();
        text.is_empty() || IGNORABLE_ERRORS.iter().any(|p| lower.contains(&p.to_lowercase()))
    }

    /// Each profile's state, and the combined state (Error > Stopped > Syncing > Idle).
    fn tray_status(&self) -> (TrayState, Vec<(String, TrayState, String)>) {
        let mut overall = TrayState::Idle;
        let mut rows = Vec::new();
        for (name, rt) in &self.runtime {
            let status = rt.status.as_str();
            let error = rt.error.as_deref().unwrap_or_default();
            let lower = status.to_lowercase();
            let state = if !rt.is_running() {
                TrayState::Stopped
            } else if !Self::is_ignorable(error)
                && !Self::is_ignorable(status)
                && (!error.is_empty() || status.contains("Error:") || ["error", "failed", "crashed"].iter().any(|w| lower.contains(w)))
            {
                TrayState::Error
            } else if SYNCING_HINTS.iter().any(|h| lower.contains(h)) {
                TrayState::Syncing
            } else {
                TrayState::Idle
            };
            overall = overall.min(state);
            rows.push((name.clone(), state, if status.is_empty() { "stopped".into() } else { status.replace('\n', " ") }));
        }
        (overall, rows)
    }

    fn tray_tooltip(overall: TrayState, rows: &[(String, TrayState, String)]) -> String {
        if rows.is_empty() {
            return "OnedriveLinux - No profiles configured".into();
        }
        let total = rows.len();
        let running = rows.iter().filter(|(_, s, _)| *s != TrayState::Stopped).count();
        let mut summary = if running == total && overall != TrayState::Error {
            format!("All {total} profile(s) running")
        } else if running == 0 {
            format!("All {total} profile(s) stopped")
        } else {
            format!("{running} of {total} profile(s) running")
        };
        summary.push_str(match overall {
            TrayState::Error => " - Sync error detected",
            TrayState::Stopped => " - Sync stopped",
            TrayState::Syncing => " - Syncing...",
            TrayState::Idle => " - All syncs complete",
        });
        let mut lines = vec![summary, String::new()];
        for (name, state, status) in rows {
            let status = if status.chars().count() > 50 { format!("{}...", status.chars().take(47).collect::<String>()) } else { status.clone() };
            lines.push(format!("[{}] {name}: {status}", state.label()));
        }
        lines.join("\n")
    }

    fn update_tray(&mut self) {
        if self.tray.is_none() {
            return;
        }
        let (state, rows) = self.tray_status();
        let tooltip = Self::tray_tooltip(state, &rows);
        let state = if rows.is_empty() { TrayState::Idle } else { state };
        if let Some(tray) = self.tray.as_mut() {
            tray.set(state, tooltip);
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events(ctx);
        self.update_tray();
        // Keep relative times ("2m ago") and the resync countdown fresh.
        ctx.request_repaint_after(Duration::from_secs(if self.dialogs.is_empty() { 30 } else { 1 }));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) {
            self.on_close_requested(&ctx);
        }
        crate::ui::main_window::show(self, ui);
        crate::ui::dialogs::show(self, &ctx);
        crate::ui::profiles_window::show(self, &ctx);
        crate::ui::gui_settings_window::show(self, &ctx);
        crate::ui::wizard::show(self, &ctx);
        crate::ui::login_window::show(self, &ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_tooltip_summarises_profiles() {
        let rows = vec![
            ("bob@live.com".to_string(), TrayState::Idle, "OneDrive sync is complete.".to_string()),
            ("work".to_string(), TrayState::Stopped, "OneDrive sync has been stopped".to_string()),
        ];
        let tip = App::tray_tooltip(TrayState::Stopped, &rows);
        assert!(tip.starts_with("1 of 2 profile(s) running - Sync stopped"), "{tip}");
        assert!(tip.contains("[IDLE] bob@live.com: OneDrive sync is complete."));
        assert!(tip.contains("[STOPPED] work"));
        assert_eq!(App::tray_tooltip(TrayState::Idle, &[]), "OnedriveLinux - No profiles configured");
    }

    #[test]
    fn some_errors_are_ignored() {
        assert!(App::is_ignorable(""));
        assert!(App::is_ignorable("Error: inotify_add_watch failed for /x"));
        assert!(!App::is_ignorable("Error: Request entity too large"));
    }
}
