//! Start OnedriveLinux on login through an XDG autostart entry, `~/.config/autostart/OnedriveLinux.desktop`.

use std::path::PathBuf;

use crate::utils::home_dir;

pub fn desktop_file() -> PathBuf {
    home_dir().join(".config/autostart/OnedriveLinux.desktop")
}

/// The command the entry runs: the AppImage when running from one, else this executable.
fn exec_command() -> String {
    let path = std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok()).unwrap_or_else(|| PathBuf::from("onedrivelinux"));
    // Desktop entry `Exec` quoting: backslash, double quote, backtick and dollar are escaped.
    let mut quoted = String::from("\"");
    for c in path.display().to_string().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

pub fn is_enabled() -> bool {
    std::fs::read_to_string(desktop_file()).is_ok_and(|text| !text.contains("Hidden=true"))
}

pub fn enable() -> Result<(), String> {
    let path = desktop_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    let exec = exec_command();
    let entry = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=OnedriveLinux\nComment=A simple GUI for OneDrive Linux client\n\
         Exec={exec}\nIcon=OnedriveLinux\nTerminal=false\nCategories=Network;Office;\nStartupNotify=false\nHidden=false\n"
    );
    std::fs::write(&path, entry).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    log::info!("Autostart enabled: {} (Exec={exec})", path.display());
    Ok(())
}

pub fn disable() -> Result<(), String> {
    let path = desktop_file();
    match std::fs::remove_file(&path) {
        Ok(()) => {
            log::info!("Autostart disabled: {}", path.display());
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("could not remove {}: {e}", path.display())),
    }
}

/// Make the autostart entry match the saved setting (on startup and after saving settings).
pub fn sync(enabled: bool) -> Result<(), String> {
    match (enabled, is_enabled()) {
        (true, _) => enable(),
        (false, true) => disable(),
        (false, false) => Ok(()),
    }
}
