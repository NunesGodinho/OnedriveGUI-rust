//! The INI dialect Python's `ConfigParser` reads and writes, for `~/.config/onedrive-gui/profiles`
//! and `gui_settings`. Files stay interchangeable with the Python OneDriveGUI.
//!
//! Section and key order is kept. Keys are lower-cased (ConfigParser's `optionxform`), `=` or `:`
//! separates key and value, `#` and `;` start comment lines, and an indented line continues the
//! previous value.

use indexmap::IndexMap;

pub type Section = IndexMap<String, String>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ini {
    pub sections: IndexMap<String, Section>,
}

impl Ini {
    pub fn parse(text: &str) -> Ini {
        let mut ini = Ini::default();
        let mut section: Option<String> = None;
        let mut last_key: Option<String> = None;
        for raw in text.lines() {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            // Continuation of the previous value.
            if raw.starts_with([' ', '\t'])
                && let (Some(s), Some(k)) = (&section, &last_key)
                && let Some(v) = ini.sections.get_mut(s).and_then(|sec| sec.get_mut(k))
            {
                v.push('\n');
                v.push_str(trimmed);
                continue;
            }
            if let Some(name) = trimmed.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                let name = name.to_string();
                ini.sections.entry(name.clone()).or_default();
                section = Some(name);
                last_key = None;
                continue;
            }
            let Some(s) = &section else { continue };
            let split = trimmed.find(['=', ':']);
            let (key, value) = match split {
                Some(i) => (trimmed.get(..i).unwrap_or_default(), trimmed.get(i + 1..).unwrap_or_default()),
                None => (trimmed, ""),
            };
            let key = key.trim().to_lowercase();
            if key.is_empty() {
                continue;
            }
            if let Some(sec) = ini.sections.get_mut(s) {
                sec.insert(key.clone(), value.trim().to_string());
            }
            last_key = Some(key);
        }
        ini
    }

    pub fn read(path: &std::path::Path) -> std::io::Result<Ini> {
        Ok(Ini::parse(&std::fs::read_to_string(path)?))
    }

    /// ConfigParser's output: `key = value` lines, a blank line after each section.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (name, section) in &self.sections {
            out.push_str(&format!("[{name}]\n"));
            for (key, value) in section {
                let value = value.replace('\n', "\n\t");
                out.push_str(&format!("{key} = {value}\n"));
            }
            out.push('\n');
        }
        out
    }

    pub fn write(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_configparser_writes() {
        let text = "[bob@live.com]\nconfig_file = /home/bob/.config/onedrive/accounts/bob@live.com/config\nauto_sync = True\n\
                    account_type = Business\nfree_space = \n\n[SETTINGS]\nQWebEngine_login = False\n# comment\nlong: a\n  b\n";
        let ini = Ini::parse(text);
        let bob = &ini.sections["bob@live.com"];
        assert_eq!(bob["config_file"], "/home/bob/.config/onedrive/accounts/bob@live.com/config");
        assert_eq!(bob["auto_sync"], "True");
        assert_eq!(bob["free_space"], "");
        assert_eq!(ini.sections["SETTINGS"]["qwebengine_login"], "False");
        assert_eq!(ini.sections["SETTINGS"]["long"], "a\nb");
        assert_eq!(Ini::parse(&ini.render()), ini);
    }

    #[test]
    fn junk_is_ignored() {
        let ini = Ini::parse("key before section = 1\n[s]\n=novalue\n[unterminated\nk\n");
        assert_eq!(ini.sections.len(), 1);
        assert_eq!(ini.sections["s"].get("k").map(String::as_str), Some(""));
    }
}
