//! The system tray icon, through the StatusNotifierItem D-Bus protocol (KDE, XFCE, Cinnamon,
//! GNOME with the AppIndicator extension, …). Its icon shows the combined state of all profiles
//! and its tooltip lists each profile's status.

use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};

use crate::assets;
use crate::events::{AppEvent, EventSender, TrayAction};

/// Combined state of all profiles, most urgent first: Error > Stopped > Syncing > Idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrayState {
    Error,
    Stopped,
    Syncing,
    Idle,
}

impl TrayState {
    pub fn label(self) -> &'static str {
        match self {
            TrayState::Error => "ERROR",
            TrayState::Stopped => "STOPPED",
            TrayState::Syncing => "SYNCING",
            TrayState::Idle => "IDLE",
        }
    }
}

/// ARGB32 in network byte order, as StatusNotifierItem wants.
fn pixmap(png: &[u8]) -> Vec<ksni::Icon> {
    let Some((width, height, rgba)) = assets::rgba(png) else { return Vec::new() };
    let data = rgba.chunks_exact(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
    vec![ksni::Icon { width: i32::try_from(width).unwrap_or(0), height: i32::try_from(height).unwrap_or(0), data }]
}

struct Icons {
    idle: Vec<ksni::Icon>,
    syncing: Vec<ksni::Icon>,
    stopped: Vec<ksni::Icon>,
    error: Vec<ksni::Icon>,
}

pub struct SyncTray {
    events: EventSender,
    state: TrayState,
    tooltip: String,
    icons: Icons,
}

impl SyncTray {
    fn item(&self, label: &str, action: TrayAction) -> MenuItem<Self> {
        StandardItem { label: label.into(), activate: Box::new(move |t: &mut Self| t.events.send(AppEvent::Tray(action))), ..Default::default() }.into()
    }
}

impl ksni::Tray for SyncTray {
    fn id(&self) -> String {
        "onedrivelinux".into()
    }

    fn title(&self) -> String {
        "OnedriveLinux".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        match self.state {
            TrayState::Idle => self.icons.idle.clone(),
            TrayState::Syncing => self.icons.syncing.clone(),
            TrayState::Stopped => self.icons.stopped.clone(),
            TrayState::Error => self.icons.error.clone(),
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "OnedriveLinux".into(), description: self.tooltip.clone(), ..Default::default() }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.events.send(AppEvent::Tray(TrayAction::Activate));
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![self.item("Show/Hide", TrayAction::ShowHide), self.item("Settings", TrayAction::Settings), self.item("Quit", TrayAction::Quit)]
    }
}

pub struct TrayHandle {
    handle: ksni::blocking::Handle<SyncTray>,
    shown: Option<(TrayState, String)>,
}

impl TrayHandle {
    /// Change the icon and tooltip (a no-op when they are unchanged).
    pub fn set(&mut self, state: TrayState, tooltip: String) {
        let next = Some((state, tooltip));
        if self.shown == next {
            return;
        }
        log::debug!("Updating tray icon and tooltip");
        if let Some((state, tooltip)) = next.clone() {
            self.handle.update(|t| {
                t.state = state;
                t.tooltip = tooltip;
            });
        }
        self.shown = next;
    }

    pub fn shutdown(&self) {
        self.handle.shutdown().wait();
    }
}

/// Show the tray icon; `None` when the desktop has no StatusNotifier host (then closing the
/// window minimizes it instead of hiding it).
pub fn spawn(events: EventSender) -> Option<TrayHandle> {
    let icons = Icons { idle: pixmap(assets::CLOUD_DONE), syncing: pixmap(assets::CLOUD_SYNC), stopped: pixmap(assets::CLOUD_STOP), error: pixmap(assets::CLOUD_ERROR) };
    let tray = SyncTray { events, state: TrayState::Stopped, tooltip: "OnedriveLinux".into(), icons };
    match tray.spawn() {
        Ok(handle) => Some(TrayHandle { handle, shown: None }),
        Err(e) => {
            log::warn!("[GUI] System tray not available: {e}");
            None
        }
    }
}
