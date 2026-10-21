//! The setup wizard: check the client, then create a profile, import an existing client
//! config, or add a SharePoint shared library (`SetupWizard` in the Python app).

use egui::{Align, Layout, RichText, Ui, ViewportBuilder, ViewportId};
use indexmap::IndexMap;

use crate::app::App;
use crate::maintenance;
use crate::onedrive_config as odc;
use crate::profiles;
use crate::ui::widgets::{OK_TEXT, WARNING_TEXT, pick_file, pick_folder};
use crate::utils::{expand_tilde, home_dir};
use crate::version::{self, CLIENT_INSTALL_DOCS, MIN_CLIENT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Welcome,
    VersionCheck,
    Choice,
    Create,
    Import,
    SharePoint,
    Finish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Create,
    Import,
    SharePoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpStage {
    Idle,
    FetchingSites,
    FetchingLibraries,
}

pub struct Wizard {
    page: Page,
    history: Vec<Page>,
    choice: Option<Choice>,
    /// `Some(installed version)` once checked (`Some(None)`: no client found).
    client: Option<Option<String>>,
    // Create
    name: String,
    sync_dir: String,
    // Import
    import_name: String,
    import_path: String,
    // SharePoint
    sp_profile: Option<String>,
    sp_sites: Vec<String>,
    sp_site: Option<String>,
    sp_libraries: IndexMap<String, String>,
    sp_library: Option<String>,
    sp_stage: SpStage,
    /// The profile this wizard added.
    created: Option<String>,
    error: Option<String>,
}

impl Wizard {
    /// Start at page 1 (welcome) or 3 (the create/import choice), like the Python wizard's ids.
    pub fn new(start_page: u8) -> Wizard {
        Wizard {
            page: if start_page >= 3 { Page::Choice } else { Page::Welcome },
            history: Vec::new(),
            choice: None,
            client: None,
            name: String::new(),
            sync_dir: String::new(),
            import_name: String::new(),
            import_path: String::new(),
            sp_profile: None,
            sp_sites: Vec::new(),
            sp_site: None,
            sp_libraries: IndexMap::new(),
            sp_library: None,
            sp_stage: SpStage::Idle,
            created: None,
            error: None,
        }
    }

    fn go(&mut self, page: Page) {
        self.history.push(self.page);
        self.page = page;
        self.error = None;
    }

    fn next_page(&self) -> Option<Page> {
        Some(match self.page {
            Page::Welcome => Page::VersionCheck,
            Page::VersionCheck => Page::Choice,
            Page::Choice => match self.choice? {
                Choice::Create => Page::Create,
                Choice::Import => Page::Import,
                Choice::SharePoint => Page::SharePoint,
            },
            Page::Create | Page::Import | Page::SharePoint => Page::Finish,
            Page::Finish => return None,
        })
    }

    fn can_advance(&self) -> bool {
        match self.page {
            Page::Choice => self.choice.is_some(),
            Page::Create | Page::Import | Page::SharePoint => self.created.is_some(),
            _ => true,
        }
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(mut wizard) = app.wizard.take() else { return };
    let mut open = true;
    let builder = ViewportBuilder::default().with_title("OnedriveLinux Setup Wizard").with_app_id(crate::APP_ID).with_inner_size([640.0, 480.0]).with_min_inner_size([560.0, 420.0]);
    ctx.show_viewport_immediate(ViewportId::from_hash_of("setup-wizard"), builder, |ui, _| {
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            open = false;
        }
        egui::Panel::bottom("wizard-buttons").frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(12, 10))).show(ui, |ui| {
            buttons(&mut wizard, ui, &mut open);
        });
        egui::CentralPanel::default().frame(egui::Frame::central_panel(ui.style()).inner_margin(20)).show(ui, |ui| page(app, &mut wizard, ui));
    });
    if open {
        app.wizard = Some(wizard);
    } else {
        // Like the Python wizard, finishing brings the main window up.
        app.show_window(ctx);
    }
}

