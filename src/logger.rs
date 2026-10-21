//! Logging set up from the GUI settings, like the Python app's `logger.py`: records go to
//! standard output (`show_debug`) and to a log file (`save_debug`) that is rotated every
//! `log_rotation_interval` hours, keeping `log_backup_count` old files (`<log>.1` is the newest).
//!
//! `RUST_LOG=<level>` overrides the configured level. Only this crate logs below `warn`;
//! dependencies (winit, zbus, …) are limited to warnings. Writing never panics: a log file that
//! can't be opened leaves standard output as the only sink.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::gui_settings::GuiSettings;

struct FileSink {
    path: PathBuf,
    file: Option<File>,
    interval: Duration,
    backups: u32,
    next_rollover: SystemTime,
}

impl FileSink {
    fn open(path: PathBuf, interval_hours: u32, backups: u32) -> FileSink {
        let interval = Duration::from_secs(u64::from(interval_hours.max(1)) * 3600);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Like TimedRotatingFileHandler: the next rollover is one interval after the file was last written.
        let since = std::fs::metadata(&path).and_then(|m| m.modified()).unwrap_or_else(|_| SystemTime::now());
        let mut sink = FileSink { file: None, interval, backups, next_rollover: since + interval, path };
        if SystemTime::now() >= sink.next_rollover {
            sink.rotate();
        } else {
            sink.file = OpenOptions::new().create(true).append(true).open(&sink.path).ok();
        }
        sink
    }

    fn rotate(&mut self) {
        self.file = None;
        let numbered = |n: u32| {
            let mut p = self.path.as_os_str().to_owned();
            p.push(format!(".{n}"));
            PathBuf::from(p)
        };
        if self.backups == 0 {
            let _ = std::fs::remove_file(&self.path);
        } else {
            let _ = std::fs::remove_file(numbered(self.backups));
            for n in (1..self.backups).rev() {
                let _ = std::fs::rename(numbered(n), numbered(n + 1));
            }
            let _ = std::fs::rename(&self.path, numbered(1));
        }
        self.file = OpenOptions::new().create(true).append(true).open(&self.path).ok();
        self.next_rollover = SystemTime::now() + self.interval;
    }

    fn write(&mut self, line: &str) {
        if SystemTime::now() >= self.next_rollover {
            self.rotate();
        }
        if let Some(f) = self.file.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

struct Logger {
    level: LevelFilter,
    stdout: bool,
    file: Option<Mutex<FileSink>>,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let ours = metadata.target().starts_with(env!("CARGO_CRATE_NAME"));
        metadata.level() <= if ours { self.level } else { self.level.min(LevelFilter::Warn) }
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S,%3f");
        let file = record.file().and_then(|f| f.rsplit('/').next()).unwrap_or("?");
        let line =
            format!("{now} [{file}:{}][{}] - {}\n", record.line().unwrap_or(0), level_name(record.level()), record.args());
        if self.stdout {
            let _ = std::io::stdout().write_all(line.as_bytes());
        }
        if let Some(sink) = &self.file {
            sink.lock().unwrap_or_else(PoisonError::into_inner).write(&line);
        }
    }

    fn flush(&self) {
        let _ = std::io::stdout().flush();
        if let Some(sink) = &self.file
            && let Some(f) = sink.lock().unwrap_or_else(PoisonError::into_inner).file.as_mut()
        {
            let _ = f.flush();
        }
    }
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::Error => "ERROR",
        Level::Warn => "WARNING",
        Level::Info => "INFO",
        Level::Debug => "DEBUG",
        Level::Trace => "TRACE",
    }
}

/// The Python level names (`CRITICAL` maps to `error`, the most severe level `log` has).
fn parse_level(name: &str) -> Option<LevelFilter> {
    Some(match name.trim().to_uppercase().as_str() {
        "TRACE" => LevelFilter::Trace,
        "DEBUG" => LevelFilter::Debug,
        "INFO" => LevelFilter::Info,
        "WARNING" | "WARN" => LevelFilter::Warn,
        "ERROR" | "CRITICAL" => LevelFilter::Error,
        "OFF" => LevelFilter::Off,
        _ => return None,
    })
}

pub fn install(settings: &GuiSettings) {
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|v| parse_level(&v))
        .or_else(|| parse_level(&settings.debug_level))
        .unwrap_or(LevelFilter::Debug);
    let file = settings.save_debug.then(|| {
        let path = crate::utils::expand_tilde(&settings.log_file);
        Mutex::new(FileSink::open(path, settings.log_rotation_interval, settings.log_backup_count))
    });
    let logger = Logger { level, stdout: settings.show_debug, file };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_level_names() {
        assert_eq!(parse_level("debug"), Some(LevelFilter::Debug));
        assert_eq!(parse_level("WARNING"), Some(LevelFilter::Warn));
        assert_eq!(parse_level("CRITICAL"), Some(LevelFilter::Error));
        assert_eq!(parse_level("loud"), None);
    }

    #[test]
    fn rotation_keeps_the_configured_number_of_backups() {
        let dir = std::env::temp_dir().join(format!("onedrivelinux-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("gui.log");
        let mut sink = FileSink::open(path.clone(), 1, 2);
        for i in 0..4 {
            sink.write(&format!("run {i}\n"));
            sink.rotate();
        }
        sink.write("current\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), "current\n");
        assert_eq!(std::fs::read_to_string(dir.join("gui.log.1")).unwrap_or_default(), "run 3\n");
        assert_eq!(std::fs::read_to_string(dir.join("gui.log.2")).unwrap_or_default(), "run 2\n");
        assert!(!dir.join("gui.log.3").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
