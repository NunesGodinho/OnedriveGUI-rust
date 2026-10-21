//! Version checks: the installed OneDrive client against the minimum this GUI needs and the
//! latest release (one GitHub API request).

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use crate::events::{AppEvent, EventSender};

pub const GUI_VERSION: &str = env!("CARGO_PKG_VERSION");
const CLIENT_RELEASES: &str = "https://api.github.com/repos/abraunegg/onedrive/releases/latest";
pub const CLIENT_INSTALL_DOCS: &str = "https://github.com/abraunegg/onedrive/blob/master/docs/install.md";

/// v2.5.11: the first client with the in-browser login flow this GUI relies on.
pub const MIN_CLIENT: (u32, u32, u32) = (2, 5, 11);

static VERSION: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"v([0-9]+(?:\.[0-9]+)*)").ok());

/// `1.2.3`, `v1.2` → (1, 2, 3), (1, 2, 0).
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().trim_start_matches('v').split('.').map(|p| p.trim().parse::<u32>());
    let major = parts.next()?.ok()?;
    let minor = parts.next().unwrap_or(Ok(0)).ok()?;
    let patch = parts.next().unwrap_or(Ok(0)).ok()?;
    Some((major, minor, patch))
}

/// The client's `--version` answer (`onedrive v2.5.11`) → its `vX.Y.Z` tag.
pub fn parse_client_version(output: &str) -> Option<String> {
    let caps = VERSION.as_ref()?.captures(output)?;
    Some(format!("v{}", caps.get(1)?.as_str()))
}

/// Run `<client> --version`.
pub fn installed_client(client: &str) -> Option<String> {
    let (lines, _) = crate::process::run_lines(client, &["--version"]).ok()?;
    let found = parse_client_version(&lines.join("\n"));
    if found.is_none() {
        log::error!("[GUI] OneDrive client not found or no version in its answer");
    }
    found
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientVersion {
    pub installed: Option<String>,
    pub latest: Option<String>,
}

impl ClientVersion {
    /// Sync can only start with a supported client.
    pub fn meets_minimum(&self) -> bool {
        self.installed.as_deref().and_then(parse_version).is_some_and(|v| v >= MIN_CLIENT)
    }

    /// Warning for the status label, and the longer text for its tooltip; `None` when all is well.
    pub fn warning(&self) -> Option<(String, String)> {
        let Some(installed) = &self.installed else {
            return Some(("OneDrive client not found!".into(), "OneDrive client not found!".into()));
        };
        let latest = self.latest.as_deref();
        if !self.meets_minimum() {
            return Some((
                "Unsupported OneDrive client! Please upgrade it.".into(),
                format!("Your OneDrive Client version not supported! Please upgrade it.\n Installed: {installed}\n Latest: {}", latest.unwrap_or("?")),
            ));
        }
        match latest {
            None => Some(("Unable to check for latest OneDrive client version!".into(), "Unable to check for latest OneDrive client version!".into())),
            Some(latest) if parse_version(latest) > parse_version(installed) => Some((
                "OneDrive client is out of date!".into(),
                format!("OneDrive client is out of date!\n Installed: {installed}\n Latest: {latest}"),
            )),
            Some(_) => None,
        }
    }
}

/// The certificate authorities the operating system trusts.
fn os_roots() -> Result<ureq::tls::RootCerts, String> {
    let found = rustls_native_certs::load_native_certs();
    let certs: Vec<ureq::tls::Certificate<'static>> = found.certs.iter().map(|c| ureq::tls::Certificate::from_der(c.as_ref()).to_owned()).collect();
    if certs.is_empty() {
        return Err("no trusted certificates found on this system".into());
    }
    Ok(ureq::tls::RootCerts::new_with_certs(&certs))
}

/// The `tag_name` of a repository's latest GitHub release.
fn latest_release(url: &str) -> Result<String, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(os_roots()?).build())
        .build()
        .new_agent();
    let mut response = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("OnedriveLinux-rs/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| format!("couldn't reach GitHub ({e})"))?;
    let body = response.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| format!("unreadable answer ({e})"))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("unreadable answer ({e})"))?;
    v["tag_name"].as_str().filter(|t| !t.is_empty() && t.len() <= 64).map(str::to_string).ok_or_else(|| "no release found".to_string())
}

/// Check the client (installed and latest) in the background.
pub fn check(events: EventSender, client: String) {
    let spawned = std::thread::Builder::new().name("version-check".into()).spawn(move || {
        let installed = installed_client(&client);
        let latest = latest_release(CLIENT_RELEASES).map_err(|e| log::error!("[GUI] client version check: {e}")).ok();
        log::info!("[GUI] Client version check: Installed: {installed:?} | Latest: {latest:?}");
        events.send(AppEvent::ClientVersion(ClientVersion { installed, latest }));
    });
    if let Err(e) = spawned {
        log::error!("[GUI] could not start the version check: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_component() {
        assert_eq!(parse_version("v2.5.9"), Some((2, 5, 9)));
        assert!(parse_version("2.5.9") < parse_version("v2.5.11"));
        assert_eq!(parse_version("1.3"), Some((1, 3, 0)));
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_client_version("onedrive v2.5.11-1+np1"), Some("v2.5.11".into()));
        assert_eq!(parse_client_version("bash: onedrive: command not found"), None);
    }

    #[test]
    fn client_warnings() {
        let v = |i: Option<&str>, l: Option<&str>| ClientVersion { installed: i.map(Into::into), latest: l.map(Into::into) };
        assert_eq!(v(None, Some("v2.5.12")).warning().map(|w| w.0), Some("OneDrive client not found!".into()));
        let old = v(Some("v2.5.9"), Some("v2.5.12"));
        assert!(!old.meets_minimum());
        assert!(old.warning().is_some_and(|w| w.0.starts_with("Unsupported")));
        let outdated = v(Some("v2.5.11"), Some("v2.5.12"));
        assert!(outdated.meets_minimum());
        assert_eq!(outdated.warning().map(|w| w.0), Some("OneDrive client is out of date!".into()));
        assert_eq!(v(Some("v2.5.12"), Some("v2.5.12")).warning(), None);
        assert!(v(Some("v2.5.12"), None).warning().is_some());
    }

}
