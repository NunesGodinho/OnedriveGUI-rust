//! One running `onedrive --monitor` process per syncing profile (`WorkerThread` in the Python
//! app). A thread reads the client's output, parses it and sends the results to the app.

use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::events::{AppEvent, EventSender, WorkerMsg};
use crate::parser::{OutputParser, ParserEvent};
use crate::process::{for_each_line, spawn_merged};
use crate::transfers::TransferItem;

pub struct SyncWorker {
    /// Unique for the app's lifetime; messages from a stopped worker are recognised and dropped.
    pub id: u64,
    child: Arc<Mutex<Option<Child>>>,
    stop_requested: Arc<AtomicBool>,
}

/// What a worker needs to know about its profile.
pub struct WorkerConfig {
    pub profile: String,
    pub client: String,
    pub config_dir: PathBuf,
    pub sync_dir: PathBuf,
    /// Extra client options: `--resync --resync-auth` or `--force` after the user agreed.
    pub extra_args: Vec<String>,
}

fn kill(child: &Mutex<Option<Child>>) {
    if let Some(c) = child.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        let _ = c.kill();
    }
}

impl SyncWorker {
    pub fn start(id: u64, config: WorkerConfig, events: EventSender) -> SyncWorker {
        let child: Arc<Mutex<Option<Child>>> = Arc::default();
        let stop_requested = Arc::new(AtomicBool::new(false));
        let worker = SyncWorker { id, child: child.clone(), stop_requested: stop_requested.clone() };
        log::info!("[GUI] Starting worker for profile {}", config.profile);
        let on_failure = events.clone();
        let spawned = std::thread::Builder::new().name(format!("sync-{}", config.profile)).spawn(move || run(id, config, events, child, stop_requested));
        if let Err(e) = spawned {
            log::error!("[GUI] could not start a worker thread: {e}");
            on_failure.send(AppEvent::Worker { id, msg: WorkerMsg::Exited(format!("Could not start syncing: {e}")) });
        }
        worker
    }

    /// Stop the client. The worker thread then reports `Exited`.
    pub fn stop(&self) {
        self.stop_requested.store(true, Ordering::SeqCst);
        kill(&self.child);
    }
}

fn run(id: u64, config: WorkerConfig, events: EventSender, child_slot: Arc<Mutex<Option<Child>>>, stop: Arc<AtomicBool>) {
    let send = |msg: WorkerMsg| events.send(AppEvent::Worker { id, msg });
    let mut args = vec![format!("--confdir={}", config.config_dir.display()), "--monitor".into(), "-v".into()];
    args.extend(config.extra_args.iter().cloned());
    log::info!("[GUI] OneDrive config dir: {}", config.config_dir.display());
    log::info!("[GUI] Monitoring command: '{} {}'", config.client, args.join(" "));

    let mut parser = OutputParser::new();
    send(WorkerMsg::Parsed(ParserEvent::Status(parser.status().clone())));

    let (child, reader) = match spawn_merged(&config.client, &args) {
        Ok(started) => started,
        Err(e) => {
            let message = format!("Could not start the OneDrive client '{}': {e}", config.client);
            log::error!("[{}] {message}", config.profile);
            send(WorkerMsg::Exited(message));
            return;
        }
    };
    *child_slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(child);
    // A stop that arrived while the process was starting.
    if stop.load(Ordering::SeqCst) {
        kill(&child_slot);
    }

    for_each_line(reader, |line| {
        log::debug!("[{}] {line}", config.profile);
        for event in parser.feed(line) {
            match event {
                ParserEvent::Transfer(t) => send(WorkerMsg::Transfer(TransferItem::new(&t, &config.sync_dir))),
                ParserEvent::LoginRequired { kill: true } => {
                    kill(&child_slot);
                    send(WorkerMsg::Parsed(ParserEvent::LoginRequired { kill: true }));
                }
                other => send(WorkerMsg::Parsed(other)),
            }
        }
    });

    let status = child_slot.lock().unwrap_or_else(PoisonError::into_inner).take().map(|mut c| c.wait());
    let how = match status {
        Some(Ok(s)) => format!("OneDrive client exited ({s})"),
        Some(Err(e)) => format!("OneDrive client ended: {e}"),
        None => "OneDrive client ended".to_string(),
    };
    log::info!("[{}] {how}", config.profile);
    send(WorkerMsg::Exited(how));
}
