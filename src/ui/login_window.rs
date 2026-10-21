//! Login through the browser when the client asks for authorisation (`ExternalLoginWindow` in
//! the Python app): open Microsoft's login page, then paste the address the browser ends on.
//! Clients from v2.5.11 usually open the browser themselves; this covers the older flow and
//! expired sessions.

use egui::{Align, Layout, RichText, ViewportBuilder, ViewportId};

use crate::app::App;
use crate::maintenance;
use crate::onedrive_config as odc;
use crate::profiles::Profile;

/// The client's own application id, used when the profile sets none.
const DEFAULT_APPLICATION_ID: &str = "d50ca740-c83f-4d1b-b616-12c519384f0c";

pub struct LoginWindow {
    login_url: String,
    response_url: String,
    /// The login command is running.
    pub busy: bool,
}

/// Microsoft's authorisation URL for the profile's application and tenant.
pub fn login_url(profile: &Profile) -> String {
    let application_id = Some(odc::text(&profile.onedrive, "application_id")).filter(|s| !s.is_empty()).unwrap_or(DEFAULT_APPLICATION_ID);
    let tenant = Some(odc::text(&profile.onedrive, "azure_tenant_id")).filter(|s| !s.is_empty()).unwrap_or("common");
    format!(
        "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize?client_id={application_id}\
         &scope=Files.ReadWrite%20Files.ReadWrite.All%20Sites.ReadWrite.All%20offline_access\
         &response_type=code&prompt=login&redirect_uri=https://login.microsoftonline.com/{tenant}/oauth2/nativeclient"
    )
}

impl LoginWindow {
    pub fn new(profile: &Profile) -> LoginWindow {
        let login_url = login_url(profile);
        log::debug!("[GUI] Built login URL: {login_url}");
        LoginWindow { login_url, response_url: String::new(), busy: false }
    }
}

/// Only a redirect carrying an authorisation code is accepted.
fn is_response_url(text: &str) -> bool {
    text.contains("nativeclient?code=")
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let mut windows = std::mem::take(&mut app.logins);
    let mut closed = Vec::new();
    for (profile, window) in windows.iter_mut() {
        let builder = ViewportBuilder::default().with_title(format!("OneDrive login for profile {profile}")).with_app_id(crate::APP_ID).with_inner_size([560.0, 300.0]);
        ctx.show_viewport_immediate(ViewportId::from_hash_of(("login", profile.as_str())), builder, |ui, _| {
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                closed.push(profile.clone());
            }
            egui::CentralPanel::default().frame(egui::Frame::central_panel(ui.style()).inner_margin(16)).show(ui, |ui| {
                ui.heading("Login to OneDrive");
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("1) Login to OneDrive in your browser by");
                    ui.hyperlink_to("clicking this link.", &window.login_url);
                });
                ui.label("2) Copy the response URI from your browser's address bar to the below field.");
                ui.label("3) Press Save.");
                ui.add_space(10.0);
                ui.add(egui::TextEdit::singleline(&mut window.response_url).hint_text("https://login.microsoftonline.com/common/oauth2/nativeclient?code=…").desired_width(f32::INFINITY));
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let valid = is_response_url(&window.response_url);
                    if ui.add_enabled(valid && !window.busy, egui::Button::new("Save").min_size([90.0, 26.0].into())).clicked()
                        && let Some(p) = app.profiles.get(profile)
                    {
                        log::info!("Login performed");
                        window.busy = true;
                        maintenance::login(app.events.clone(), app.client(), profile.clone(), p.config_dir(), window.response_url.trim().to_string());
                    }
                    if window.busy {
                        ui.spinner();
                    } else if !window.response_url.is_empty() && !valid {
                        ui.label(RichText::new("That address has no login code.").weak());
                    }
                });
            });
        });
    }
    for profile in closed {
        windows.shift_remove(&profile);
    }
    // Windows opened or closed while these were shown (a login answer) take precedence.
    for (profile, window) in windows {
        if app.profiles.contains_key(&profile) {
            app.logins.entry(profile).or_insert(window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_url_uses_the_profile_tenant() {
        let mut p = crate::profiles::new_profile("x", "~/X", None);
        assert!(login_url(&p).contains("/common/oauth2/v2.0/authorize?client_id=d50ca740-c83f-4d1b-b616-12c519384f0c"));
        p.onedrive.insert("azure_tenant_id".into(), "contoso".into());
        p.onedrive.insert("application_id".into(), "my-app".into());
        let url = login_url(&p);
        assert!(url.contains("/contoso/oauth2/v2.0/authorize?client_id=my-app"));
        assert!(url.ends_with("redirect_uri=https://login.microsoftonline.com/contoso/oauth2/nativeclient"));
        assert!(is_response_url("https://login.microsoftonline.com/common/oauth2/nativeclient?code=M.abc"));
        assert!(!is_response_url("https://example.com"));
    }
}
