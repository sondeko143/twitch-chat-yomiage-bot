use eframe::egui;
use std::sync::Arc;

/// Japanese-capable fonts bundled with Windows, in preference order.
pub fn font_candidates() -> &'static [&'static str] {
    &[
        r"C:\Windows\Fonts\YuGothM.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ]
}

/// Install a Japanese system font as the top-priority family. If none is found,
/// logs a warning and leaves egui's (empty) default fonts in place.
pub fn install_japanese_font(ctx: &egui::Context) {
    let Some((path, bytes)) = font_candidates()
        .iter()
        .find_map(|p| std::fs::read(p).ok().map(|b| (*p, b)))
    else {
        eprintln!(
            "warning: 日本語システムフォントが見つかりません。日本語が表示されない可能性があります"
        );
        return;
    };
    eprintln!("loaded Japanese font: {path}");
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("jp".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "jp".to_owned());
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_candidates_non_empty_and_windows_fonts() {
        let c = font_candidates();
        assert!(!c.is_empty());
        assert!(c.iter().all(|p| p.contains("Fonts")));
    }
}
