//! The interface fonts, the same as PdfCraft's: Inter (regular, medium and semibold) and
//! JetBrains Mono for monospace text, at PdfCraft's text sizes. Both are under the SIL Open Font
//! License (`assets/fonts/OFL-*.txt`). egui's built-in fonts stay behind them as fallbacks, for
//! the symbols and scripts these faces don't cover.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily, FontId, TextStyle};

const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");
const JETBRAINS_MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");

fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [("Inter", INTER_REGULAR), ("Inter-Medium", INTER_MEDIUM), ("Inter-SemiBold", INTER_SEMIBOLD), ("JetBrainsMono", JETBRAINS_MONO)] {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "Inter".to_owned());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "JetBrainsMono".to_owned());
    // Letters missing from JetBrains Mono come from Inter before egui's defaults.
    fonts.families.entry(FontFamily::Monospace).or_default().insert(1, "Inter".to_owned());
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (family, primary) in [("medium", "Inter-Medium"), ("semibold", "Inter-SemiBold")] {
        let mut stack = vec![primary.to_owned()];
        stack.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(family.into()), stack);
    }
    fonts
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

/// Install the fonts and PdfCraft's text sizes, for both the light and the dark theme.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
    ctx.global_style_mut(|s| {
        s.text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(13.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(11.0));
        s.text_styles.insert(TextStyle::Heading, semibold(17.0));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(13.0));
    });
    let style = ctx.global_style();
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_face_is_installed_first_in_its_family() {
        let fonts = super::definitions();
        for name in ["Inter", "Inter-Medium", "Inter-SemiBold", "JetBrainsMono"] {
            assert!(fonts.font_data.contains_key(name), "{name}");
        }
        assert_eq!(fonts.families[&egui::FontFamily::Proportional].first().map(String::as_str), Some("Inter"));
        assert_eq!(fonts.families[&egui::FontFamily::Monospace].first().map(String::as_str), Some("JetBrainsMono"));
        assert_eq!(fonts.families[&egui::FontFamily::Name("semibold".into())].first().map(String::as_str), Some("Inter-SemiBold"));
    }
}
