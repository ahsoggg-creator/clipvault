//! Look & feel: palette, egui style, small painting helpers.

use clipvault_core::Kind;
use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, Response, RichText, Sense, Stroke, Ui, Vec2, vec2};

pub const ACCENT: Color32 = Color32::from_rgb(124, 108, 240);
pub const GOOD: Color32 = Color32::from_rgb(63, 185, 120);
pub const WARN: Color32 = Color32::from_rgb(230, 160, 50);
pub const BAD: Color32 = Color32::from_rgb(225, 90, 90);
pub const PANEL: Color32 = Color32::from_rgb(26, 28, 38);
pub const CARD: Color32 = Color32::from_rgb(34, 37, 50);
pub const ROW_HOVER: Color32 = Color32::from_rgb(44, 48, 66);
pub const ROW_SELECTED: Color32 = Color32::from_rgb(58, 52, 112);

pub fn install(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(10.0, 4.0);
        s.spacing.menu_margin = Margin::same(6);
        s.spacing.interact_size.y = 24.0;
        let v = &mut s.visuals;
        let r = CornerRadius::same(6);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = r;
        }
        v.selection.bg_fill = Color32::from_rgb(70, 62, 140);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        v.panel_fill = PANEL;
        v.window_fill = CARD;
        v.extreme_bg_color = Color32::from_rgb(18, 19, 27);
        v.faint_bg_color = CARD;
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(46, 50, 68);
        v.widgets.inactive.bg_fill = Color32::from_rgb(46, 50, 68);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(60, 65, 88);
        v.widgets.hovered.bg_fill = Color32::from_rgb(60, 65, 88);
    });
}

pub fn kind_color(k: Kind) -> Color32 {
    match k {
        Kind::Text => Color32::from_rgb(140, 155, 190),
        Kind::Rich => Color32::from_rgb(190, 140, 240),
        Kind::Image => Color32::from_rgb(80, 200, 150),
        Kind::Files => Color32::from_rgb(240, 180, 80),
        Kind::Color => Color32::from_rgb(240, 110, 150),
        Kind::Link => Color32::from_rgb(90, 160, 255),
        Kind::Path => Color32::from_rgb(230, 150, 100),
    }
}

/// Small rounded label with a colored background.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let font = FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE);
    let size = galley.size() + vec2(12.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 9.0, color.gamma_multiply(0.85));
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
    resp
}

/// Primary (accent-filled) button.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(ACCENT)
            .min_size(Vec2::new(0.0, 26.0)),
    )
}

/// A filter chip: highlighted when selected.
pub fn chip(ui: &mut Ui, text: &str, selected: bool, color: Color32) -> Response {
    let mut b = egui::Button::new(RichText::new(text).size(12.5));
    if selected {
        b = b.fill(color.gamma_multiply(0.55)).stroke(Stroke::new(1.0, color));
    }
    ui.add(b)
}

/// Card-like frame for grouped content.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(10))
}
