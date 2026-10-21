//! Icons and small widgets shared by the windows.
//!
//! The icons are Lucide SVGs, the same set as PdfCraft's (`assets/lucide/`, ISC licence), drawn
//! the same way: recoloured to white with a 1.75 stroke, rasterized by egui_extras at the size
//! they're shown, and tinted for each use.

use std::collections::HashMap;
use std::sync::OnceLock;

use egui::{Color32, Image, ImageSource, Response, Ui, Vec2};

/// The OneDrive blue of the Python app's header (`rgb(0, 120, 212)`).
pub const ONEDRIVE_BLUE: Color32 = Color32::from_rgb(0, 120, 212);
pub const WARNING_TEXT: Color32 = Color32::from_rgb(200, 40, 40);
pub const OK_TEXT: Color32 = Color32::from_rgb(30, 140, 60);
/// Tints of the warning and running/stopped status icons.
pub const AMBER: Color32 = Color32::from_rgb(250, 190, 40);
pub const RUNNING: Color32 = Color32::from_rgb(70, 200, 90);
pub const STOPPED: Color32 = Color32::from_rgb(235, 70, 70);

macro_rules! lucide {
    ($($name:literal),* $(,)?) => {
        [$(($name, include_str!(concat!("../../assets/lucide/", $name, ".svg")))),*]
    };
}

const SVGS: [(&str, &str); 21] = lucide![
    "power", "x", "play", "square", "folder", "user-round", "settings", "hard-drive", "triangle-alert", "circle-check",
    "circle-x", "cloud", "trash-2", "image", "film", "file", "file-text", "file-spreadsheet", "file-code", "file-archive", "file-music",
];

/// Each icon recoloured white (so tinting gives any colour) with PdfCraft's stroke weight.
fn white_svgs() -> &'static HashMap<&'static str, &'static [u8]> {
    static SVGS_WHITE: OnceLock<HashMap<&'static str, &'static [u8]>> = OnceLock::new();
    SVGS_WHITE.get_or_init(|| {
        SVGS.iter()
            .map(|(name, svg)| {
                let white = svg.replace("currentColor", "#ffffff").replace("stroke-width=\"2\"", "stroke-width=\"1.75\"");
                // Made once for the app's lifetime, so leaking keeps the bytes `'static` for egui.
                let bytes: &'static [u8] = Box::leak(white.into_bytes().into_boxed_slice());
                (*name, bytes)
            })
            .collect()
    })
}

/// The image source of a Lucide icon; an unknown name shows `square`.
pub fn source(name: &str) -> ImageSource<'static> {
    let svgs = white_svgs();
    let (name, bytes) = match svgs.get_key_value(name).or_else(|| svgs.get_key_value("square")) {
        Some((n, b)) => (*n, *b),
        None => ("square", &[][..]),
    };
    ImageSource::Bytes { uri: format!("bytes://lucide/{name}.svg").into(), bytes: bytes.into() }
}

/// Lucide icon `name` at `size` points, tinted.
pub fn icon(name: &str, size: f32, tint: Color32) -> Image<'static> {
    Image::new(source(name)).fit_to_exact_size(Vec2::splat(size)).tint(tint)
}

/// A frameless button showing only an icon (tinted with `tint`), with a tooltip.
pub fn icon_button(ui: &mut Ui, name: &str, size: f32, tint: Color32, enabled: bool, tooltip: &str) -> Response {
    let button = egui::Button::image(icon(name, size, tint)).frame(false);
    ui.add_enabled(enabled, button).on_hover_text(tooltip).on_disabled_hover_text(tooltip)
}

/// A field label with an explanation on hover (the Python app's label tooltips).
pub fn hint_label(ui: &mut Ui, text: &str, hint: &str) -> Response {
    let response = ui.label(text);
    if hint.is_empty() { response } else { response.on_hover_text(hint) }
}

/// A checkbox bound to a `"true"`/`"false"` client option; returns whether it changed.
pub fn option_checkbox(ui: &mut Ui, config: &mut crate::onedrive_config::OdConfig, key: &str, label: &str, hint: &str, enabled: bool) -> bool {
    let mut value = crate::onedrive_config::is_true(config, key);
    let response = ui.add_enabled(enabled, egui::Checkbox::new(&mut value, label));
    let response = if hint.is_empty() { response } else { response.on_hover_text(hint).on_disabled_hover_text(hint) };
    if response.changed() {
        log::info!("[GUI] {key} is {}", if value { "checked" } else { "unchecked" });
        crate::onedrive_config::set_bool(config, key, value);
        true
    } else {
        false
    }
}

/// A whole-number client option; out-of-range or unparsable values show clamped.
pub fn option_number(ui: &mut Ui, config: &mut crate::onedrive_config::OdConfig, key: &str, range: std::ops::RangeInclusive<i64>, enabled: bool) {
    let mut value = crate::onedrive_config::int(config, key).clamp(*range.start(), *range.end());
    let response = ui.add_enabled(enabled, egui::DragValue::new(&mut value).range(range).speed(1.0));
    if response.changed() {
        config.insert(key.to_string(), value.to_string());
    }
}

/// A free-text client option.
pub fn option_text(ui: &mut Ui, config: &mut crate::onedrive_config::OdConfig, key: &str, hint: &str, enabled: bool) {
    let mut value = crate::onedrive_config::text(config, key).to_string();
    let edit = egui::TextEdit::singleline(&mut value).hint_text(hint).desired_width(f32::INFINITY);
    if ui.add_enabled(enabled, edit).changed() {
        config.insert(key.to_string(), value);
    }
}

/// Pick a folder; starts in `start` (or the home folder).
pub fn pick_folder(start: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let start = start.filter(|p| p.is_dir()).map(std::path::Path::to_path_buf).unwrap_or_else(crate::utils::home_dir);
    rfd::FileDialog::new().set_directory(start).pick_folder()
}

/// Pick a file; starts in `start` (or the home folder).
pub fn pick_file(start: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let start = start.filter(|p| p.is_dir()).map(std::path::Path::to_path_buf).unwrap_or_else(crate::utils::home_dir);
    rfd::FileDialog::new().set_directory(start).pick_file()
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_icon_is_white_and_parses() {
        let svgs = super::white_svgs();
        assert_eq!(svgs.len(), super::SVGS.len());
        for (name, bytes) in svgs {
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(text.contains("#ffffff") && !text.contains("currentColor"), "{name}");
            assert!(text.contains("<svg") && text.contains("</svg>"), "{name}");
        }
        assert!(matches!(super::source("no-such-icon"), egui::ImageSource::Bytes { uri, .. } if uri == "bytes://lucide/square.svg"));
    }

    /// Every icon draws white strokes when rasterized like egui_extras does.
    #[test]
    fn every_icon_renders() {
        use resvg::{tiny_skia, usvg};
        for (name, bytes) in super::white_svgs() {
            let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut pixmap = tiny_skia::Pixmap::new(48, 48).unwrap();
            resvg::render(&tree, tiny_skia::Transform::from_scale(2.0, 2.0), &mut pixmap.as_mut());
            let drawn = pixmap.pixels().iter().filter(|p| p.alpha() > 128).count();
            assert!(drawn > 20, "{name} drew {drawn} pixels");
            assert!(pixmap.pixels().iter().filter(|p| p.alpha() == 255).all(|p| p.red() == 255 && p.green() == 255 && p.blue() == 255), "{name} isn't white");
        }
    }
}