fn buttons(w: &mut Wizard, ui: &mut Ui, open: &mut bool) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if w.page == Page::Finish {
            if ui.add(egui::Button::new("Finish").min_size([90.0, 26.0].into())).clicked() {
                *open = false;
            }
        } else {
            if ui.button("Cancel").clicked() {
                *open = false;
            }
            if ui.add_enabled(w.can_advance(), egui::Button::new("Next >").min_size([90.0, 26.0].into())).clicked()
                && let Some(next) = w.next_page()
            {
                w.go(next);
            }
        }
        // Going back past a created profile would offer to create it again.
        let can_go_back = !w.history.is_empty() && w.created.is_none();
        if ui.add_enabled(can_go_back, egui::Button::new("< Back")).clicked()
            && let Some(previous) = w.history.pop()
        {
            w.page = previous;
            w.error = None;
        }
    });
}

fn title(ui: &mut Ui, text: &str) {
    ui.heading(text);
    ui.add_space(12.0);
}

fn page(app: &mut App, w: &mut Wizard, ui: &mut Ui) {
    match w.page {
        Page::Welcome => {
            title(ui, "Welcome to OnedriveLinux");
            ui.label("This wizard will help you with initial OneDrive profile creation/import.");
        }
        Page::VersionCheck => version_page(app, w, ui),
        Page::Choice => {
            title(ui, "Add OneDrive profile");
            for (choice, label) in [
                (Choice::Create, "Create new OneDrive profile"),
                (Choice::Import, "Import existing OneDrive profile/config"),
                (Choice::SharePoint, "Add SharePoint Shared Library"),
            ] {
                ui.radio_value(&mut w.choice, Some(choice), label);
            }
        }
        Page::Create => create_page(app, w, ui),
        Page::Import => import_page(app, w, ui),
        Page::SharePoint => sharepoint_page(app, w, ui),
        Page::Finish => {
            title(ui, "Finished");
            ui.label("Your OneDrive profile has been created.");
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    "You can adjust advanced profile options if needed. Once you are happy with the profile settings, start sync with \
                     OneDrive. You will be automatically presented with a OneDrive login page if you have not logged on previously.",
                )
                .wrap(),
            );
        }
    }
    if let Some(e) = &w.error {
        ui.add_space(8.0);
        ui.colored_label(WARNING_TEXT, e);
    }
}

fn version_page(app: &mut App, w: &mut Wizard, ui: &mut Ui) {
    title(ui, "OneDrive version check");
    let installed = w.client.get_or_insert_with(|| {
        log::info!("Checking installed OneDrive version");
        version::installed_client(&app.client())
    });
    match installed.as_deref() {
        Some(v) if version::parse_version(v).is_some_and(|v| v >= MIN_CLIENT) => {
            ui.colored_label(OK_TEXT, format!("OneDrive {v} detected."));
        }
        Some(v) => {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(WARNING_TEXT, format!("OneDrive {v} is not supported (v{}.{}.{} or newer is needed). Please", MIN_CLIENT.0, MIN_CLIENT.1, MIN_CLIENT.2));
                ui.hyperlink_to("upgrade", CLIENT_INSTALL_DOCS);
                ui.colored_label(WARNING_TEXT, "it.");
            });
        }
        None => {
            ui.colored_label(WARNING_TEXT, "OneDrive not detected.");
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label("OneDrive Client for Linux does not seem to be installed. Please install it by following the");
                ui.hyperlink_to("instructions", CLIENT_INSTALL_DOCS);
                ui.label("for your distro, or set its path in the OnedriveLinux settings.");
            });
        }
    }
}

/// Why a new profile name can't be used, if it can't.
fn name_problem(app: &App, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        Some(String::new())
    } else if name.contains('/') {
        Some("A profile name can't contain '/'.".into())
    } else if app.profiles.contains_key(name) {
        Some(format!("Profile name '{name}' is already used!"))
    } else {
        None
    }
}

