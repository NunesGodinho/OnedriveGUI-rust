//! OneDrive profiles: the list in `~/.config/onedrive-gui/profiles` (one INI section per
//! profile) and each profile's OneDrive client config. The Python app called this `global_config`.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::ini::{Ini, Section};
use crate::onedrive_config::{self as odc, OdConfig};
use crate::utils::{expand_tilde, home_dir, unquote};

/// `~/.config/onedrive-gui`, shared with the Python OneDriveGUI (which uses this exact path).
pub fn gui_config_dir() -> PathBuf {
    home_dir().join(".config/onedrive-gui")
}

pub fn profiles_file() -> PathBuf {
    gui_config_dir().join("profiles")
}

/// Where new profiles keep their client config: `~/.config/onedrive/accounts/<name>/config`.
pub fn new_config_path(profile_name: &str) -> PathBuf {
    home_dir().join(".config/onedrive/accounts").join(profile_name).join("config")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// Path of the OneDrive client config file, as stored in the profiles file.
    pub config_file: String,
    /// Start syncing this profile when the GUI starts.
    pub auto_sync: bool,
    /// `Personal`, `Business`, … as last reported by the client.
    pub account_type: String,
    /// Remaining space as last reported by the client (`1.2TiB`).
    pub free_space: String,
    /// Keys of the profiles file the GUI doesn't use, kept as they are.
    pub extra: Section,
    /// The client options: the defaults overlaid with the config file's values.
    pub onedrive: OdConfig,
}

impl Profile {
    pub fn new(config_file: &Path, onedrive: OdConfig) -> Profile {
        Profile {
            config_file: config_file.display().to_string(),
            auto_sync: false,
            account_type: String::new(),
            free_space: String::new(),
            extra: Section::new(),
            onedrive,
        }
    }

    pub fn config_path(&self) -> PathBuf {
        expand_tilde(&self.config_file)
    }

    /// The client's `--confdir`: the folder of the config file.
    pub fn config_dir(&self) -> PathBuf {
        let path = self.config_path();
        path.parent().map(Path::to_path_buf).unwrap_or(path)
    }

    pub fn sync_list_path(&self) -> PathBuf {
        self.config_dir().join("sync_list")
    }

    pub fn sync_dir(&self) -> PathBuf {
        expand_tilde(odc::text(&self.onedrive, "sync_dir"))
    }

    pub fn is_business(&self) -> bool {
        self.account_type == "Business"
    }

    fn to_section(&self) -> Section {
        let mut s = Section::new();
        s.insert("config_file".into(), self.config_file.clone());
        s.insert("auto_sync".into(), if self.auto_sync { "True" } else { "False" }.into());
        s.insert("account_type".into(), self.account_type.clone());
        s.insert("free_space".into(), self.free_space.clone());
        for (k, v) in &self.extra {
            s.entry(k.clone()).or_insert_with(|| v.clone());
        }
        s
    }

    fn from_section(section: &Section) -> Profile {
        let config_file = unquote(section.get("config_file").map(String::as_str).unwrap_or_default()).to_string();
        let path = expand_tilde(&config_file);
        let onedrive = match odc::read_with_defaults(&path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("[GUI] could not read OneDrive config {}: {e}; using defaults", path.display());
                odc::defaults().clone()
            }
        };
        let known = ["config_file", "auto_sync", "account_type", "free_space"];
        Profile {
            config_file,
            auto_sync: section.get("auto_sync").is_some_and(|v| v.trim() == "True"),
            account_type: section.get("account_type").cloned().unwrap_or_default(),
            free_space: section.get("free_space").cloned().unwrap_or_default(),
            extra: section.iter().filter(|(k, _)| !known.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect(),
            onedrive,
        }
    }
}

pub type Profiles = IndexMap<String, Profile>;

