//! Turns the OneDrive client's `--monitor -v` output into status updates for the GUI.
//! A line-by-line port of the Python `WorkerThread.read_stdout`, kept free of process handling
//! so it can be tested with captured output.

use std::sync::LazyLock;
use std::time::SystemTime;

use regex::Regex;

use crate::utils::{humanize_file_size, wrap_status_error};

/// What the status page shows for a profile (`profile_status` in the Python worker).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileStatus {
    pub status_message: String,
    pub free_space: String,
    pub account_type: String,
    /// The full error behind a shortened status message; shown as the warning icon's tooltip.
    pub error_message: Option<String>,
}

/// One upload, download or delete reported by the client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    /// `Uploading`, `Downloading`, `Deleting` or `Moving`.
    pub operation: String,
    /// Path relative to the sync directory.
    pub file_path: String,
    pub progress: u8,
    pub complete: bool,
    pub timestamp: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParserEvent {
    Status(ProfileStatus),
    Transfer(Transfer),
    /// The client needs (re-)authorisation. With `kill`, the client process must be stopped:
    /// it would otherwise wait for an answer on its standard input.
    LoginRequired { kill: bool },
    /// The client opened the login page in the browser and waits for it (client v2.5.11+).
    BrowserLogin,
    ResyncRequired,
    BigDeleteRequired,
    /// A sync cycle started or completed cleanly: drop the warning icon.
    ClearWarning,
}

const TASKS: [&str; 9] = [
    "Downloading file",
    "Downloading new file",
    "Uploading file",
    "Uploading new file",
    "Uploading modified file",
    "Downloading modified file",
    "Deleting item",          // deleted locally, then removed online
    "Deleting local file",    // deleted online, then removed locally
    "Moving this local file", // deleted online, then moved to the local recycle bin
];

const SYNC_COMPLETE: [&str; 3] = [
    "Sync with Microsoft OneDrive is complete",
    "Total number of local file(s) added or changed",
    "No changes or items that can be applied were discovered",
];

/// Lines that continue an `ERROR:` block before its `Error Message:` line.
const ERROR_CONTINUATIONS: [&str; 3] = ["Calling Function:", "Path:", "Disk Space"];

/// At most this many failed files are listed in the tooltip.
const MAX_FAILED_FILES: usize = 25;

pub const INSTALL_DOCS: &str = "https://github.com/abraunegg/onedrive/blob/master/docs/install.md";

fn re(pattern: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(r) => Some(r),
        Err(e) => {
            log::error!("[GUI] bad pattern {pattern}: {e}");
            None
        }
    }
}

static FREE_SPACE: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"([0-9]+)\sbytes"));
static ACCOUNT_TYPE: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"\s(\w+)$"));
static PROCESSING: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"^Processing\s([0-9]+)\sOneDrive\sitems"));
static TRANSFER_PATH: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"\b[file:]+\s(.+)\s+\.\.\."));
static DELETE_PATH: LazyLock<Option<Regex>> = LazyLock::new(|| re(r".+:\s(.+)$"));
static PROGRESS: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"(\w[Downloading|Uploading]+):\s+(.+?)[\.]*\s(\d{1,3})%"));
static FAILED_COUNT: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"Failed items .+?: (\d+)"));
static FAILED_FILE: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"Failed to (upload|download): (.+)$"));
static UNKNOWN_KEY: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"Unknown key in config file:\s*(.+)$"));

/// The first capture group of `regex` in `line`.
fn capture<'a>(regex: &LazyLock<Option<Regex>>, line: &'a str, group: usize) -> Option<&'a str> {
    regex.as_ref()?.captures(line)?.get(group).map(|m| m.as_str())
}