fn create_page(app: &mut App, w: &mut Wizard, ui: &mut Ui) {
    title(ui, "Create OneDrive profile");
    let done = w.created.is_some();
    let mut create = false;
    ui.add_enabled_ui(!done, |ui| {
        egui::Grid::new("create").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
            ui.label("New profile name");
            let name = ui.add(egui::TextEdit::singleline(&mut w.name).hint_text("E.g. john@live.com").desired_width(320.0));
            if name.changed() {
                w.sync_dir = format!("~/OneDrive_{}", w.name.trim());
            }
            ui.end_row();
            ui.label("Sync directory");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut w.sync_dir).hint_text("E.g. ~/OneDrive_john@live.com/").desired_width(320.0));
                if ui.button("Browse").clicked()
                    && let Some(dir) = pick_folder(Some(&home_dir()))
                {
                    w.sync_dir = dir.display().to_string();
                }
            });
            ui.end_row();
        });
        ui.add_space(10.0);
        let used_dirs: Vec<String> = app.profiles.values().map(|p| odc::text(&p.onedrive, "sync_dir").to_string()).collect();
        let problem = name_problem(app, &w.name).or_else(|| {
            let dir = w.sync_dir.trim();
            if dir.is_empty() {
                Some(String::new())
            } else if used_dirs.iter().any(|d| d == dir) {
                Some(format!("Sync dir '{dir}' is already used by different profile!"))
            } else {
                None
            }
        });
        if let Some(p) = problem.as_ref().filter(|p| !p.is_empty()) {
            ui.colored_label(WARNING_TEXT, p);
        }
        create = ui.add_enabled(problem.is_none(), egui::Button::new(if done { "Done" } else { "Create new profile" }).min_size([200.0, 28.0].into())).clicked();
    });
    if create {
        let name = w.name.trim().to_string();
        let profile = profiles::new_profile(&name, w.sync_dir.trim(), None);
        if let Err(e) = app.add_profile(name.clone(), profile) {
            w.error = Some(e);
        }
        log::info!("Account {name} has been created");
        w.created = Some(name);
        w.go(Page::Finish);
    }
}

fn import_page(app: &mut App, w: &mut Wizard, ui: &mut Ui) {
    title(ui, "Import OneDrive profile");
    let done = w.created.is_some();
    let mut import = false;
    ui.add_enabled_ui(!done, |ui| {
        egui::Grid::new("import").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
            ui.label("Profile name");
            ui.add(egui::TextEdit::singleline(&mut w.import_name).hint_text("E.g. john@live.com").desired_width(320.0));
            ui.end_row();
            ui.label("Config file path");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut w.import_path).hint_text("E.g. ~/.config/onedrive/config").desired_width(320.0));
                if ui.button("Browse").clicked()
                    && let Some(file) = pick_file(Some(&home_dir().join(".config/onedrive")))
                {
                    w.import_path = file.display().to_string();
                }
            });
            ui.end_row();
        });
        ui.add_space(10.0);
        let path = w.import_path.trim();
        let problem = name_problem(app, &w.import_name).or_else(|| {
            if path.is_empty() {
                Some(String::new())
            } else if !path.ends_with("/config") {
                Some("Path to config file not specified (it ends with /config).".into())
            } else if !expand_tilde(path).is_file() {
                Some(format!("Specified config file '{path}' not found!"))
            } else {
                None
            }
        });
        if let Some(p) = problem.as_ref().filter(|p| !p.is_empty()) {
            ui.colored_label(WARNING_TEXT, p);
        }
        ui.label(RichText::new("Back up the config first: comments are removed and multi-line skip_file/skip_dir options are combined into one line when it is saved.").small().weak());
        import = ui.add_enabled(problem.is_none(), egui::Button::new(if done { "Done" } else { "Import" }).min_size([200.0, 28.0].into())).clicked();
    });
    if import {
        let name = w.import_name.trim().to_string();
        match profiles::import_profile(&expand_tilde(w.import_path.trim())) {
            Ok(profile) => {
                if let Err(e) = app.add_profile(name.clone(), profile) {
                    w.error = Some(e);
                }
                log::info!("Account {name} has been imported");
                w.created = Some(name);
            }
            Err(e) => w.error = Some(e),
        }
    }
}

fn combo(ui: &mut Ui, id: &str, enabled: bool, items: &[String], selected: &mut Option<String>, empty: &str) {
    ui.add_enabled_ui(enabled && !items.is_empty(), |ui| {
        let text = selected.clone().unwrap_or_else(|| if items.is_empty() { empty.to_string() } else { "Select…".into() });
        egui::ComboBox::from_id_salt(id).width(300.0).selected_text(text).show_ui(ui, |ui| {
            for item in items {
                ui.selectable_value(selected, Some(item.clone()), item);
            }
        });
    });
}

