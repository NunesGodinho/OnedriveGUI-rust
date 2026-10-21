//! Messages from background threads (sync workers, client commands, version checks, the tray)
//! to the app. Each send wakes the UI, which handles messages in `App::logic`, so they are
//! processed even while the window is hidden in the tray.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;

use crate::parser::ParserEvent;
use crate::transfers::TransferItem;
use crate::version::ClientVersion;

#[derive(Debug)]
pub enum WorkerMsg {
    Parsed(ParserEvent),
    Transfer(TransferItem),
    /// The client process ended (or never started); the text says how.
    Exited(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// Left click on the tray icon.
    Activate,
    ShowHide,
    Settings,
    Quit,
}

#[derive(Debug)]
pub enum AppEvent {
    Worker { id: u64, msg: WorkerMsg },
    Tray(TrayAction),
    /// `Ok` when the client accepted the login response URL.
    Login { profile: String, result: Result<(), String> },
    Logout { profile: String, result: Result<(), String> },
    SharePointSites(Vec<String>),
    /// Library name → drive id.
    SharePointLibraries(IndexMap<String, String>),
    ClientVersion(ClientVersion),
}

/// The sending half, cheap to clone into any thread.
#[derive(Clone)]
pub struct EventSender {
    tx: Sender<AppEvent>,
    /// Set once the UI exists; the tray starts before it does.
    ctx: Arc<OnceLock<egui::Context>>,
}

impl EventSender {
    pub fn send(&self, event: AppEvent) {
        if self.tx.send(event).is_ok()
            && let Some(ctx) = self.ctx.get()
        {
            ctx.request_repaint();
        }
    }

    pub fn attach(&self, ctx: &egui::Context) {
        let _ = self.ctx.set(ctx.clone());
    }
}

pub fn channel_pair() -> (EventSender, Receiver<AppEvent>) {
    let (tx, rx) = channel();
    (EventSender { tx, ctx: Arc::new(OnceLock::new()) }, rx)
}
