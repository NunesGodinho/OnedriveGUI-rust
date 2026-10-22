//! The desktop's light/dark preference, for the "System" theme.
//!
//! winit doesn't report it on X11 (which includes XWayland, used when there is a tray), so it is
//! read from the XDG desktop portal (`org.freedesktop.appearance` › `color-scheme`), which KDE,
//! GNOME and others provide. A background thread reads it once and then follows the portal's
//! `SettingChanged` signal, so switching the desktop between light and dark restyles the app.

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

use crate::events::{AppEvent, EventSender};

const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "color-scheme";

/// The portal's `color-scheme`: 1 prefers dark, 2 prefers light, 0 (no preference) and anything
/// else give `None`.
pub fn from_color_scheme(value: u32) -> Option<egui::Theme> {
    match value {
        1 => Some(egui::Theme::Dark),
        2 => Some(egui::Theme::Light),
        _ => None,
    }
}

/// The `u32` in a portal value; older portals wrap it in one more variant.
fn as_u32(value: &Value<'_>) -> Option<u32> {
    match value {
        Value::U32(v) => Some(*v),
        Value::Value(inner) => as_u32(inner),
        _ => None,
    }
}

fn portal(connection: &Connection) -> zbus::Result<Proxy<'static>> {
    Proxy::new(connection, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Settings")
}

fn watch_blocking(events: &EventSender) -> zbus::Result<()> {
    let connection = Connection::session()?;
    let proxy = portal(&connection)?;
    let current: OwnedValue = proxy.call("ReadOne", &(NAMESPACE, KEY))?;
    let theme = as_u32(&current).and_then(from_color_scheme);
    log::info!("[GUI] Desktop colour scheme: {theme:?}");
    events.send(AppEvent::SystemTheme(theme));
    for message in proxy.receive_signal("SettingChanged")? {
        let Ok((namespace, key, value)) = message.body().deserialize::<(String, String, OwnedValue)>() else { continue };
        if namespace == NAMESPACE && key == KEY {
            let theme = as_u32(&value).and_then(from_color_scheme);
            log::info!("[GUI] Desktop colour scheme changed: {theme:?}");
            events.send(AppEvent::SystemTheme(theme));
        }
    }
    Ok(())
}

/// Report the desktop's preference now and whenever it changes. Without a portal, nothing is
/// reported and "System" falls back to dark.
pub fn watch(events: EventSender) {
    let spawned = std::thread::Builder::new().name("system-theme".into()).spawn(move || {
        if let Err(e) = watch_blocking(&events) {
            log::warn!("[GUI] Could not read the desktop colour scheme from the XDG portal: {e}");
        }
    });
    if let Err(e) = spawned {
        log::warn!("[GUI] could not start the colour scheme watcher: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_values() {
        assert_eq!(from_color_scheme(1), Some(egui::Theme::Dark));
        assert_eq!(from_color_scheme(2), Some(egui::Theme::Light));
        assert_eq!(from_color_scheme(0), None);
        assert_eq!(from_color_scheme(7), None);
        assert_eq!(as_u32(&Value::U32(2)), Some(2));
        assert_eq!(as_u32(&Value::Value(Box::new(Value::U32(1)))), Some(1));
        assert_eq!(as_u32(&Value::from("dark")), None);
    }

    /// Live: reads the running desktop's preference (`cargo test -- --ignored system_theme`).
    #[test]
    #[ignore = "needs a desktop session with the XDG portal"]
    fn reads_the_desktop_preference() {
        let connection = Connection::session().unwrap();
        let value: OwnedValue = portal(&connection).unwrap().call("ReadOne", &(NAMESPACE, KEY)).unwrap();
        let scheme = as_u32(&value);
        println!("color-scheme = {scheme:?} → {:?}", scheme.and_then(from_color_scheme));
        assert!(scheme.is_some());
    }
}
