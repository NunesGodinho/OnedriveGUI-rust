//! The window and tray images bundled into the executable (Icons8, see README › Attribution).
//! The in-app icons are Lucide SVGs (`ui::widgets`).

pub const APP_ICON: &[u8] = include_bytes!("../assets/icons/icons8-cloud-80.png");
pub const CLOUD_DONE: &[u8] = include_bytes!("../assets/icons/icons8-cloud-done-80.png");
pub const CLOUD_ERROR: &[u8] = include_bytes!("../assets/icons/icons8-cloud-error-80.png");
pub const CLOUD_STOP: &[u8] = include_bytes!("../assets/icons/icons8-cloud-stop-80.png");
pub const CLOUD_SYNC: &[u8] = include_bytes!("../assets/icons/icons8-cloud-sync-80.png");

/// Decode a bundled PNG to (width, height, RGBA pixels).
pub fn rgba(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    match image::load_from_memory_with_format(png, image::ImageFormat::Png) {
        Ok(img) => {
            let img = img.to_rgba8();
            Some((img.width(), img.height(), img.into_raw()))
        }
        Err(e) => {
            log::error!("[GUI] could not decode a bundled icon: {e}");
            None
        }
    }
}

pub fn icon_data(png: &[u8]) -> Option<egui::IconData> {
    rgba(png).map(|(width, height, rgba)| egui::IconData { rgba, width, height })
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_icon_decodes() {
        for png in [
            super::APP_ICON,
            super::CLOUD_DONE,
            super::CLOUD_ERROR,
            super::CLOUD_STOP,
            super::CLOUD_SYNC,
        ] {
            assert!(super::rgba(png).is_some());
        }
    }
}
