//! One-off client commands run in the background (`MaintenanceWorker` in the Python app):
//! logging in with a response URL, logging out, and listing SharePoint sites and libraries.

use std::path::PathBuf;
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;

use crate::events::{AppEvent, EventSender};
use crate::process::run_lines;

fn confdir_arg(config_dir: &std::path::Path) -> String {
    format!("--confdir={}", config_dir.display())
}

fn in_background(name: &str, job: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.to_string()).spawn(job) {
        log::error!("[GUI] could not start {name}: {e}");
    }
}

/// `onedrive --reauth --auth-response <url>`. A line mentioning an "error reason" means failure.
pub fn login(events: EventSender, client: String, profile: String, config_dir: PathBuf, response_url: String) {
    in_background("login", move || {
        log::info!("[GUI] Trying login for {profile}...");
        let args = [confdir_arg(&config_dir), "--reauth".into(), "--auth-response".into(), response_url];
        let result = match run_lines(&client, &args) {
            Ok((lines, _)) => match lines.into_iter().find(|l| l.to_lowercase().contains("error reason")) {
                Some(reason) => Err(reason),
                None => Ok(()),
            },
            Err(e) => Err(format!("could not run {client}: {e}")),
        };
        log::info!("[GUI] - Login response: {result:?}");
        events.send(AppEvent::Login { profile, result });
    });
}

/// `onedrive --logout`, after the profile's sync was stopped (a running client keeps the
/// session files locked, and the logout then silently fails).
pub fn logout(events: EventSender, client: String, profile: String, config_dir: PathBuf) {
    in_background("logout", move || {
        let result = match run_lines(&client, &[confdir_arg(&config_dir), "--logout".into()]) {
            Ok((_, status)) if status.success() => Ok(()),
            Ok((lines, status)) => Err(format!("onedrive --logout failed ({status}): {}", lines.join(" "))),
            Err(e) => Err(format!("could not run {client}: {e}")),
        };
        match &result {
            Ok(()) => log::info!("Profile {profile} has been logged out."),
            Err(e) => log::error!("[{profile}] {e}"),
        }
        events.send(AppEvent::Logout { profile, result });
    });
}

static SITE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^\s\*\s(.+)").ok());
static VALUE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^.+:\s+(.+)$").ok());

fn capture1(regex: &LazyLock<Option<Regex>>, line: &str) -> Option<String> {
    Some(regex.as_ref()?.captures(line)?.get(1)?.as_str().trim().to_string())
}

/// SharePoint site names, from asking for a library that doesn't exist: the client then lists
/// every site the account can see (` * Site name`).
pub fn parse_sites(lines: &[String]) -> Vec<String> {
    let mut sites: Vec<String> = lines.iter().filter(|l| l.contains(" * ")).filter_map(|l| capture1(&SITE, l.trim_end())).collect();
    sites.sort_by_key(|s| s.to_lowercase());
    sites.dedup();
    sites
}

/// Library name → drive id, from `--get-sharepoint-drive-id '<site>'`.
pub fn parse_libraries(lines: &[String]) -> IndexMap<String, String> {
    let mut libraries = IndexMap::new();
    let mut last: Option<String> = None;
    for line in lines {
        let line = line.trim_end();
        if line.contains("Library Name:") {
            if let Some(name) = capture1(&VALUE, line) {
                libraries.insert(name.clone(), String::new());
                last = Some(name);
            }
        } else if line.contains("drive_id:")
            && let (Some(name), Some(id)) = (&last, capture1(&VALUE, line))
        {
            libraries.insert(name.clone(), id);
        }
    }
    libraries.retain(|_, id| !id.is_empty());
    libraries.sort_by(|a, _, b, _| a.to_lowercase().cmp(&b.to_lowercase()));
    libraries
}

pub fn sharepoint_sites(events: EventSender, client: String, config_dir: PathBuf) {
    in_background("sharepoint-sites", move || {
        log::info!("[GUI] Trying to get list of SharePoint Sites...");
        let args = [confdir_arg(&config_dir), "--get-sharepoint-drive-id".into(), "non-existent-library".into()];
        let sites = match run_lines(&client, &args) {
            Ok((lines, _)) => parse_sites(&lines),
            Err(e) => {
                log::error!("[GUI] could not run {client}: {e}");
                Vec::new()
            }
        };
        log::info!("[GUI] - Number of retrieved SharePoint sites: {}", sites.len());
        events.send(AppEvent::SharePointSites(sites));
    });
}

pub fn sharepoint_libraries(events: EventSender, client: String, config_dir: PathBuf, site: String) {
    in_background("sharepoint-libraries", move || {
        log::info!("[GUI] Trying to get Drive IDs of the libraries of {site}...");
        let args = [confdir_arg(&config_dir), "--get-sharepoint-drive-id".into(), site];
        let libraries = match run_lines(&client, &args) {
            Ok((lines, _)) => parse_libraries(&lines),
            Err(e) => {
                log::error!("[GUI] could not run {client}: {e}");
                IndexMap::new()
            }
        };
        events.send(AppEvent::SharePointLibraries(libraries));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn sites_are_listed_sorted() {
        let out = lines("Configuring Global Azure AD Endpoints\nERROR: The requested SharePoint site could not be found.\n\
                         The following SharePoint site names were returned:\n * Marketing\n * engineering\n * Marketing\n");
        assert_eq!(parse_sites(&out), ["engineering", "Marketing"]);
    }

    #[test]
    fn libraries_pair_names_with_drive_ids() {
        let out = lines(
            "Office 365 Library Name Query: Engineering\n-----------------------------------------------\n\
             Site Name:    Engineering\nLibrary Name: Documents\ndrive_id:     b!abc\nLibrary URL:  https://x/y\n\
             Library Name: Specs and Notes\ndrive_id:     b!def\nLibrary Name: Broken\n",
        );
        let libs = parse_libraries(&out);
        assert_eq!(libs.get("Documents").map(String::as_str), Some("b!abc"));
        assert_eq!(libs.get("Specs and Notes").map(String::as_str), Some("b!def"));
        assert!(!libs.contains_key("Broken"));
    }
}