fn sharepoint_page(app: &mut App, w: &mut Wizard, ui: &mut Ui) {
    title(ui, "Add SharePoint Shared Library");
    // Lookup results arrive through the app's event handling.
    if w.sp_stage == SpStage::FetchingSites
        && let Some(sites) = app.sharepoint_sites.take()
    {
        w.sp_stage = SpStage::Idle;
        if sites.is_empty() {
            w.error = Some("Failed to fetch SharePoint Site list.".into());
        }
        w.sp_sites = sites;
    }
    if w.sp_stage == SpStage::FetchingLibraries
        && let Some(libraries) = app.sharepoint_libraries.take()
    {
        w.sp_stage = SpStage::Idle;
        if libraries.is_empty() {
            w.error = Some("Failed to fetch Shared Libraries.".into());
        }
        w.sp_libraries = libraries;
    }

    // Account types are only learnt while syncing, so the list is rebuilt every time.
    let business: Vec<String> = app.profiles.iter().filter(|(_, p)| p.is_business()).map(|(n, _)| n.clone()).collect();
    let done = w.created.is_some();
    let busy = w.sp_stage != SpStage::Idle;
    let mut create = false;
    egui::Grid::new("sharepoint").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
        ui.label("1. Select profile with access to SharePoint Library.");
        combo(ui, "sp-profile", w.sp_sites.is_empty() && !busy && !done, &business, &mut w.sp_profile, "No Business accounts detected.");
        ui.end_row();

        ui.label("2. Obtain list of SharePoint Sites");
        let label = match w.sp_stage {
            SpStage::FetchingSites => "Please wait...",
            _ if !w.sp_sites.is_empty() => "Done",
            _ => "Get SharePoint Sites",
        };
        if ui.add_enabled(w.sp_profile.is_some() && w.sp_sites.is_empty() && !busy, egui::Button::new(label)).clicked()
            && let Some(p) = w.sp_profile.as_ref().and_then(|n| app.profiles.get(n))
        {
            app.sharepoint_sites = None;
            w.error = None;
            w.sp_stage = SpStage::FetchingSites;
            log::info!("[GUI] Starting maintenance worker to obtain SharePoint site list.");
            maintenance::sharepoint_sites(app.events.clone(), app.client(), p.config_dir());
        }
        ui.end_row();

        ui.label("3. Select a SharePoint Site");
        combo(ui, "sp-site", w.sp_libraries.is_empty() && !busy, &w.sp_sites, &mut w.sp_site, "");
        ui.end_row();

        ui.label("4. Obtain list of Shared Libraries");
        let label = match w.sp_stage {
            SpStage::FetchingLibraries => "Please wait...",
            _ if !w.sp_libraries.is_empty() => "Done",
            _ => "Get Shared Libraries",
        };
        if ui.add_enabled(w.sp_site.is_some() && w.sp_libraries.is_empty() && !busy, egui::Button::new(label)).clicked()
            && let (Some(p), Some(site)) = (w.sp_profile.as_ref().and_then(|n| app.profiles.get(n)), w.sp_site.clone())
        {
            app.sharepoint_libraries = None;
            w.error = None;
            w.sp_stage = SpStage::FetchingLibraries;
            maintenance::sharepoint_libraries(app.events.clone(), app.client(), p.config_dir(), site);
        }
        ui.end_row();

        ui.label("5. Select a SharePoint Shared Library");
        let names: Vec<String> = w.sp_libraries.keys().cloned().collect();
        combo(ui, "sp-library", !done, &names, &mut w.sp_library, "");
        ui.end_row();

        ui.label("6. Create profile");
        create = ui.add_enabled(w.sp_library.is_some() && !done, egui::Button::new(if done { "Done" } else { "Create Profile" })).clicked();
        ui.end_row();
    });
    if busy {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Asking the OneDrive client…");
        });
    }

    if create
        && let (Some(site), Some(library)) = (w.sp_site.clone(), w.sp_library.clone())
        && let Some(drive_id) = w.sp_libraries.get(&library).cloned()
    {
        let name = format!("SharePoint_{}_{}", site.replace(' ', "_"), library.replace(' ', "_")).replace('/', "_");
        if app.profiles.contains_key(&name) {
            w.error = Some(format!("Profile '{name}' already exists."));
            return;
        }
        let sync_dir = home_dir().join(&name).display().to_string();
        let profile = profiles::new_profile(&name, &sync_dir, Some(&drive_id));
        if let Err(e) = app.add_profile(name.clone(), profile) {
            w.error = Some(e);
        }
        log::info!("Account {name} has been created");
        w.created = Some(name);
    }
}
