//! The OneDrive client's config file (`~/.config/onedrive/accounts/<profile>/config`).
//!
//! It isn't an INI file: there is no section header, values are quoted (`sync_dir = "~/OneDrive"`)
//! and `skip_file` / `skip_dir` may appear on several lines. Reading consolidates those into one
//! `|`-separated value, as the client accepts. Values are kept unquoted in memory.
//!
//! Saving writes only the keys that differ from the client's defaults, since the client refuses
//! to start on some default or empty values, and backs the previous file up to `config_backup`.

use std::path::Path;
use std::sync::OnceLock;

use indexmap::IndexMap;

use crate::utils::unquote;

/// Every option the GUI knows, with the client's default value.
const DEFAULT_CONFIG: &str = include_str!("../assets/default_config");

/// Option name → unquoted value, in file order.
pub type OdConfig = IndexMap<String, String>;

/// The client's defaults (parsed once).
pub fn defaults() -> &'static OdConfig {
    static DEFAULTS: OnceLock<OdConfig> = OnceLock::new();
    DEFAULTS.get_or_init(|| parse(DEFAULT_CONFIG))
}

pub fn parse(text: &str) -> OdConfig {
    let mut config = OdConfig::new();
    let mut skip_file: Option<Vec<String>> = None;
    let mut skip_dir: Option<Vec<String>> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim().to_lowercase();
        let value = unquote(value).to_string();
        match key.as_str() {
            "skip_file" => skip_file.get_or_insert_default().push(value),
            "skip_dir" => skip_dir.get_or_insert_default().push(value),
            _ if !key.is_empty() => {
                config.insert(key, value);
            }
            _ => {}
        }
    }
    for (key, parts) in [("skip_file", skip_file), ("skip_dir", skip_dir)] {
        if let Some(parts) = parts {
            let joined = parts.iter().flat_map(|p| p.split('|')).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("|");
            config.insert(key.to_string(), joined);
        }
    }
    config
}

/// The defaults overlaid with `path`'s values: what the settings pages edit.
pub fn read_with_defaults(path: &Path) -> std::io::Result<OdConfig> {
    let user = parse(&std::fs::read_to_string(path)?);
    Ok(with_defaults(user))
}

pub fn with_defaults(user: OdConfig) -> OdConfig {
    let mut config = defaults().clone();
    config.extend(user);
    config
}

/// The file contents for `config`: `key = "value"` for every non-default value.
pub fn render(config: &OdConfig) -> String {
    let defaults = defaults();
    let mut out = String::new();
    for (key, value) in config {
        if defaults.get(key) == Some(value) {
            continue;
        }
        out.push_str(&format!("{key} = \"{value}\"\n"));
    }
    out
}

/// Save `config` to `path`, keeping the previous file as `<path>_backup`.
pub fn write(path: &Path, config: &OdConfig) -> std::io::Result<()> {
    if path.exists() {
        let mut backup = path.as_os_str().to_owned();
        backup.push("_backup");
        if let Err(e) = std::fs::copy(path, &backup) {
            log::warn!("[GUI] could not back up {}: {e}", path.display());
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, render(config))
}

/// `"true"` (any case) reads as checked.
pub fn is_true(config: &OdConfig, key: &str) -> bool {
    config.get(key).is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
}

pub fn set_bool(config: &mut OdConfig, key: &str, value: bool) {
    config.insert(key.to_string(), if value { "true" } else { "false" }.to_string());
}

/// An integer option, falling back to the default (then 0) when it doesn't parse.
pub fn int(config: &OdConfig, key: &str) -> i64 {
    let parse = |c: &OdConfig| c.get(key).and_then(|v| v.trim().parse::<i64>().ok());
    parse(config).or_else(|| parse(defaults())).unwrap_or(0)
}

pub fn text<'a>(config: &'a OdConfig, key: &str) -> &'a str {
    config.get(key).map(String::as_str).unwrap_or_default()
}

/// A `|`-separated list option (`skip_file`, `skip_dir`) as its entries.
pub fn list(config: &OdConfig, key: &str) -> Vec<String> {
    text(config, key).split('|').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

pub fn set_list(config: &mut OdConfig, key: &str, items: &[String]) {
    config.insert(key.to_string(), items.join("|"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        let d = defaults();
        assert_eq!(d["sync_dir"], "~/OneDrive");
        assert_eq!(d["skip_file"], "~*|.~*|*.tmp|*.swp|*.partial");
        assert_eq!(d["monitor_interval"], "300");
        assert_eq!(d["monitor_authoritative_sync"], "monitor_and_signal");
        assert!(d.len() > 50);
    }

    #[test]
    fn multi_line_skips_are_consolidated() {
        let c = parse("# comment\nsync_dir = \"~/OD\"\nskip_file = \"*.tmp\"\nskip_file = \"*.bak|*.old\"\nskip_dir = \"Cache\"\nskip_dir = \"\"\n");
        assert_eq!(c["sync_dir"], "~/OD");
        assert_eq!(c["skip_file"], "*.tmp|*.bak|*.old");
        assert_eq!(c["skip_dir"], "Cache");
        // An explicit empty skip_file still overrides the default.
        assert_eq!(parse("skip_file = \"\"\n")["skip_file"], "");
    }

    #[test]
    fn only_non_defaults_are_written() {
        let mut c = with_defaults(parse("sync_dir = \"~/Work\"\n"));
        set_bool(&mut c, "upload_only", true);
        c.insert("unknown_option".into(), "kept".into());
        let text = render(&c);
        assert_eq!(text, "sync_dir = \"~/Work\"\nupload_only = \"true\"\nunknown_option = \"kept\"\n");
        assert_eq!(with_defaults(parse(&text)), c);
    }

    #[test]
    fn typed_access() {
        let mut c = with_defaults(OdConfig::new());
        assert_eq!(int(&c, "threads"), 8);
        c.insert("threads".into(), "lots".into());
        assert_eq!(int(&c, "threads"), 8);
        assert!(!is_true(&c, "dry_run"));
        set_bool(&mut c, "dry_run", true);
        assert!(is_true(&c, "dry_run"));
        set_list(&mut c, "skip_dir", &["a".into(), "b".into()]);
        assert_eq!(list(&c, "skip_dir"), vec!["a".to_string(), "b".to_string()]);
        set_list(&mut c, "skip_dir", &[]);
        assert!(list(&c, "skip_dir").is_empty());
    }
}
