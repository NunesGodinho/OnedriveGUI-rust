//! Small helpers shared by the UI and the workers (port of `utils/utils.py`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

/// `1536` → `1.5KiB`, like the Python `humanize_file_size`.
pub fn humanize_file_size(bytes: f64) -> String {
    let mut num = if bytes.is_finite() { bytes } else { 0.0 };
    for unit in ["", "Ki", "Mi", "Gi", "Ti", "Pi", "Ei", "Zi"] {
        if num.abs() < 1024.0 {
            return format!("{num:.1}{unit}B");
        }
        num /= 1024.0;
    }
    format!("{num:.1}YiB")
}

/// Shorten a relative folder path to about `limit` characters by replacing middle segments
/// with `...`, keeping the first segment and as many of the last ones as fit
/// (`Documents/Projects/2026/Clients/Invoices` → `Documents/.../Clients/Invoices`).
///
/// The deepest folder is always kept, since it says where the file is. (The Python app's
/// version could drop it.)
pub fn shorten_path(path: &str, limit: usize) -> String {
    let len = |s: &str| s.chars().count();
    if len(path) <= limit {
        return path.to_string();
    }
    let segments: Vec<&str> = path.split('/').collect();
    let (Some(first), Some(last)) = (segments.first(), segments.last()) else { return path.to_string() };
    if segments.len() < 3 {
        return if segments.len() == 2 && len(last) + 4 <= limit { format!(".../{last}") } else { path.to_string() };
    }
    // `first/.../tail` with the longest tail that fits; at least `first/.../last` or `.../last`.
    let middle = segments.get(1..).unwrap_or_default();
    let mut tail_start = middle.len().saturating_sub(1);
    let tail = |from: usize| middle.get(from..).unwrap_or_default().join("/");
    while tail_start > 0 && len(first) + 5 + len(&tail(tail_start - 1)) <= limit {
        tail_start -= 1;
    }
    let shortened = format!("{first}/.../{}", tail(tail_start));
    if len(&shortened) <= limit { shortened } else { format!(".../{last}") }
}

