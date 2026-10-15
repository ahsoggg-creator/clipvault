//! The compact quick-paste window the hotkey opens: a narrow list like Win+V's, where one click pastes.

use std::sync::atomic::Ordering;
use std::time::Duration;

use clipvault_core::classify::parse_color;
use clipvault_core::clipfmt;
use clipvault_core::config::Lang;
use clipvault_core::i18n::{S, fmt};
use clipvault_core::{Item, Kind};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, TextureHandle, Ui, vec2,
};

use crate::shared::Shared;
use crate::theme::{self, Icon};
use crate::ui::{MARK, RowAction, View, ago, entry_label, row_menu};
use crate::{App, platform};

const ROW_H: f32 = 46.0;

/// What a row reports back besides the click on the row itself.
struct RowOut {
    resp: egui::Response,
    pin: bool,
    delete: bool,
}

impl App {
    pub(crate) fn compact_header(&mut self, ui: &mut Ui) {
        let lang = self.shared.lang();
        let (paused, incognito) = (
            self.shared.settings.lock().unwrap_or_else(|e| e.into_inner()).paused,
            self.shared.incognito.load(Ordering::Relaxed),
        );
        self.drag_strip(ui, 26.0);
        ui.horizontal(|ui| {
            if let Some(icon) = &self.icon {
                ui.add(egui::Image::new(icon).fit_to_exact_size(vec2(18.0, 18.0)));
            }
            ui.label(RichText::new("Clipvault").strong().size(14.0));
            if self.shared.admin {
                theme::badge(ui, S::AdminBadge.get(lang), theme::BAD);
            }
            if paused {
                theme::badge(ui, S::PausedBanner.get(lang), theme::WARN);
            } else if incognito {
                theme::badge(ui, S::IncognitoBanner.get(lang), theme::ACCENT);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::icon_button(ui, Icon::Close, S::Close.get(lang)).clicked() {
                    platform::hide_popup();
                }
                if theme::icon_button(ui, Icon::Gear, S::TraySettings.get(lang)).clicked() {
                    self.popup.view = View::Settings;
                    platform::set_compact(false);
                }
                if theme::icon_button(ui, Icon::Expand, S::BtnExpand.get(lang)).clicked() {
                    platform::set_compact(false);
                }
            });
        });
        self.search_box(ui, lang);
        // a chip for every kind that is actually in the history
        let counts = self.shared.history.lock().unwrap_or_else(|e| e.into_inner()).counts();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if theme::chip(ui, S::All.get(lang), self.popup.kind.is_none(), theme::ACCENT).clicked() {
                self.popup.kind = None;
            }
            for k in Kind::ALL {
                if !counts.iter().any(|(ck, n)| *ck == k && *n > 0) {
                    continue;
                }
                let on = self.popup.kind == Some(k);
                if theme::chip(ui, self.shared.kind_label(k), on, theme::kind_color(k)).clicked() {
                    self.popup.kind = if on { None } else { Some(k) };
                }
            }
            ui.toggle_value(&mut self.popup.pinned_only, S::PinnedOnly.get(lang));
        });
        ui.add_space(2.0);
    }

    pub(crate) fn compact_footer(&mut self, ui: &mut Ui) {
        let lang = self.shared.lang();
        if !self.popup.marked.is_empty() {
            ui.horizontal(|ui| {
                let label = fmt(S::PasteSelected, lang, &[&self.popup.marked.len().to_string()]);
                if theme::primary_button(ui, &label).clicked() {
                    self.paste_marked();
                }
                if ui.button(S::ClearSelection.get(lang)).clicked() {
                    self.popup.marked.clear();
                }
            });
        } else if let Some((text, at)) = &self.popup.toast
            && at.elapsed() < Duration::from_secs(4)
        {
            ui.label(RichText::new(text).color(theme::GOOD).small());
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        } else {
            self.popup.toast = None;
            ui.add(egui::Label::new(RichText::new(S::CompactHints.get(lang)).weak().small()).wrap());
        }
    }

    pub(crate) fn compact_list(&mut self, ui: &mut Ui, now: u64) {
        let lang = self.shared.lang();
        if self.popup.results.is_empty() {
            let empty = self.shared.history.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                let msg = if empty { S::EmptyHistory } else { S::NothingFound };
                ui.label(RichText::new(msg.get(lang)).weak());
            });
            return;
        }
        let selected = self.selected_index();
        let scroll = std::mem::take(&mut self.popup.scroll_to_selected);
        let total = self.popup.results.len();
        let mut action: Option<(u64, RowAction)> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, total, |ui, range| {
                if scroll && let Some(sel) = selected {
                    let top = sel as f32 * ROW_H;
                    ui.scroll_to_rect(
                        Rect::from_min_size(
                            ui.min_rect().min + vec2(0.0, top - range.start as f32 * ROW_H),
                            vec2(10.0, ROW_H),
                        ),
                        None,
                    );
                }
                let h = self.shared.history.lock().unwrap_or_else(|e| e.into_inner());
                for idx in range {
                    let Some(item) = h.get(self.popup.results[idx]) else {
                        continue;
                    };
                    let id = item.id;
                    let marked = self.popup.marked.contains(&id);
                    let out = compact_row(ui, item, idx, selected == Some(idx), marked, now, lang, &self.shared);
                    if out.pin {
                        action = Some((id, RowAction::Pin));
                    } else if out.delete {
                        action = Some((id, RowAction::Delete));
                    } else if out.resp.clicked() {
                        let m = ui.input(|i| i.modifiers);
                        action = Some((
                            id,
                            if m.ctrl || m.command {
                                RowAction::Mark
                            } else if m.shift {
                                RowAction::PasteText
                            } else {
                                RowAction::Paste
                            },
                        ));
                    }
                    if let Some(a) = row_menu(&out.resp, item, lang) {
                        action = Some((id, a));
                    }
                    let tex = &mut self.popup.tex;
                    out.resp
                        .on_hover_ui(|ui| hover_preview(ui, item, tex, || h.blob(id).ok().flatten(), lang));
                }
            });
        if let Some((id, a)) = action {
            self.row_action(id, a);
        }
    }
}