/// Every profile in the profiles file, with its client config.
pub fn load() -> Profiles {
    let path = profiles_file();
    let ini = match Ini::read(&path) {
        Ok(ini) => ini,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ini::default(),
        Err(e) => {
            log::error!("[GUI] could not read {}: {e}", path.display());
            Ini::default()
        }
    };
    ini.sections.iter().map(|(name, section)| (name.clone(), Profile::from_section(section))).collect()
}

/// Write the profiles file only (not the client configs).
pub fn save_profiles_file(profiles: &Profiles) -> Result<(), String> {
    let ini = Ini { sections: profiles.iter().map(|(n, p)| (n.clone(), p.to_section())).collect() };
    let path = profiles_file();
    ini.write(&path).map_err(|e| format!("could not save {}: {e}", path.display()))
}

/// Save the profiles file and every profile's client config (`save_global_config`).
pub fn save_all(profiles: &Profiles) -> Result<(), String> {
    save_profiles_file(profiles)?;
    let mut errors = Vec::new();
    for (name, profile) in profiles {
        let path = profile.config_path();
        match odc::write(&path, &profile.onedrive) {
            Ok(()) => log::info!("{name} config saved"),
            Err(e) => errors.push(format!("{name}: could not save {}: {e}", path.display())),
        }
    }
    if errors.is_empty() {
        log::info!("All configs saved");
        Ok(())
    } else {
        for e in &errors {
            log::error!("[GUI] {e}");
        }
        Err(errors.join("\n"))
    }
}

/// The selective-sync list next to the config, or empty when there is none.
pub fn read_sync_list(profile: &Profile) -> String {
    std::fs::read_to_string(profile.sync_list_path()).unwrap_or_default()
}

/// Write the selective-sync list. An empty list that doesn't exist yet isn't created, because
/// the client asks for a resync whenever the file changes.
pub fn write_sync_list(profile: &Profile, text: &str) -> Result<(), String> {
    let path = profile.sync_list_path();
    if text.is_empty() && !path.exists() {
        return Ok(());
    }
    if std::fs::read_to_string(&path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    std::fs::write(&path, text).map_err(|e| format!("could not save {}: {e}", path.display()))
}

/// A new profile with default client options and `sync_dir` (and `drive_id` for SharePoint).
pub fn new_profile(name: &str, sync_dir: &str, drive_id: Option<&str>) -> Profile {
    let mut onedrive = odc::defaults().clone();
    onedrive.insert("sync_dir".into(), sync_dir.to_string());
    if let Some(id) = drive_id {
        onedrive.insert("drive_id".into(), id.to_string());
    }
    Profile::new(&new_config_path(name), onedrive)
}

/// A profile for an existing client config file (its values over the defaults).
pub fn import_profile(config_path: &Path) -> Result<Profile, String> {
    let onedrive = odc::read_with_defaults(config_path).map_err(|e| format!("could not read {}: {e}", config_path.display()))?;
    Ok(Profile::new(config_path, onedrive))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_round_trips_through_its_section() {
        let mut p = new_profile("bob@live.com", "~/OneDrive_bob", None);
        p.account_type = "Business".into();
        p.auto_sync = true;
        p.extra.insert("custom".into(), "x".into());
        let section = p.to_section();
        assert_eq!(section["auto_sync"], "True");
        assert_eq!(section["custom"], "x");
        assert!(p.is_business());
        assert_eq!(p.sync_dir(), home_dir().join("OneDrive_bob"));
        assert!(p.config_dir().ends_with(".config/onedrive/accounts/bob@live.com"));
    }

    #[test]
    fn sharepoint_profiles_get_a_drive_id() {
        let p = new_profile("SharePoint_Site_Docs", "/home/x/SharePoint_Site_Docs", Some("b!abc"));
        assert_eq!(odc::text(&p.onedrive, "drive_id"), "b!abc");
        assert!(odc::render(&p.onedrive).contains("drive_id = \"b!abc\""));
    }
}