/// Compact relative time: `now`, `2m ago`, `3h ago`, `2d ago`.
pub fn format_relative_time(then: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(then).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..60 => "now".into(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

/// The user's home directory (`/` when it is unknown, so paths stay absolute).
pub fn home_dir() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Strip one pair of surrounding double quotes, as the OneDrive config stores most values quoted.
pub fn unquote(value: &str) -> &str {
    let v = value.trim();
    v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v)
}

/// `~` and `~/x` → the home directory; anything else unchanged.
pub fn expand_tilde(path: &str) -> PathBuf {
    let path = unquote(path);
    if path == "~" {
        home_dir()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        PathBuf::from(path)
    }
}

/// Show `path` (a folder or file) in the desktop's file manager.
pub fn open_path(path: &Path) {
    spawn_detached("xdg-open", &[path.as_os_str()]);
}

/// A desktop notification through `notify-send`, best effort (the tray message of the Qt app).
pub fn notify(title: &str, body: &str) {
    spawn_detached("notify-send", &[std::ffi::OsStr::new("--app-name=OnedriveLinux"), title.as_ref(), body.as_ref()]);
}

fn spawn_detached(program: &str, args: &[&std::ffi::OsStr]) {
    let child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    match child {
        // Reap it in the background so it doesn't linger as a zombie.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::warn!("[GUI] could not run {program}: {e}"),
    }
}

/// Lay out a long error for the status label: at most three lines of about 50 characters,
/// truncated with `...` beyond 150 characters (the full text goes to the tooltip).
pub fn wrap_status_error(full: &str) -> String {
    const PER_LINE: usize = 50;
    const MAX_LINES: usize = 3;
    const MAX_TOTAL: usize = PER_LINE * MAX_LINES;
    let display: String = if full.chars().count() > MAX_TOTAL {
        let mut s: String = full.chars().take(MAX_TOTAL - 3).collect();
        s.push_str("...");
        s
    } else {
        full.to_string()
    };
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in display.split_whitespace() {
        if current.chars().count() + word.chars().count() < PER_LINE {
            current.push_str(word);
            current.push(' ');
        } else {
            if !current.is_empty() {
                lines.push(current.trim().to_string());
            }
            current = format!("{word} ");
            if lines.len() >= MAX_LINES {
                break;
            }
        }
    }
    if !current.trim().is_empty() && lines.len() < MAX_LINES {
        lines.push(current.trim().to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn sizes_are_humanized() {
        assert_eq!(humanize_file_size(0.0), "0.0B");
        assert_eq!(humanize_file_size(1536.0), "1.5KiB");
        assert_eq!(humanize_file_size(5.0 * 1024.0 * 1024.0 * 1024.0), "5.0GiB");
        assert_eq!(humanize_file_size(f64::NAN), "0.0B");
    }

    #[test]
    fn short_paths_are_kept_and_long_ones_shortened() {
        assert_eq!(shorten_path("docs/work", 32), "docs/work");
        let long = "Documents/Projects/2026/Clients/Acme Corporation/Invoices";
        assert_eq!(shorten_path(long, 32), "Documents/.../Invoices");
        assert_eq!(shorten_path(long, 40), "Documents/.../Acme Corporation/Invoices");
        assert_eq!(shorten_path(long, 10), ".../Invoices");
        assert_eq!(shorten_path("a/b/c/d/e/f/g/h/i/j/k/l/m", 10), "a/.../l/m");
        assert_eq!(shorten_path("aaaaaaaaaaaaaaaaaaaa/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", 32), "aaaaaaaaaaaaaaaaaaaa/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert_eq!(shorten_path("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/bbbbbbbb", 32), ".../bbbbbbbb");
        for (p, limit) in [("Documents/Projects/2026/Clients/Acme Corporation/Invoices", 32), ("a/b/c/d/e/f/g/h/i/j/k/l/m", 10)] {
            assert!(shorten_path(p, limit).chars().count() <= limit);
        }
        // A single long segment can't be shortened.
        let one = "a".repeat(50);
        assert_eq!(shorten_path(&one, 32), one);
        // Never panics on odd input.
        for p in ["/", "//", "a//b//c//d//e//f//g//h//i", &"x/".repeat(40)] {
            let _ = shorten_path(p, 4);
        }
    }

    #[test]
    fn relative_times() {
        let now = SystemTime::now();
        assert_eq!(format_relative_time(now, now), "now");
        assert_eq!(format_relative_time(now - Duration::from_secs(150), now), "2m ago");
        assert_eq!(format_relative_time(now - Duration::from_secs(3 * 3600 + 5), now), "3h ago");
        assert_eq!(format_relative_time(now - Duration::from_secs(2 * 86400), now), "2d ago");
        // A timestamp in the future reads as now.
        assert_eq!(format_relative_time(now + Duration::from_secs(10), now), "now");
    }

    #[test]
    fn quotes_and_tilde() {
        assert_eq!(unquote("\"~/OneDrive\""), "~/OneDrive");
        assert_eq!(unquote("plain"), "plain");
        assert_eq!(unquote("\""), "\"");
        assert_eq!(expand_tilde("\"~/OneDrive\""), home_dir().join("OneDrive"));
        assert_eq!(expand_tilde("/abs"), PathBuf::from("/abs"));
    }

    #[test]
    fn long_errors_are_wrapped_to_three_lines() {
        let msg = "word ".repeat(60);
        let wrapped = wrap_status_error(&msg);
        assert!(wrapped.lines().count() <= 3, "{wrapped}");
        assert!(wrapped.lines().all(|l| l.chars().count() <= 50), "{wrapped}");
        assert_eq!(wrap_status_error("short error"), "short error");
    }
}