/// What shows in the tooltip when the mouse rests on an entry: the whole text, the picture, the list of files.
fn hover_preview(
    ui: &mut Ui,
    item: &Item,
    tex: &mut Option<(u64, TextureHandle)>,
    blob: impl FnOnce() -> Option<Vec<u8>>,
    lang: Lang,
) {
    ui.set_max_width(340.0);
    match item.kind {
        Kind::Image => {
            if tex.as_ref().map(|(i, _)| *i) != Some(item.id) {
                let ctx = ui.ctx().clone();
                *tex = blob().as_deref().and_then(clipfmt::png_decode).map(|img| {
                    let ci = egui::ColorImage::from_rgba_unmultiplied(
                        [img.width as usize, img.height as usize],
                        &img.pixels,
                    );
                    (item.id, ctx.load_texture("preview", ci, egui::TextureOptions::LINEAR))
                });
            }
            if let Some((_, t)) = tex {
                let size = t.size_vec2();
                let scale = (330.0 / size.x).min(260.0 / size.y).min(1.0);
                ui.add(egui::Image::new(&*t).fit_to_exact_size(size * scale));
            }
        }
        _ if item.volatile => {
            ui.label(RichText::new(entry_label(item, lang)).weak());
        }
        Kind::Files => {
            for line in item.text.lines().take(12) {
                ui.label(RichText::new(line).monospace().small());
            }
        }
        Kind::Color => {
            if let Some([r, g, b, a]) = parse_color(&item.text) {
                let (rect, _) = ui.allocate_exact_size(vec2(120.0, 50.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, 6.0, Color32::from_rgba_unmultiplied(r, g, b, a));
            }
            ui.label(RichText::new(&item.text).monospace());
        }
        _ => {
            let text: String = item.text.chars().take(700).collect();
            ui.add(egui::Label::new(RichText::new(text).monospace().small()).wrap());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compact_row(
    ui: &mut Ui,
    item: &Item,
    idx: usize,
    selected: bool,
    marked: bool,
    now: u64,
    lang: Lang,
    shared: &Shared,
) -> RowOut {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    let p = ui.painter().clone();
    let body = rect.shrink2(vec2(0.0, 1.0));
    if selected || marked {
        p.rect_filled(body, 6.0, theme::ROW_SELECTED);
    } else if hovered {
        p.rect_filled(body, 6.0, theme::ROW_HOVER);
    }
    if marked {
        p.rect_stroke(body, 6.0, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
    }
    let cy = rect.center().y;
    let x0 = rect.left() + 10.0;
    if marked {
        let st = Stroke::new(2.0, MARK);
        p.line_segment([egui::pos2(x0 + 1.0, cy), egui::pos2(x0 + 5.0, cy + 4.5)], st);
        p.line_segment([egui::pos2(x0 + 5.0, cy + 4.5), egui::pos2(x0 + 12.0, cy - 5.0)], st);
    } else if idx < 9 {
        p.text(
            egui::pos2(x0 + 6.0, cy),
            Align2::CENTER_CENTER,
            (idx + 1).to_string(),
            FontId::monospace(13.0),
            Color32::from_gray(150),
        );
    }
    let kind_col = theme::kind_color(item.kind);
    let sw = Rect::from_center_size(egui::pos2(x0 + 30.0, cy), vec2(14.0, 14.0));
    match (item.kind, parse_color(&item.text)) {
        (Kind::Color, Some([r, g, b, a])) => {
            p.rect_filled(sw, 4.0, Color32::from_rgba_unmultiplied(r, g, b, a));
        }
        _ => {
            p.rect_filled(sw, 4.0, kind_col.gamma_multiply(0.9));
        }
    }

    // the pin and delete buttons appear under the mouse; they sit on top of the row and take their own clicks
    let del_rect = Rect::from_center_size(egui::pos2(rect.right() - 20.0, cy), vec2(26.0, 24.0));
    let pin_rect = del_rect.translate(vec2(-28.0, 0.0));
    let (mut pin, mut delete) = (false, false);
    let right_edge = if hovered {
        pin_rect.left() - 4.0
    } else {
        rect.right() - 26.0
    };
    if hovered {
        let del = ui.interact(del_rect, ui.id().with(("del", item.id)), Sense::click());
        theme::icon_button_paint(ui, &del, del_rect, Icon::Close);
        delete = del.clicked();
        del.on_hover_text(S::DeleteHover.get(lang));
        let pin_resp = ui.interact(pin_rect, ui.id().with(("pin", item.id)), Sense::click());
        if pin_resp.hovered() {
            p.rect_filled(pin_rect, 5.0, Color32::from_rgb(60, 65, 88));
        }
        p.text(
            pin_rect.center(),
            Align2::CENTER_CENTER,
            "📌",
            FontId::proportional(13.0),
            if item.pinned {
                theme::WARN
            } else {
                Color32::from_gray(150)
            },
        );
        pin = pin_resp.clicked();
        pin_resp.on_hover_text(S::PinHover.get(lang));
    } else if item.pinned {
        p.text(
            egui::pos2(rect.right() - 20.0, cy),
            Align2::CENTER_CENTER,
            "📌",
            FontId::proportional(13.0),
            theme::WARN,
        );
    }

    let text_x = x0 + 50.0;
    let clip = Rect::from_min_max(egui::pos2(text_x, rect.top()), egui::pos2(right_edge, rect.bottom()));
    let main_col = if item.masked || item.volatile {
        Color32::from_gray(160)
    } else {
        Color32::from_gray(228)
    };
    p.with_clip_rect(clip).text(
        egui::pos2(text_x, rect.top() + 15.0),
        Align2::LEFT_CENTER,
        entry_label(item, lang),
        FontId::proportional(14.0),
        main_col,
    );
    let mut sub = format!("{} · {}", shared.kind_label(item.kind), ago(lang, now, item.last_used));
    if !item.source_app.is_empty() {
        sub.push_str(&format!(" · {}", item.source_app.trim_end_matches(".exe")));
    }
    if item.count > 1 {
        sub.push_str(&format!(" · ×{}", item.count));
    }
    if item.volatile {
        sub.push_str(&format!(" · {}", S::InMemoryOnly.get(lang)));
    } else if item.masked {
        sub.push_str(&format!(" · {}", S::Masked.get(lang)));
    }
    p.with_clip_rect(clip).text(
        egui::pos2(text_x, rect.bottom() - 12.0),
        Align2::LEFT_CENTER,
        sub,
        FontId::proportional(11.0),
        kind_col,
    );
    RowOut { resp, pin, delete }
}
