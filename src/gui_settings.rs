//! OnedriveLinux's own settings, `~/.config/onedrive-gui/gui_settings` (`[SETTINGS]` section).
//! Same keys and defaults as the Python app; keys this port doesn't use (`qwebengine_login`)
//! are kept when saving.

use std::path::PathBuf;

use crate::ini::{Ini, Section};
use crate::profiles::gui_config_dir;

const SECTION: &str = "SETTINGS";

pub const DEBUG_LEVELS: [&str; 5] = ["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"];

/// The look of the windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    /// Follow the desktop's light/dark preference.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Theme::System => "System",
            Theme::Light => "Light",
            Theme::Dark => "Dark",
        }
    }

    /// The value in the settings file.
    fn key(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    fn parse(value: &str) -> Option<Theme> {
        Theme::ALL.into_iter().find(|t| t.key().eq_ignore_ascii_case(value.trim()))
    }

    pub fn preference(self) -> egui::ThemePreference {
        match self {
            Theme::System => egui::ThemePreference::System,
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuiSettings {
    pub start_minimized: bool,
    pub frameless_window: bool,
    pub combined_start_stop_button: bool,
    pub show_debug: bool,
    pub save_debug: bool,
    /// Hours between log file rotations.
    pub log_rotation_interval: u32,
    /// Rotated log files kept.
    pub log_backup_count: u32,
    pub log_file: String,
    /// One of [`DEBUG_LEVELS`].
    pub debug_level: String,
    /// The OneDrive client program (`onedrive` on the PATH by default).
    pub client_bin_path: String,
    pub autostart_enabled: bool,
    pub theme: Theme,
    /// Every key read from the file, so unknown ones survive a save.
    raw: Section,
}

impl Default for GuiSettings {
    fn default() -> Self {
        GuiSettings {
            start_minimized: false,
            frameless_window: false,
            combined_start_stop_button: true,
            show_debug: true,
            save_debug: true,
            log_rotation_interval: 24,
            log_backup_count: 3,
            log_file: "/tmp/onedrive-gui/onedrive-gui.log".into(),
            debug_level: "DEBUG".into(),
            client_bin_path: "onedrive".into(),
            autostart_enabled: false,
            theme: Theme::System,
            raw: Section::new(),
        }
    }
}

pub fn settings_file() -> PathBuf {
    gui_config_dir().join("gui_settings")
}

impl GuiSettings {
    pub fn load() -> GuiSettings {
        let ini = Ini::read(&settings_file()).unwrap_or_default();
        GuiSettings::from_section(ini.sections.get(SECTION).cloned().unwrap_or_default())
    }

    fn from_section(raw: Section) -> GuiSettings {
        let d = GuiSettings::default();
        // ConfigParser booleans are written `True`/`False`; the Python app tested `"True" in value`.
        let flag = |k: &str, default: bool| raw.get(k).map_or(default, |v| v.contains("True"));
        let num = |k: &str, default: u32| raw.get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(default);
        let text = |k: &str, default: &str| raw.get(k).map_or_else(|| default.to_string(), |v| v.trim().to_string());
        let level = text("debug_level", &d.debug_level).to_uppercase();
        GuiSettings {
            start_minimized: flag("start_minimized", d.start_minimized),
            frameless_window: flag("frameless_window", d.frameless_window),
            combined_start_stop_button: flag("combined_start_stop_button", d.combined_start_stop_button),
            show_debug: flag("show_debug", d.show_debug),
            save_debug: flag("save_debug", d.save_debug),
            log_rotation_interval: num("log_rotation_interval", d.log_rotation_interval).max(1),
            log_backup_count: num("log_backup_count", d.log_backup_count),
            log_file: text("log_file", &d.log_file),
            debug_level: if DEBUG_LEVELS.contains(&level.as_str()) { level } else { d.debug_level.clone() },
            client_bin_path: text("client_bin_path", &d.client_bin_path),
            autostart_enabled: flag("autostart_enabled", d.autostart_enabled),
            theme: raw.get("theme").and_then(|v| Theme::parse(v)).unwrap_or(d.theme),
            raw,
        }
    }

    /// The client program to run; empty means `onedrive`.
    pub fn client_bin(&self) -> String {
        let p = self.client_bin_path.trim();
        if p.is_empty() { "onedrive".into() } else { p.to_string() }
    }

    pub fn save(&self) -> Result<(), String> {
        let b = |v: bool| if v { "True" } else { "False" }.to_string();
        let mut s = self.raw.clone();
        for (k, v) in [
            ("start_minimized", b(self.start_minimized)),
            ("frameless_window", b(self.frameless_window)),
            ("combined_start_stop_button", b(self.combined_start_stop_button)),
            ("show_debug", b(self.show_debug)),
            ("save_debug", b(self.save_debug)),
            ("log_rotation_interval", self.log_rotation_interval.to_string()),
            ("log_backup_count", self.log_backup_count.to_string()),
            ("log_file", self.log_file.clone()),
            ("debug_level", self.debug_level.clone()),
            ("client_bin_path", self.client_bin_path.clone()),
            ("autostart_enabled", b(self.autostart_enabled)),
            ("theme", self.theme.key().to_string()),
        ] {
            s.insert(k.to_string(), v);
        }
        let path = settings_file();
        let mut ini = Ini::read(&path).unwrap_or_default();
        ini.sections.insert(SECTION.to_string(), s);
        log::info!("[GUI][SETTINGS] Saving GUI settings to {}", path.display());
        ini.write(&path).map_err(|e| format!("could not save {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_settings_are_read() {
        let ini = Ini::parse(
            "[SETTINGS]\nstart_minimized = True\ncombined_start_stop_button = False\nlog_rotation_interval = 0\n\
             debug_level = info\nqwebengine_login = False\nclient_bin_path = \n",
        );
        let s = GuiSettings::from_section(ini.sections["SETTINGS"].clone());
        assert!(s.start_minimized);
        assert!(!s.combined_start_stop_button);
        assert_eq!(s.log_rotation_interval, 1);
        assert_eq!(s.debug_level, "INFO");
        assert_eq!(s.client_bin(), "onedrive");
        assert_eq!(s.raw["qwebengine_login"], "False");
        // Missing keys take the defaults.
        assert!(s.save_debug);
        assert_eq!(s.log_backup_count, 3);
        assert_eq!(s.theme, Theme::System);
    }

    #[test]
    fn theme_is_read() {
        let read = |v: &str| GuiSettings::from_section(Ini::parse(&format!("[SETTINGS]\ntheme = {v}\n")).sections["SETTINGS"].clone()).theme;
        assert_eq!(read("light"), Theme::Light);
        assert_eq!(read("Dark"), Theme::Dark);
        assert_eq!(read("system"), Theme::System);
        assert_eq!(read("purple"), Theme::System);
    }
}
