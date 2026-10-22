//! Look & feel: spacing, fonts, accent colours and status chips.

use egui::{Color32, CornerRadius, FontId, Stroke, TextStyle, Theme, ThemePreference, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(0x3b, 0x6e, 0xd8);
pub const GREEN: Color32 = Color32::from_rgb(0x2e, 0x9e, 0x5b);
pub const AMBER: Color32 = Color32::from_rgb(0xd9, 0x8c, 0x1f);
pub const RED: Color32 = Color32::from_rgb(0xd0, 0x45, 0x45);
pub const GREY: Color32 = Color32::from_rgb(0x8a, 0x8f, 0x98);

pub fn apply(ctx: &egui::Context, dark: bool) {
    ctx.set_theme(if dark {
        ThemePreference::Dark
    } else {
        ThemePreference::Light
    });
}

/// One-off style setup at start-up.
pub fn install(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        use egui::FontFamily::{Monospace, Proportional};
        style.text_styles = [
            (TextStyle::Heading, FontId::new(22.0, Proportional)),
            (TextStyle::Body, FontId::new(15.0, Proportional)),
            (TextStyle::Monospace, FontId::new(14.0, Monospace)),
            (TextStyle::Button, FontId::new(15.0, Proportional)),
            (TextStyle::Small, FontId::new(12.0, Proportional)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.interact_size.y = 26.0;
    });
    for theme in [Theme::Light, Theme::Dark] {
        let mut v = match theme {
            Theme::Light => Visuals::light(),
            Theme::Dark => Visuals::dark(),
        };
        let radius = CornerRadius::same(6);
        v.window_corner_radius = CornerRadius::same(10);
        v.widgets.noninteractive.corner_radius = radius;
        v.widgets.inactive.corner_radius = radius;
        v.widgets.hovered.corner_radius = radius;
        v.widgets.active.corner_radius = radius;
        v.widgets.open.corner_radius = radius;
        v.selection.bg_fill = ACCENT.gamma_multiply(if theme == Theme::Dark { 0.7 } else { 0.35 });
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        if theme == Theme::Light {
            v.panel_fill = Color32::from_rgb(0xf3, 0xf5, 0xf8);
            v.window_fill = Color32::WHITE;
            v.faint_bg_color = Color32::from_rgb(0xf0, 0xf3, 0xf7);
            // Input fields: a light grey well with a visible border, distinct from white cards.
            v.extreme_bg_color = Color32::from_rgb(0xf4, 0xf6, 0xf9);
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xd0, 0xd5, 0xdd));
        } else {
            v.window_fill = Color32::from_rgb(0x24, 0x26, 0x2b);
            v.panel_fill = Color32::from_rgb(0x1b, 0x1d, 0x21);
            v.extreme_bg_color = Color32::from_rgb(0x14, 0x15, 0x18);
        }
        ctx.set_visuals_of(theme, v);
    }
}

/// Small rounded label with a coloured background.
pub fn chip(ui: &mut egui::Ui, text: impl Into<String>, color: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text.into()).small().color(color));
        })
        .response
}

/// A titled card used to group sections in the detail panel and dashboard.
pub fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::group(ui.style())
        .fill(ui.visuals().window_fill)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(egui::RichText::new(title).strong().size(16.0));
                ui.add_space(4.0);
            }
            add(ui)
        })
        .inner
}

#[cfg(test)]
mod tests {
    /// Every non-ASCII character shown in the UI must exist in the bundled
    /// fonts, otherwise it renders as an empty box on some systems.
    #[test]
    fn ui_glyphs_are_available() {
        let sources = [
            include_str!("../app.rs"),
            include_str!("advisee.rs"),
            include_str!("dashboard.rs"),
            include_str!("fyp.rs"),
            include_str!("lock.rs"),
            include_str!("placement.rs"),
            include_str!("settings.rs"),
            include_str!("student_form.rs"),
            include_str!("student_list.rs"),
            include_str!("widgets.rs"),
        ];
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        let font = egui::FontId::proportional(14.0);
        let mut missing: Vec<char> = sources
            .iter()
            .flat_map(|src| src.lines())
            .filter(|line| !line.trim_start().starts_with("//"))
            .flat_map(str::chars)
            .filter(|c| !c.is_ascii() && !ctx.fonts_mut(|f| f.has_glyph(&font, *c)))
            .collect();
        missing.dedup();
        assert!(missing.is_empty(), "missing glyphs: {missing:?}");
    }
}