/// Python's `str.capitalize`.
fn capitalize(word: &str) -> String {
    let lower = word.to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[derive(Debug)]
pub struct OutputParser {
    status: ProfileStatus,
    pending_error: Option<String>,
    pending_error_path: Option<String>,
    failed_files: Vec<String>,
    failed_files_count: usize,
    collecting_failed_files: bool,
    awaiting_browser_login: bool,
}

impl Default for OutputParser {
    fn default() -> Self {
        OutputParser::new()
    }
}

impl OutputParser {
    pub fn new() -> OutputParser {
        OutputParser {
            status: ProfileStatus { status_message: "OneDrive sync is starting...".into(), ..ProfileStatus::default() },
            pending_error: None,
            pending_error_path: None,
            failed_files: Vec::new(),
            failed_files_count: 0,
            collecting_failed_files: false,
            awaiting_browser_login: false,
        }
    }

    pub fn status(&self) -> &ProfileStatus {
        &self.status
    }

    fn set_message(&mut self, message: impl Into<String>) -> ParserEvent {
        self.status.status_message = message.into();
        ParserEvent::Status(self.status.clone())
    }

    fn error_status(&mut self, full: String) -> ParserEvent {
        self.status.status_message = format!("Error: {}", wrap_status_error(&full));
        self.status.error_message = Some(full);
        ParserEvent::Status(self.status.clone())
    }

    fn failed_files_status(&mut self) -> Option<ParserEvent> {
        if self.failed_files.is_empty() && self.failed_files_count == 0 {
            return None;
        }
        let count = if self.failed_files_count > 0 { self.failed_files_count } else { self.failed_files.len() };
        let mut tooltip = vec![format!("Failed items to upload to/from Microsoft OneDrive: {count}")];
        tooltip.extend(self.failed_files.iter().cloned());
        if count > self.failed_files.len() {
            tooltip.push(format!("... and {} more", count - self.failed_files.len()));
        }
        self.status.status_message = format!("Sync completed with {count} failed file(s)");
        self.status.error_message = Some(tooltip.join("\n"));
        Some(ParserEvent::Status(self.status.clone()))
    }

    /// Feed one line of client output; returns what the GUI should do about it.
    pub fn feed(&mut self, raw: &str) -> Vec<ParserEvent> {
        let line = raw.trim();
        let mut events = Vec::new();
        if line.is_empty() {
            return events;
        }

        // An `ERROR:` line not continued by an `Error Message:` line is shown as it is.
        if self.pending_error.is_some()
            && !line.contains("Error Message:")
            && !ERROR_CONTINUATIONS.iter().any(|m| line.contains(m))
            && let Some(pending) = self.pending_error.take()
        {
            self.pending_error_path = None;
            events.push(self.error_status(pending));
        }

        // The list of failed files ends at the first line that isn't one.
        if self.collecting_failed_files && !line.contains("Failed to upload:") && !line.contains("Failed to download:") {
            events.extend(self.failed_files_status());
            self.collecting_failed_files = false;
        }

        if line.contains("Calling Function: testNetwork()") {
            events.push(self.set_message("Testing network connection to Microsoft OneDrive Service..."));
        } else if line.to_lowercase().contains("authorise this application by") || line.contains("--reauth and re-authorise this client") {
            events.push(self.set_message("OneDrive login is required."));
            events.push(ParserEvent::LoginRequired { kill: true });
        } else if line.contains("Opening the Microsoft authorisation URL in your default browser") {
            self.awaiting_browser_login = true;
            events.push(self.set_message("Action required: please complete the OneDrive login\nthat just opened in your web browser."));
            events.push(ParserEvent::BrowserLogin);
        } else if self.awaiting_browser_login && line.contains("The OneDrive API was initialised successfully") {
            self.awaiting_browser_login = false;
            events.push(self.set_message("OneDrive login successful."));
        } else if SYNC_COMPLETE.iter().any(|m| line.contains(m)) {
            self.status.error_message = None;
            events.push(ParserEvent::ClearWarning);
            events.push(self.set_message("OneDrive sync is complete."));
        } else if line.contains("Sync with Microsoft OneDrive has completed, however there are items that failed to sync") {
            events.push(self.set_message("OneDrive sync completed with errors."));
        } else if line.contains("Remaining Free Space") {
            self.status.free_space = capture(&FREE_SPACE, line, 1)
                .and_then(|b| b.parse::<f64>().ok())
                .map_or_else(|| "Not Available".to_string(), humanize_file_size);
            log::info!("Free Space: {}", self.status.free_space);
            events.push(ParserEvent::Status(self.status.clone()));
        } else if line.contains("Account Type") {
            if let Some(kind) = capture(&ACCOUNT_TYPE, line, 1) {
                self.status.account_type = capitalize(kind);
                log::info!("Account type: {kind}");
                events.push(ParserEvent::Status(self.status.clone()));
            }
        } else if line.contains("Initializing the OneDrive API") {
            self.failed_files.clear();
            self.failed_files_count = 0;
            self.collecting_failed_files = false;
            events.push(self.set_message("Initializing the OneDrive API"));
        } else if line.contains("Starting a sync with Microsoft OneDrive") {
            self.status.error_message = None;
            events.push(ParserEvent::ClearWarning);
            events.push(self.set_message("Starting a sync with Microsoft OneDrive"));
        } else if line.contains("Processing:")
            || line.contains("Number of items to download from Microsoft OneDrive")
            || line.contains("OneDrive Client requested to create")
        {
            let message = match capture(&PROCESSING, line, 1) {
                Some(n) => format!("OneDrive is processing {n} items..."),
                None => "OneDrive is processing items...".to_string(),
            };
            events.push(self.set_message(message));
        } else if line.contains("--resync is required") || line.contains("before using --resync") {
            log::warn!("{line} - OneDrive resync authorization is required.");
            events.push(ParserEvent::ResyncRequired);
        } else if line.contains("To delete a large volume of data use") {
            log::warn!("{line} - OneDrive big delete authorization is required.");
            events.push(self.set_message("OneDrive big delete authorization is required."));
            events.push(ParserEvent::BigDeleteRequired);
        } else if TASKS.iter().any(|t| line.contains(t)) {
            events.extend(self.transfer(line));
        } else if line.contains("% ") {
            events.extend(self.progress(line));
        } else if line.contains("sync_business_shared_folders") {
            events.push(self.set_message("Business Shared Folders have been deprecated (see the client's business-shared-items docs)."));
        } else if line.contains("Network Connection Issue") {
            events.push(self.set_message("Cannot connect to Microsoft OneDrive Service."));
        } else if line.contains("command not found") {
            let message = format!("Onedrive does not seem to be installed. Please install it as per instruction at {INSTALL_DOCS}");
            log::error!("{message}");
            events.push(self.set_message(message));
        } else if line.contains("/dlang/") {
            log::error!("OneDrive client crashed. Please check logs.");
            events.push(self.set_message("OneDrive client crashed. Please check logs."));
        } else if line.contains(" refresh_token ") || line.contains("'refresh_token'") {
            events.push(self.set_message("Logon details expired. Please re-authenticate."));
            events.push(ParserEvent::LoginRequired { kill: false });
        } else if line.contains("Error Message:") {
            if let Some(pending) = self.pending_error.take() {
                let extra = line.split_once("Error Message:").map(|(_, rest)| rest.trim()).unwrap_or_default();
                let full = match self.pending_error_path.take() {
                    Some(path) => format!("{pending} Path: {path} - {extra}"),
                    None => format!("{pending} {extra}"),
                };
                log::error!("{full}");
                events.push(self.error_status(full));
            }
        } else if self.pending_error.is_some() && line.contains("Path:") {
            if let Some((_, path)) = line.split_once("Path:") {
                let path = path.trim();
                if !path.is_empty() && path != "(not available)" {
                    self.pending_error_path = Some(path.to_string());
                }
            }
        } else if let Some((_, text)) = line.split_once("ERROR:") {
            self.pending_error = Some(text.trim().to_string());
            self.pending_error_path = None;
        } else if line.contains("Failed items to upload to/from Microsoft OneDrive:") {
            if let Some(n) = capture(&FAILED_COUNT, line, 1).and_then(|n| n.parse().ok()) {
                self.failed_files_count = n;
                self.collecting_failed_files = true;
            }
        } else if line.contains("Failed to upload:") || line.contains("Failed to download:") {
            if let (Some(op), Some(path)) = (capture(&FAILED_FILE, line, 1), capture(&FAILED_FILE, line, 2))
                && self.failed_files.len() < MAX_FAILED_FILES
            {
                self.failed_files.push(format!("Failed to {op}: {}", path.trim()));
            }
        } else if line.contains("Unknown key in config file:") {
            let message = match capture(&UNKNOWN_KEY, line, 1) {
                Some(key) => {
                    format!("Configuration error: Unknown key '{}'. Please check your config file and remove invalid options.", key.trim())
                }
                None => "Configuration error: Unknown key in config file. Please check your configuration.".to_string(),
            };
            log::error!("{message}");
            events.push(self.error_status(message));
        }
        events
    }

    /// `Uploading new file: docs/a.txt ... done`, `Deleting item from Microsoft OneDrive: docs/a.txt`, …
    fn transfer(&mut self, line: &str) -> Option<ParserEvent> {
        let operation = ["Uploading", "Downloading", "Deleting", "Moving"]
            .into_iter()
            .filter_map(|op| line.find(op).map(|at| (at, op)))
            .min_by_key(|(at, _)| *at)
            .map(|(_, op)| op)?;
        let removal = matches!(operation, "Deleting" | "Moving");
        let path = if removal { capture(&DELETE_PATH, line, 1) } else { capture(&TRANSFER_PATH, line, 1) };
        let complete = removal || line.contains("done");
        if !complete && self.status.status_message != "OneDrive sync in progress..." {
            log::info!("OneDrive sync in progress...");
        }
        if !complete {
            self.status.status_message = "OneDrive sync in progress...".into();
        }
        Some(ParserEvent::Transfer(Transfer {
            operation: operation.to_string(),
            file_path: path.map_or_else(|| "unknown file name".to_string(), |p| p.trim().to_string()),
            progress: 0,
            complete,
            timestamp: complete.then(SystemTime::now),
        }))
    }

    /// `Uploading: dir/50MB.zip ... 80% | ETA 00:00:04`. 100% is skipped: a `... done` line follows.
    fn progress(&mut self, line: &str) -> Vec<ParserEvent> {
        let caps = PROGRESS.as_ref().and_then(|r| r.captures(line));
        let Some(caps) = caps else { return Vec::new() };
        let (Some(op), Some(path), Some(pct)) = (caps.get(1), caps.get(2), caps.get(3)) else { return Vec::new() };
        let Ok(progress) = pct.as_str().parse::<u8>() else { return Vec::new() };
        if progress >= 100 {
            return Vec::new();
        }
        let transfer = Transfer {
            operation: op.as_str().to_string(),
            file_path: path.as_str().trim().to_string(),
            progress,
            complete: false,
            timestamp: None,
        };
        vec![ParserEvent::Transfer(transfer), self.set_message("OneDrive sync in progress...")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(p: &mut OutputParser, lines: &[&str]) -> Vec<ParserEvent> {
        lines.iter().flat_map(|l| p.feed(l)).collect()
    }

    fn last_message(events: &[ParserEvent]) -> Option<String> {
        events.iter().rev().find_map(|e| match e {
            ParserEvent::Status(s) => Some(s.status_message.clone()),
            _ => None,
        })
    }

    #[test]
    fn a_sync_cycle() {
        let mut p = OutputParser::new();
        let events = feed_all(
            &mut p,
            &[
                "Initializing the OneDrive API ...",
                "Account Type:          business",
                "Remaining Free Space:  1099511627776 bytes",
                "Starting a sync with Microsoft OneDrive",
            ],
        );
        assert_eq!(p.status().account_type, "Business");
        assert_eq!(p.status().free_space, "1.0TiB");
        assert!(events.contains(&ParserEvent::ClearWarning));
        assert_eq!(last_message(&events).as_deref(), Some("Starting a sync with Microsoft OneDrive"));

        let events = p.feed("Sync with Microsoft OneDrive is complete");
        assert_eq!(last_message(&events).as_deref(), Some("OneDrive sync is complete."));
    }

    #[test]
    fn uploads_downloads_and_deletes() {
        let mut p = OutputParser::new();
        let t = |events: Vec<ParserEvent>| {
            events.into_iter().find_map(|e| match e {
                ParserEvent::Transfer(t) => Some(t),
                _ => None,
            })
        };
        let up = t(p.feed("Uploading new file: Documents/report final.pdf ... done")).unwrap_or_else(|| panic!("no transfer"));
        assert_eq!((up.operation.as_str(), up.file_path.as_str(), up.complete), ("Uploading", "Documents/report final.pdf", true));
        assert!(up.timestamp.is_some());

        let down = t(p.feed("Downloading file: Photos/a.jpg ... ")).unwrap_or_else(|| panic!("no transfer"));
        assert_eq!((down.operation.as_str(), down.file_path.as_str(), down.complete), ("Downloading", "Photos/a.jpg", false));
        assert_eq!(p.status().status_message, "OneDrive sync in progress...");

        let del = t(p.feed("Deleting item from Microsoft OneDrive: Old/notes.txt")).unwrap_or_else(|| panic!("no transfer"));
        assert_eq!((del.operation.as_str(), del.file_path.as_str(), del.complete), ("Deleting", "Old/notes.txt", true));

        let progress = p.feed("Uploading: dir1/another dir 2 - 123/dir3/50MBa.zip ... 80%  |  ETA    00:00:04");
        let tr = t(progress).unwrap_or_else(|| panic!("no progress"));
        assert_eq!((tr.operation.as_str(), tr.file_path.as_str(), tr.progress), ("Uploading", "dir1/another dir 2 - 123/dir3/50MBa.zip", 80));
        // 100% is followed by a "done" line, so it is skipped.
        assert!(p.feed("Uploading: a.zip ... 100% | DONE in 00:00:01").is_empty());
    }

    #[test]
    fn multi_line_errors_are_combined() {
        let mut p = OutputParser::new();
        let events = feed_all(
            &mut p,
            &[
                "ERROR: An error was returned from OneDrive",
                "Calling Function:  uploadFile()",
                "Path:              Documents/big.iso",
                "Error Message:     Request entity too large",
            ],
        );
        let full = "An error was returned from OneDrive Path: Documents/big.iso - Request entity too large";
        assert_eq!(p.status().error_message.as_deref(), Some(full));
        assert!(last_message(&events).is_some_and(|m| m.starts_with("Error: ")));

        // An ERROR: line followed by something unrelated is shown on its own.
        let events = feed_all(&mut p, &["ERROR: Something odd", "Sync with Microsoft OneDrive is complete"]);
        assert!(events.iter().any(|e| matches!(e, ParserEvent::Status(s) if s.status_message == "Error: Something odd")));
        assert_eq!(p.status().error_message, None);
    }

    #[test]
    fn failed_files_are_listed() {
        let mut p = OutputParser::new();
        let events = feed_all(
            &mut p,
            &[
                "Failed items to upload to/from Microsoft OneDrive: 30",
                "Failed to upload: a/b.txt",
                "Failed to download: c.txt",
                "Attempting to perform a database vacuum",
            ],
        );
        assert_eq!(last_message(&events).as_deref(), Some("Sync completed with 30 failed file(s)"));
        let tip = p.status().error_message.clone().unwrap_or_default();
        assert!(tip.contains("Failed to upload: a/b.txt") && tip.contains("... and 28 more"), "{tip}");
    }

    #[test]
    fn prompts_for_login_resync_and_big_delete() {
        let mut p = OutputParser::new();
        assert!(p.feed("Authorise this application by visiting:").contains(&ParserEvent::LoginRequired { kill: true }));
        assert!(p.feed("Opening the Microsoft authorisation URL in your default browser ...").contains(&ParserEvent::BrowserLogin));
        assert_eq!(last_message(&p.feed("The OneDrive API was initialised successfully")).as_deref(), Some("OneDrive login successful."));
        assert_eq!(p.feed("An application configuration change has been detected where a --resync is required"), vec![ParserEvent::ResyncRequired]);
        assert!(p.feed("To delete a large volume of data use --force").contains(&ParserEvent::BigDeleteRequired));
        let mut p = OutputParser::new();
        assert!(p.feed("Your 'refresh_token' has expired").contains(&ParserEvent::LoginRequired { kill: false }));
    }

    #[test]
    fn config_errors_and_odd_lines() {
        let mut p = OutputParser::new();
        let events = p.feed("Unknown key in config file: sync_bussiness");
        assert!(last_message(&events).is_some_and(|m| m.contains("sync_bussiness")));
        // Nothing here may panic.
        for line in ["", "   ", "% ", "Account Type", "Remaining Free Space: lots", "Uploading file", "Deleting item", "Processing:", "ERROR:", "Path:", "Error Message:"] {
            let _ = p.feed(line);
        }
    }
}
